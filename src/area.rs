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
impl TileRegion {
    pub fn get_tile_size(&self) -> usize {
        (180 * 2) / 2_usize.pow(self.depth as u32)
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


const SPLIT: &str = " : ";

// depth : x : y

pub fn update_rooms(client_socket: &SocketRef, prev: &Option<TileRegion>, curr: &TileRegion)  {

    // leave old rooms
    if let Some(prev) = prev {
        if prev.depth != curr.depth {
            // leave all rooms
            println!("leaving all old rooms", );
            client_socket.leave_all().unwrap();
        }
        else {
            for room in client_socket.rooms().unwrap() {
                
                let [x, y] = room.split(SPLIT)
                                .map(|str| str.parse::<f64>().unwrap())
                                .collect::<Vec<f64>>()[1..] 
                                else { panic!("{room:?} should be depth:x:y format") };

                
                // if this tile is outside region, leave
                if !curr.area.contains(x, y) {

                    // leave this room
                    println!("left {room:?}", );
                    client_socket.leave(room).unwrap();
                }
            }
        }
    }

    let tile_size = curr.get_tile_size();
    let width = ((curr.area.right - curr.area.left) / tile_size as f64) as usize;
    let height = ((curr.area.top - curr.area.bottom) / tile_size as f64) as usize;

    // join curr rooms    
    for x in 0..width {
        for y in 0..height {

            let room = format!("{}{}{}{}{}", 
                curr.depth,
                SPLIT,
                curr.area.left + (x * tile_size) as f64,
                SPLIT,
                curr.area.bottom + (y * tile_size) as f64,
            );

            // join this room 
            println!("joined {}", &room);
            client_socket.join(room).unwrap();
        }
    }

}

