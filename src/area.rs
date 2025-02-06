use mongodb::bson::{doc, Bson, Document};
use serde::{Serialize, Deserialize};

use num_cmp::NumCmp;
use socketioxide::extract::SocketRef;

#[derive(Serialize, Deserialize, Debug)]
pub struct TileRegion {
    pub depth: usize,
    pub area: Rect<f64>
}
impl TileRegion {
    const BOUND: usize = 180;

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


pub fn emit_at_pos<T: Sized + Serialize>(io: SocketRef, pos: [f64; 2], event: &str, data: &T) {
    let [x, y] = pos;

    let mut area = Rect {
        left: -(TileRegion::BOUND as f64), 
        right: TileRegion::BOUND as f64, 
        top: TileRegion::BOUND as f64, 
        bottom: -(TileRegion::BOUND as f64)
    };
    
    io.to(get_room(0, area.left, area.bottom)).emit(event, data).unwrap();
    
    for depth in 1..=19 {
        
        let mid_x = (area.left + area.right) / 2.0;
        let mid_y = (area.top + area.bottom) / 2.0;
        
        if x >= mid_x { area.left = mid_x; }
        else { area.right = mid_x; }
        
        if y >= mid_y { area.bottom = mid_y; }
        else { area.top = mid_y; }
        
        io.to(get_room(depth, area.left, area.bottom)).emit(event, data).unwrap();
    }
}


const SPLIT: &str = " : ";

pub fn update_rooms(client_socket: &SocketRef, tilereg: &TileRegion)  {

    client_socket.leave_all().unwrap();

    let tile_size = tilereg.get_tile_size();
    let width = ((tilereg.area.right - tilereg.area.left) / tile_size).ceil() as usize;
    let height = ((tilereg.area.top - tilereg.area.bottom) / tile_size).ceil() as usize;
    
    for x in 0..width {
        for y in 0..height {

            let room = get_room(
                tilereg.depth, 
                tilereg.area.left + (x as f64 * tile_size), 
                tilereg.area.bottom + (y as f64 * tile_size)
            );

            // join this room 
            client_socket.join(room).unwrap();
        }
    }
}

fn get_room(depth: usize, left: f64, bottom: f64) -> String {
    format!("{}{}{}{}{}", depth, SPLIT, to_5_decimals(left), SPLIT, to_5_decimals(bottom))
}

fn to_5_decimals(x: f64) -> f64 {
    (x * 100000.0).round() / 100000.0
}