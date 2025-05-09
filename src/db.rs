

use std::time::SystemTime;

use bcrypt::{hash, DEFAULT_COST};
use chrono::Utc;
use futures::TryStreamExt;
use mongodb::{ 
    bson::{doc, Document}, error::{Error as MongoError, ErrorKind, WriteError, WriteFailure}, options::Hint, results::UpdateResult, Client, Cursor, Database
};
use nearsay_server::NearsayError;
use serde::de::DeserializeOwned;
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::{area::Rect, cache::{MapCache, UserPOI}, cluster::{cluster, get_cluster_radius_degrees, Cluster, MAX_ZOOM_LEVEL}, types::{get_blurb_from_body, Post, User, VoteKind, POI}};



/// returns # of days since the epoch
fn today() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap()
        .as_secs() / 60 / 60 / 24
}

#[derive(Clone)]
pub struct NearsayDB {
    cache: MapCache,
    mongo_db: Database,
}
impl NearsayDB {
    pub async fn new() -> Self {
        let nearsay_db = Self { 
            cache: MapCache::new().await.unwrap(), 
            mongo_db: Client::with_uri_str("mongodb://localhost:27017").await.unwrap().database("nearsay")
        };
        
        nearsay_db.clone().start_nightly_cleanup_job().await;

        nearsay_db
    }

