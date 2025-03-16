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
    
    // let res = nearsay_db.insert_post(None, &[7.0, 7.0], "first").await.unwrap();
    // println!("inserted post {:?}", res);
    // let res = nearsay_db.insert_post(None, &[7.0, 7.0], "second post long body").await.unwrap();
    // println!("inserted post {:?}", res);
    // let res = nearsay_db.insert_post(None, &[70.0, 70.0], "faraway").await.unwrap();
    // println!("inserted post {:?}", res);
        
    let res = nearsay_db.geoquery_post_pts(6, &Rect { top: 90.0, bottom: 5.0, left: 5.0, right: 100.0 }).await;
    println!("{:#?}", res);
    
}


#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use rand::Rng;

    use crate::db::NearsayDB;

    fn trunc_2_decimals(x: f64) -> f64 {
        (x * 100.0).round() / 100.0
    } 

    #[tokio::test]
    async fn populate_random() {
        let mut nearsay_db = NearsayDB::new().await;
        
        let mut rng = rand::thread_rng();
        
        for _ in 0..500 {
            let x = trunc_2_decimals(rng.gen_range(-180.0..=180.0));
            let y = trunc_2_decimals(rng.gen_range(-85.0..=85.0));
            nearsay_db.insert_post(
                Some("author_id"), 
                &[x, y], 
                &format!("blurb{}", rng.gen_range(-180.0..=180.0))
            ).await.unwrap();
        }
    }

}