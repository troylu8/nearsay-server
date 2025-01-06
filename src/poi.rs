use mongodb::bson::{bson, Bson, DateTime, Document};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Post {
    pub body: String,
    pub likes: usize,
    pub dislikes: usize,
    pub expiry: usize,
    pub views: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct POI {
    pub _id: String,
    pub pos: [f64; 2],
    pub variant: String,
    pub data: Document,
    pub timestamp: usize,
}