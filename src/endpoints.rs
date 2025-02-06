
use axum::{body::Body, extract::Path, http::{HeaderMap, StatusCode}, response::Response, routing::{get, post}, Json};
use hmac::Hmac;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use socketioxide::SocketIo;
use nearsay_server::{clone_into_closure, NearsayError};


use crate::{auth::{authenticate_with_header, create_jwt, create_user}, db::NearsayDB};

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

#[derive(Deserialize, Debug)]
struct UserInfo {
    username: String,
    userhash: String
}


pub fn get_endpoints_router(db: NearsayDB, io: SocketIo, key: Hmac<Sha256>) -> axum::Router {
    axum::Router::new()
        .route("/sign-up", post(
            clone_into_closure! {
                (db, key)
                |Json(UserInfo{username, userhash})| async move {
                    create_user(&key, &db, username, userhash).await
                }
            }
        ))
        .route("/sign-in", post(
            clone_into_closure!{
                (db, key)
                |Json(UserInfo{username, userhash})| async move {
                    match db.get_user(username).await {
                        Err(_) => Err(NearsayError::ServerError),
                        Ok(None) => Err(NearsayError::UserNotFound),
                        
                        Ok(Some(user)) => {
                            
                            match bcrypt::verify(userhash, &user.hash[..]) {
                                Ok(verified) => match verified {
                                    true => match create_jwt(&key, user._id) {
                                        Ok(auth_pair) => Ok(auth_pair),
                                        Err(_) => Err(NearsayError::ServerError),
                                    },
                                    false => Err(NearsayError::Unauthorized),
                                },
                                Err(bcrypt_err) => {
                                    eprintln!("bcrypt error when authorizing user: {}", bcrypt_err);
                                    Err(NearsayError::ServerError)
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
                        Ok(Some(uid)) => {
                            println!("voted {} as {} ", vote_type, uid);

                            match db.insert_vote(uid, post_id, vote_type.into()).await {
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
                    match db.get_post(post_id.clone()).await {
                        Err(_) => empty_response(500),
                        Ok(None) => empty_response(404),
                        Ok(Some(post)) => {
                            match authenticate_with_header(&key, &headers) {

                                // if authentication fails, respond with just the post anyway
                                Err(_) | Ok(None) => json_response(200, json! ({"post": post})),

                                Ok(Some(uid)) => {
                                    match db.get_vote(uid, post_id).await {

                                        // if getting vote fails, respond with just the post
                                        Err(_) => json_response(200, json! ({"post": post})),

                                        Ok(vote) => json_response(200, json! ({
                                            "vote": Into::<String>::into(vote),
                                            "post": post
                                        })),
                                    }
                                },
                            }
                        },
                    }

                }
            }
        ))
        // .route("/posts", post(
        //     clone_into_closure! {
        //         (db, io)
        //         |Json(req): Json<NewPostRequest>| async move {
                    
        //             match db.insert_post(&req.pos, req.body).await {
        //                 Err(_) => empty_response(500),
        //                 Ok((_id, updated)) => {
        //                     emit_at_pos_with_data(io, req.pos, "new-poi", & POI{ 
        //                         _id: _id.clone(),
        //                         pos: req.pos, 
        //                         variant: String::from("post"), 
        //                         updated: updated as u64
        //                     });

        //                     json_response(200, json!({"_id": _id, "updated": updated}))
        //                 },
        //             }

        //         }
        //     }
        // ))
}