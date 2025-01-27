use std::collections::HashMap;

use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use socketioxide::{extract::{AckSender, Data, SocketRef}, SocketIo};

use crate::{area::{TileRegion, update_rooms}, clone_into_closure, db::NearsayDB, types::POI};

#[derive(Serialize, Deserialize, Debug)]
struct MoveRequest {
    curr: [Option<TileRegion>; 2],
    prev: [Option<TileRegion>; 2],
    timestamps: HashMap<String, u64>
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
        "move",
        |client_socket: SocketRef, Data(MoveRequest {curr, prev, timestamps}), ack: AckSender| async move {
            let mut resp = MoveResponse::default();
            
            for i in 0..curr.len() {
                if let Some(curr_region) = &curr[i] {
                    update_rooms(&client_socket, curr_region);
                    add_to_move_reponse(&db, &prev[i], curr_region, &timestamps, &mut resp).await;
                }
            }

            ack.send( &json!(resp) ).unwrap();

        },
    );

}

async fn add_to_move_reponse(db: &NearsayDB, prev_region: &Option<TileRegion>, curr_region: &TileRegion, timestamps: &HashMap<String, u64>, res: &mut MoveResponse) {
    let exclude = match prev_region {
        Some(prev_region) => {
            if prev_region.area.envelops(&curr_region.area) {
                return;
            }

            Some(&prev_region.area)
        },
        None => None
    };
    
    let mut cursor = db.search_pois(
        &curr_region.area, 
        exclude
    ).await;

    while let Some(poi) = cursor.try_next().await.unwrap() {
        let has_been_updated = match timestamps.get(&poi._id.clone()) {
            Some(prev_timestamp) => poi.updated > *prev_timestamp,
            None => true,
        };
        if has_been_updated {
            res.fresh.push(poi);
        }
    }
}

pub fn attach_socket_events(db: NearsayDB, io: SocketIo) {
    io.ns("/", clone_into_closure! { (db) move |client_socket| on_socket_connect(client_socket, db) } );
}