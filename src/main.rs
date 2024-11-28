
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use socketioxide::{
    extract::{AckSender, Data, SocketRef, TryData},
    SocketIo,
};
use tiles::{update_rooms, Rect};

mod tiles;

#[derive(Serialize, Deserialize)]
struct MoveBundle {
    prev_snapped: Option<Rect<f64>>,
    curr_snapped: Rect<f64>,
    // timestamps
}

fn on_connect(client_socket: SocketRef) {

    client_socket.on(
        "move",
        |client_socket: SocketRef, Data(MoveBundle {prev_snapped, curr_snapped}), ack: AckSender| {
            update_rooms(&client_socket, &prev_snapped, &curr_snapped);

            // read from db

            // return data in ack
        },
    );
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let (socketio_layer, io) = SocketIo::new_layer();

    io.ns("/", on_connect);

    let app = axum::Router::new().layer(socketio_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:5000").await?;

    axum::serve(listener, app).await?;

    Ok(())
}