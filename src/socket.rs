use std::collections::HashMap;

use futures::TryStreamExt;
use hmac::Hmac;
use mongodb::bson::{doc, Document};
use serde::{Deserialize, Serialize};
use nearsay_server::clone_into_closure;
use serde_json::json;
use sha2::Sha256;
use socketioxide::extract::{AckSender, Data, SocketRef};

use crate::{area::{Rect, TileRegion}, auth::{authenticate_jwt, create_jwt, verify_password, JWTPayload}, cluster::Cluster, db::{gen_id, NearsayDB}, types::{Post, User, POI}};

#[derive(Deserialize, Debug)]
struct ViewShiftData {
    view: [Option<TileRegion>; 2],
}

#[derive(Serialize, Default, Debug)]
struct ViewShiftResponse {
    posts: Vec<Cluster>,
    users: Vec<Cluster>,
}


#[derive(Serialize, Deserialize, Debug)]
struct MoveData {
    jwt: String,
    pos: [f64; 2],
}

#[derive(Serialize, Deserialize, Debug)]
struct NewPostData {
    jwt: Option<String>,
    pos: [f64; 2],
    body: String
}

#[derive(Deserialize, Debug)]
struct NewGuestData {
    pos: [f64; 2],
    avatar: usize,
}

#[derive(Deserialize, Debug)]
struct SignInData {
    username: String,
    userhash: String,
    pos: [f64; 2]
}

#[derive(Deserialize, Debug)]
struct SignUpData {
    guest_jwt: Option<String>,
    username: String,
    userhash: String,
    avatar: usize
}

#[derive(Deserialize, Debug)]
struct StartSessionData {
    jwt: String,
    pos: [f64; 2]
}

#[derive(Deserialize, Debug)]
struct SignOutData {
    jwt: String,
    stay_online: Option<bool>,
    delete_account: Option<bool>
}

#[derive(Deserialize, Debug)]
struct EditUserData {
    jwt: String,
    update: Document
}

#[derive(Deserialize, Debug)]
struct ChatData {
    jwt: String,
    msg: String,
    pos: [f64; 2]
}

