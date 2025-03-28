use std::{collections::HashMap, sync::Arc, time::Instant};

use futures::{TryFutureExt, TryStreamExt};
use hmac::Hmac;
use mongodb::bson::{doc, Document};
use serde::{Deserialize, Serialize};
use nearsay_server::{clone_into_closure, clone_into_closure_mut};
use serde_json::json;
use sha2::Sha256;
use socketioxide::{extract::{AckSender, Data, SocketRef}, operators::BroadcastOperators, socket::DisconnectReason};

use crate::{area::{get_tile_layer_and_size, Rect, MAX_TILE_LAYER, WORLD_MAX_BOUND}, auth::{authenticate_jwt, create_jwt, verify_password, JWTPayload}, cache::UserPOI, cluster::{Cluster, MAX_ZOOM_LEVEL, MIN_ZOOM_LEVEL}, db::{gen_id, NearsayDB}, types::{Post, User, POI}};

/// if a `uid` is given, exclude that user from returned users 
#[derive(Deserialize, Debug)]
struct ViewShiftData {
    uid: Option<String>,
    zoom: usize,
    tile_layer: usize,
    view: [Option<Rect>; 2]
}
#[derive(Serialize, Default, Debug)]
struct ViewShiftResponse {
    posts: Vec<Cluster>,
    users: Vec<UserPOI>,
}


#[derive(Deserialize, Debug)]
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
struct SignUpFromGuestData {
    guest_jwt: String,
    username: String,
    password: String,
}

#[derive(Deserialize, Debug)]
struct SignUpData {
    username: String,
    password: String,
    avatar: usize,
    pos: Option<[f64; 2]>,
}

#[derive(Deserialize, Debug)]
struct SignInData {
    username: String,
    password: String,
    pos: Option<[f64; 2]>,
    
    guest_jwt: Option<String>
}

#[derive(Deserialize, Debug)]
struct EnterWorldData {
    jwt: String,
    pos: [f64; 2]
}

#[derive(Deserialize, Debug)]
struct ExitWorldData {
    jwt: String,
    stay_online: Option<bool>,
    delete_account: Option<bool>
}

#[derive(Deserialize, Debug)]
struct EditUserData {
    jwt: String,
    avatar: Option<i32>,      // mongodb doesn't take usize
    username: Option<String>, 
    // bio??
}

#[derive(Deserialize, Debug)]
struct ChatData {
    jwt: String,
    msg: String,
    pos: [f64; 2]
}

