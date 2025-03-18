
use std::time::SystemTime;

use bcrypt::{hash, DEFAULT_COST};
use futures::{TryFutureExt, TryStreamExt};
use mongodb::{ 
    bson::{bson, doc, Document}, error::{Error as MongoError, ErrorKind, WriteError, WriteFailure}, options::Hint, results::{DeleteResult, UpdateResult}, Client, Cursor, Database
};
use nearsay_server::NearsayError;
use serde::{de::DeserializeOwned, Deserialize};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::{area::Rect, cache::{MapLayersCache, UserPOI}, cluster::{cluster, get_cluster_radius_degrees, Cluster, MAX_ZOOM_LEVEL}, types::{get_blurb_from_body, Guest, Post, User, UserType, Vote, VoteKind, POI}};



/// returns # of days since the epoch
fn today() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap()
        .as_secs() / 60 / 60 / 24
}

#[derive(Clone)]
pub struct NearsayDB {
    cache: MapLayersCache,
    mongo_db: Database,
}
impl NearsayDB {
    pub async fn new() -> Self {
        let mongo_db = Client::with_uri_str("mongodb://localhost:27017").await.unwrap().database("nearsay");
        let nearsay_db = Self { cache: MapLayersCache::new().await.unwrap(), mongo_db };

        nearsay_db.clone().start_nightly_cleanup_job().await;

        nearsay_db
    }

    pub async fn get<T>(&self, collection: &str, id: &str) -> Result<Option<T>, ()> 
    where T: Send + Sync + DeserializeOwned
    {
        match 
            self.mongo_db.collection::<T>(collection)
            .find_one( doc!{ "_id": id } )
            .await
        {
            Err(mongo_err) => {
                eprintln!("error getting item {}", mongo_err);
                Err(())
            },
            Ok(item) => Ok(item)
        }
    }

    async fn delete(&self, collection: &str, id: &str) -> Result<(), ()> {
        match 
            self.mongo_db.collection::<Document>(collection)
            .delete_one( doc!{ "_id": id } )
            .await
        {
            Err(mongo_err) => {
                eprintln!("error deleting item {}", mongo_err);
                Err(())
            },
            Ok(_) => Ok(())
        }
    }

    /// moves self into a closure, so requires ownership
    async fn start_nightly_cleanup_job(self) {

        let sched = JobScheduler::new().await.unwrap();

        sched.add(
            // run every day at 04:00
            Job::new_async("0 0 4 * * *", 
                move |_, _| {
                    let mut nearsay_db = self.clone();
                    Box::pin(
                        async move {
                            nearsay_db.run_nightly_cleanup().await.unwrap()
                        } 
                    )
                }
            ).unwrap()
        ).await.unwrap();

        sched.start().await.unwrap();
    }
    async fn run_nightly_cleanup(&mut self) -> Result<(), mongodb::error::Error> {
        let delete_old_posts_res = 
            self.mongo_db.collection::<Document>("posts")
            .delete_many(doc! { "expiry": {"$lt": today() as i32} })
            .await?;
        println!("delete old posts result: {:?}", delete_old_posts_res);
        
        self.cache.flush_all_posts().await.unwrap();
        println!("cleared map layers cache");
        
        let mut all_posts = self.mongo_db.collection::<Post>("posts").find(doc! {}).await?;
        
        while let Some(post) = all_posts.try_next().await.unwrap() {
            self.cache.add_post_pt(&post._id, post.pos[0], post.pos[1], &get_blurb_from_body(&post.body)).await.unwrap();
        }
        println!("added all posts back into cache");
        
        Ok(())
    }

    pub async fn get_user_from_username(&self, username: &str) -> Result<Option<User>, ()> {
        match 
            self.mongo_db.collection::<User>("users")
            .find_one(doc! {"username": username})
            .hint(Hint::Name("username_1".to_string()))
            .await
        {
            Err(mongo_err) => {
                eprintln!("mongodb error when getting user: {}", &mongo_err);
                Err(())
            },
            Ok(user) => Ok(user),
        }
    }

