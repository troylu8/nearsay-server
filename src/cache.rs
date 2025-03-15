use std::{error::Error, usize};
use futures::future::try_join;
use geoutils::Location;
use redis::aio::MultiplexedConnection;
use redis::geo::RadiusSearchResult;
use redis::{AsyncCommands, RedisResult};
use redis::geo::{Coord, Unit};
use serde::Deserialize;

use crate::area::Rect;
use crate::cluster::{get_cluster_radius_meters, Cluster};

const MIN_CACHED_LAYER: usize = 3;
const MAX_CACHED_LAYER: usize = 5;

/// iterator of (layer name, radius)
fn cached_layers_iter() -> impl Iterator<Item = (String, f64)> {
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
                redis:  redis::Client::open("redis://localhost")?
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
    pub async fn del_blurb(&mut self, post_id: &str) -> RedisResult<()> {
        self.redis.del(format!("blurb:{post_id}")).await
    }

    /// `true` if this cluster is a single on any layer
    async fn requires_blurb(&mut self, cluster_id: &str) -> bool {
        for (layer, _) in cached_layers_iter() {
            if let Ok(1) = self.get_cluster_size(&layer, cluster_id).await {
                return true
            }
        }

        false
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
        
        for nearby_cluster in nearby_clusters {
            new_cluster.absorb(&nearby_cluster);

            let id = nearby_cluster.id.unwrap();
            
            self.del_cluster(layer, &id).await.unwrap();
            
            if !merged_cluster_ids_out.contains(&id) {
                merged_cluster_ids_out.push(id);                         
            }
            
        }


        // add cluster id to layer
        self.redis.geo_add::<_, _, ()>(layer, (Coord::lon_lat(new_cluster.x(), new_cluster.y()), cluster_id)).await?;
        
        self.set_cluster_size(layer, cluster_id, new_cluster.size).await.unwrap();

        Ok(())
    }

    /// `radius` in meters
    async fn geoquery_radius(&mut self, layer: &str, x: f64, y: f64, radius: f64) -> Result<Vec<Cluster>, Box<dyn Error>> {
        let search_results: Vec<RadiusSearchResult> = redis::cmd("GEOSEARCH")
            .arg(layer)
            .arg("FROMLONLAT")
            .arg(x)
            .arg(y)
            .arg("BYRADIUS")
            .arg(radius)
            .arg(Unit::Meters)
            .arg("WITHCOORD")
            .query_async(&mut self.redis).await?;

        let mut res = vec![];
        for search_res in search_results {            
            res.push(self.search_res_to_cluster(layer, search_res, true).await?);
        }
        Ok(res)
    }

    /// clusters of `size > 1` won't have an id unless `include_all_ids == true`, 
    async fn search_res_to_cluster(&mut self, layer: &str, search_res: RadiusSearchResult, include_all_ids: bool) -> Result<Cluster, Box<dyn Error>> {
        let Coord {longitude: x, latitude: y} = search_res.coord.unwrap();
        
        let size = self.get_cluster_size(layer, &search_res.name).await?;

        let blurb = if size == 1 { self.get_blurb(&search_res.name).await? } else { None };
        let id = if include_all_ids || size == 1 { Some(search_res.name) } else { None };
 
        Ok(Cluster {pos: (x, y), size, id, blurb})
    }

    /// returns `(cluster_id, cluster)`
    pub async fn try_get_post_pts(&mut self, layer: usize, within: &Rect) -> Result<Vec<Cluster>, ()> {
        if layer < MIN_CACHED_LAYER || MAX_CACHED_LAYER < layer { return Err(()) }

        let mid_x = (within.left + within.right) / 2.0;
        let mid_y = (within.top + within.bottom) / 2.0;

        // width = bottom left -> bottom right, in meters
        let width = Location::new(within.left, within.bottom).haversine_distance_to(&Location::new(within.right, within.bottom)).meters();
        
        // height = middle top -> middle bottom, in meters
        let height = Location::new(mid_x, within.top).haversine_distance_to(&Location::new(mid_x, within.bottom)).meters();

        let layer_name = format!("L{layer}");

        let search_results: Vec<RadiusSearchResult> = redis::cmd("GEOSEARCH")
            .arg(&layer_name)
            .arg("FROMLONLAT")
            .arg(mid_x)
            .arg(mid_y)
            .arg("BYBOX")
            .arg(width)
            .arg(height)
            .arg(Unit::Meters)
            .arg("WITHCOORD")
            .query_async(&mut self.redis).await.map_err(|_| ())?;
        
        println!("{:?}", search_results.len());

        let mut res = vec![];
        for search_res in search_results {            
            res.push(
                self.search_res_to_cluster(&layer_name, search_res, false).await.map_err(|_| ())? 
            );
        }
        Ok(res)
    }

    pub async fn save_post_pt(&mut self, post_id: &str, x: f64, y: f64, blurb: &str) -> Result<(), ()> {
        
        let mut merged_cluster_ids = Vec::new();

        for (layer, radius) in cached_layers_iter() {
            self.add_cluster(&layer, radius, post_id, x, y, &mut merged_cluster_ids).await.unwrap();
        }
        
        if self.requires_blurb(post_id).await {
            self.set_blurb(post_id, blurb).await.unwrap();
        }
        else {
            for id in merged_cluster_ids {
                if !self.requires_blurb(&id).await {
                    self.del_blurb(&id).await.unwrap();
                }
            }
        }

        Ok(())
    }

    pub async fn flush_all(&mut self) -> RedisResult<()> {
        redis::cmd("FLUSHALL").exec_async(&mut self.redis).await
    }
}

