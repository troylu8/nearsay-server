use std::collections::HashMap;

use futures::{io::Cursor, TryStreamExt};
use hmac::Hmac;
use mongodb::bson::{doc, Document};
use serde::{Deserialize, Serialize};
use nearsay_server::clone_into_closure;
use serde_json::{json, Value};
use sha2::Sha256;
use socketioxide::extract::{AckSender, Data, SocketRef};

use crate::{area::{broadcast_at, update_rooms, BroadcastTargets, Rect, TileRegion}, auth::{authenticate_jwt, JWTPayload}, db::NearsayDB, types::{Post, User, POI}};

#[derive(Serialize, Deserialize, Debug)]
struct ViewShiftedData {
    curr: [Option<TileRegion>; 2],
    prev: [Option<TileRegion>; 2],
    timestamps: HashMap<String, i64>
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct ViewShiftedResponse {
    /// list of poi ids to delete
    delete: Vec<String>,
    
    /// list of pois to add/update
    fresh: Vec<Document>,
}


#[derive(Serialize, Deserialize, Debug)]
struct MoveData {
    jwt: Option<String>,
    uid: Option<String>,
    pos: [f64; 2],
}

#[derive(Serialize, Deserialize, Debug)]
struct NewPostData {
    jwt: Option<String>,
    pos: [f64; 2],
    body: String
}

struct StartSessionData {
    jwt: Option<String>,
    uid: Option<String>,
    
}

pub fn on_socket_connect(client_socket: SocketRef, db: &NearsayDB, key: &Hmac<Sha256>) {

    client_socket.on(
        "view-shift",
        clone_into_closure! {
            (db)
            |client_socket: SocketRef, Data(ViewShiftedData {curr, prev, timestamps}), ack: AckSender| async move {
                let mut resp = ViewShiftedResponse::default();
                
                for i in 0..curr.len() {
                    if let Some(curr_region) = &curr[i] {
                        update_rooms(&client_socket, curr_region);
                        add_pois_to_move_resp::<User>(&db, "users", &prev[i], curr_region, &timestamps, &mut resp).await;
                        add_pois_to_move_resp::<Post>(&db, "posts", &prev[i], curr_region, &timestamps, &mut resp).await;
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

                let moved_user_id;

                if let Some(jwt) = move_data.jwt {
                    let Ok(JWTPayload {uid, ..}) = authenticate_jwt(&key, &jwt) else { return };
                    moved_user_id = uid;
                }
                else if let Some(uid) = move_data.uid {
                    moved_user_id = uid;
                } 
                else { return }
                
                if db.move_user(&moved_user_id, &move_data.pos).await.is_ok() {
                    broadcast_at(client_socket, move_data.pos, "someone-moved", BroadcastTargets::ExcludingSelf, 
                        &doc! {
                            "uid": moved_user_id,
                            "pos": &move_data.pos as &[f64]
                        }
                    );
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
                        Ok(JWTPayload {uid, ..}) => uid
                    }
                };
                
                if let Ok((post_id, ms_created)) = db.insert_post(&author, &pos, &body).await {
                    
                    const BLURB_LENGTH: usize = 10;

                    let blurb = 
                        if body.len() <= BLURB_LENGTH { body } 
                        else { format!("{}...", body[..BLURB_LENGTH].to_string()) };

                    broadcast_at(client_socket, pos, "new-poi", BroadcastTargets::IncludingSelf,
                        & doc! {
                            "_id": post_id.clone(),
                            "pos": &pos as &[f64],
                            "kind": "post",
                            "updated": ms_created,
            
                            "blurb": blurb,
                        }
                    );
                }
            }
        }
    );

}

async fn add_pois_to_move_resp<T: Send + Sync + POI>(db: &NearsayDB, collection: &str, prev_region: &Option<TileRegion>, curr_region: &TileRegion, timestamps: &HashMap<String, i64>, resp: &mut ViewShiftedResponse) -> Option<Box<dyn std::error::Error>> {
    let exclude = match prev_region {
        Some(prev_region) => {
            if prev_region.area.envelops(&curr_region.area) { return None }
            Some(&prev_region.area)
        },
        None => None
    };
    
    let mut cursor = db.get_pois::<T>(collection, &curr_region.area, exclude).await;
    
    while let Some(poi) = cursor.try_next().await.unwrap() {
        
        let has_been_updated = match timestamps.get(poi.get("_id")?.as_str()?) {
            Some(prev_timestamp) => poi.get("updated")?.as_i64()? > *prev_timestamp,
            None => true,
        };
        if has_been_updated {
            resp.fresh.push(poi);
        }
    }

    None
}

