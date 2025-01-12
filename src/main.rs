
use axum::{extract::Path, routing::get};
use db::NearsayDB;
use mongodb::bson::Document;
use futures::TryStreamExt;
use poi::{Post, POI};
use serde::{Deserialize, Serialize};
use serde_json::json;
use socketioxide::{
    extract::{AckSender, Data, SocketRef},
    SocketIo,
};
use tower_http::cors::CorsLayer;
use tower::ServiceBuilder;

use area::TileRegion;

mod area;
mod poi;
mod db;

#[macro_use]
mod clone_into_closure;


#[derive(Serialize, Deserialize, Debug)]
struct MoveRequest {
    curr: [Option<TileRegion>; 2],
    prev: [Option<TileRegion>; 2],
    timestamps: Document
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct MoveResponse {
    /// list of poi ids to delete
    delete: Vec<String>,
    
    /// list of pois to add/update
    fresh: Vec<POI>,
}


fn on_socket_connect(client_socket: SocketRef, db: NearsayDB) {

    client_socket.on(
        "test",
        || {
            println!("received test");
        }
    );
    
    client_socket.on(
        "move",
        |client_socket: SocketRef, Data(MoveRequest {curr, prev, timestamps}), ack: AckSender| async move {
            // update_rooms(&client_socket, &prev_snapped, &curr_snapped);
            
            let mut res = MoveResponse::default();

            
            for i in 0..curr.len() {
                if let Some(curr_deep_rect) = &curr[i] {

                    let exclude = match &prev[i] {
                        Some(prev_deep_rect) => {
                            if prev_deep_rect.area.envelops(&curr_deep_rect.area) {
                                continue;
                            }

                            Some(&prev_deep_rect.area)
                        },
                        None => None
                    };
                    
                    let mut cursor = db.search_pois(
                        &curr_deep_rect.area, 
                        exclude
                    ).await;

                    while let Some(poi) = cursor.try_next().await.unwrap() {
                        let has_been_updated = match timestamps.get(poi._id.clone()) {
                            Some(prev_timestamp) => poi.timestamp as i32 > prev_timestamp.as_i32().expect("timestamp values should always be i32"),
                            None => true,
                        };
                        if has_been_updated {
                            res.fresh.push(poi);
                        }
                    }

                }
            }

            ack.send( &json!(res) ).unwrap();

        },
    );

}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let db = NearsayDB::new().await;
    
    let (socketio_layer, io) = SocketIo::new_layer();
    
    io.ns("/", clone_into_closure! { (db) move |client_socket| on_socket_connect(client_socket, db) } );

    let app = axum::Router::new()
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) 
                .layer(socketio_layer)
        )
        .route("/posts/:id", get(
            clone_into_closure! {
                (db)
                |Path(id): Path<String>| async move { db.get_poi_data::<Post>(id).await }
            }
        ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:5000").await?;

    axum::serve(listener, app).await?;

    Ok(())
}

 
// for y in -90..=90 {
//     for x in -180..=180 {
//         db.db.collection("poi").insert_one(doc! {
//             "_id": format!("{x},{y}"),
//             "pos": [x, y],
//             "variant": "post",
//             "data": {
//                 "body": format!("lorem {x},{y}"),
//                 "likes": (y as i32).abs(),
//                 "dislikes": (x as i32).abs(),
//                 "expiry": (y+x as i32).abs(),
//                 "views": 10
//             },
//             "timestamp": 111
//         }).await?;
//     }
// }