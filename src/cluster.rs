use std::collections::{HashSet, HashMap};
use mongodb::bson::Document;
use serde::Serialize;

use crate::db::gen_id;

pub const MIN_ZOOM_LEVEL: usize = 3;
pub const MAX_ZOOM_LEVEL: usize = 19;

pub fn get_cluster_radius_meters(zoom_level: usize) -> f64 {
    const FIFTY_PX_IN_METERS_AT_ZOOM_0: f64 = 7827151.696402048;

    FIFTY_PX_IN_METERS_AT_ZOOM_0 / 2.0_f64.powf(zoom_level as f64)
}

pub fn get_cluster_radius_degrees(zoom_level: usize) -> f64 {
    const FIFTY_PX_IN_DEG_AT_ZOOM_0: f64 = 70.3125;

    FIFTY_PX_IN_DEG_AT_ZOOM_0 / 2.0_f64.powf(zoom_level as f64)
}

// returns `(x, y, size)`
pub fn merge_clusters(x1: f64, y1: f64, size1: usize, x2: f64, y2: f64, size2: usize) -> (f64, f64, usize) {
    (
        (size1 as f64 * x1 + size2 as f64 * x2) / (size1 + size2) as f64,
        (size1 as f64 * y1 + size2 as f64 * y2) / (size1 + size2) as f64,
        size1 + size2
    )
}


#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Cluster {
    pub pos: (f64, f64),
    pub size: Option<usize>,

    pub id: String,
    pub blurb: Option<String>,
}
impl Cluster {
    
    pub fn new(x: f64, y: f64) -> Self {
        Self { pos: (x, y), size: None, id: gen_id(), blurb: None }
    }
    
    pub fn with_blurb(mut self, blurb: String) -> Self {
        self.blurb = Some(blurb);
        self
    }
    
    pub fn x(&self) -> f64 { self.pos.0 }
    pub fn y(&self) -> f64 { self.pos.1 }
    pub fn size(&self) -> usize { self.size.unwrap_or(1) }
    
    pub fn absorb_cluster(&mut self, other: &Cluster) {
        let (merged_x, merged_y, merged_size) = merge_clusters(
            self.x(), 
            self.y(), 
            self.size(), 
            
            other.x(), 
            other.y(), 
            other.size()
        );
        
        self.pos = (merged_x, merged_y);
        self.size = Some(merged_size);
        self.blurb = None;
    }
    
    pub fn dist_to(&self, other: &Cluster) -> f64 {
        ((self.x() - other.x()).powf(2.0) + (self.y() - other.y()).powf(2.0)).sqrt()
    }
    
}

impl From<Document> for Cluster {
    fn from(poi_doc: Document) -> Self {
        
        let [ref x, ref y] = poi_doc.get_array("pos").unwrap()[..2] 
        else { panic!("'pos' array doesn't have enough elements") };
        
        Self {
            pos: (x.as_f64().unwrap(), y.as_f64().unwrap()),
            size: None,
            id: poi_doc.get_str("_id").unwrap().to_string(),
            blurb: Some(poi_doc.get_str("blurb").unwrap().to_string())
        }
    }
}

pub fn cluster(pts: &[Cluster], radius: f64) -> Vec<Cluster> {
    if radius <= 0.0 { return pts.to_vec() }

    // tile pos -> cluster
    let mut grid: HashMap<(i32, i32), Cluster> = HashMap::new();

    // sort pois into grid of clusters or pois
    for new_pt in pts {
        let bucket = (
            (new_pt.x() / radius).floor() as i32,
            (new_pt.y() / radius).floor() as i32,
        );

        match grid.get_mut(&bucket) {
            None => { grid.insert(bucket, new_pt.clone()); },
            Some(inhabitant) => { inhabitant.absorb_cluster(new_pt); }
        }
    }

    let mut res = vec![];

    let buckets = grid.keys().map(|pos| *pos).collect::<Vec<(i32, i32)>>();
    let mut visited = HashSet::new();

    for bucket_pos in buckets {

        let mut final_item: Option<Cluster> = None;

        cluster_grid_dfs(&mut grid, radius, bucket_pos, &mut final_item, &mut visited);

        if let Some(cluster) = final_item {
            res.push(cluster);
        }
    }

    res
}

