
use bcrypt::{hash, DEFAULT_COST};
use mongodb::{ 
    bson::{bson, doc, Document}, error::{Error as MongoError, ErrorKind, WriteError, WriteFailure}, options::Hint, results::UpdateResult, Client, Cursor, Database
};
use nearsay_server::{current_time_ms, NearsayError};
use serde::{de::DeserializeOwned, Deserialize};

use crate::{area::Rect, delete_old::today, types::{Guest, Post, User, UserType, Vote, VoteKind, POI}};



#[derive(Clone)]
pub struct NearsayDB {
    pub db: Database
}
impl NearsayDB {
    pub async fn new() -> Self {
        Self {
            db: Client::with_uri_str("mongodb://localhost:27017").await.unwrap().database("nearsay")
        }
    }

    pub async fn get<T>(&self, collection: &str, id: &str) -> Result<Option<T>, ()> 
    where T: Send + Sync + DeserializeOwned
    {
        match 
            self.db.collection::<T>(collection)
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
            self.db.collection::<Document>(collection)
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

    pub async fn get_user_from_username(&self, username: &str) -> Result<Option<User>, ()> {
        match 
            self.db.collection::<User>("users")
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
        if let Err(mongo_err) = self.db.collection("users").insert_one(doc! {
            "_id": uid,
            "pos": pos,
            "avatar": avatar as i32,
            "updated": current_time_ms() as i64,
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
            self.db.collection::<User>("users")
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
            self.db.collection::<User>("users")
            .update_one(
                doc! { "_id": uid },
                doc! {
                    "$set": {
                        // no position field yet
                        "updated": current_time_ms() as i64,
                        
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

    pub async fn set_user_pos(&self, uid: &str, new_pos: &[f64]) -> Result<(), ()> {
        self.update_user(uid, &mut doc! { "pos": &new_pos as &[f64] }).await.map_err(|_| ())
    }

    pub async fn update_user(&self, uid: &str, update: &mut Document) -> Result<(), NearsayError> {
        update.insert("updated", current_time_ms() as i64);

        match
            self.db.collection::<User>("users")
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
            self.db.collection::<Document>("users")
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
                    self.db.collection::<User>("users")
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
            self.db.collection::<Document>("votes")
            .delete_many(doc! { "uid": uid })
            .hint(Hint::Name("uid_1".to_string()))
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(())
        }
    }

    pub async fn delete_post(&self, post_id: &str) -> Result<(), ()> {
        self.delete("posts", post_id).await?;

        // delete post votes
        match 
            self.db.collection::<Document>("votes")
            .delete_many(doc! { "postId": post_id })
            .hint(Hint::Name("postId_1".to_string()))
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(())
        }
    }

    pub async fn insert_post(&self, author_id: Option<&str>, pos: &[f64], body: &str) -> Result<(String, i64), MongoError> {
        
        let post_id = gen_id();
        let millis: i64 = current_time_ms() as i64;
        
        if let Err(mongo_err) = self.db.collection("posts").insert_one(doc! {
            "_id": post_id.clone(),
            "pos": pos,
            "updated": millis,

            "authorId": author_id,
            "body": body,
            "likes": 0,
            "dislikes": 0,
            "views": 0,
            "expiry": (today() + 7) as i64,
        }).await {
            eprintln!("error inserting new post {}", mongo_err);
            return Err(mongo_err);
        }

        Ok((post_id, millis))
    }
    

    pub async fn get_vote(&self, uid: &str, post_id: &str) -> Result<VoteKind, ()> {
        match 
            self.db.collection::<Document>("votes")
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
            self.db.collection::<Post>("posts")
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
                self.db.collection::<Document>("votes")
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
                self.db.collection::<Document>("votes")
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

        let res = self.db.collection::<Post>("posts")
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

    pub async fn get_pois<T>(&self, collection: &str, within: &Rect<f64>, exclude: Option<&Rect<f64>>) -> Cursor<Document>
    where T: Send + Sync + POI
    {

        let query = match exclude {
            Some(exclude) => {
                doc! {
                    "$match": {
                        "$and": [
                            {"pos": { "$geoWithin": within.as_geo_json() }},
                            {"pos": { "$not": { "$geoWithin": exclude.as_geo_json() } }},
                        ] 
                    }
                }
            },
            None => doc! {
                "$match": { "pos": { "$geoWithin": within.as_geo_json() } }
            }
        };

        self.db.collection::<T>(collection)
            .aggregate(vec! [
                query,
                T::get_poi_projection()
            ])
            .hint( Hint::Name("pos_2dsphere".to_string()) )
            .await.unwrap()
    }
}

use rand::Rng;

pub fn gen_id() -> String {

    let str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";

    let mut res: [char; 10] = [' '; 10];

    for i in 0..10 {
        res[i] = str.chars().nth(rand::thread_rng().gen_range(0..64)).unwrap();
    }

    res.iter().collect()
}