
use bcrypt::{hash, DEFAULT_COST};
use mongodb::{ 
    bson::{bson, doc, Document}, error::Error as MongoError, options::Hint, results::UpdateResult, Client, Cursor, Database
};
use nearsay_server::{current_time_ms, NearsayError};
use serde::{de::DeserializeOwned, Deserialize};

use crate::{area::Rect, delete_old::today, types::{Post, Viewer, User, UserType, UserVotes, Vote, POI}};



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
            .projection(doc! {"votes": 0})
            .await
        {
            Err(mongo_err) => {
                eprintln!("error getting item {}", mongo_err);
                Err(())
            },
            Ok(item) => Ok(item)
        }
    }

    pub async fn delete(&self, collection: &str, id: &str) -> Result<(), ()> {
        match 
            self.db.collection::<Document>(collection)
            .delete_one( doc!{ "_id": id } )
            .await
        {
            Err(mongo_err) => {
                eprintln!("error getting item {}", mongo_err);
                Err(())
            },
            Ok(_) => Ok(())
        }
    }

    pub async fn get_user(&self, username: &str) -> Result<Option<User>, ()> {
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

        //TODO: test if votes are included in return obj
    }

    pub async fn insert_viewer(&self, uid: &str, avatar: usize, pos: &[f64]) -> Result<(), ()> {
        if let Err(mongo_err) = self.db.collection("users").insert_one(doc! {
            "_id": uid,
            "pos": pos,
            "avatar": avatar as i32,
            "updated": current_time_ms() as i64,
        }).await {
            eprintln!("mongodb error when starting anonymous viewer: {}", &mongo_err);
            return Err(())
        }

        Ok(())
    }

    /// will replace anonymous viewers, but not preexisting users
    /// 
    /// returns `Ok(position of anonymous viewer)` if an anonymous viewer was replaced
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
        let serverhash = match hash(userhash, DEFAULT_COST) {
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
                    // no position field yet
                    "updated": current_time_ms() as i64,
                    
                    "username": username,
                    "avatar": avatar as i32,
                    "hash": serverhash,
                    "votes": {}
                }
            )
            .upsert(true)
            .await
        {
            Err(mongo_err) => {
                eprintln!("mongodb error when adding new user: {}", &mongo_err);
                Err(NearsayError::ServerError)
            },
            Ok(UpdateResult {upserted_id, ..}) => {
                match upserted_id {
                    None => Ok(None),
                    Some(viewer_id) => {
                        let viewer_id = viewer_id.as_str().expect("upserted id should be a str");

                        let Ok(Some(viewer)) = 
                            self.get::<Viewer>("users", viewer_id).await
                            else { return Err(NearsayError::ServerError) };

                        Ok(Some(viewer.pos))
                    },
                }
            }
        }
    }

    pub async fn move_user(&self, uid: &str, new_pos: &[f64]) -> Result<(), ()> {

        match
            self.db.collection::<User>("users")
            .update_one(
                doc! { "_id": uid },
                doc! { 
                    "pos": new_pos,
                    "updated": current_time_ms() as i64
                }
            )
            .await
        {
            Err(mongo_err) => {
                eprintln!("error moving user: {}", mongo_err);
                Err(())
            },
            Ok(_) => Ok(()),
        }
    }

    async fn get_user_type(&self, uid: &str) -> Result<Option<UserType>, ()> {
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
                false => Ok(Some(UserType::Viewer)),
            },
        }
    }

    pub async fn sign_out(&self, uid: &str) -> Result<(), NearsayError> {
        match self.get_user_type(uid).await {
            Err(_) => Err(NearsayError::ServerError),
            Ok(None) => Err(NearsayError::UserNotFound),
            Ok(Some(UserType::Viewer)) => {
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

    pub async fn set_avatar(&self, uid: &str, new_avatar: usize) -> Result<(), ()> {
        if let Err(mongo_err) = self.db.collection::<User>("users").update_one(
                doc! { "_id": uid },
                doc! { 
                    "avatar": new_avatar as i32,
                    "updated": current_time_ms() as i64
                }
            ).await
        {
            eprintln!("error moving user: {}", mongo_err);
            return Err(());
        }

        Ok(())
    }


    pub async fn insert_post(&self, author: &str, pos: &[f64], body: &str) -> Result<(String, i64), MongoError> {
        
        let post_id = gen_id();
        let millis: i64 = current_time_ms() as i64;
        
        if let Err(mongo_err) = self.db.collection("posts").insert_one(doc! {
            "_id": post_id.clone(),
            "pos": pos,
            "updated": millis,

            "author": author,
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
    

    pub async fn get_vote(&self, uid: &str, post_id: &str) -> Result<Vote, MongoError> {
        let res = self.db.collection::<UserVotes>("users")
            .find_one(doc! { "_id": uid })
            .projection(doc! { format!("votes.{}", post_id): 1 }) 
            .await;
        
        match res {
            Ok(Some(user_vote)) => {
                match user_vote.votes.get(post_id) {
                    Some(vote) => Ok(vote.clone().into()),
                    None => Ok(Vote::None),
                }
            },
            Ok(None) => Ok(Vote::None),
            Err(mongo_err) => {
                eprintln!("error getting vote {}", mongo_err);
                Err(mongo_err)
            },
        }
    }

    pub async fn insert_vote(&self, uid: &str, post_id: &str, vote: Vote) -> Result<(), MongoError> {

        let prev_vote = self.get_vote(uid, post_id).await?;

        if vote == prev_vote { return Ok(()); }

        let update_user_vote_res = match vote {
            Vote::None => {
                self.db.collection::<User>("users")
                    .update_one( 
                        doc! {"_id": uid}, 
                        doc! { "$unset": { format!("votes.{}", post_id): 1 }}
                    ).await
            },
            _ => {
                self.db.collection::<User>("users")
                    .update_one( 
                        doc! {"_id": uid}, 
                        doc! { "$set": { format!("votes.{}", post_id): Into::<String>::into(vote.clone()) }}
                    ).await
            }
        };
        
        if let Err(mongo_err) = update_user_vote_res {
            eprintln!("error updating user vote: {}", mongo_err);
            return Err(mongo_err);
        }
        
        let delta_likes = match vote {
            Vote::Like => 1,
            _ => match prev_vote {
                Vote::Like => -1,
                _ => 0,
            },
        };
        let delta_dislikes = match vote {
            Vote::Dislike => 1,
            _ => match prev_vote {
                Vote::Dislike => -1,
                _ => 0,
            },
        };

        if let Err(mongo_err) = self.db.collection::<Post>("posts")
            .update_one(
                doc! {"_id": post_id},
                doc! {
                    "$inc": {
                        "likes": delta_likes,
                        "dislikes": delta_dislikes,
                        "expiry": vote.as_lifetime_weight() - prev_vote.as_lifetime_weight()
                    }
                }
            ).await
        {
            eprintln!("error updating like/dislike/expiry for post on vote: {}", mongo_err);
            return Err(mongo_err);
        }

        Ok(())
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