fn cluster_grid_dfs(
    grid: &mut HashMap<(i32, i32), Cluster>,
    radius: f64,
    bucket_pos: (i32, i32),
    final_item: &mut Option<Cluster>,
    visited: &mut HashSet<(i32, i32)>
) {

    if visited.contains(&bucket_pos) { return; }
    visited.insert(bucket_pos);

    match final_item {
        // this is where dfs started
        None => { final_item.replace(grid[&bucket_pos].clone()); }, 
        
        // add current item to final cluster
        Some(final_cluster) => {
            final_cluster.absorb_cluster(grid.get_mut(&bucket_pos).unwrap());
        },
    }

    let (x, y) = bucket_pos;
    for adj_bucket_pos in [
        (x + 1, y),
        (x - 1, y),
        (x, y + 1),
        (x, y - 1),
        (x - 1, y - 1),
        (x + 1, y - 1),
        (x - 1, y + 1),
        (x + 1, y + 1),
    ] {
        if let Some(adj_inhabitant) = grid.get( &adj_bucket_pos ) {
            if adj_inhabitant.dist_to(final_item.as_ref().unwrap()) <= radius {
                cluster_grid_dfs(grid, radius, adj_bucket_pos, final_item, visited);
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::{cluster, Cluster};

    #[test]
    fn empty() {
        let pts = &[];

        let res = cluster(pts, 1.0);
        assert_eq!(0, res.len());
    }
    
    fn has_cluster(clusters: &Vec<Cluster>, pos: (f64, f64), size: Option<usize>, blurb: Option<&str>) -> bool {
        for c in clusters {
            if c.pos == pos && c.size == size && c.blurb == blurb.map(|s| s.to_string()) {
                return true;
            }
        }
        false
    }

    #[test]
    fn no_long_chaining() {
        let pts = &[
            Cluster::new(0.0, 0.0),
            Cluster::new(1.0, 0.0),
            Cluster::new(2.0, 0.0),
            Cluster::new(3.0, 0.0),
        ];
        
        let res = cluster(pts, 1.0);
        assert_eq!(true, res.len() > 1);
    }
    
    #[test]
    fn cluster_diagonally() {
        let pts = &[
            Cluster::new(0.9, 0.9),
            Cluster::new(1.1, 1.1),
        ];
            
        let res = cluster(pts, 1.0);
        assert_eq!(1, res.len());
        assert_eq!(true, has_cluster(&res, (1.0, 1.0), Some(2), None));
    }

    #[test]
    fn cluster_many_same_pts() {
        let pts = &[
            Cluster::new(0.9, 0.9),
            Cluster::new(0.9, 0.9),
            Cluster::new(0.9, 0.9),
        ];
            
        let res = cluster(pts, 1.0);
        assert_eq!(1, res.len());
        assert_eq!(true, has_cluster(&res, (0.9, 0.9), Some(3), None));
    }

    #[test]
    fn blurb_stays_only_when_not_clustered() {
        let pts = &[
            Cluster::new(9.0, 9.0).with_blurb("blurb a".to_string()),
            Cluster::new(0.0, 0.0).with_blurb("blurb a".to_string()),
            Cluster::new(1.0, 1.0).with_blurb("blurb a".to_string()),
        ];
            
        let res = cluster(pts, 2.0);
        assert_eq!(2, res.len());
        assert_eq!(true, has_cluster(&res, (0.5, 0.5), Some(2), None));
        assert_eq!(true, has_cluster(&res, (9.0, 9.0), None, Some("blurb a")));
    }
}