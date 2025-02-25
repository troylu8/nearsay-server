use std::collections::BTreeMap;
use mongodb::bson::{doc, Document};
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

#[derive(Deserialize)]
pub struct UserVotes {
    pub _id: String,
    pub votes: BTreeMap<String, String>
}

#[derive(Debug, PartialEq, Clone)]
pub enum Vote { Like, Dislike, None }

impl Vote {
    /// number of days added/subtracted from post expiry as a result of this vote
    pub fn as_lifetime_weight(&self) -> i32 {
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

#[derive(Deserialize, Debug)]
pub struct Guest {
    pub pos: [f64; 2],
    pub avatar: usize
}

pub enum UserType { User, Guest }