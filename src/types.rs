use mongodb::bson::{doc, Document};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct POI {
    pub _id: String,
    pub pos: [f64; 2],
    pub variant: String,
    pub timestamp: u64,
}

pub trait AsDbProjection {
    fn as_db_projection() -> Document;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Post {
    pub pos: [f64; 2],
    pub body: String,
    pub likes: usize,
    pub dislikes: usize,
    pub expiry: usize,
    pub views: usize,
}
impl AsDbProjection for Post {
    fn as_db_projection() -> Document {
        doc! {
            "_id": 0,
            "pos": 1,
            "body": 1,
            "likes": 1,
            "dislikes": 1,
            "expiry": 1,
            "views": 1
        }
    }
}

#[derive(Deserialize)]
pub struct User {
    pub _id: String,
    pub username: String,
    pub hash: String,
}