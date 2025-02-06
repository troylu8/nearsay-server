use std::collections::HashMap;

use futures::TryStreamExt;
use hmac::Hmac;
use serde::{Deserialize, Serialize};
use nearsay_server::clone_into_closure;
use serde_json::json;
use sha2::Sha256;
use socketioxide::extract::{AckSender, Data, SocketRef};

use crate::{area::{emit_at_pos, update_rooms, TileRegion}, auth::authenticate_jwt, db::NearsayDB, types::POI};

#[derive(Serialize, Deserialize, Debug)]
struct ViewShiftedData {
    curr: [Option<TileRegion>; 2],
    prev: [Option<TileRegion>; 2],
    timestamps: HashMap<String, u64>
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct ViewShiftedResponse {
    /// list of poi ids to delete
    delete: Vec<String>,
    
    /// list of pois to add/update
    fresh: Vec<POI>,
}


#[derive(Serialize, Deserialize, Debug)]
struct MoveData {
    jwt: String,
    pos: [f64; 2]
}

#[derive(Serialize, Deserialize, Debug)]
struct NewPostData {
    jwt: Option<String>,
    pos: [f64; 2],
    body: String
}

pub fn on_socket_connect(client_socket: SocketRef, db: NearsayDB, key: Hmac<Sha256>) {
    
    client_socket.on(
        "shift-view",
        clone_into_closure! {
            (db)
            |client_socket: SocketRef, Data(ViewShiftedData {curr, prev, timestamps}), ack: AckSender| async move {
                let mut resp = ViewShiftedResponse::default();
                
                for i in 0..curr.len() {
                    if let Some(curr_region) = &curr[i] {
                        update_rooms(&client_socket, curr_region);
                        add_to_move_reponse(&db, &prev[i], curr_region, &timestamps, &mut resp).await;
                    }
                }
    
                ack.send( &json!(resp) ).unwrap();
    
            }
        }
    );
    
    client_socket.on(
        "move",
        clone_into_closure! {
            (db, key)
            |client_socket: SocketRef, Data(move_data): Data<MoveData>| async move {
                let Ok(uid) = authenticate_jwt(&key, &move_data.jwt) else { return };

                if db.move_user(uid, &move_data.pos).await.is_ok() {
                    emit_at_pos(client_socket, move_data.pos, "someone-moved", &move_data);
                }
            }
        }
    );

    client_socket.on(
        "post",
        clone_into_closure! {
            (db, key)
            |client_socket: SocketRef, Data(NewPostData {jwt, pos, body})| async move {
                
                let author = match jwt {
                    None => "anonymous".to_string(),
                    Some(jwt) => match authenticate_jwt(&key, &jwt) {
                        Err(()) => return,
                        Ok(uid) => uid
                    }
                };
                
                if let Ok((post_id, ms_created)) = db.insert_post(&author, &pos, body).await {
                    emit_at_pos(client_socket, pos, "someone-posted", 
                        &POI { 
                            _id: post_id, 
                            pos, 
                            variant: "POST".to_string(), 
                            updated: ms_created as u64
                        }
                    );
                }
            }
        }
    );

}

async fn add_to_move_reponse(db: &NearsayDB, prev_region: &Option<TileRegion>, curr_region: &TileRegion, timestamps: &HashMap<String, u64>, res: &mut ViewShiftedResponse) {
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
