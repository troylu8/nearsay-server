use std::{collections::HashMap, error::Error, sync::Mutex, usize};

use r2d2::{Pool, PooledConnection};
use redis::{geo::{Coord, Unit}, Client, Commands, ErrorKind, FromRedisValue, RedisError, RedisResult, ToRedisArgs};
use redis_macros::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};

pub struct Cluster {
    pub x: f64,
    pub y: f64,
    pub size: usize,
}
impl Cluster {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x, y,
            size: 1
        }
    }
    pub fn merge_with(&mut self, other: Cluster) {
        self.x = (self.size as f64 * self.x + other.size as f64 * other.x) / (self.size + other.size) as f64;
        self.y = (self.size as f64 * self.y + other.size as f64 * other.y) / (self.size + other.size) as f64;
        
        self.size += other.size;
    }
}

#[derive(Debug, Clone)]
pub struct NearsayCache {
    conn_pool: Pool<redis::Client>
}
impl NearsayCache {

    pub fn new(conn_pool: Pool<redis::Client>) -> Result<Self, Box<dyn Error>> {
        Ok( Self { conn_pool } )
    }

    fn redis(&self) -> Result<PooledConnection<redis::Client>, r2d2::Error> {
        self.conn_pool.get()
    }

    fn save_cluster(&mut self, id: &str, cluster: &Cluster) -> Result<(), Box<dyn Error>> {
        Ok(
            self.redis()?.hset_multiple::<_, _, _, ()>(id, &[
                ("x", format!("{}", cluster.x)), 
                ("y", format!("{}", cluster.y)), 
                ("size", format!("{}", cluster.size))
            ])?
        )
    }

    fn get_cluster(&mut self, id: &str) -> Result<Cluster, Box<dyn Error>> {
        let hash: HashMap<String, String> = self.redis()?.hgetall(id)?;

        println!("getting clusher {}", id);
        println!("{:?}", hash);
        
        Ok(Cluster {
            x: hash.get("x").unwrap().parse()?,
            y: hash.get("y").unwrap().parse()?,
            size: hash.get("size").unwrap().parse()?,
        })
    }

    fn add_pt(&mut self, x: f64, y: f64, id: &str) -> Result<(), Box<dyn Error>> {
        let mut to_delete = Vec::with_capacity(15); // initial capacity = total layers

        for layer in 1..=3 {
            self.add_cluster_to_layer(layer, &format!("cluster-l{layer}-{id}"), x, y, &mut to_delete)?;
        }
        
        for id in to_delete {
            let _: () = self.redis()?.del(id)?;
        }

        Ok(())
    }

    fn add_cluster_to_layer(&mut self, layer: usize, new_cluster_id: &str, x: f64, y: f64, to_delete: &mut Vec<String>) -> Result<(), Box<dyn Error>> {
        
        let (layer_name, radius) = get_layer_info(layer);
        let mut new_cluster = Cluster::new(x, y);

        let nearby_cluster_ids: Vec<String> = redis::cmd("GEOSEARCH")
            .arg(&layer_name)
            .arg("FROMLONLAT")
            .arg(y)     // lon
            .arg(x)     // lat
            .arg("BYRADIUS")
            .arg(radius)
            .arg(Unit::Kilometers)
            .query::<Vec<String>>(self.redis().as_deref_mut().unwrap())?;

        // merge nearby clusters to new one + delete them from redis
        for id in nearby_cluster_ids {
            let nearby_cluster = self.get_cluster(&id)?;
            
            // remove this cluster from the layer
            let _: () = self.redis()?.zrem(&layer_name, &id)?; 
            
            // mark this cluster to be deleted later 
            if !to_delete.contains(&id) {
                to_delete.push(id);                         
            }

            new_cluster.merge_with(nearby_cluster);
        }

        // save new cluster to redis
        self.redis()?.geo_add::<_, _, ()>(layer_name, (Coord::lon_lat(new_cluster.y, new_cluster.x), new_cluster_id))?;
        self.save_cluster(new_cluster_id, &new_cluster)?;

        Ok(())
    }

    pub fn insert_post(&mut self, x: f64, y: f64, id: &str, blurb: &str) -> Result<(), Box<dyn Error>> {
        self.redis()?.hset_multiple::<_, _, _, ()>(id, &[
            ("x", format!("{x}")), 
            ("y", format!("{y}")), 
            ("blurb", blurb.to_string()) 
        ])?;

        self.add_pt(x, y, id)?;

        Ok(())
    }

    // pub fn get_post

}

const FIFTY_PX_IN_METERS_AT_ZOOM_0: f64 = 3913575.848201024;

/// returns `(layer name, cluster radius in km)` 
fn get_layer_info(layer: usize) -> (String, f64) {
    (
        format!("layer-{layer}"),
        FIFTY_PX_IN_METERS_AT_ZOOM_0 / 2.0_f64.powf(layer as f64)
    )
}