use mongodb::bson::{doc, Bson, Document};
use serde::{Serialize, Deserialize};

pub const WORLD_BOUND_X: f64 = 180.0;
pub const WORLD_BOUND_Y: f64 = 90.0;
pub const WORLD_MAX_BOUND: f64 = 180.0; 

pub const MAX_TILE_LAYER: usize = 16;

/// returns `(tile layer, tile size)`
pub fn get_tile_layer_and_size(view: &Rect) -> (usize, f64) {
    
    let view_size_min = (view.top - view.bottom).min(view.right - view.left);
    if view_size_min == 0.0 { panic!("rect needs either width or height to be >= 1") }
    
    let mut layer = 0;
    let mut tile_size = WORLD_MAX_BOUND * 2.0;  
    
    while tile_size > view_size_min && layer < MAX_TILE_LAYER {
        tile_size /= 2.0;
        layer += 1;
    }
    
    (layer, tile_size)
}

#[cfg(test)]
mod tests {
    use crate::area::{get_tile_layer_and_size, Rect};

    #[test]
    fn tile_size() {
        assert_eq!((0, 360.0), get_tile_layer_and_size(&Rect {top: 90., bottom: -90., left: -180., right: 180.}));
        assert_eq!((2, 90.0), get_tile_layer_and_size(&Rect {top: 0., bottom: 0., left: 100., right: 180.}));
        assert_eq!((5, 11.25), get_tile_layer_and_size(&Rect {top: -10., bottom: -20., left: 0., right: 0.}));
    }
}


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