pub fn on_socket_connect(client_socket: SocketRef, db: &NearsayDB, key: &Hmac<Sha256>) {

    async fn create_guest(db: &NearsayDB, key: &Hmac<Sha256>, client_socket: SocketRef, Data(NewGuestData {pos, avatar}): Data<NewGuestData>, ack: AckSender) {
        let uid = gen_id();
        let Ok(jwt) = create_jwt(&key, uid.clone()) else { return ack.send(&500).unwrap(); };
        
        if db.insert_guest(&uid, avatar, &pos).await.is_ok() {
            broadcast_at(&client_socket, pos, "user-joined", BroadcastTargets::ExcludingSelf, 
                &json! ({
                    "uid": uid,
                    "pos": pos,
                    "avatar": avatar
                })
            );
            ack.send(&jwt).unwrap();
        }
        else { ack.send(&500).unwrap(); };
    }

    client_socket.on("sign-in-guest", clone_into_closure! {
        (db, key)
        |client_socket: SocketRef, new_guest_data: Data<NewGuestData>, ack: AckSender| async move {
            create_guest(&db, &key, client_socket, new_guest_data, ack).await;
        }
    });

    // for getting the jwt from username and password
    client_socket.on(
        "sign-in",
        clone_into_closure! {
            (db, key)
            |client_socket: SocketRef, Data(SignInData{username, userhash, pos}), ack: AckSender| async move {
                
                // check if user exists
                let user = match db.get_user_from_username(&username).await {
                    Err(_) => return ack.send(&500).unwrap(),
                    Ok(None) => return ack.send(&404).unwrap(),
                    Ok(Some(user)) => user
                };
                
                // verify password
                match verify_password(&userhash, &user.hash[..]) {
                    Err(_) => return ack.send(&500).unwrap(),
                    Ok(false) => return ack.send(&401).unwrap(),
                    Ok(true) => {},
                }

                // create jwt with this uid
                let Ok(jwt) = create_jwt(&key, user._id.clone()) 
                else { return ack.send(&500).unwrap() };

                if let Err(()) = db.set_user_pos(&user._id.clone(), &pos).await {
                    return ack.send(&500).unwrap()
                }

                broadcast_at(&client_socket, pos, "user-joined", BroadcastTargets::ExcludingSelf,
                    &json! ({
                        "uid": user._id,
                        "pos": pos,
                        "avatar": user.avatar
                    })
                );
                ack.send(
                    &json!({
                        "jwt": jwt,
                        "avatar": user.avatar
                    })
                ).unwrap()

            }
        
        }
    );

    // for creating an account
    client_socket.on(
        "sign-up",
        clone_into_closure! {
            (db, key)
            |client_socket: SocketRef, Data(SignUpData{guest_jwt, username, userhash, avatar}), ack: AckSender| async move {
                
                // extract the uid from the guest jwt, or make a new one
                let (uid, jwt) = match guest_jwt {
                    Some(guest_jwt) => {
                        let Ok(JWTPayload{uid}) = authenticate_jwt(&key, &guest_jwt)
                        else { return ack.send(&401).unwrap() };
                        (uid, guest_jwt)
                    }
                    None => {
                        let uid = gen_id();
                        let Ok(jwt) = create_jwt(&key, uid.clone())
                        else { return ack.send(&500).unwrap() };
                        (uid, jwt)
                    }
                };
                
                match db.insert_user(&uid, &username, &userhash, avatar).await {
                    Err(err) => return ack.send(&err.to_status_code()).unwrap(),

                    // a guest was replaced
                    Ok(Some(pos)) => {
                        // tell everyone someone signed in
                        broadcast_at(&client_socket, pos, "user-joined", BroadcastTargets::ExcludingSelf, 
                            &json!({
                                "uid": uid,
                                "username": username,
                                "avatar": avatar
                            })
                        );
                    }
                    
                    _ => {}
                }

                ack.send(&jwt).unwrap()
            }
        }
    );

    client_socket.on(
        "start-session",
        clone_into_closure! {
            (db, key)
            |client_socket: SocketRef, Data(StartSessionData{ jwt, pos }), ack: AckSender| async move {
                let Ok(JWTPayload{uid}) = authenticate_jwt(&key, &jwt)
                else { return ack.send(&401).unwrap() };

                match db.set_user_pos(&uid, &pos).await {
                    Err(()) => ack.send(&500).unwrap(),
                    Ok(()) => {
                        broadcast_at(&client_socket, pos, "user-joined", BroadcastTargets::ExcludingSelf, 
                            &json!({
                                "uid": uid,
                                "pos": &pos as &[f64]
                            })
                        );
                        
                        ack.send(&()).unwrap()
                    },
                }
            }
        }
    );

    client_socket.on(
        "sign-out",
        clone_into_closure! {
            (db, key)
            |client_socket: SocketRef, Data(SignOutData{jwt, stay_online, delete_account}), ack: AckSender| async move {

                // get uid from jwt
                let Ok(JWTPayload { uid }) = authenticate_jwt(&key, &jwt)
                else { return ack.send(&500).unwrap() };

                // get position and avatar of this user
                let (pos, avatar) = match db.get::<Document>("users", &uid).await {
                    Err(_) => return ack.send(&500).unwrap(),
                    Ok(None) => return ack.send(&404).unwrap(),
                    Ok(Some(user)) => {
                        let pos = user.get("pos").expect("user should have a 'pos' field").as_array().unwrap();
                        let avatar = user.get("avatar").expect("user should have a 'avatar' field").as_i32().unwrap();
                        (
                            [pos[0].as_f64().unwrap(), pos[1].as_f64().unwrap()],
                            avatar as usize
                        )
                    },
                };

                // deleting account
                if Some(true) == delete_account {
                    if db.delete_user(&uid).await.is_err() {
                        return ack.send(&500).unwrap();
                    }
                }

                // signing out
                else {
                    if let Err(nearsay_err) = db.sign_out(&uid).await {
                        return ack.send(&nearsay_err.to_status_code()).unwrap()
                    }
                }

                broadcast_at(&client_socket, pos, "user-left", BroadcastTargets::ExcludingSelf,
                    &json!( { "uid": uid } )
                );

                if let Some(true) = stay_online {
                    create_guest(&db, &key, client_socket, Data(NewGuestData {pos, avatar}), ack).await;
                }
                else {
                    ack.send(&()).unwrap();
                }

            }
        }
    );

    client_socket.on(
        "view-shift",
        clone_into_closure! {
            (db)
            |client_socket: SocketRef, Data(ViewShiftData {view}), ack: AckSender| async move {
                let mut resp = ViewShiftResponse::default();
                
                for region in view {
                    if let Some(region) = region {
                        update_rooms(&client_socket, &region);
                        append_in_region::<User>(&db, "users", &region, &mut resp.users).await;
                        append_in_region::<Post>(&db, "posts", &region, &mut resp.posts).await;
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
            |client_socket: SocketRef, Data(MoveData {jwt, pos})| async move {
                let Ok(JWTPayload {uid, ..}) = authenticate_jwt(&key, &jwt) else { return };

                if db.set_user_pos(&uid, &pos).await.is_ok() {
                    broadcast_at(&client_socket, pos, "user-updated", BroadcastTargets::ExcludingSelf, 
                        &json!({
                            "uid": uid,
                            "pos": &pos as &[f64]
                        })
                    );
                }
            }
        }
    );

    client_socket.on(
        "edit-user",
        clone_into_closure! {
            (db, key)
            |client_socket: SocketRef, Data( EditUserData{ jwt, mut update }), ack: AckSender| async move {
                let Ok(JWTPayload {uid, ..}) = authenticate_jwt(&key, &jwt) else { return };

                update.remove("_id");
                update.remove("pos");

                if let Err(nearsay_err) = db.update_user(&uid, &mut update).await {
                    return ack.send(&nearsay_err.to_status_code()).unwrap();
                }

                let Ok(Some(user)) = db.get::<User>("users", &uid).await 
                else { return ack.send(&404).unwrap() };

                if let Some(pos) = user.pos {
                    update.insert("uid", uid);
                    broadcast_at(&client_socket, pos, "user-edited", BroadcastTargets::ExcludingSelf, &update);
                }

                ack.send(&()).unwrap()
                
            }
        }
    );



    client_socket.on(
        "post",
        clone_into_closure! {
            (db, key)
            |client_socket: SocketRef, Data(NewPostData {jwt, pos, body})| async move {
                let author_id_owned;
                let author_id = match jwt {
                    None => None,
                    Some(jwt) => match authenticate_jwt(&key, &jwt) {
                        Err(()) => return,
                        Ok(JWTPayload {uid, ..}) => {
                            author_id_owned = uid;
                            Some(&author_id_owned[..])
                        }
                    }
                };

                if let Ok((post_id, blurb)) = db.clone().insert_post(author_id, &pos, &body).await {
                    
                    broadcast_at(&client_socket, pos, "new-poi", BroadcastTargets::IncludingSelf,
                        & json! ({
                            "_id": post_id.clone(),
                            "pos": &pos as &[f64],
                            "kind": "post",
            
                            "blurb": blurb,
                        })
                    );
                }
            }
        }
    );

    client_socket.on(
        "chat",
        clone_into_closure! {
            (key)
            |client_socket: SocketRef, Data(ChatData { jwt, msg, pos }), ack: AckSender| async move {
                let Ok( JWTPayload{ uid } ) = authenticate_jwt(&key, &jwt)
                else { return };

                println!("{uid}", );

                broadcast_at(&client_socket, pos, "chat", BroadcastTargets::IncludingSelf,
                    &json!({
                        "uid": uid,
                        "msg": msg
                    })
                );

            }
        }
    )

}

async fn append_in_region<T: Send + Sync + POI>(db: &NearsayDB, collection: &str, curr_region: &TileRegion, resp: &mut Vec<Cluster>) -> Option<Box<dyn std::error::Error>> {
    
    // let mut cursor = db.get_pois::<T>(collection, &curr_region.area, exclude).await;
    
    // while let Some(poi) = cursor.try_next().await.unwrap() {
        
    //     let has_been_updated = match timestamps.get(poi.get("_id")?.as_str()?) {
    //         Some(prev_timestamp) => poi.get("updated")?.as_i64()? > *prev_timestamp,
    //         None => true,
    //     };
    //     if has_been_updated {
    //         resp.fresh.push(poi);
    //     }
    // }

    // None
    todo!()
}


#[derive(Debug)]
pub enum BroadcastTargets { IncludingSelf, ExcludingSelf }

pub fn broadcast_at<T: Sized + Serialize>(io: &SocketRef, pos: [f64; 2], event: &str, targets: BroadcastTargets, data: &T) {
    let [x, y] = pos;

    let mut area = Rect {
        left: -(TileRegion::BOUND as f64), 
        right: TileRegion::BOUND as f64, 
        top: TileRegion::BOUND as f64, 
        bottom: -(TileRegion::BOUND as f64)
    };

    let broadcast = |room: String,| {
        match targets {
            BroadcastTargets::IncludingSelf => io.within(room.clone()),
            BroadcastTargets::ExcludingSelf => io.to(room.clone()),
        }.emit(event, data).unwrap();
    };
    
    broadcast(get_room(0, area.left, area.bottom));
    
    for depth in 1..=23 {
        
        let mid_x = (area.left + area.right) / 2.0;
        let mid_y = (area.top + area.bottom) / 2.0;
        
        if x >= mid_x { area.left = mid_x; }
        else { area.right = mid_x; }
        
        if y >= mid_y { area.bottom = mid_y; }
        else { area.top = mid_y; }

        broadcast(get_room(depth, area.left, area.bottom));
    }
}


const SPLIT: &str = " : ";

pub fn update_rooms(client_socket: &SocketRef, tilereg: &TileRegion)  {

    client_socket.leave_all().unwrap();

    let tile_size = tilereg.get_tile_size();
    let width = ((tilereg.area.right - tilereg.area.left) / tile_size).ceil() as usize;
    let height = ((tilereg.area.top - tilereg.area.bottom) / tile_size).ceil() as usize;    
    
    for x in 0..width {
        for y in 0..height {

            let room = get_room(
                tilereg.depth, 
                tilereg.area.left + (x as f64 * tile_size), 
                tilereg.area.bottom + (y as f64 * tile_size)
            );

            // join this room 
            client_socket.join(room).unwrap();
        }
    }
}

fn get_room(depth: usize, left: f64, bottom: f64) -> String {
    format!("{}{}{}{}{}", depth, SPLIT, to_5_decimals(left), SPLIT, to_5_decimals(bottom))
}

fn to_5_decimals(x: f64) -> f64 {
    (x * 100000.0).round() / 100000.0
}