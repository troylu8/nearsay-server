use std::env;
use db::NearsayDB;
use endpoints::get_endpoints_router;
use socket::on_socket_connect;
use socketioxide::SocketIo;
use tower_http::cors::CorsLayer;
use nearsay_server::clone_into_closure;

mod area;
mod types;
mod db;
mod delete_old;
mod endpoints;
mod socket;
mod auth;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv()?;

    let db = NearsayDB::new().await;
    let (socketio_layer, io) = SocketIo::new_layer();

    // delete_old::begin(db.db.clone());

    let key = auth::get_auth_key();

    io.ns("/", clone_into_closure! { 
        (db, key) 
        move |client_socket| on_socket_connect(client_socket, &db, &key) 
    });


    let app = axum::Router::new()
        .merge(get_endpoints_router(&db, &key))
        .layer(socketio_layer)
        .layer(CorsLayer::permissive());


    let listener = tokio::net::TcpListener::bind("127.0.0.1:5000").await?;

    axum::serve(listener, app).await?;

    Ok(())
}