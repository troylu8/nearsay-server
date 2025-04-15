use std::collections::HashSet;
use std::time::Duration;
use std::{error::Error, usize};
use geoutils::Location;
use redis::aio::MultiplexedConnection;
use redis::{from_redis_value, AsyncCommands, Cmd, Pipeline, RedisResult, Value};
use redis::geo::{Coord, Unit};
use rslock::LockManager;
use serde::Serialize;
use thousands::Separable;

use crate::area::Rect;
use crate::cluster::{get_cluster_radius_meters, merge_clusters, Cluster};

const MIN_CACHED_ZOOM_LEVEL: usize = 3;
const MAX_CACHED_ZOOM_LEVEL: usize = 5;
const CACHED_ZOOM_LEVELS: usize = MAX_CACHED_ZOOM_LEVEL - MIN_CACHED_ZOOM_LEVEL + 1;

/// `radius` in meters
fn geoquery_radius<'a>(pipeline: &'a mut Pipeline, zoom: usize, x: f64, y: f64, radius: f64, with_coord: bool) -> &'a mut Pipeline {
    let query = pipeline.cmd("GEOSEARCH")
                .arg(format!("Z{zoom}"))
                .arg("FROMLONLAT")
                .arg(x)
                .arg(y)
                .arg("BYRADIUS")
                .arg(radius)
                .arg(Unit::Meters);
            
    match with_coord {
        true => query.arg("WITHCOORD"),
        false => query,
    }
}

fn get_cluster_pos<'a>(pipeline: &'a mut Pipeline, zoom: usize, cluster_ids: &[&str]) -> &'a mut Pipeline {
    pipeline.geo_pos(format!("Z{zoom}"), cluster_ids)
}

fn get_cluster_size<'a>(pipeline: &'a mut Pipeline, zoom: usize, cluster_id: &str) -> &'a mut Pipeline {
    pipeline.get(format!("size:Z{zoom}:{cluster_id}"))
}
fn set_cluster_size<'a>(pipeline: &'a mut Pipeline, zoom: usize, cluster_id: &str, size: usize) -> &'a mut Pipeline {
    pipeline.set(format!("size:Z{zoom}:{cluster_id}"), size).ignore()
}
fn del_cluster_size<'a>(pipeline: &'a mut Pipeline, zoom: usize, cluster_id: &str) -> &'a mut Pipeline {
    pipeline.del(format!("size:Z{zoom}:{cluster_id}")).ignore()
}

fn add_cluster<'a>(pipeline: &'a mut Pipeline, zoom: usize, cluster_id: &str, x: f64, y: f64) -> &'a mut Pipeline {
    pipeline.geo_add(format!("Z{zoom}"), (Coord::lon_lat(x, y), cluster_id)).ignore()
}
/// note: doesn't delete shared `blurb` value!
fn del_cluster<'a>(mut pipeline: &'a mut Pipeline, zoom: usize, cluster_id: &str) -> &'a mut Pipeline {
    pipeline = pipeline.zrem(format!("Z{zoom}"), cluster_id).ignore();
    del_cluster_size(pipeline, zoom, cluster_id)
}

fn get_blurb<'a>(pipeline: &'a mut Pipeline, cluster_id: &str) -> &'a mut Pipeline {
    pipeline.get(format!("blurb:{cluster_id}"))
}
fn set_blurb<'a>(pipeline: &'a mut Pipeline, cluster_id: &str, blurb: &str) -> &'a mut Pipeline {
    pipeline.set(format!("blurb:{cluster_id}"), blurb).ignore()
}
fn del_blurb<'a>(pipeline: &'a mut Pipeline, post_id: &str) -> &'a mut Pipeline {
    pipeline.del(format!("blurb:{post_id}")).ignore()
}

fn get_avatar<'a>(pipeline: &'a mut Pipeline, uid: &str) -> &'a mut Pipeline {
    pipeline.get(format!("avatar:{uid}"))
}
fn set_avatar<'a>(pipeline: &'a mut Pipeline, uid: &str, avatar: usize) -> &'a mut Pipeline {
    pipeline.set(format!("avatar:{uid}"), avatar).ignore()
}
fn del_avatar<'a>(pipeline: &'a mut Pipeline, uid: &str) -> &'a mut Pipeline {
    pipeline.del(format!("avatar:{uid}")).ignore()
}

