
use db::NearsayDB;
use endpoints::get_endpoints_router;
use socket::attach_socket_events;
use socketioxide::SocketIo;
use tower_http::cors::CorsLayer;
use tower::ServiceBuilder;

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
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) 
                .layer(socketio_layer)
        )
        .nest("/", get_endpoints_router(db.clone(), io.clone()));
        

    let listener = tokio::net::TcpListener::bind("127.0.0.1:5000").await?;

    axum::serve(listener, app).await?;

    Ok(())
}