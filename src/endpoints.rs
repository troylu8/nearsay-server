
use axum::{body::Body, extract::Path, http::Response, routing::{get, post}, Json};
use serde::{Deserialize, Serialize};
use socketioxide::SocketIo;

use crate::{area::TileRegion, clone_into_closure, db::NearsayDB, types::Post};

fn option_to_response<T: Serialize>(option: Option<T>) -> Response<Body> {
    let status = match option {
        Some(_) => 200,
        None => 404,
    };

    let body = Into::<Body>::into(serde_json::to_vec(&option).unwrap());

    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(body)
        .unwrap()
}

fn build_empty_response(status: u16) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .unwrap()
}

#[derive(Deserialize, Debug)]
struct PostRequest {
    pos: [i32; 2],
    body: String
}

pub fn get_endpoints_router(db: NearsayDB, io: SocketIo) -> axum::Router {
    axum::Router::new()
        .route("/posts/:id", get(
            clone_into_closure! {
                (db)
                |Path(id): Path<String>| async move { 
                    option_to_response(db.get_poi_data::<Post>(id).await)
                }
            }
        ))
        .route("/posts", post(
            clone_into_closure! {
                (db, io)
                |Json(req): Json<PostRequest>| async move { 
                    let res = db.add_post(&req.pos, req.body).await;

                    match res {
                        Ok(_) => {
                            // send socket event

                            build_empty_response(200)
                        },
                        Err(err) => {
                            println!("error adding post: {:?}", err);
                            build_empty_response(500)
                        }
                    }
                }
            }
        ))
}