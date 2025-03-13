use std::env;

use area::Rect;
use hmac::{Hmac, Mac};
use db::NearsayDB;
use endpoints::get_endpoints_router;
use socket::on_socket_connect;
use socketioxide::SocketIo;
use tower_http::cors::CorsLayer;
use nearsay_server::clone_into_closure;

mod area;
mod types;
mod cache;
mod cluster;
mod db;
mod clear_old_posts;
mod endpoints;
mod socket;
mod auth;

// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {
//     dotenvy::dotenv()?;

//     let nearsay_db = NearsayDB::new().await;

//     let (socketio_layer, io) = SocketIo::new_layer();

//     let key = Hmac::new_from_slice(env::var("JWT_SECRET").unwrap().as_bytes()).unwrap();

//     io.ns("/", clone_into_closure! { 
//         (nearsay_db, key) 
//         move |client_socket| on_socket_connect(client_socket, &nearsay_db, &key) 
//     });


//     let app = axum::Router::new()
//         .merge(get_endpoints_router(&nearsay_db, &key))
//         .layer(socketio_layer)
//         .layer(CorsLayer::permissive());


//     let listener = tokio::net::TcpListener::bind("127.0.0.1:5000").await?;

//     axum::serve(listener, app).await?;

//     Ok(())
// }
#[tokio::main]
async fn main() {

    let mut nearsay_db = NearsayDB::new().await;

    // let (post_id, ..) = nearsay_db.insert_post(None, &[7.0, 7.0], "first post").await.unwrap();
    // println!("inserted post {}", post_id);
    // let (post_id, ..) = nearsay_db.insert_post(None, &[7.0, 7.0], "second post long body").await.unwrap();
    // println!("inserted post {}", post_id);
    let (post_id, ..) = nearsay_db.insert_post(None, &[70.0, 70.0], "faraway post").await.unwrap();
    println!("inserted post {}", post_id);


    let posts = 
        nearsay_db.geoquery_post_pts(3, &Rect {top: 10.0, bottom: 0.0, left: 0.0, right: 10.0 }).await;


    println!("{:?}", posts);

}