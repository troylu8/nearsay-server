use std::collections::{HashSet, HashMap};
use mongodb::bson::Document;
use redis::{aio::MultiplexedConnection, geo::{Coord, RadiusSearchResult}, AsyncCommands};
use serde::Serialize;


#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Cluster {
    pub x: f64,
    pub y: f64,
    pub size: usize,

    pub blurb: Option<String>,
}
impl Cluster {

    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x, y,
            size: 1,
            blurb: None
        }
    }

    pub fn new_with_blurb(x: f64, y: f64, blurb: String) -> Self {
        Self {
            x, y,
            size: 1,
            blurb: Some(blurb)
        }
    }

    pub fn absorb(&mut self, other: &Cluster) {
        self.x = (self.size as f64 * self.x + other.size as f64 * other.x) / (self.size + other.size) as f64;
        self.y = (self.size as f64 * self.y + other.size as f64 * other.y) / (self.size + other.size) as f64;
        
        self.size += other.size;

        self.blurb = None;
    }

    pub fn dist_to(&self, other: &Cluster) -> f64 {
        ((self.x - other.x).powf(2.0) + (self.y - other.y).powf(2.0)).sqrt()
    }
    pub fn dist_to_pt(&self, (x, y): (f64, f64)) -> f64 {
        ((self.x - x).powf(2.0) + (self.y - y).powf(2.0)).sqrt()
    }
}

impl From<Document> for Cluster {
    fn from(doc: Document) -> Self {
        
        let [ref x, ref y] = doc.get_array("pos").unwrap()[..2] 
        else { panic!("'pos' array doesn't have enough elements") };
        
        Self {
            x: x.as_f64().unwrap(),
            y: y.as_f64().unwrap(),
            size: doc.get_i32("size").unwrap() as usize,
            blurb: 
                match doc.get_str("blurb") {
                    Err(_) => None,
                    Ok(blurb) => Some(blurb.to_string()),
                }
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
            (new_pt.x / radius).floor() as i32,
            (new_pt.y / radius).floor() as i32,
        );

        match grid.get_mut(&bucket) {
            None => { grid.insert(bucket, new_pt.clone()); },
            Some(inhabitant) => { inhabitant.absorb(new_pt); }
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
            final_cluster.absorb(grid.get_mut(&bucket_pos).unwrap());
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



mod tests {
    use super::{cluster, Cluster};

    #[test]
    fn empty() {
        let pts = &[];

        let res = cluster(pts, 1.0);
        assert_eq!(0, res.len());
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
        assert_eq!(2, res.len());
        assert_eq!(true, res.contains(&Cluster { x: 0.5, y: 0.0, size: 2, blurb: None }));
        assert_eq!(true, res.contains(&Cluster { x: 2.5, y: 0.0, size: 2, blurb: None }));
    }
    
    #[test]
    fn cluster_diagonally() {
        let pts = &[
            Cluster::new(0.9, 0.9),
            Cluster::new(1.1, 1.1),
        ];
            
        let res = cluster(pts, 1.0);
        assert_eq!(1, res.len());
        assert_eq!(true, res.contains(&Cluster { x: 1.0, y: 1.0, size: 2, blurb: None }));
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
        assert_eq!(true, res.contains(&Cluster { x: 0.9, y: 0.9, size: 3, blurb: None }));
    }

    #[test]
    fn blurb_stays_only_when_not_clustered() {
        let pts = &[
            Cluster::new_with_blurb(9.0, 9.0, "blurb a".to_string()),
            Cluster::new_with_blurb(0.0, 0.0, "blurb b".to_string()),
            Cluster::new_with_blurb(1.0, 1.0, "blurb c".to_string()),
        ];
            
        let res = cluster(pts, 2.0);
        assert_eq!(2, res.len());
        assert_eq!(true, res.contains(&Cluster { x: 0.5, y: 0.5, size: 2, blurb: None }));
        assert_eq!(true, res.contains(&Cluster { x: 9.0, y: 9.0, size: 1, blurb: Some("blurb a".to_string()) }));

    }
}