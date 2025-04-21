use std::env;

use hmac::{Hmac, Mac};
use db::NearsayDB;
use endpoints::get_endpoints_router;
use socket::on_socket_connect;
use socketioxide::SocketIo;
use tower_http::cors::CorsLayer;
use nearsay_server::clone_into_closure;
use axum_server::tls_rustls::RustlsConfig;
use std::net::SocketAddr;

mod area;
mod types;
mod cache;
mod cluster;
mod db;
mod endpoints;
mod socket;
mod auth;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv()?;
    let key = Hmac::new_from_slice(env::var("JWT_SECRET").unwrap().as_bytes()).unwrap();

    let nearsay_db = NearsayDB::new().await;

    let (socketio_layer, io) = SocketIo::new_layer();
    io.ns("/", clone_into_closure! { 
        (nearsay_db, key) 
        move |client_socket| on_socket_connect(client_socket, &nearsay_db, &key) 
    });

    let app = axum::Router::new()
        .merge(get_endpoints_router(&nearsay_db, &key))
        .layer(socketio_layer)
        .layer(CorsLayer::permissive());
    
    let addr = SocketAddr::from(([0, 0, 0, 0], 5000));
    let config = RustlsConfig::from_pem_file(
        "/etc/letsencrypt/live/nearsay.troylu.com/fullchain.pem",
        "/etc/letsencrypt/live/nearsay.troylu.com/privkey.pem"
    ).await?;
    
    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await?;
    
    Ok(())
}
// #[tokio::main]
// async fn main() -> Result<(), ()> {

//     let mut nearsay_db = NearsayDB::new().await;
    
//     _ = nearsay_db.insert_post(None, &[20.0, 20.0], "post body").await?;
//     _ = nearsay_db.insert_post(None, &[20.0, 20.0], "post body").await?;
    
//     let (post_id, _) = nearsay_db.insert_post(None, &[20.0, 20.0], "post body").await.unwrap();
    
//     _ = nearsay_db.delete_post(&post_id).await;
    
//     Ok(())
// }


#[cfg(test)]
mod tests {
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