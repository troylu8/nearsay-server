
use db::NearsayDB;
use endpoints::get_endpoints_router;
use socket::attach_socket_events;
use socketioxide::SocketIo;
use tower_http::cors::CorsLayer;

mod area;
mod types;
mod db;
mod delete_old;
mod endpoints;
mod clone_into_closure;
mod socket;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    
    let db = NearsayDB::new().await;
    let (socketio_layer, io) = SocketIo::new_layer();

    // delete_old::begin(db.db.clone());
    
    attach_socket_events(db.clone(), io.clone());

    let app = axum::Router::new()
        .merge(get_endpoints_router(db.clone(), io.clone()))
        .layer(socketio_layer)
        .layer(CorsLayer::permissive());


    let listener = tokio::net::TcpListener::bind("127.0.0.1:5000").await?;

    axum::serve(listener, app).await?;

    Ok(())
}