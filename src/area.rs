use mongodb::bson::{doc, Document};
use serde::{Serialize, Deserialize};

pub const WORLD_BOUND_X: f64 = 180.0;
pub const WORLD_BOUND_Y: f64 = 90.0;
pub const WORLD_MAX_BOUND: f64 = 180.0; 

pub const MAX_TILE_LAYER: usize = 19;

#[derive(Debug, Serialize, Deserialize)]
pub struct Rect { pub top: f64, pub bottom: f64, pub left: f64, pub right: f64 }

impl Rect {
    
    
    /// a valid view has 
    /// - `top >= bottom` and `right >= left` 
    /// - either width/height must be >= 1 and within
    /// - within world bounds
    pub fn valid_as_view(&self) -> bool {
        (self.top >= self.bottom && self.right >= self.left) &&     
        (self.top > self.bottom || self.right > self.right)  &&         // either width/height must be >= 1
        self.within_world_bounds()
    }
    
    pub fn within_world_bounds(&self) -> bool {
        return self.left >= -WORLD_BOUND_X && self.right <= WORLD_BOUND_X && self.bottom >= -WORLD_BOUND_Y && self.top <= WORLD_BOUND_Y; 
    }
    
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

