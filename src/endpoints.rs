
use axum::{body::Body, extract::{Path, Request}, http::{HeaderMap, Response, StatusCode}, routing::{get, post}, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use socketioxide::SocketIo;

use crate::{area::emit_at_pos_with_data, auth::authenticate, clone_into_closure, db::{NearsayDB, NewUserError}, types::{Post, POI}};

fn json_response<T: Serialize>(status: u16, serializable: T) -> Response<Body> {
    let body = Into::<Body>::into(serde_json::to_vec(&serializable).unwrap());
    
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(body)
        .unwrap()
}

#[derive(Deserialize, Debug)]
struct NewUserRequest {
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
                |Json(req): Json<NewUserRequest>| async move {
                    match db.add_new_user(req.username, req.userhash).await {
                        Ok((jwt_token, csrf_token)) => {
                            Response::builder()
                                .status(200)
                                .header("CSRF-TOKEN", csrf_token)
                                .body(jwt_token)
                                .unwrap()
                        },
                        Err(err) => {
                            let status = match err {
                                NewUserError::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
                                NewUserError::UsernameTaken => StatusCode::CONFLICT,
                            };

                            Response::builder().status(status).body(String::new()).unwrap()
                        },
                    }
                }
            }
        ))
        .route("/posts/{id}/vote", post(
            clone_into_closure! {
                (db)
                |headers: HeaderMap, vote_type: String| async move {

                    if let Ok(uid) = authenticate(&headers) {
                        println!("voted {} as {} ", vote_type, uid);

                        StatusCode::OK
                    }
                    else {
                        StatusCode::UNAUTHORIZED
                    }
                }
            }
        ))
        .route("/posts/{id}", get(
            clone_into_closure! {
                (db)
                |Path(id): Path<String>| async move { 
                    let res = db.get_poi_data::<Post>(id).await;
                    let status = match res {
                        Some(_) => 200,
                        None => 404
                    };

                    json_response(status, res)
                }
            }
        ))
        .route("/posts", post(
            clone_into_closure! {
                (db, io)
                |Json(req): Json<NewPostRequest>| async move {
                    
                    let res = db.add_post(&req.pos, req.body).await;

                    match res {
                        Ok((_id, timestamp)) => {
                            emit_at_pos_with_data(io, req.pos, "new-poi", & POI{ 
                                _id: _id.clone(), 
                                pos: req.pos, 
                                variant: String::from("post"), 
                                timestamp: timestamp as u64
                            });

                            json_response(200, json!({"_id": _id, "timestamp": timestamp}))
                        },
                        Err(err) => {
                            eprintln!("error adding post: {:?}", err);

                            json_response(500, Value::Null)
                        }
                    }

                }
            }
        ))
}