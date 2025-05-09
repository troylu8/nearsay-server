use mongodb::bson::{doc, Document};
use serde::{Deserialize, Serialize};

pub trait POI {
    fn get_poi_projection() -> Document;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[allow(non_snake_case)]
pub struct Post {
    pub _id: String,
    pub pos: [f64; 2],

    pub authorId: Option<String>,
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
                "blurb": { "$substrCP": [ "$body", 0, BLURB_LENGTH as i32 ]},
            }
        }
    }
}

pub const BLURB_LENGTH: usize = 25;
pub fn get_blurb_from_body(post_body: &str) -> String {
    if post_body.len() <= BLURB_LENGTH { post_body.to_string() } 
    else { post_body[..BLURB_LENGTH].to_string() }
}


#[derive(Serialize, Deserialize, Debug)]
pub struct User {
    pub _id: String,
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

                "username": 1,
                "avatar": 1,
            }
        }
    }
}


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