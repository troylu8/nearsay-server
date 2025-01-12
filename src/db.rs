use std::fmt::Debug;
use axum::{body::Body, response::{IntoResponse, Response}};
use mongodb::{ 
    bson::doc, options::Hint, Client, Cursor, Database
};
use serde::{de::DeserializeOwned, Serialize};

use crate::{area::Rect, poi::{WrappedItem, POI}};

pub struct GetDataResult<T>(Option<T>);
impl<T: Serialize> IntoResponse for GetDataResult<T> {
    fn into_response(self) -> Response {
        let status = match self.0 {
            Some(_) => 200,
            None => 404,
        };

        let body = Into::<Body>::into(serde_json::to_vec(&self.0).unwrap());

        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(body)
            .unwrap()
    }
}

#[derive(Clone)]
pub struct NearsayDB {
    db: Database
}
impl NearsayDB {
    pub async fn new() -> Self {
        Self {
            db: Client::with_uri_str("mongodb://localhost:27017").await.unwrap().database("nearsay")
        }
    }

    pub async fn get_poi_data<T>(&self, id: String) -> GetDataResult<T>
    where 
        T: Send + Sync + DeserializeOwned + Debug
    {
        let result = self.db.collection::<WrappedItem<T>>("poi")
            .find_one(doc!{"_id": id})
            .projection(doc! {"_id": 0, "data": 1})
            .await.expect("error getting data of id");

        match result {
            Some(document) => GetDataResult(Some(document.data)),
            None => GetDataResult(None),
        }
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
            .hint( Hint::Name(String::from("pos")) )
            .await.unwrap()
    }
}