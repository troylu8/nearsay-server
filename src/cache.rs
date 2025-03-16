use std::collections::HashSet;
use std::{error::Error, usize};
use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, Pipeline, RedisResult};
use redis::geo::{Coord, Unit};

use crate::cluster::{get_cluster_radius_meters, Cluster};

const MIN_CACHED_LAYER: usize = 3;
const MAX_CACHED_LAYER: usize = 5;
const LAYERS_COUNT: usize = MAX_CACHED_LAYER - MIN_CACHED_LAYER + 1;

/// `radius` in meters
fn geoquery_radius<'a>(pipeline: &'a mut Pipeline, layer: usize, x: f64, y: f64, radius: f64) -> &'a mut Pipeline {
    pipeline.cmd("GEOSEARCH")
            .arg(format!("L{layer}"))
            .arg("FROMLONLAT")
            .arg(x)
            .arg(y)
            .arg("BYRADIUS")
            .arg(radius)
            .arg(Unit::Meters)
            .arg("WITHCOORD")
}

// let mut res = vec![];
//     for search_res in search_results {
//         res.push(self.search_res_to_cluster(layer, search_res, true).await?);
//     }
fn get_cluster_size<'a>(pipeline: &'a mut Pipeline, layer: usize, cluster_id: &str) -> &'a mut Pipeline {
    pipeline.get(format!("size:L{layer}:{cluster_id}"))
}
fn set_cluster_size<'a>(pipeline: &'a mut Pipeline, layer: usize, cluster_id: &str, size: usize) -> &'a mut Pipeline {
    pipeline.set(format!("size:L{layer}:{cluster_id}"), size).ignore()
}
fn del_cluster_size<'a>(pipeline: &'a mut Pipeline, layer: usize, cluster_id: &str) -> &'a mut Pipeline {
    pipeline.del(format!("size:L{layer}:{cluster_id}")).ignore()
}

fn add_cluster<'a>(pipeline: &'a mut Pipeline, layer: usize, cluster_id: &str, x: f64, y: f64) -> &'a mut Pipeline {
    pipeline.geo_add(format!("L{layer}"), (Coord::lon_lat(x, y), cluster_id)).ignore()
}
/// note: doesn't delete shared `blurb` value!
fn del_cluster<'a>(mut pipeline: &'a mut Pipeline, layer: usize, cluster_id: &str) -> &'a mut Pipeline {
    pipeline = pipeline.zrem(format!("L{layer}"), cluster_id).ignore();
    del_cluster_size(pipeline, layer, cluster_id)
}