fn get_username<'a>(pipeline: &'a mut Pipeline, uid: &str) -> &'a mut Pipeline {
    pipeline.get(format!("username:{uid}"))
}
fn set_username<'a>(pipeline: &'a mut Pipeline, uid: &str, username: &str) -> &'a mut Pipeline {
    pipeline.set(format!("username:{uid}"), username).ignore()
}
fn del_username<'a>(pipeline: &'a mut Pipeline, uid: &str) -> &'a mut Pipeline {
    pipeline.del(format!("username:{uid}")).ignore()
}

fn set_socket<'a>(pipeline: &'a mut Pipeline, socket_id: &str, uid: &str) -> &'a mut Pipeline {
    pipeline.set(format!("socket:{socket_id}"), uid).ignore()
}
fn del_socket<'a>(pipeline: &'a mut Pipeline, socket_id: &str) -> &'a mut Pipeline {
    pipeline.del(format!("socket:{socket_id}")).ignore()
}

fn meters_between(lng1: f64, lat1: f64, lng2: f64, lat2: f64) -> f64 {
    let loc1 = Location::new(lat1, lng1);
    let loc2 = Location::new(lat2, lng2);
    loc1.distance_to(&loc2).unwrap_or_else(|_| loc1.haversine_distance_to(&loc2) ).meters()
}


fn geosearch_cmd(key: &str, within: &Rect) -> Cmd {
    let mid_x = (within.left + within.right) / 2.0;
    let mid_y = (within.top + within.bottom) / 2.0;
    
    let width_meters =
        if within.bottom >= 0.0 { // north hemisphere
            meters_between(within.left, within.bottom, within.right, within.bottom)
        }
        else if within.top <= 0.0 { // south hemisphere 
            meters_between(within.left, within.top, within.right, within.top)
        }
        else { // around equator
            meters_between(within.left, 0.0, within.right, 0.0)
        };

    // height = middle top -> middle bottom, in meters
    let height_meters = meters_between(mid_x, within.top, mid_x, within.bottom);
    
    redis::cmd("GEOSEARCH")
        .arg(key)
        .arg("FROMLONLAT")
        .arg(mid_x)
        .arg(mid_y)
        .arg("BYBOX")
        .arg(width_meters)
        .arg(height_meters)
        .arg(Unit::Meters)
        .arg("WITHCOORD")
        .to_owned()
}


#[derive(Debug, Clone)]
pub struct MapCache {
    posts_cache: MultiplexedConnection,
    users_cache: MultiplexedConnection,
}
impl MapCache {

    pub async fn new() -> Result<Self, Box<dyn Error>> {
        Ok(
            Self {
                posts_cache:  redis::Client::open("redis://localhost:6000")?.get_multiplexed_async_connection().await?,
                users_cache:  redis::Client::open("redis://localhost:6001")?.get_multiplexed_async_connection().await?
            }
        )
    }
    
