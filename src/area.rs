use mongodb::bson::{doc, Bson, Document};
use serde::{Serialize, Deserialize};

use num_cmp::NumCmp;

#[derive(Serialize, Deserialize, Debug)]
pub struct TileRegion {
    pub depth: usize,
    pub area: Rect<f64>
}
impl TileRegion {
    pub const BOUND: usize = 180;

    pub fn get_tile_size(&self) -> f64 {
        (TileRegion::BOUND * 2) as f64 / 2.0_f64.powf(self.depth as f64) 
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Rect<T> { pub top: T, pub bottom: T, pub left: T, pub right: T }

impl<T: Copy + std::fmt::Debug> Rect<T> {

    /// bottom left inclusive, top right exclusive 
    pub fn contains<NumType: NumCmp<T>>(&self, x: NumType, y: NumType) -> bool {
        x.num_ge(self.left) && x.num_lt(self.right) && y.num_ge(self.bottom) && y.num_lt(self.top)
    }

    pub fn envelops<NumType: NumCmp<T> + std::fmt::Debug>(&self, smaller: &Rect<NumType>) -> bool {
        return  smaller.top.num_le(self.top) && 
                smaller.bottom.num_ge(self.bottom) && 
                smaller.left.num_ge(self.left) && 
                smaller.right.num_le(self.right);
    }
}

impl<T: Into<Bson> + Copy> Rect<T> {
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

