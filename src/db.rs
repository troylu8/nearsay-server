
use std::time::SystemTime;
use bcrypt::{hash, DEFAULT_COST};
use mongodb::{ 
    bson::doc, error::Error as MongoError, options::Hint, results::UpdateResult, Client, Cursor, Database
};
use nearsay_server::NearsayError;

use crate::{area::Rect, delete_old::today, types::{Post, User, UserVotes, POI}};

#[derive(Debug, PartialEq, Clone)]
pub enum Vote { Like, Dislike, None }

impl Vote {
    /// number of days added/subtracted from post expiry as a result of this vote
    fn as_lifetime_weight(&self) -> i32 {
        match self {
            Vote::Like => 2,
            Vote::Dislike => -1,
            Vote::None => 0,
        }
    }
}

impl From<String> for Vote {
    fn from(value: String) -> Self {
        match value.as_str() {
            "like" => Vote::Like,
            "dislike" => Vote::Dislike,
            _ => Vote::None,
        }
    }
}
impl Into<String> for Vote {
    fn into(self) -> String {
        match self {
            Vote::Like => "like".to_string(),
            Vote::Dislike => "dislike".to_string(),
            Vote::None => "none".to_string(),
        }
    }
}

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

    pub async fn get_user(&self, username: &str) -> Result<Option<User>, MongoError> {
        let get_user_req = self.db.collection::<User>("users").find_one(doc! {"username": username}).await;

        if let Err(mongo_err) = &get_user_req {
            eprintln!("mongodb error when getting user: {}", &mongo_err);
        }

        get_user_req
    }

    pub async fn insert_user(&self, id: &str, username: &str, userhash: &str) -> Result<(), NearsayError> {

        // check if username is taken
        let count_result = self.db.collection::<User>("users").count_documents(doc! {
            "username": username
        }).limit(1).await;

        match count_result {
            Ok(count) => if count != 0 { return Err(NearsayError::UsernameTaken); },
            Err(mongo_err) => {
                eprintln!("mongodb error when checking if username taken: {}", &mongo_err);
                return Err(NearsayError::ServerError);
            },
        }

        // hash password (again) to store in db
        let serverhash = match hash(userhash, DEFAULT_COST) {
            Ok(res) => res,
            Err(bcrypt_err) => {
                eprintln!("bcrypt error when hashing userhash: {}", &bcrypt_err);
                return Err(NearsayError::ServerError);
            },
        };
        
        // insert user data into db
        if let Err(mongo_err) = self.db.collection("users").insert_one(doc! {
            "_id": id,
            "username": username,
            "hash": serverhash,
            "votes": {}
        }).await {
            eprintln!("mongodb error when adding new user: {}", &mongo_err);
            return Err(NearsayError::ServerError);
        }


        Ok(())
    }

    pub async fn move_poi(&self, poi_id: &str, new_pos: &[f64]) -> Result<(), MongoError> {
        
        let res = self.db.collection::<POI>("pois").update_one(
                doc! { "_id": poi_id },
                doc! { "pos": new_pos }
            )
            .upsert(true)
            .await;

        if let Err(mongo_err) = &res { eprintln!("error moving poi: {}", mongo_err); }
        
        Ok(())
    }

    pub async fn get_post(&self, post_id: &str) -> Result<Option<Post>, MongoError> {
        match self.db.collection::<Post>("posts")
            .find_one(doc!{"_id": post_id})
            .await
        {
            Err(mongo_err) => {
                eprintln!("error getting post {}", mongo_err);
                Err(mongo_err)
            },
            other => other
        }
    }

    pub async fn insert_post(&self, author: &str, pos: &[f64], body: &str) -> Result<(String, i64), MongoError> {
        
        let post_id = gen_id();
        let millis: i64 = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis().try_into().expect("current time millis doesnt fit into i64");

        if let Err(mongo_err) = self.db.collection("pois").insert_one(doc! {
            "_id": post_id.clone(),
            "pos": pos,
            "variant": "post".to_string(),
            "updated": millis,
        }).await {
            eprintln!("error inserting new post poi {}", mongo_err);
            return Err(mongo_err);
        }

        if let Err(mongo_err) = self.db.collection("posts").insert_one(doc! {
            "_id": post_id.clone(),
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

    pub async fn get_poi(&self, id: &str) -> Result<Option<POI>, MongoError> {
        let res = self.db.collection::<POI>("pois").find_one(doc! { "_id": id }).await;

        if let Err(mongo_err) = &res { eprintln!("error getting poi: {}", mongo_err); }

        res
    }

    pub async fn search_pois(&self, within: &Rect<f64>, exclude: Option<&Rect<f64>>) -> Cursor<POI> {

        let query = match exclude {
            Some(exclude) => {
                doc! {
                    "$and": [
                        {"pos": { "$geoWithin": within.as_geo_json() }},
                        {"pos": { "$not": { "$geoWithin": exclude.as_geo_json() } }},
                    ] 
                }
            },
            None => doc! {
                "pos": { "$geoWithin": within.as_geo_json() }
            }
        };
    
        self.db.collection::<POI>("pois")
            .find(query)
            .projection(doc! { "data": 0 })
            .hint( Hint::Name(String::from("pos_2dsphere")) )
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