    pub async fn add_post_pt(&mut self, cluster_id: &str, x: f64, y: f64, blurb: &str) -> Result<(), Box<dyn Error>> {
        let lock_manager = LockManager::new(vec!["redis://127.0.0.1:6000"]);
        
        let lock = loop {
            if let Ok(lock) = lock_manager
                .lock("add post pt".as_bytes(), Duration::from_millis(1000))
                .await
            {
                break lock;
            }
        };
        
        // get ids and positions of nearby clusters
        let mut pipe_geoquery = &mut redis::pipe();
        for zoom in MIN_CACHED_ZOOM_LEVEL..=MAX_CACHED_ZOOM_LEVEL {
            pipe_geoquery = geoquery_radius(pipe_geoquery, zoom, x, y, get_cluster_radius_meters(zoom), true);
        }
        
        // nearby_clusters[x] = (id, pos) of each nearby cluster on zoom x
        let nearby_clusters: [ Vec<(String, (f64, f64))> ; CACHED_ZOOM_LEVELS] = pipe_geoquery.query_async(&mut self.posts_cache).await.unwrap();
        let mut nearby_clusters_ids = HashSet::new();
        
        let mut pipe_nearby = &mut redis::pipe();
        
        let mut zooms_new_cluster_was_merged_on = [false; CACHED_ZOOM_LEVELS];
        
        // for each zoom level...
        for (i, nearby_clusters_in_zoom) in nearby_clusters.iter().enumerate() {
            let zoom = i + MIN_CACHED_ZOOM_LEVEL;
            
            // get sizes of nearby clusters + delete them
            for (nearby_id, _) in nearby_clusters_in_zoom {
                pipe_nearby = get_cluster_size(pipe_nearby, zoom, &nearby_id);
                pipe_nearby = del_cluster(pipe_nearby, zoom, &nearby_id);
                nearby_clusters_ids.insert(nearby_id);
                zooms_new_cluster_was_merged_on[i] = true;
            }
        }

        let nearby_cluster_sizes: Vec<usize> = pipe_nearby.query_async(&mut self.posts_cache).await.unwrap();
        let mut sizes_i = 0;
        
        let mut pipe_save = &mut redis::pipe(); // saves new cluster + its size, gets cluster sizes of nearby clusters after merging
        
        // for each zoom level...
        for (zoom, nearby_clusters_in_zoom) in nearby_clusters.iter().enumerate() {
            let zoom = zoom + MIN_CACHED_ZOOM_LEVEL;
            
            // create a new cluster
            let mut new_x = x;
            let mut new_y = y;
            let mut new_size = 1;
            
            // merge all nearby clusters into the new cluster
            for (_, (nearby_x, nearby_y)) in nearby_clusters_in_zoom {
                (new_x, new_y, new_size) = merge_clusters(new_x, new_y, new_size, *nearby_x, *nearby_y, nearby_cluster_sizes[sizes_i]);
                sizes_i += 1;
            }
            
            // save new cluster and its size to redis
            pipe_save = add_cluster(pipe_save, zoom, cluster_id, new_x, new_y);
            pipe_save = set_cluster_size(pipe_save, zoom, cluster_id, new_size);
        }
        
        // get the size of each deleted cluster on each zoom
        for id in &nearby_clusters_ids {
            for zoom in MIN_CACHED_ZOOM_LEVEL..=MAX_CACHED_ZOOM_LEVEL {
                pipe_save = get_cluster_size(pipe_save, zoom, id);
            }
        }
        
        sizes_i = 0;
        
        let nearby_cluster_sizes: Vec<Option<usize>> = pipe_save.query_async(&mut self.posts_cache).await.unwrap();
        
        let mut pipe_blurbs = &mut redis::pipe();
        
        for deleted_cluster_id in &nearby_clusters_ids {
            
            // a blurb is required if the cluster is a single on any zoom
            let mut blurb_required = false; 
            for _ in 0..CACHED_ZOOM_LEVELS {
                if let Some(1) = nearby_cluster_sizes[sizes_i] {
                    blurb_required = true;
                }
                sizes_i += 1;
            }
            if !blurb_required {
                pipe_blurbs = del_blurb(pipe_blurbs, deleted_cluster_id);
            }
        }
        
        // save blurb if new cluster didn't do a merge on any zoom
        for merged_on_zoom in zooms_new_cluster_was_merged_on {
            if !merged_on_zoom {
                pipe_blurbs = set_blurb(pipe_blurbs, cluster_id, blurb);
                break;
            }
        }
        
        pipe_blurbs.exec_async(&mut self.posts_cache).await.unwrap();
        
        // Unlock the lock
        lock_manager.unlock(&lock).await;

        Ok(())
    }
    
