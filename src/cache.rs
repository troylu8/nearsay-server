use std::{collections::HashMap, error::Error, usize};
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use redis::{geo::{Coord, Unit}, Commands};
use serde::Serialize;

use crate::area::Rect;
use crate::cluster::Cluster;

#[derive(Debug, Clone)]
pub struct NearsayCache {
    r: MultiplexedConnection,
}
impl NearsayCache {

    pub async fn new() -> Result<Self, Box<dyn Error>> {
        println!("creating new nearsay cache ", );
        Ok( 
            Self { 
                r:  redis::Client::open("redis://127.0.0.1/")?
                    .get_multiplexed_async_connection().await?
            } 
        )
    }

    async fn save_cluster(&mut self, id: &str, cluster: &Cluster) -> Result<(), Box<dyn Error>> {
        self.r.hset_multiple::<_, _, _, ()>(id, &[
            ("x", format!("{}", cluster.x)), 
            ("y", format!("{}", cluster.y)), 
            ("size", format!("{}", cluster.size))
        ]).await?;

        Ok(())
    }

    async fn get_cluster(&mut self, id: &str) -> Result<Cluster, Box<dyn Error>> {
        let hash: HashMap<String, String> = self.r.hgetall(id).await?;

        println!("getting clusher {}", id);
        println!("{:?}", hash);
        
        Ok(Cluster {
            x: hash.get("x").unwrap().parse()?,
            y: hash.get("y").unwrap().parse()?,
            size: hash.get("size").unwrap().parse()?,
            blurb: None
        })
    }

    async fn add_pt(&mut self, x: f64, y: f64, id: &str) -> Result<(), Box<dyn Error>> {
        let mut to_delete = Vec::with_capacity(15); // initial capacity = total layers

        for layer in 1..=3 {
            self.add_cluster_to_layer(layer, &format!("cluster-l{layer}-{id}"), x, y, &mut to_delete).await?;
        }
        
        for id in to_delete {
            let _: () = self.r.del(id).await?;
        }

        Ok(())
    }

    async fn add_cluster_to_layer(&mut self, layer: usize, new_cluster_id: &str, x: f64, y: f64, to_delete: &mut Vec<String>) -> Result<(), Box<dyn Error>> {
        
        let (layer_name, radius) = get_layer_info(layer);
        let mut new_cluster = Cluster::new(x, y);

        let nearby_cluster_ids: Vec<String> = redis::cmd("GEOSEARCH")
            .arg(&layer_name)
            .arg("FROMMEMBER")
            .arg(new_cluster_id)     
            .arg("BYRADIUS")
            .arg(radius)
            .arg(Unit::Meters)
            .query_async::<Vec<String>>(&mut self.r).await?;

        // merge nearby clusters to new one + delete them from redis
        for id in nearby_cluster_ids {
            let nearby_cluster = self.get_cluster(&id).await?;
            
            // remove this cluster from the layer
            let _: () = self.r.zrem(&layer_name, &id).await?; 
            
            // mark this cluster to be deleted later 
            if !to_delete.contains(&id) {
                to_delete.push(id);                         
            }

            new_cluster.merge_with(nearby_cluster);
        }

        // save new cluster to redis
        self.r.geo_add::<_, _, ()>(layer_name, (Coord::lon_lat(new_cluster.y, new_cluster.x), new_cluster_id)).await?;
        self.save_cluster(new_cluster_id, &new_cluster).await?;

        Ok(())
    }

    /// query should be in meters
    pub async fn geoquery_posts(&mut self, layer: usize, query: &Rect) -> Result<Option<Vec<Cluster>>, Box<dyn Error>> {
        
        let width = query.right - query.left;
        let height = query.top - query.bottom;
        let center_x = query.left + width / 2.0;
        let center_y = query.bottom + height / 2.0;

        let cluster_ids = redis::cmd("GEOSEARCH")
            .arg(&format!("layer-{layer}"))
            .arg("FROMLONLAT")
            .arg(center_y)     // lon
            .arg(center_x)     // lat
            .arg("BYBOX")
            .arg(height)
            .arg(width)
            .arg(Unit::Meters)
            .query_async::<Vec<String>>(&mut self.r).await?;

        if cluster_ids.is_empty() { return Ok(None) }

        let mut clusters = Vec::with_capacity(cluster_ids.len());
        for id in cluster_ids {
            let mut cluster = self.get_cluster(&id).await?;

            if let Ok(blurb) = self.r.hget::<_, _, String>(&id, "blurb").await {
                cluster.blurb = Some(blurb);
            }

            clusters.push(self.get_cluster(&id).await?);
        }
        
        Ok(Some(clusters))
    }

    // pub fn insert_post(&mut self, x: f64, y: f64, id: &str, blurb: &str) -> Result<(), Box<dyn Error>> {
    //     self.r.hset::<_, _, _, ()>(id, "blurb", blurb)?;

    //     self.add_pt(x, y, id)?;

    //     Ok(())
    // }

    // pub fn get_post(&mut self, id: &str) -> Result<(), Box<dyn Error>> {
    //     // self.r.hset::<_, _, _, ()>(id, "blurb", blurb)?;

    //     // self.add_pt(x, y, id)?;

    //     // Ok(())
    // }

}

const FIFTY_PX_IN_METERS_AT_ZOOM_0: f64 = 3913575.848201024;

/// returns `(layer name, cluster radius in km)` 
fn get_layer_info(layer: usize) -> (String, f64) {
    (
        format!("layer-{layer}"),
        FIFTY_PX_IN_METERS_AT_ZOOM_0 / 2.0_f64.powf(layer as f64)
    )
}