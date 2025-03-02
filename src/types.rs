use std::collections::BTreeMap;
use mongodb::bson::{doc, oid::ObjectId, Document};
use serde::{Deserialize, Serialize};

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

#[derive(Deserialize, Debug)]
pub struct Guest {
    pub pos: [f64; 2],
    pub avatar: usize
}

pub enum UserType { User, Guest }



#[derive(Debug, PartialEq, Clone)]
pub struct Vote {
    pub post_id: String,
    pub uid: String,
    pub kind: VoteKind
}

impl From<Document> for Vote {
    fn from(document: Document) -> Self {
        Self {
            post_id: document.get_str("post_id").unwrap().to_string(),   // rename _id -> post_id
            uid: document.get_str("uid").unwrap().to_string(),
            kind: VoteKind::from_str(document.get_str("kind").unwrap()),             // convert to `VoteKind`
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum VoteKind { Like, Dislike, None }

impl VoteKind {
    /// number of days added/subtracted from post expiry as a result of this vote
    pub fn get_lifetime_weight(&self) -> i32 {
        match self {
            VoteKind::None => 0,
            VoteKind::Like => 2,
            VoteKind::Dislike => -1
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            VoteKind::Like => "like".to_string(),
            VoteKind::Dislike => "dislike".to_string(),
            VoteKind::None => "none".to_string()
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "like" => VoteKind::Like,
            "dislike" => VoteKind::Dislike,
            _ => VoteKind::None
        }
    }
}