    pub async fn del_post(&mut self, post_id: &str, [x, y]: [f64; 2]) -> RedisResult<()> {
        
        let lock_manager = LockManager::new(vec!["redis://127.0.0.1:6000"]);
        let lock = loop {
            if let Ok(lock) = lock_manager
                .lock("delete post".as_bytes(), Duration::from_millis(1000))
                .await
            {
                break lock;
            }
        };
        
        let mut pipe_sizes = &mut redis::pipe();
        
        // get sizes of each cluster with this id
        for zoom in MIN_CACHED_ZOOM_LEVEL..=MAX_CACHED_ZOOM_LEVEL {
            pipe_sizes = get_cluster_size(pipe_sizes, zoom, post_id);
        }
        
        let cluster_sizes: [Option<usize>; CACHED_ZOOM_LEVELS] = pipe_sizes.query_async(&mut self.posts_cache).await.unwrap();
        
        let mut pipe_del = &mut redis::pipe();
        
        // delete clusters with a size of 1
        for (i, size) in cluster_sizes.iter().enumerate() {
            if Some(1) == *size {
                pipe_del = del_cluster(pipe_del, i + MIN_CACHED_ZOOM_LEVEL, post_id);
            }
        }
        
        // regardless of whether clusters were deleted on all zoom levels, delete the blurb
        pipe_del = del_blurb(pipe_del, post_id);
        
        pipe_del.exec_async(&mut self.posts_cache).await.unwrap();
        
        lock_manager.unlock(&lock).await;
        
        Ok(())
    }
    
    /// returns `(cluster_id, cluster)`
    pub async fn geoquery_post_pts(&mut self, zoom: usize, within: &Rect) -> Result<Vec<Cluster>, ()> {
        if zoom < MIN_CACHED_ZOOM_LEVEL || MAX_CACHED_ZOOM_LEVEL < zoom { return Err(()) }

        let search_results: Vec<(String, (f64, f64))> = 
            geosearch_cmd(&format!("Z{zoom}"), within)
            .query_async(&mut self.posts_cache).await.map_err(|_| ())?;

        let mut p = &mut redis::pipe();
        
        // for each cluster found, get its size and blurb
        for (cluster_id, _) in &search_results {
            p = get_cluster_size(p, zoom, cluster_id);
            p = get_blurb(p, cluster_id);
        }
        
        // [size, blurb, size, blurb, size, blurb, ...]
        let sizes_and_blurbs: Vec<redis::Value> = p.query_async(&mut self.posts_cache).await.unwrap();
        
        let mut res = Vec::with_capacity(search_results.len());
        
        // combine `search_results` and `sizes_and_blurbs` into a `Cluster` array
        for (i, (cluster_id, pos)) in search_results.iter().enumerate() {
            
            // only attach size if its not 1
            let size: Option<usize> = match from_redis_value(&sizes_and_blurbs[i * 2]).unwrap() {
                Some(1) => None,
                other => other
            };
            
            // only attach blurb if not attaching size
            let blurb: Option<String> = match size {
                None => from_redis_value(&sizes_and_blurbs[i * 2 + 1]).unwrap(),
                _ => None
            };
            
            res.push(Cluster { pos: *pos, size, id: cluster_id.to_string(), blurb });
        }
        
        Ok(res)
    }
    
    pub async fn flush_all_posts(&mut self) -> RedisResult<()> {
        redis::cmd("FLUSHALL").exec_async(&mut self.posts_cache).await
    }
    
    pub async fn user_exists(&mut self, uid: &str) -> RedisResult<bool> {
        self.users_cache.exists(format!("avatar:{uid}")).await
    }
    
    pub async fn get_username(&mut self, uid: &str) -> RedisResult<Option<String>> {
        self.users_cache.get(format!("username:{uid}")).await
    }
    
    pub async fn get_pos_and_avatar(&mut self, uid: &str) -> RedisResult<Option<((f64, f64), usize)>> {
        let mut p = &mut redis::pipe();
        
        p = p.geo_pos("users", uid);
        p = get_avatar(p, uid);
        
        let ((pos,), avatar): ( (redis::Value,) , redis::Value ) = p.query_async(&mut self.users_cache).await?;
        
        if avatar == redis::Value::Nil { Ok(None) }
        else { Ok( Some( ( from_redis_value(&pos)?, from_redis_value(&avatar)? ) ) ) }
    }
    
