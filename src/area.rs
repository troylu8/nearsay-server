

// get snapped prev + curr

// convert curr to tile region
// add to all rooms in tile region

// query db with snapped curr - prev

// return data

use mongodb::bson::{doc, Bson, Document};
use serde::{Serialize, Deserialize};

use num_cmp::NumCmp;
use socketioxide::extract::SocketRef;


fn round_down(n: f64, size: f64) -> f64 {
    (n / size).floor() * size
}
fn round_up(n: f64, size: f64) -> f64 {
    (n / size).ceil() * size
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TileRegion {
    pub depth: usize,
    pub area: Rect<f64>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Rect<T> { pub top: T, pub bottom: T, pub left: T, pub right: T }

impl<T: Copy + std::fmt::Debug> Rect<T> {

    pub fn contains<NumType: NumCmp<T>>(&self, x: NumType, y: NumType) -> bool {
        x.num_ge(self.left) && x.num_le(self.right) && y.num_le(self.top) && y.num_ge(self.bottom)
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



// const BOUND: Rect<f64> = Rect {top: 90.0, bottom: -90.0, left: -180.0, right: 180.0 };
// pub const BOUND: Rect<f64> = Rect {top: 10.0, bottom: 0.0, left: 0.0, right: 10.0 };

// fn get_depth_and_tile_size(rect: &Rect<f64>) -> (usize, f64) {
//     let mut depth = 0;
//     let mut tile_size = BOUND.right - BOUND.left;

//     let rect_size = (rect.right - rect.left).min(rect.top - rect.bottom);

//     while rect_size < tile_size {
//         tile_size /= 2.0;
//         depth += 1;
//     }

//     (depth, tile_size)
// } 

// fn to_tile_reg(snapped_view: &Rect<f64>) -> TileRegion {
//     let (depth, tile_size) = get_depth_and_tile_size(&snapped_view);

//     let bottom = ((snapped_view.bottom - BOUND.bottom) / tile_size).floor() as usize;
//     let left = ((snapped_view.left - BOUND.left) / tile_size).floor() as usize;
//     let top = ((snapped_view.top - BOUND.bottom) / tile_size - 1.0).ceil() as usize;
//     let right = ((snapped_view.right - BOUND.left) / tile_size - 1.0).ceil() as usize;

//     TileRegion {
//         depth, 
//         tile_size,
//         tile_region: Rect { top, bottom, left, right }
//     }
// }


// const SPLIT: &str = " : ";

// pub fn update_rooms(client_socket: &SocketRef, prev: &Option<Rect<f64>>, curr: &Rect<f64>)  {

//     let curr = to_tile_reg(curr);

//     // leave rooms
//     if let Some(prev_snapped) = prev {

//         let (prev_depth, prev_tile_size) = get_depth_and_tile_size(&prev_snapped);
    
//         if prev_depth != curr.depth {
//             // leave all rooms
//             println!("leaving all old rooms", );
//             client_socket.leave_all().unwrap();
//         }
//         else {
//             println!("new region {:?}", curr.tile_region);

//             for room in client_socket.rooms().unwrap() {
                
//                 let [tile_x, tile_y] = room.split(SPLIT)
//                                 .map(|str| str.parse::<f64>().unwrap() / prev_tile_size)
//                                 .collect::<Vec<f64>>()[1..] 
//                                 else { panic!("{room:?} should be depth:x:y format") };

                
//                 // if this tile is outside region, leave
//                 if !curr.tile_region.contains(tile_x, tile_y) {
//                     // dbg!(&curr.region);
//                     // dbg!([tile_delta_x, tile_delta_y]);
//                     println!("{:?} is outside", [tile_x, tile_y]);

//                     // leave this room
//                     println!("left {room:?}", );
//                     client_socket.leave(room).unwrap();
//                 }
//             }
//         }

//     }

//     // join rooms in curr    
//     for x in curr.tile_region.left ..= curr.tile_region.right {
//         for y in curr.tile_region.bottom ..= curr.tile_region.top {

//             let room = format!("{}{}{}{}{}", 
//                 curr.depth,
//                 SPLIT,
//                 (x as f64) * curr.tile_size + BOUND.left,
//                 SPLIT,
//                 (y as f64) * curr.tile_size + BOUND.bottom
//             );
//             // join this room 
//             println!("joined {}", &room);
//             client_socket.join(room).unwrap();
//         }
//     }

// }