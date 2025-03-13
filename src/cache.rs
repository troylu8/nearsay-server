use std::{error::Error, usize};
use redis::aio::MultiplexedConnection;
use redis::geo::RadiusSearchResult;
use redis::{AsyncCommands, RedisResult};
use redis::geo::{Coord, Unit};

use crate::area::Rect;
use crate::cluster::{get_cluster_radius_meters, Cluster};

const MIN_CACHED_LAYER: usize = 2;
const MAX_CACHED_LAYER: usize = 5;

/// iterator of (layer name, radius)
fn all_layers_iter() -> impl Iterator<Item = (String, f64)> {
    (MIN_CACHED_LAYER..=MAX_CACHED_LAYER)
        .map(|num| (
            format!("L{num}"), 
            get_cluster_radius_meters(num)
        ))
        .into_iter()
}

#[derive(Debug, Clone)]
pub struct MapLayersCache {
    redis: MultiplexedConnection,
}
impl MapLayersCache {

    pub async fn new() -> Result<Self, Box<dyn Error>> {
        Ok( 
            Self { 
                redis:  redis::Client::open("redis://localhost:5001")?
                        .get_multiplexed_async_connection().await?
            }
        )
    }

    async fn get_blurb(&mut self, cluster_id: &str) -> RedisResult<Option<String>> {
        self.redis.get(format!("blurb:{cluster_id}")).await
    }
    async fn set_blurb(&mut self, cluster_id: &str, blurb: &str) -> RedisResult<()> {
        self.redis.set(format!("blurb:{cluster_id}"), format!(" '{blurb}' ")).await
    }
    async fn try_del_blurb(&mut self, cluster_id: &str) -> bool {
        for (layer, _) in all_layers_iter() {
            let cluster_size = self.get_cluster_size(&layer, cluster_id).await;
            
            if let Ok(1) = cluster_size { return false }
        }
        
        let _: () = self.redis.del(format!("blurb:{cluster_id}")).await.unwrap();

        true
    }

    async fn get_cluster_size(&mut self, layer: &str, cluster_id: &str) -> RedisResult<usize> {
        self.redis.get(format!("size:{layer}:{cluster_id}")).await
    }
    async fn set_cluster_size(&mut self, layer: &str, cluster_id: &str, size: usize) -> RedisResult<()> {
        self.redis.set(format!("size:{layer}:{cluster_id}"), size).await
    }
    async fn del_cluster_size(&mut self, layer: &str, cluster_id: &str) -> RedisResult<()> {
        self.redis.del(format!("size:{layer}:{cluster_id}")).await
    }

    /// note: doesn't delete shared `blurb` value!
    async fn del_cluster(&mut self, layer: &str, cluster_id: &str) -> RedisResult<()> {
        let _: () = self.redis.zrem(layer, cluster_id).await?;
        self.del_cluster_size(layer, cluster_id).await
    }

    async fn add_cluster(&mut self, layer: &str, radius: f64, cluster_id: &str, x: f64, y: f64, merged_cluster_ids_out: &mut Vec<String>) -> Result<(), Box<dyn Error>> {
        
        let mut new_cluster = Cluster::new(x, y);

        let nearby_clusters = self.geoquery_radius(layer, x, y, radius).await?;

        for (nearby_cluster_id, nearby_cluster) in nearby_clusters {
            new_cluster.absorb(&nearby_cluster);
            
            self.del_cluster(layer, &nearby_cluster_id).await.unwrap();
            
            if !merged_cluster_ids_out.contains(&nearby_cluster_id) {
                merged_cluster_ids_out.push(nearby_cluster_id);                         
            }
        }

        // add cluster id to layer
        self.redis.geo_add::<_, _, ()>(layer, (Coord::lon_lat(new_cluster.y, new_cluster.x), cluster_id)).await?;
        
        self.set_cluster_size(layer, cluster_id, new_cluster.size).await.unwrap();

        Ok(())
    }

    /// `radius` in meters
    /// 
    /// returns `(cluster_id, cluster)`
    async fn geoquery_radius(&mut self, layer: &str, x: f64, y: f64, radius: f64) -> Result<Vec<(String, Cluster)>, Box<dyn Error>> {
        let search_results: Vec<RadiusSearchResult> = redis::cmd("GEOSEARCH")
            .arg(layer)
            .arg("FROMLONLAT")
            .arg(y)
            .arg(x)
            .arg("BYRADIUS")
            .arg(radius)
            .arg(Unit::Meters)
            .arg("WITHCOORD")
            .query_async(&mut self.redis).await?;

        let mut res = vec![];
        for search_res in search_results {            
            res.push((search_res.name.clone(), self.search_res_to_cluster(layer, search_res).await? ));
        }
        Ok(res)
    }

    async fn search_res_to_cluster(&mut self, layer: &str, search_res: RadiusSearchResult) -> Result<Cluster, Box<dyn Error>> {
        let Coord {latitude: x, longitude: y} = search_res.coord.unwrap();
        
        let size = self.get_cluster_size(layer, &search_res.name).await?;
            
        let blurb = 
            if size == 1 { self.get_blurb(&search_res.name).await? } 
            else { None };

        Ok(Cluster {x, y, size, blurb})
    }

    /// `within` should be in degrees
    /// 
    /// returns `(cluster_id, cluster)`
    pub async fn try_get_post_pts(&mut self, layer: usize, within: &Rect) -> Result<Vec<Cluster>, ()> {
        if layer < MIN_CACHED_LAYER || MAX_CACHED_LAYER < layer { return Err(()) }

        let layer_name = format!("L{layer}");
        
        let width = within.right - within.left;
        let height = within.top - within.bottom;
        let center_x = within.left + width / 2.0;
        let center_y = within.bottom + height / 2.0;

        let search_results: Vec<RadiusSearchResult> = redis::cmd("GEOSEARCH")
            .arg(&layer_name)
            .arg("FROMLONLAT")
            .arg(center_y)
            .arg(center_x)
            .arg("BYBOX")
            .arg(height)
            .arg(width)
            .arg(Unit::Meters)
            .arg("WITHCOORD")
            .query_async(&mut self.redis).await.map_err(|_| ())?;
        
        println!("{:?}", search_results.len());

        let mut res = vec![];
        for search_res in search_results {            
            res.push(
                self.search_res_to_cluster(&layer_name, search_res).await.map_err(|_| ())? 
            );
        }
        Ok(res)
    }

    pub async fn save_post_pt(&mut self, post_id: &str, x: f64, y: f64, blurb: &str) -> Result<(), ()> {
        
        let mut merged_cluster_ids = Vec::new();

        for (layer, radius) in all_layers_iter() {
            self.add_cluster(&layer, radius, post_id, x, y, &mut merged_cluster_ids).await.unwrap();
        }
        
        // if didnt merge with anyone, save blurb
        if merged_cluster_ids.is_empty() {
            self.set_blurb(post_id, blurb).await.unwrap();
        }
        else {
            for id in merged_cluster_ids {
                self.try_del_blurb(&id).await;
            }
        }

        Ok(())
    }

    pub async fn flush_all(&mut self) -> RedisResult<()> {
        redis::cmd("FLUSHALL").exec_async(&mut self.redis).await
    }
}