    /// returns old position of user
    pub async fn set_user_pos(&mut self, uid: &str, x: f64, y: f64) -> Result<(f64, f64), ()> {
            
        let old_pos = match self.get_pos_and_avatar(uid).await {
            Err(_) | Ok(None) => return Err(()),    // user must already exist in cache
            Ok(Some((old_pos, _))) => old_pos,
        };
        
        let _: () = self.users_cache.geo_add("users", (Coord::lon_lat(x, y), uid)).await.map_err(|e| eprintln!("when setting pos: {e}"))?;
        
        Ok(old_pos)
    }
    
    pub async fn edit_user_if_exists(&mut self, uid: &str, avatar: &Option<usize>, username: &Option<String>) -> RedisResult<()> {
        if avatar.is_none() && username.is_none()   { return Ok(()) }
        if !self.user_exists(uid).await?            { return Ok(()) }
        
        let mut p = &mut redis::pipe();
        
        if let Some(avatar) = avatar {
            p = set_avatar(p, uid, *avatar);
        }
        if let Some(username) = username {
            p = set_username(p, uid, &username);
        }
        
        let _:() = p.query_async(&mut self.users_cache).await?;
        
        Ok(())
    }
    
    pub async fn add_user(&mut self, uid: &str, socket_id: &str, x: f64, y: f64, avatar: usize, username: Option<&str>) -> RedisResult<()>  {
        let mut p = &mut redis::pipe();
        
        p.geo_add("users", (Coord::lon_lat(x, y), uid)); // add user to geomap
        p = set_avatar(p, uid, avatar);
        if let Some(username) = username {
            p = set_username(p, uid, username);
        }
        p = set_socket(p, socket_id, uid);
        
        let _: () = p.query_async(&mut self.users_cache).await?;
        
        Ok(())
    }
    
    pub async fn get_uid_from_socket(&mut self, socket_id: &str) -> RedisResult<Option<String>> {
        let uid: Option<String> = self.users_cache.get(format!("socket:{socket_id}")).await?;
        Ok(uid)
    }
    
    pub async fn del_user(&mut self, uid: &str, socket_id: &str) -> RedisResult<()> {
        let mut p = &mut redis::pipe();
        
        p = p.zrem("users", &uid).ignore(); // delete user from geomap
        p = del_avatar(p, &uid);
        p = del_username(p, &uid);
        p = del_socket(p, socket_id);
        
        let _: () = p.query_async(&mut self.users_cache).await?;
        
        Ok(())
    }
    
    pub async fn del_user_from_socket(&mut self, socket_id: &str) -> RedisResult<()> {
        match self.get_uid_from_socket(socket_id).await? {
            Some(uid) => self.del_user(&uid, socket_id).await,
            None => Ok(())
        }
    }
    
    pub async fn geoquery_users(&mut self, within: &Rect) -> Result<Vec<UserPOI>, Box<dyn Error>> {
        let search_results: Vec<(String, (f64, f64))> =  geosearch_cmd("users", within).query_async(&mut self.users_cache).await?;
        let mut p = &mut redis::pipe();
        
        // for each user, get their avatar and username
        for (uid, _) in &search_results {
            p = get_avatar(p, uid);
            p = get_username(p, uid);
        }
        
        // [avatar, username, avatar, username, avatar, username, ...]
        let avatars_and_names: Vec<redis::Value> = p.query_async(&mut self.users_cache).await?;
        let mut res = Vec::with_capacity(search_results.len());
        
        // combine `search_results` and `avatars_and_names` into a `UserPOI` array
        for (i, (uid, pos)) in search_results.iter().enumerate() {
            let avatar: Option<usize> = from_redis_value(&avatars_and_names[i * 2])?;
            
            if let Some(avatar) = avatar {
                res.push(UserPOI {
                    id: uid.to_string(),
                    pos: *pos,
                    avatar,
                    username: from_redis_value(&avatars_and_names[i * 2 + 1])?, 
                });
            }
        }
        
        Ok(res)
    }
}

#[derive(Debug, Serialize)]
pub struct UserPOI {
    pub id: String,
    pub pos: (f64, f64),
    pub avatar: usize,
    pub username: Option<String>
}