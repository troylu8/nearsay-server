
use std::{fmt::Debug, time::SystemTime};
use bcrypt::{hash, DEFAULT_COST};
use mongodb::{ 
    bson::doc, options::Hint, Client, Cursor, Database, error::Error as MongoError
};
use serde::de::DeserializeOwned;

use crate::{area::Rect, auth::NearsayError, delete_old::today, types::{AsDbProjection, User, POI}};



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

    pub async fn get_user(&self, username: String) -> Result<Option<User>, MongoError> {
        let get_user_req = self.db.collection::<User>("users").find_one(doc! {"username": username}).await;

        if let Err(mongo_err) = &get_user_req {
            eprintln!("mongodb error when getting user: {}", &mongo_err);
        }

        get_user_req
    }

    pub async fn insert_user(&self, user: User) -> Result<(), NearsayError> {

        // check if username is taken
        let count_result = self.db.collection::<User>("users").count_documents(doc! {
            "username": user.username.clone(),
        }).limit(1).await;

        match count_result {
            Ok(count) => if count != 0 { return Err(NearsayError::UsernameTaken); },
            Err(mongo_err) => {
                eprintln!("mongodb error when checking if username taken: {}", &mongo_err);
                return Err(NearsayError::ServerError);
            },
        }

        // hash password (again) to store in db
        let serverhash = match hash(user.hash, DEFAULT_COST) {
            Ok(res) => res,
            Err(bcrypt_err) => {
                eprintln!("bcrypt error when hashing userhash: {}", &bcrypt_err);
                return Err(NearsayError::ServerError);
            },
        };
        
        // insert user data into db
        let insert_result = self.db.collection("users").insert_one(doc! {
            "_id": user._id,
            "username": user.username,
            "hash": serverhash,
        }).await;
        if let Err(mongo_err) = insert_result {
            eprintln!("mongodb error when adding new user: {}", &mongo_err);
            return Err(NearsayError::ServerError);
        }

        Ok(())
    }

    pub async fn add_post(&self, pos: &[f64], body: String) -> Result<(String, i64), MongoError> {
        
        let _id = gen_id();
        let millis: i64 = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis().try_into().expect("current time millis doesnt fit into i64");
        
        let res = self.db.collection("poi").insert_one(doc! {
            "_id": _id.clone(),
            "variant": "post".to_string(),
            "timestamp": millis,
            "pos": pos,
            "body": body,
            "likes": 0,
            "dislikes": 0,
            "expiry": (today() + 7) as i64,
            "views": 0
        }).await;

        match res {
            Ok(_) => Ok((_id, millis)),
            Err(err) => Err(err)
        }
    }

    pub async fn get_poi_data<T>(&self, id: String) -> Option<T>
    where 
        T: Send + Sync + DeserializeOwned + Debug + AsDbProjection
    {
        self.db.collection::<T>("poi")
            .find_one(doc!{"_id": id})
            .projection(T::as_db_projection())
            .await.expect("error getting data of id")
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
    
        self.db.collection::<POI>("poi")
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