    pub async fn insert_guest(&self, uid: &str, avatar: usize, pos: &[f64]) -> Result<(), ()> {
        if let Err(mongo_err) = self.mongo_db.collection("users").insert_one(doc! {
            "_id": uid,
            "pos": pos,
            "avatar": avatar as i32,
        }).await {
            eprintln!("mongodb error when starting  guest: {}", &mongo_err);
            return Err(())
        }

        Ok(())
    }

    /// will replace guests, but not preexisting users
    /// 
    /// returns `Ok(position of guest)` if a guest was replaced
    pub async fn insert_user(&self, uid: &str, username: &str, userhash: &str, avatar: usize) -> Result<Option<[f64; 2]>, NearsayError> {

        // check if username is taken
        match
            self.mongo_db.collection::<User>("users")
            .count_documents(doc! {"username": username })
            .limit(1)
            .await 
        {
            Ok(count) => if count != 0 { return Err(NearsayError::UsernameTaken); },
            Err(mongo_err) => {
                eprintln!("mongodb error when checking if username taken: {}", &mongo_err);
                return Err(NearsayError::ServerError);
            },
        }

        // hash password (again) to store in db
        let userhash = match hash(userhash, DEFAULT_COST) {
            Err(bcrypt_err) => {
                eprintln!("bcrypt error when hashing userhash: {}", &bcrypt_err);
                return Err(NearsayError::ServerError);
            },
            Ok(res) => res,
        };
        
        // insert user data into db
        match 
            self.mongo_db.collection::<User>("users")
            .update_one(
                doc! { "_id": uid },
                doc! {
                    "$set": {
                        // no position field yet
                        
                        "username": username,
                        "avatar": avatar as i32,
                        "hash": userhash,
                    }
                }
            )
            .upsert(true)
            .await
        {
            Err(mongo_err) => {
                eprintln!("mongodb error when adding new user: {}", &mongo_err);
                Err(NearsayError::ServerError)
            },
            Ok(UpdateResult {upserted_id, modified_count, ..}) => {
                if modified_count == 0 { return Ok(None) };

                match upserted_id {
                    None => Ok(None),
                    Some(guest_id) => {
                        let guest_id = guest_id.as_str().expect("upserted id should be a str");

                        let Ok(Some(guest)) = 
                            self.get::<Guest>("users", guest_id).await
                            else { return Err(NearsayError::ServerError) };

                        Ok(Some(guest.pos))
                    },
                }
            }
        }
    }

    pub async fn set_user_pos(&mut self, uid: &str, pos: &[f64]) -> Result<(), ()> {
        self.cache.set_user_pos(uid, pos[0], pos[1]).await.map_err(|_| ())
    }

    pub async fn edit_user(&mut self, uid: &str, update: &Document) -> Result<(), NearsayError> {
        
        self.cache.edit_user_if_exists(
            uid,
            update.get("avatar").map(|a| a.as_i32().unwrap() as usize),
            update.get("username").map(|u| u.as_str().unwrap()),
        ).await.map_err(|_| NearsayError::ServerError)?;

        match
            self.mongo_db.collection::<User>("users")
            .update_one(
                doc! { "_id": uid },
                doc! { "$set": update }
            )
            .await
        {
            Err(mongo_err) => {

                match *mongo_err.kind {
                    ErrorKind::Write(WriteFailure::WriteError(WriteError {code, ..})) 
                    if code == 11000 => {

                        Err(NearsayError::UsernameTaken)
                    },
                    other => {
                        eprintln!("error updating user: {}", other);
                        Err(NearsayError::ServerError)
                    }
                }
                    
            },
            Ok(_) => Ok(()),
        }
    }

