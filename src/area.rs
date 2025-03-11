use mongodb::bson::{doc, Bson, Document};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct TileRegion {
    pub depth: usize,
    pub area: Rect
}
impl TileRegion {
    pub const BOUND: usize = 180;

    pub fn get_tile_size(&self) -> f64 {
        (TileRegion::BOUND * 2) as f64 / 2.0_f64.powf(self.depth as f64) 
    }
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