fn set_blurb<'a>(pipeline: &'a mut Pipeline, cluster_id: &str, blurb: &str) -> &'a mut Pipeline {
    pipeline.set(format!("blurb:{cluster_id}"), format!(" '{blurb}' ")).ignore()
}
fn del_blurb<'a>(pipeline: &'a mut Pipeline, post_id: &str) -> &'a mut Pipeline {
    pipeline.del(format!("blurb:{post_id}")).ignore()
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
    
    pub async fn add_post_pt(&mut self, cluster_id: &str, x: f64, y: f64, blurb: &str) -> Result<(), Box<dyn Error>> {
        
        // get ids and positions of nearby clusters
        let mut pipe_geoquery = &mut redis::pipe();
        for layer in MIN_CACHED_LAYER..=MAX_CACHED_LAYER {
            pipe_geoquery = geoquery_radius(pipe_geoquery, layer, x, y, get_cluster_radius_meters(layer));
        }
        // nearby_clusters[x] = (id, pos) of each nearby cluter on layer x
        let nearby_clusters: [ Vec<(String, (f64, f64))> ; LAYERS_COUNT] = pipe_geoquery.query_async(&mut self.redis).await?;
        let mut deleted_clusters_ids = HashSet::new();
        
        let mut pipe_nearby = &mut redis::pipe();
        
        let mut layers_new_cluster_was_merged_on = [false; LAYERS_COUNT];
        
        // for each layer...
        for (i, nearby_clusters_in_layer) in nearby_clusters.iter().enumerate() {
            let layer = i + MIN_CACHED_LAYER;
            
            // get sizes of nearby clusters + delete them
            for (nearby_id, _) in nearby_clusters_in_layer {
                pipe_nearby = get_cluster_size(pipe_nearby, layer, &nearby_id);
                pipe_nearby = del_cluster(pipe_nearby, layer, &nearby_id);
                deleted_clusters_ids.insert(nearby_id);
                layers_new_cluster_was_merged_on[i] = true;
            }
        }

        let deleted_clusters_sizes: Vec<usize> = pipe_nearby.query_async(&mut self.redis).await?;
        let mut sizes_i = 0;
        
        let mut pipe_save = &mut redis::pipe();
        
        // for each layer...
        for (layer, nearby_clusters_in_layer) in nearby_clusters.iter().enumerate() {
            let layer = layer + MIN_CACHED_LAYER;
            
            // create a new cluster
            let mut new_cluster = Cluster::new(x, y);
            
            // merge all nearby clusters into the new cluster
            for (_, nearby_pos) in nearby_clusters_in_layer {
                new_cluster.absorb(*nearby_pos, deleted_clusters_sizes[sizes_i]);
                sizes_i += 1;
            }
            
            // save new cluster and its size to redis
            pipe_save = add_cluster(pipe_save, layer, cluster_id, new_cluster.x(), new_cluster.y());
            pipe_save = set_cluster_size(pipe_save, layer, cluster_id, new_cluster.size);
        }
        
        // get the size of each deleted cluster on each layer...
        for deleted_cluster_id in &deleted_clusters_ids {
            for layer in MIN_CACHED_LAYER..=MAX_CACHED_LAYER {
                pipe_save = get_cluster_size(pipe_save, layer, deleted_cluster_id);
            }
        }
        
        sizes_i = 0;
        
        let deleted_clusters_sizes: Vec<Option<usize>> = pipe_save.query_async(&mut self.redis).await?;
        
        let mut pipe_blurbs = &mut redis::pipe();
        
        for deleted_cluster_id in &deleted_clusters_ids {
            
            // a blurb is required if the cluster is a single on any layer
            let mut blurb_required = false; 
            for _ in 0..LAYERS_COUNT {
                if let Some(1) = deleted_clusters_sizes[sizes_i] {
                    blurb_required = true;
                }
                sizes_i += 1;
            }
            if !blurb_required {
                pipe_blurbs = del_blurb(pipe_blurbs, deleted_cluster_id);
            }
        }
        
        // save blurb if new cluster didn't do a merge on any layer
        for was_merged_on_layer in layers_new_cluster_was_merged_on {
            if !was_merged_on_layer {
                pipe_blurbs = set_blurb(pipe_blurbs, cluster_id, blurb);
                break;
            }
        }
        
        pipe_blurbs.exec_async(&mut self.redis).await?;

        Ok(())
    }

    /// returns `(cluster_id, cluster)`
    // pub async fn try_get_post_pts(&mut self, within: &Rect) -> Result<Vec<Cluster>, ()> {
    //     if layer < MIN_CACHED_LAYER || MAX_CACHED_LAYER < layer { return Err(()) }

    //     let mid_x = (within.left + within.right) / 2.0;
    //     let mid_y = (within.top + within.bottom) / 2.0;

    //     // width = bottom left -> bottom right, in meters
    //     let width = Location::new(within.left, within.bottom).haversine_distance_to(&Location::new(within.right, within.bottom)).meters();

    //     // height = middle top -> middle bottom, in meters
    //     let height = Location::new(mid_x, within.top).haversine_distance_to(&Location::new(mid_x, within.bottom)).meters();

    //     let layer_name = format!("L{layer}");

    //     let search_results: Vec<(String, (f64, f64))> = redis::cmd("GEOSEARCH")
    //         .arg(&layer_name)
    //         .arg("FROMLONLAT")
    //         .arg(mid_x)
    //         .arg(mid_y)
    //         .arg("BYBOX")
    //         .arg(width)
    //         .arg(height)
    //         .arg(Unit::Meters)
    //         .arg("WITHCOORD")
    //         .query_async(&mut self.redis).await.map_err(|_| ())?;

    //     //TODO: test
    //     let p = &mut redis::pipe();
        
    //     for (cluster_id, _) in search_results {
    //         p = get_cluster_size(p, layer, cluster_id)
    //     }
        
    //     Ok(vec![])
    // }

    pub async fn flush_all(&mut self) -> RedisResult<()> {
        redis::cmd("FLUSHALL").exec_async(&mut self.redis).await
    }
}