    pub async fn get<T>(&self, collection: &str, id: &str) -> Result<Option<T>, ()> 
    where T: Send + Sync + DeserializeOwned
    {
        self.mongo_db.collection::<T>(collection)
            .find_one( doc!{ "_id": id } )
            .await
            .map_err(|e| eprintln!("error getting item {e}"))
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

    async fn start_nightly_cleanup_job(&mut self) {

        let sched = JobScheduler::new().await.unwrap();
        
        self.run_nightly_cleanup().await.unwrap();
        
        let db_clone = self.clone();
        sched.add(
            // run every day at 00:00
            Job::new_async("0 0 0 * * *", 
                move |_, _| {
                    let mut db_clone = db_clone.clone();
                    Box::pin(
                        async move {
                            db_clone.run_nightly_cleanup().await.unwrap()
                        } 
                    )
                }
            ).unwrap()
        ).await.unwrap();

        sched.start().await.unwrap();
    }
    async fn run_nightly_cleanup(&mut self) -> Result<(), mongodb::error::Error> {
        
        println!("running nightly cleanup at: {}", Utc::now());
        
        let delete_old_posts_res = 
            self.mongo_db.collection::<Document>("posts")
            .delete_many(doc! { "expiry": {"$lt": today() as i32} })
            .await?;
        println!("- delete old posts result: {:?}", delete_old_posts_res);
        
        self.cache.flush_all_posts().await.unwrap();
        println!("- cleared posts in map cache");
        
        let mut all_posts = self.mongo_db.collection::<Post>("posts").find(doc! {}).await?;
        
        while let Some(post) = all_posts.try_next().await.unwrap() {
            self.cache.add_post_pt(&post._id, post.pos[0], post.pos[1], &get_blurb_from_body(&post.body)).await.unwrap();
        }
        println!("- added all posts back into cache");
        
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
    
    pub async fn get_cache_username(&mut self, uid: &str) -> Result<Option<String>, ()> {
        self.cache.get_username(uid).await.map_err(|e| eprintln!("when getting cached username {e}"))
    }
    pub async fn get_cache_pos_and_avatar(&mut self, uid: &str) -> Result<Option<((f64, f64), usize)>, ()> {
        self.cache.get_pos_and_avatar(uid).await.map_err(|e| eprintln!("when getting guest: {e}"))
    }
    
    pub async fn add_user_to_cache(&mut self, uid: &str, socket_id: &str, pos: &[f64], avatar: usize, username: Option<&str>) -> Result<(), ()> {
        self.cache.add_user(uid, socket_id, pos[0], pos[1], avatar, username).await
        .map_err(|e| eprintln!("when adding user to cache: {e}"))
    }
    
    pub async fn delete_user_from_cache(&mut self, uid: Option<&str>, socket_id: &str) -> Result<(), ()> {
        match uid {
            Some(uid) => self.cache.del_user(uid, socket_id).await,
            None => self.cache.del_user_from_socket(socket_id).await,
        }
        .map_err(|e| eprintln!("when deleting user from cache: {e}"))
    }
    
    pub async fn get_uid_from_socket(&mut self, socket_id: &str) -> Result<Option<String>, ()> {
        self.cache.get_uid_from_socket(socket_id).await
        .map_err(|e| eprintln!("when getting uid from socket: {e}"))
    }

    pub async fn insert_user(&mut self, uid: &str, username: &str, password: &str, avatar: usize) -> Result<(), NearsayError> {

        // check if username is taken
        match
            self.mongo_db.collection::<User>("users")
            .count_documents(doc! {"username": username })
            .limit(1)
            .await 
        {
            Ok(count) => if count != 0 { return Err(NearsayError::UsernameTaken) },
            Err(e) => {
                eprintln!("=when checking if username taken: {e}");
                return Err(NearsayError::ServerError)
            },
        }

        // hash password (again) to store in db
        let userhash = hash(password, DEFAULT_COST).map_err(|e| {
            eprintln!("when hashing password again: {e}");
            NearsayError::ServerError
        })?;
        
        // insert user data into db
        self.mongo_db.collection("users")
            .insert_one(
                doc! {
                    "_id": uid,
                    "username": username,
                    "avatar": avatar as i32,
                    "hash": userhash,
                }
            ).await
            .map_err(|e| {
                eprintln!("mongodb error when adding new user: {e}");
                NearsayError::ServerError
            })?;
            
        self.cache.edit_user_if_exists(uid, &Some(avatar), &Some(username.to_string())).await.map_err(|e| {
            eprintln!("when updating cache while adding new user: {e}");
            NearsayError::ServerError
        })
    }
    
    /// returns old position of user
    pub async fn set_user_pos(&mut self, uid: &str, pos: &[f64]) -> Result<(f64, f64), ()> {
        self.cache.set_user_pos(uid, pos[0], pos[1]).await
    }

    pub async fn edit_user(&mut self, uid: &str, avatar: &Option<usize>, username: &Option<String>) -> Result<(), NearsayError> {
        
        let mut update = doc! {};
        if let Some(avatar) = avatar {
            update.insert("avatar", *avatar as i32);
        }
        if let Some(username) = username {
            update.insert("username", username);
        }
        
        self.mongo_db.collection::<User>("users")
            .update_one(
                doc! { "_id": uid },
                doc! { "$set": update }
            )
            .await
            .map_err(|e| match *e.kind {
                ErrorKind::Write(WriteFailure::WriteError(WriteError {code, ..})) if code == 11000 => NearsayError::UsernameTaken,
                other => {
                    eprintln!("error updating user: {}", other);
                    NearsayError::ServerError
                }
            })?;
        
        self.cache.edit_user_if_exists(uid, avatar, username).await.map_err(|_| NearsayError::ServerError)?;
        
        Ok(())
    }

    pub async fn delete_user(&mut self, uid: &str, socket_id: Option<&str>) -> Result<(), ()> {
        if let Some(socket_id) = socket_id {
            self.delete_user_from_cache(Some(uid), socket_id).await?;
        }
        
        self.delete("users", uid).await?;

        // delete user's votes
        match 
            self.mongo_db.collection::<Document>("votes")
            .delete_many(doc! { "uid": uid })
            .hint(Hint::Name("uid_1".to_string()))
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(eprintln!("err deleting user votes: {e}"))
        }
    }

    pub async fn delete_post(&mut self, post_id: &str) -> Result<(), ()> {
        self.cache.del_post(post_id).await.map_err(|e| {
            eprintln!("when deleting post from cache: {e}")
        })?;
        
        self.delete("posts", post_id).await?;

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
            // .map_err(|e| eprintln!("when adding post pt: {e}"))?;
        
        Ok((post_id, blurb))
    }
    

    pub async fn get_vote(&self, uid: &str, post_id: &str) -> Result<VoteKind, ()> {
        match 
            self.mongo_db.collection::<Document>("votes")
            .find_one( doc!{ "postId": post_id, "uid": uid } )
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
                    .delete_one(doc! { "postId": post_id, "uid": uid } )
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
                        doc! { "postId": post_id, "uid": uid },
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

    pub async fn geoquery_post_pts(&mut self, zoom: usize, within: &Rect) -> Result<Vec<Cluster>, ()> {
        
        if let Ok(posts) = self.cache.geoquery_post_pts(zoom, within).await {
            return Ok(posts);
        }

        let mut post_docs = self.geoquery::<Post>("posts", within).await
        .map_err(|e| eprintln!("when geoquery post pts{e}"))?;
    
        let mut res: Vec<Cluster> = vec![];
        
        while let Some(doc) = post_docs.try_next().await.map_err(|_| ())? {            
            res.push(doc.into());
        }

        // don't cluster if zoomed all the way in
        if zoom >= MAX_ZOOM_LEVEL { Ok(res) }
        else { Ok(cluster(&res[..], get_cluster_radius_degrees(zoom)))  }
    }

    pub async fn geoquery_users(&mut self, within: &Rect) -> Result<Vec<UserPOI>, ()> {
        self.cache.geoquery_users(within).await.map_err(|e| eprintln!("when geoquery users: {e}"))
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