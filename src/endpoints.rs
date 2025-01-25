
use axum::{body::Body, extract::Path, http::{HeaderMap, StatusCode}, routing::{get, post}, Json, response::Response};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use socketioxide::SocketIo;

use crate::{area::emit_at_pos_with_data, auth::{authenticate, get_auth_info, create_user, NearsayError}, clone_into_closure, db::NearsayDB, types::{Post, POI}};

fn json_response<T: Serialize>(status: u16, serializable: T) -> Response<Body> {
    let body = Into::<Body>::into(serde_json::to_vec(&serializable).unwrap());
    
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(body)
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
        .route("/vote/{id}", post(
            clone_into_closure! {
                (db)
                |headers: HeaderMap, cookies: CookieJar, vote_type: String| async move {

                    if let Ok(uid) = authenticate(&headers, &cookies) {
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