    pub async fn get_user_type(&self, uid: &str) -> Result<Option<UserType>, ()> {
        match 
            self.mongo_db.collection::<Document>("users")
            .find_one(doc! { "_id": uid })
            .await
        {
            Err(mongo_err) => {
                eprintln!("error getting user type: {}", mongo_err);
                Err(())
            },
            Ok(None) => Ok(None),
            Ok(Some(document)) => match document.contains_key("username") {
                true => Ok(Some(UserType::User)),
                false => Ok(Some(UserType::Guest)),
            },
        }
    }

    pub async fn sign_out(&self, uid: &str) -> Result<(), NearsayError> {
        match self.get_user_type(uid).await {
            Err(_) => Err(NearsayError::ServerError),
            Ok(None) => Err(NearsayError::UserNotFound),
            Ok(Some(UserType::Guest)) => {
                self.delete("users", uid).await.map_err(|_| NearsayError::ServerError)
            },
            Ok(Some(UserType::User)) => {
                match 
                    self.mongo_db.collection::<User>("users")
                    .update_one(
                        doc! { "_id": uid }, 
                        doc! { "$unset": { "pos": "" } }
                    )
                    .await
                {
                    Err(mongo_err) => {
                        eprintln!("mongodb error when signing out user and removing 'pos' field: {}", &mongo_err);
                        Err(NearsayError::ServerError)
                    },
                    Ok(_) => Ok(()),
                }
            },
        }
        
    }