pub fn on_socket_connect(client_socket: SocketRef, db: &NearsayDB, key: &Hmac<Sha256>) {
    async fn create_guest(db: &mut NearsayDB, key: &Hmac<Sha256>, client_socket: SocketRef, pos: [f64; 2], avatar: usize, ack: AckSender) {
        let uid = gen_id();
        
        let Ok(jwt) = create_jwt(&key, uid.clone()) else { return ack.send(&500).unwrap(); };
        
        if db.add_user_to_cache(&uid, client_socket.id.as_str(), &pos, avatar, None).await.is_ok() {
            broadcast_at(&client_socket, pos, "user-update", false, 
                &json! ({
                    "uid": uid,
                    "pos": pos,
                    "avatar": avatar,
                    "username": null
                })
            );
            ack.send(&jwt).unwrap();
        }
        else { ack.send(&500).unwrap(); };
    }

    client_socket.on(
        "sign-up-as-guest", 
        clone_into_closure_mut! {
            (db, key)
            |client_socket: SocketRef, Data(NewGuestData { pos, avatar }), ack: AckSender| async move {
                create_guest(&mut db, &key, client_socket, pos, avatar, ack).await;
            }
        }
    );
    
    client_socket.on(
        "sign-up-from-guest",
        clone_into_closure_mut! {
            (db, key)
            |client_socket: SocketRef, Data(SignUpFromGuestData{ guest_jwt, username, password }), ack: AckSender| async move {
                
                let Ok(JWTPayload{uid}) = authenticate_jwt(&key, &guest_jwt)
                else { return ack.send(&401).unwrap() };
                
                let ((x, y), avatar) = match db.get_pos_and_avatar(&uid).await {
                    Err(_) => return ack.send(&500).unwrap(),
                    Ok(None) => return ack.send(&404).unwrap(),
                    Ok(Some(vals)) => vals,
                };
                
                match db.insert_user(&uid, &username, &password, avatar).await {
                    Err(nearsay_err) => ack.send(&nearsay_err.to_status_code()).unwrap(),
                    Ok(_) => {
                        broadcast_at(&client_socket, [x, y], "user-update", false, 
                            &json!({
                                "uid": uid,
                                "username": username,
                                "avatar": avatar
                            })
                        );
                        
                        ack.send(&()).unwrap()
                    },
                }
            }
        }
    );
    
    
    async fn enter_world(db: &mut NearsayDB, client_socket: SocketRef, uid: &str, pos: [f64; 2], avatar: usize, username: Option<&str>) -> Result<(), ()> {
        
        db.add_user_to_cache(uid, client_socket.id.as_str(), &pos, avatar, username).await?;

        broadcast_at(&client_socket, pos, "user-enter", false,
            &json! ({
                "uid": uid,
                "pos": pos,
                "avatar": avatar,
                "username": username
            })
        );
        
        Ok(())   
    }

    client_socket.on(
        "sign-up",
        clone_into_closure_mut! {
            (db, key)
            |client_socket: SocketRef, Data(SignUpData{ username, password, avatar, pos }), ack: AckSender| async move {
                
                let uid = gen_id();
                let Ok(jwt) = create_jwt(&key, uid.clone()) else { return ack.send(&500).unwrap() };
                
                if let Err(err) = db.insert_user(&uid, &username, &password, avatar).await {
                    return ack.send(&err.to_status_code()).unwrap();
                }
                
                if let Some(pos) = pos {
                    let _ = enter_world(&mut db, client_socket, &uid, pos, avatar, Some(&username)).await;
                }

                ack.send(&jwt).unwrap()
            }
        }
    );
    
    // for getting the jwt from username and password
    client_socket.on(
        "sign-in",
        clone_into_closure_mut! {
            (db, key)
            |client_socket: SocketRef, Data(SignInData{username, password, pos, guest_jwt}), ack: AckSender| async move {
                
                // check if user exists
                let user = match db.get_user_from_username(&username).await {
                    Err(_) => return ack.send(&500).unwrap(),
                    Ok(None) => return ack.send(&404).unwrap(),
                    Ok(Some(user)) => user
                };
                
                // verify password
                match verify_password(&password, &user.hash[..]) {
                    Err(_) => return ack.send(&500).unwrap(),
                    Ok(false) => return ack.send(&401).unwrap(),
                    Ok(true) => {},
                }
                
                // if guest jwt was given, verify it before removing guest from cache
                if let Some(guest_jwt) = guest_jwt {
                    if let Ok(JWTPayload { uid, .. }) = authenticate_jwt(&key, &guest_jwt) {
                        if let Ok(Some((pos, _))) = db.get_pos_and_avatar(&uid).await {
                            db.delete_user_from_cache(Some(&uid), client_socket.id.as_str()).await.unwrap();
                            broadcast_at(&client_socket, pos.into(), "user-leave", false, &uid);
                        }
                    }
                }

                // create jwt with this uid
                let Ok(jwt) = create_jwt(&key, user._id.clone()) 
                else { return ack.send(&500).unwrap() };
                
                if let Some(pos) = pos {
                    enter_world(&mut db, client_socket, &user._id, pos, user.avatar, Some(&username)).await.unwrap();
                }
                
                ack.send( &json!({ "jwt": jwt, "avatar": user.avatar })).unwrap();
                
            }
        }
    );

    client_socket.on(
        "enter-world",
        clone_into_closure_mut! {
            (db, key)
            |client_socket: SocketRef, Data(EnterWorldData{ jwt, pos }), ack: AckSender| async move {
                let Ok(JWTPayload{uid}) = authenticate_jwt(&key, &jwt)
                else { return ack.send(&401).unwrap() };
                
                match db.set_user_pos(&uid, &pos).await {
                    Err(()) => ack.send(&500).unwrap(),
                    Ok(_) => {
                        broadcast_at(&client_socket, pos, "user-enter", false, 
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
        "exit-world",
        clone_into_closure_mut! {
            (db, key)
            |client_socket: SocketRef, Data(ExitWorldData{jwt, stay_online, delete_account}), ack: AckSender| async move {

                // get uid from jwt
                let Ok(JWTPayload { uid }) = authenticate_jwt(&key, &jwt)
                else { return ack.send(&500).unwrap() };

                let ((x, y), avatar) = match db.get_pos_and_avatar(&uid).await {
                    Err(_) => return ack.send(&500).unwrap(),
                    Ok(None) => return ack.send(&404).unwrap(),
                    Ok(Some(vals)) => vals,
                };
                
                let res = match delete_account {
                    Some(true) =>   db.delete_user(&uid, Some(client_socket.id.as_str())).await,
                    _ =>            db.delete_user_from_cache(Some(&uid),  client_socket.id.as_str()).await
                };
                if res.is_err() {
                    return ack.send(&500).unwrap();
                }
                
                // create a guest poi if stay_online == true
                match stay_online {
                    Some(true) => create_guest(&mut db, &key, client_socket, [x, y], avatar, ack).await,
                    _ => {
                        broadcast_at(&client_socket, [x, y], "user-leave", false, &uid );
                        ack.send(&()).unwrap()
                    }
                }
            }
        }
    );

    client_socket.on(
        "view-shift",
        clone_into_closure_mut! {
            (db)
            |client_socket: SocketRef, Data(ViewShiftData { uid, zoom, tile_layer, view}), ack: AckSender| async move {
                client_socket.leave_all().unwrap();
                
                if zoom < MIN_ZOOM_LEVEL || MAX_ZOOM_LEVEL < zoom { 
                    return ack.send(&422).unwrap() 
                }
                
                let mut resp = ViewShiftResponse::default();

                for aligned_rect in view {
                    if let Some(aligned_rect) = aligned_rect {
                        if !aligned_rect.valid_as_view() { return ack.send(&422).unwrap() }
                        
                        join_rooms(&client_socket, tile_layer, &aligned_rect);
                        
                        if let Ok(post_pts) = db.geoquery_post_pts(zoom, &aligned_rect).await {
                            resp.posts.extend(post_pts);
                        }
                        
                        if let Ok(user_pts) = db.geoquery_users(&aligned_rect).await {
                            resp.users.extend(user_pts);
                        }
                    }
                }
                
                // remove user of `uid` from result
                if let Some(uid) = uid {
                    if let Some(i) = resp.users.iter().position(|u| u.id == uid) {
                        resp.users.swap_remove(i);
                    }
                }
    
                ack.send( &json!(resp) ).unwrap();
            }
        }
    );
    
    client_socket.on(
        "move",
        clone_into_closure_mut! {
            (db, key)
            |client_socket: SocketRef, Data(MoveData {jwt, pos})| async move {
                let Ok(JWTPayload {uid, ..}) = authenticate_jwt(&key, &jwt) else { return };
                
                if let Ok(Some(old_pos)) = db.set_user_pos(&uid, &pos).await {
                    broadcast_at_multiple(&client_socket, &[old_pos.into(), pos], "user-updated", false, &json!({
                        "uid": uid,
                        "pos": &pos as &[f64]
                    }));
                }
            }
        }
    );

    client_socket.on(
        "edit-user",
        clone_into_closure_mut! {
            (db, key)
            |client_socket: SocketRef, Data( EditUserData{ jwt, avatar, username }), ack: AckSender| async move {
                let Ok(JWTPayload {uid, ..}) = authenticate_jwt(&key, &jwt) else { return };
                
                let Ok(Some(user)) = db.get::<User>("users", &uid).await 
                else { return ack.send(&404).unwrap() };
                
                let mut update = doc! {
                    "avatar": avatar,
                    "username": username,
                };
                
                if let Err(nearsay_err) = db.edit_user(&uid, &update).await {
                    return ack.send(&nearsay_err.to_status_code()).unwrap();
                }

                if let Some(pos) = user.pos {
                    update.insert("uid", uid);
                    broadcast_at(&client_socket, pos, "user-edited", false, &update);
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
                    
                    broadcast_at(&client_socket, pos, "new-post", true,
                        & json! ({
                            "id": post_id,
                            "pos": &pos as &[f64],
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
            |client_socket: SocketRef, Data(ChatData { jwt, msg, pos })| async move {
                let Ok( JWTPayload{ uid } ) = authenticate_jwt(&key, &jwt)
                else { return };

                broadcast_at(&client_socket, pos, "chat", true,
                    &json!({
                        "uid": uid,
                        "msg": msg
                    })
                );

            }
        }
    );
    
    
    client_socket.on_disconnect(clone_into_closure_mut!(
        (db)
        |client_socket: SocketRef| async move {
            db.delete_user_from_cache(None, client_socket.id.as_str()).await.unwrap();
        }
    ));
}

fn broadcast_at<T: Sized + Serialize>(io: &SocketRef, pos: [f64; 2], event: &str, include_self: bool, data: &T) {
    broadcast_at_multiple(io, &[pos], event, include_self, data);
}

fn broadcast_at_multiple<T: Sized + Serialize>(io: &SocketRef, pts: &[[f64; 2]], event: &str, include_self: bool, data: &T) {
    let mut targets = match include_self {
        true => io.to(io.id),
        false => io.except(io.id),
    };
    
    for [x, y] in pts {
        
        let mut area = Rect {
            left: -(WORLD_MAX_BOUND as f64), // use WORLD_MAX_BOUND instead of WORLD_BOUND_X/Y bc tiles are square
            right: WORLD_MAX_BOUND as f64, 
            top: WORLD_MAX_BOUND as f64, 
            bottom: -(WORLD_MAX_BOUND as f64)
        };
        
        targets = io.within(room_name(0, area.left, area.bottom));
        // println!("broadcasting {} to {}", event, room_name(0, area.left, area.bottom));
        
        for tile_layer in 1..=MAX_TILE_LAYER {
            
            let mid_x = (area.left + area.right) / 2.0;
            let mid_y = (area.top + area.bottom) / 2.0;
            
            if *x >= mid_x { area.left = mid_x; }
            else { area.right = mid_x; }
            
            if *y >= mid_y { area.bottom = mid_y; }
            else { area.top = mid_y; }
            
            targets = targets.within(room_name(tile_layer, area.left, area.bottom));
            // println!("broadcasting {} to {}", event, room_name(tile_layer, area.left, area.bottom));
        }
    }
    
    
    targets.emit(event, data).unwrap();
}

const SPLIT: &str = " : ";

pub fn join_rooms(client_socket: &SocketRef, tile_layer: usize, aligned_rect: &Rect)  {
    
    let tile_size = (WORLD_MAX_BOUND * 2.0) / 2f64.powf(tile_layer as f64);
    
    let width = ((aligned_rect.right - aligned_rect.left) / tile_size).round() as usize;
    let height = ((aligned_rect.top - aligned_rect.bottom) / tile_size).round() as usize;
    
    for x in 0..width {
        for y in 0..height {

            let room = room_name(
                tile_layer, 
                aligned_rect.left + (x as f64 * tile_size), 
                aligned_rect.bottom + (y as f64 * tile_size)
            );
            
            client_socket.join(room).unwrap();
        }
    }
}

fn room_name(zoom_level: usize, left: f64, bottom: f64) -> String {
    format!("{}{}{}{}{}", zoom_level, SPLIT, to_5_decimals(left), SPLIT, to_5_decimals(bottom))
}

fn round_up_nearest_n(num: f64, n: f64) -> f64 {
    if n == 0.0 { num }
    else { (num / n).ceil() * n }
}
fn round_down_nearest_n(num: f64, n: f64) -> f64 {
    if n == 0.0 { num }
    else { (num / n).floor() * n }
}
fn to_5_decimals(x: f64) -> f64 {
    (x * 100000.0).round() / 100000.0
}

#[cfg(test)]
mod tests {
    use crate::socket::{round_down_nearest_n, round_up_nearest_n};

    #[test]
    fn round_tests() {
        assert_eq!(2., round_up_nearest_n(2., 2.));   
        assert_eq!(2., round_down_nearest_n(2., 2.)); 
          
        assert_eq!(3., round_down_nearest_n(4., 3.));   
        assert_eq!(-7., round_down_nearest_n(-2., 7.));   
        assert_eq!(0., round_down_nearest_n(0., 0.));   
        assert_eq!(6., round_up_nearest_n(4., 3.));   
        assert_eq!(0., round_up_nearest_n(-2., 7.));   
        assert_eq!(0., round_up_nearest_n(0., 0.));   
    }
}