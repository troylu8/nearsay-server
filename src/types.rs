use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct POI {
    pub _id: String,
    pub pos: [f64; 2],
    pub variant: String,
    pub updated: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Post {
    pub pos: [f64; 2],
    pub body: String,
    pub likes: usize,
    pub dislikes: usize,
    pub views: usize,
    pub expiry: usize,
}


#[derive(Deserialize)]
pub struct User {
    pub _id: String,
    pub username: String,
    pub hash: String,
}

#[derive(Deserialize)]
pub struct UserVotes {
    pub _id: String,
    pub votes: BTreeMap<String, String>
}