    pub async fn delete_user(&self, uid: &str) -> Result<(), ()> {
        self.delete("users", uid).await?;

        // delete user's votes
        match 
            self.mongo_db.collection::<Document>("votes")
            .delete_many(doc! { "uid": uid })
            .hint(Hint::Name("uid_1".to_string()))
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(())
        }
    }

    pub async fn delete_post(&mut self, post_id: &str) -> Result<(), ()> {
        self.delete("posts", post_id).await?;

        // delete blurb from cache TODO
        // self.cache.del_blurb(post_id).await.map_err(|_| ())?;
        
        // delete post votes
        match 
            self.mongo_db.collection::<Document>("votes")
            .delete_many(doc! { "postId": post_id })
            .hint(Hint::Name("postId_1".to_string()))
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(())
        }
    }

    /// returns (post id, blurb)
    pub async fn insert_post(&mut self, author_id: Option<&str>, pos: &[f64], body: &str) -> Result<(String, String), ()> {
        
        let post_id = gen_id();
        
        if let Err(mongo_err) = self.mongo_db.collection("posts").insert_one(doc! {
            "_id": post_id.clone(),
            "pos": pos,

            "authorId": author_id,
            "body": body,
            "likes": 0,
            "dislikes": 0,
            "views": 0,
            "expiry": (today() + 7) as i64,
        }).await {
            eprintln!("error inserting new post: {}", mongo_err);
            return Err(());
        }

        let blurb = get_blurb_from_body(body);
        
        self.cache.add_post_pt(&post_id, pos[0], pos[1], &blurb).await.unwrap();
        
        Ok((post_id, blurb))
    }
    

    pub async fn get_vote(&self, uid: &str, post_id: &str) -> Result<VoteKind, ()> {
        match 
            self.mongo_db.collection::<Document>("votes")
            .find_one( doc!{ "post_id": post_id, "uid": uid } )
            .hint(Hint::Name("postId_1_uid_1".to_string()))
            .await
        {
            Err(mongo_err) => {
                eprintln!("error getting vote {}", mongo_err);
                Err(())
            },
            Ok(None) => Ok(VoteKind::None),
            Ok(Some(document)) => Ok(VoteKind::from_str(document.get_str("kind").unwrap()))
        }
    }

    pub async fn insert_vote(&self, uid: &str, post_id: &str, vote: VoteKind) -> Result<(), ()> {
        let prev_vote = self.get_vote(uid, post_id).await?;

        if vote == prev_vote { return Ok(()) }

        let delta_likes = match vote {
            VoteKind::Like => 1,
            _ => match prev_vote {
                VoteKind::Like => -1,
                _ => 0,
            },
        };
        let delta_dislikes = match vote {
            VoteKind::Dislike => 1,
            _ => match prev_vote {
                VoteKind::Dislike => -1,
                _ => 0,
            },
        };

        // update counters in posts
        if let Err(mongo_err) =  
            self.mongo_db.collection::<Post>("posts")
            .update_one(
                doc! {"_id": post_id},
                doc! {
                    "$inc": {
                        "likes": delta_likes,
                        "dislikes": delta_dislikes,
                        "expiry": vote.get_lifetime_weight() - prev_vote.get_lifetime_weight()
                    }
                }
            ).await
        {
            eprintln!("error updating like/dislike/expiry for post on vote: {}", mongo_err);
            return Err(());
        }

        // update votes collection
        match vote {
            VoteKind::None => match 
                self.mongo_db.collection::<Document>("votes")
                    .delete_one(doc! { "post_id": post_id, "uid": uid } )
                    .await
            {
                Err(mongo_err) => {
                    eprintln!("error deleting vote {}", mongo_err);
                    Err(())
                },
                Ok(_) => Ok(()),
            }

            other => match 
                self.mongo_db.collection::<Document>("votes")
                    .update_one(
                        doc! { "post_id": post_id, "uid": uid },
                        doc! { "$set": { "kind": other.as_str() } }
                    )
                    .upsert(true)
                    .await
            {
                Err(mongo_err) => {
                    eprintln!("error updating vote {}", mongo_err);
                    Err(())
                },
                Ok(_) => Ok(()),
            }
            
        }

    }

    pub async fn increment_view(&self, post_id: &str) -> Result<UpdateResult, MongoError> {

        let res = self.mongo_db.collection::<Post>("posts")
            .update_one(
                doc! { "_id": post_id }, 
                doc! { "$inc": { 
                    "views": 1,
                    "expiry": 1,
                } }
            ).await;

        match res {
            Err(mongo_err) => {
                eprintln!("error incrementing view: {}", mongo_err);
                return Err(mongo_err);
            },
            other => other,
        }   
    }

    pub async fn geoquery_post_pts(&mut self, layer: usize, within: &Rect) -> Result<Vec<Cluster>, MongoError> {
        
        if let Ok(posts) = self.cache.geoquery_post_pts(layer, within).await {
            return Ok(posts);
        }

        let mut post_docs = self.geoquery::<Post>("posts", within).await?;
        let mut res: Vec<Cluster> = vec![];
        
        while let Some(doc) = post_docs.try_next().await? {
            res.push(doc.into());
        }

        // don't cluster if zoomed all the way in
        if layer == MAX_ZOOM_LEVEL { Ok(res) }
        else { Ok(cluster(&res[..], get_cluster_radius_degrees(layer)))  }
    }

    pub async fn geoquery_users(&mut self, within: &Rect) -> Result<Vec<UserPOI>, Box<dyn std::error::Error>> {
        self.cache.geoquery_users(within).await
    }

    async fn geoquery<T>(&self, collection: &str, within: &Rect) -> Result<Cursor<Document>, MongoError>
    where T: Send + Sync + POI
    {
        self.mongo_db.collection::<T>(collection)
            .aggregate(vec! [
                doc! {
                    "$match": { "pos": { "$geoWithin": within.as_geo_json() } }
                },
                T::get_poi_projection()
            ])
            .hint( Hint::Name("pos_2dsphere".to_string()) )
            .await
    }
}

pub fn gen_id() -> String {
    let mut res = [' '; 10];
    
    for i in 0..res.len() {
        res[i] = to_base64_symbol(rand::random::<u8>() & 63)
    }

    res.iter().collect()
}

fn to_base64_symbol(num: u8) -> char {
    if      num < 10    { ('0' as u8 + num) as char }
    else if num < 36    { (num - 10 + 'a' as u8) as char }
    else if num < 62    { (num - 36 + 'A' as u8) as char }
    else if num == 62   { '-' }
    else if num == 63   { '_' }
    else                { panic!("cant convert num > 63 to a base64 symbol") }
}