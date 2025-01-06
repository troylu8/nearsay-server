use std::sync::Arc;

use mongodb::{ 
    bson::{bson, doc, Bson, Document},
    Client,
    Collection, Database
};
use futures::TryStreamExt;
use poi::POI;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use socketioxide::{
    extract::{AckSender, Data, SocketRef},
    SocketIo,
};
use tower_http::cors::CorsLayer;
use tower::ServiceBuilder;

use tiles::{update_rooms, Rect};

mod tiles;
mod poi;

#[derive(Serialize, Deserialize, Debug)]
struct TileRegion {
    depth: usize,
    area: Rect<f64>
}

#[derive(Serialize, Deserialize)]
struct MoveRequest {
    curr: TileRegion,
    prev: Option<TileRegion>,
    timestamps: Document
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct MoveResponse {
    /// list of poi ids to delete
    delete: Vec<String>,
    
    /// list of pois to add/update
    fresh: Vec<POI>,
}


fn on_connect(client_socket: SocketRef, db: &Arc<Database>) {

    client_socket.on(
        "test",
        || {
            println!("received test");
        }
    );
    
    let db_clone_move = Arc::clone(db);
    client_socket.on(
        "move",
        |client_socket: SocketRef, Data(MoveRequest {curr, prev, timestamps}), ack: AckSender| async move {
            // update_rooms(&client_socket, &prev_snapped, &curr_snapped);
            
            let query = match prev {
                Some(prev) => {
                    if prev.area.envelops(&curr.area) {
                        ack.send( &json!(null) ).unwrap();
                        return;
                    }

                    doc! {
                        "$and": [
                            {"pos": { "$geoWithin": curr.area.as_geo_json() }},
                            {"pos": { "$not": { "$geoWithin": prev.area.as_geo_json() } }},
                        ] 
                    }
                },
                None => doc! {
                    "pos": { "$geoWithin": curr.area.as_geo_json() }
                }
            };
            
            let mut res = MoveResponse::default();
            
            let mut cursor = db_clone_move.collection::<POI>("poi")
                                .find(query).await.unwrap();
        
            while let Some(poi) = cursor.try_next().await.unwrap() {
                let has_been_updated = match timestamps.get(poi._id.clone()) {
                    Some(prev_timestamp) => poi.timestamp as i32 > prev_timestamp.as_i32().expect("timestamp values should always be i32"),
                    None => true,
                };
                if has_been_updated {
                    res.fresh.push(poi);
                }
            }

            ack.send( &json!(res) ).unwrap();

        },
    );

}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let db = Arc::new(
        Client::with_uri_str("mongodb://localhost:27017").await?.database("nearsay")
    );
    

    // for y in -90..=90 {
    //     for x in -180..=180 {
    //         db.collection("poi").insert_one(doc! {
    //             "_id": format!("{x},{y}"),
    //             "pos": [x, y],
    //             "variant": "post",
    //             "data": {
    //                 "body": format!("lorem {x},{y}"),
    //                 "likes": y,
    //                 "dislikes": x,
    //                 "expiry": y+x,
    //                 "views": 10
    //             },
    //             "timestamp": 111
    //         }).await?;
    //     }
    // }


    let (socketio_layer, io) = SocketIo::new_layer();

    io.ns("/", move |client_socket| on_connect(client_socket, &db));

    let app = axum::Router::new()
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) 
                .layer(socketio_layer),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:5000").await?;

    axum::serve(listener, app).await?;

    Ok(())
}