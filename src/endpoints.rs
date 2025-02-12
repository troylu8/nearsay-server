
use axum::{body::Body, extract::Path, http::{HeaderMap, StatusCode}, response::{IntoResponse, Response}, routing::{get, post}, Json};
use hmac::Hmac;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use nearsay_server::{clone_into_closure, NearsayError};


use crate::{auth::{authenticate_with_header, create_jwt, JWTPayload}, db::{gen_id, NearsayDB}, types::{Post, User}};



fn json_response<T: Serialize>(status: u16, json: T) -> Response<Body> {
    let body = Into::<Body>::into(serde_json::to_vec(&json).unwrap());

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

#[derive(Deserialize, Debug)]
struct SignUpData {
    username: String,
    userhash: String,
    avatar: usize
}

#[derive(Deserialize, Debug)]
struct SignInData {
    username: String,
    userhash: String,
}


pub fn get_endpoints_router(db: &NearsayDB, key: &Hmac<Sha256>) -> axum::Router {
    axum::Router::new()
    
        // for creating an account
        .route("/sign-up", post(
            clone_into_closure! {
                (db, key)
                |Json(SignUpData{username, userhash, avatar})| async move {
                    let uid = gen_id();

                    let Ok(jwt) = create_jwt(&key, uid.clone(), username.clone()) else { return empty_response(500) };

                    match db.insert_user(&uid, &username, &userhash, avatar).await {
                        Err(err) => err.into_response(),
                        Ok(()) => jwt.into_response(),
                    }
                }
            }
        ))

        // for getting the jwt from username and password
        .route("/sign-in", post(
            clone_into_closure!{
                (db, key)
                |Json(SignInData{username, userhash})| async move {
                    match db.get_user(&username).await {
                        Err(_) => Err(NearsayError::ServerError),
                        Ok(None) => Err(NearsayError::UserNotFound),
                        
                        Ok(Some(user)) => {
                            
                            match bcrypt::verify(userhash, &user.hash[..]) {
                                Err(bcrypt_err) => {
                                    eprintln!("bcrypt error when authorizing user: {}", bcrypt_err);
                                    Err(NearsayError::ServerError)
                                },
                                Ok(verified) => match verified {
                                    true => match create_jwt(&key, user._id, username) {
                                        Ok(jwt) => Ok(jwt),
                                        Err(_) => Err(NearsayError::ServerError),
                                    },
                                    false => Err(NearsayError::Unauthorized),
                                },
                            }
                        }
                    }
                }
            }
        ))

        .route("/vote/{post_id}", post(
            clone_into_closure! {
                (db, key)
                |headers: HeaderMap, Path(post_id): Path<String>, vote_type: String| async move {

                    match authenticate_with_header(&key, &headers) {
                        Err(_) | Ok(None) => StatusCode::UNAUTHORIZED,
                        Ok(Some(JWTPayload {uid, ..} )) => {
                            match db.insert_vote(&uid, &post_id, vote_type.into()).await {
                                Ok(_) => StatusCode::OK,
                                Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
                            }
                        },
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

                    match db.get::<Post>(&post_id).await {
                        Err(_) => empty_response(500),
                        Ok(None) => empty_response(404),
                        Ok(Some(post)) => {

                            // add author name to response
                            let author_name = match db.get::<User>(&post.author).await {
                                Ok(Some(user)) => user.username,
                                _ => "anonymous".to_string()
                            };
                            let mut post = json!(post);
                            post.as_object_mut().unwrap().insert("author_name".to_string(), Value::String(author_name));

                            let mut response_body = json! ({"post": post});

                            // if authentication fails, respond with just the post anyway
                            let Ok(Some(JWTPayload {uid, ..})) = authenticate_with_header(&key, &headers) else { return json_response(200, response_body) };
                            
                            // if getting vote fails, respond with just the post
                            let Ok(vote) = db.get_vote(&uid, &post_id).await else { return json_response(200, response_body) };
                            
                            response_body.as_object_mut().unwrap().insert("vote".to_string(), Value::String(vote.into()));
                            
                            json_response(200, response_body)
                        },
                    }

                }
            }
        ))
        
}