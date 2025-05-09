
use axum::{body::Body, extract::Path, http::{HeaderMap, StatusCode}, response::Response, routing::{get, post}};
use hmac::Hmac;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::Sha256;
use nearsay_server::{clone_into_closure, clone_into_closure_mut};


use crate::{auth::{authenticate_with_header, JWTPayload}, db::NearsayDB, types::{Post, User, VoteKind}};



fn json_response<T: Serialize>(status: u16, serializable: T) -> Response<Body> {
    let body = Into::<Body>::into(serde_json::to_vec(&serializable).unwrap());

    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(body)
        .unwrap()
}

fn empty_response(status: u16) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .unwrap()
}


pub fn get_endpoints_router(db: &NearsayDB, key: &Hmac<Sha256>) -> axum::Router {
    axum::Router::new()

        .route("/vote/{post_id}", post(
            clone_into_closure! {
                (db, key)
                |headers: HeaderMap, Path(post_id): Path<String>, vote_kind: String| async move {
                    
                    let Ok(Some(JWTPayload {uid, ..})) = authenticate_with_header(&key, &headers)
                    else { return StatusCode::UNAUTHORIZED };
                    
                    match db.insert_vote(&uid, &post_id, VoteKind::from_str(&vote_kind)).await {
                        Ok(_) => StatusCode::OK,
                        Err(_) => StatusCode::INTERNAL_SERVER_ERROR
                    }
                }
            }
        ))
        .route("/posts/{post_id}", get(
            clone_into_closure! {
                (db, key)
                |headers: HeaderMap, Path(post_id): Path<String>| async move { 

                    if headers.contains_key("Increment-View") {
                        if let Ok(res) = db.increment_view(&post_id).await {
                            if res.modified_count == 0 { return empty_response(404) }
                        }
                    }
                    match db.get::<Post>("posts", &post_id).await {
                        Err(_) => empty_response(500),
                        Ok(None) => empty_response(404),
                        Ok(Some(post)) => {
                            
                            let author_info = match &post.authorId {
                                None => None,
                                Some(author_id) => 
                                    match db.get::<User>("users", &author_id).await {
                                        Ok(Some(user)) => Some((user.avatar, user.username)),
                                        _ => None
                                    }
                            };

                            let mut post = json!(post);
                            
                            if let Some((avatar, username)) = author_info {
                                post.as_object_mut().unwrap().insert("authorAvatar".to_string(), Value::Number(avatar.into()));
                                post.as_object_mut().unwrap().insert("authorName".to_string(), Value::String(username));
                            }
                            post.as_object_mut().unwrap().remove("authorId");

                            let mut response_body = json! ({"post": post});

                            // if authentication fails, respond with just the post anyway
                            let Ok(Some(JWTPayload {uid, ..})) = authenticate_with_header(&key, &headers) else { return json_response(200, response_body) };
                            
                            // if getting vote fails, respond with just the post
                            let Ok(vote) = db.get_vote(&uid, &post_id).await else { return json_response(200, response_body) };
                            
                            response_body.as_object_mut().unwrap().insert("vote".to_string(), Value::String(VoteKind::as_str(&vote)));
                            
                            json_response(200, response_body)
                        },
                    }

                }
            }
        ))
        .route("/users/{query_type}/{query}", get(
            clone_into_closure_mut! {
                (db)
                |Path((query_type, query)): Path<(String, String)>| async move {
                    
                    if query_type == "online" {
                        let Ok(Some((_, avatar))) = db.get_cache_pos_and_avatar(&query).await 
                        else { return empty_response(404) };
                        
                        let Ok(username) = db.get_cache_username(&query).await
                        else { return empty_response(500) };
                        
                        return json_response(200, &json!({
                            "id": query,
                            "avatar": avatar,
                            "username": username
                        }));
                    }
                    
                    let res = 
                        if query_type == "id" {
                            db.get::<User>("users", &query).await
                        }
                        else {
                            db.get_user_from_username(&query).await
                        };
                        
                    match res { 
                        Err(e) => {
                            eprintln!("in user endpoint: {e:?}");
                            empty_response(500)
                        },
                        Ok(None) => empty_response(404),
                        Ok(Some(user)) => {
                            json_response(200, &json!({
                                "id": user._id,
                                "username": user.username,
                                "avatar": user.avatar
                            }))
                        },
                    }
                }
            }
        ))
}