use mongodb::bson::{doc, Bson, Document};
use serde::{Serialize, Deserialize};

pub const WORLD_BOUND: usize = 180;
pub fn get_tile_size(layer: usize, view: &Rect) -> f64 {
    (WORLD_BOUND * 2) as f64 / 2.0_f64.powf(layer as f64) 
}


#[derive(Debug, Serialize, Deserialize)]
pub struct Rect { pub top: f64, pub bottom: f64, pub left: f64, pub right: f64 }

impl Rect {
    pub fn as_geo_json(&self) -> Document {
        doc! {
            "$geometry": {
                "type": "Polygon",
                "coordinates": [[
                    [self.left, self.bottom],
                    [self.right, self.bottom],
                    [self.right, self.top],
                    [self.left, self.top],
                    [self.left, self.bottom],
                ]]
            }
        }
    }
}

