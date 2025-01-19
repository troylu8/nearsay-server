use std::{fmt::Debug, time::SystemTime};
use mongodb::{ 
    bson::doc, options::Hint, Client, Cursor, Database
};
use serde::de::DeserializeOwned;

use crate::{area::Rect, delete_old::today, types::{AsDbProjection, POI}};

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

    pub async fn add_post(&self, pos: &[f64], body: String) -> Result<(String, i64), mongodb::error::Error> {
        
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

fn gen_id() -> String {

    let str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";

    let mut res: [char; 10] = [' '; 10];

    for i in 0..10 {
        res[i] = str.chars().nth(rand::thread_rng().gen_range(0..64)).unwrap();
    }

    res.iter().collect()
}