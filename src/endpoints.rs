
use axum::{body::Body, extract::Path, http::{HeaderMap, StatusCode}, routing::{get, post}, Json, response::Response};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;
use socketioxide::SocketIo;

use crate::{area::emit_at_pos_with_data, auth::{authenticate, get_auth_info, create_user}, clone_into_closure, db::NearsayDB, types::POI};

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

#[derive(Deserialize, Debug)]
struct NewPostRequest {
    pos: [f64; 2],
    body: String
}


pub fn get_endpoints_router(db: NearsayDB, io: SocketIo) -> axum::Router {
    axum::Router::new()
        .route("/sign-up", post(
            clone_into_closure! {
                (db)
                |Json(req): Json<UserInfo>| async move {
                    create_user(&db, req.username, req.userhash).await
                }
            }
        ))
        .route("/sign-in", post(
            clone_into_closure!{
                (db)
                |Json(req): Json<UserInfo>| async move {
                    get_auth_info(&db, req.username, req.userhash).await
                }
            }
        ))
        .route("/vote/{post_id}", post(
            clone_into_closure! {
                (db)
                |headers: HeaderMap, cookies: CookieJar, Path(post_id): Path<String>, vote_type: String| async move {

                    match authenticate(&headers, &cookies) {
                        Err(_) => StatusCode::UNAUTHORIZED,
                        Ok(uid) => {
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
                (db)
                |headers: HeaderMap, cookies: CookieJar, Path(post_id): Path<String>| async move { 
                    match db.get_post(post_id.clone()).await {
                        Err(_) => empty_response(500),
                        Ok(None) => empty_response(404),
                        Ok(Some(post)) => {
                            match authenticate(&headers, &cookies) {

                                // if authentication fails, respond with just the post anyway
                                Err(_) => json_response(200, json! ({"post": post})),

                                Ok(uid) => {
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
        .route("/posts", post(
            clone_into_closure! {
                (db, io)
                |Json(req): Json<NewPostRequest>| async move {
                    
                    match db.insert_post(&req.pos, req.body).await {
                        Err(_) => empty_response(500),
                        Ok((_id, updated)) => {
                            emit_at_pos_with_data(io, req.pos, "new-poi", & POI{ 
                                _id: _id.clone(), 
                                pos: req.pos, 
                                variant: String::from("post"), 
                                updated: updated as u64
                            });

                            json_response(200, json!({"_id": _id, "updated": updated}))
                        },
                    }

                }
            }
        ))
}