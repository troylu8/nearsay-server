use std::collections::BTreeMap;
use mongodb::bson::{doc, Document};
use serde::{Deserialize, Serialize};

// #[derive(Serialize, Deserialize, Clone, Debug)]
// pub struct POI {
//     pub _id: String,
//     pub pos: [f64; 2],
//     pub variant: String,
//     pub updated: u64,
// }

pub trait POI {
    fn get_poi_projection() -> Document;
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Post {
    pub _id: String,
    pub pos: [f64; 2],
    pub updated: u64,

    pub author: String,
    pub body: String,
    pub likes: usize,
    pub dislikes: usize,
    pub views: usize,
    pub expiry: usize,
}
impl POI for Post {
    fn get_poi_projection() -> Document {
        doc! {
            "$project": {
                "pos": 1,
                "kind": "post",
                "updated": 1,

                "blurb": { "$substrCP": [ "$body", 0, 10 ]},
            }
        }
    }
}


#[derive(Serialize, Deserialize, Debug)]
pub struct User {
    pub _id: String,
    pub pos: Option<[f64; 2]>,
    pub updated: u64,

    pub username: String,
    pub avatar: usize,
    pub hash: String,
}
impl POI for User {
    fn get_poi_projection() -> Document {
        doc! {
            "$project": {
                "pos": 1,
                "kind": "user",
                "updated": 1,

                "username": 1,
                "avatar": 1,
            }
        }
    }
}

#[derive(Deserialize)]
pub struct UserVotes {
    pub _id: String,
    pub votes: BTreeMap<String, String>
}