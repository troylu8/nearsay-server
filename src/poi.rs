use axum::{http::request, response::{IntoResponse, Response}};
use mongodb::bson::{bson, doc, Bson, DateTime, Document};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Post {
    pub body: String,
    pub likes: usize,
    pub dislikes: usize,
    pub expiry: usize,
    pub views: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WrappedItem<I> {
    pub data: I
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct POI {
    pub _id: String,
    pub pos: [f64; 2],
    pub variant: String,
    pub timestamp: usize,
}