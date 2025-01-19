

// 01 11
// 00 10
// xy


enum Quadrant { TopLeft = 0b01, TopRight = 0b11, BottomLeft = 0b00, BottomRight = 0b10 }
enum Direction { Top, Bottom, Left, Right }

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Geohash {
    pub bits: u32,
    pub mask: u32
}

impl Geohash {

    fn len(&self) -> u32 {
        let mut cursor = 1;
        while self.mask & cursor != 0 {
            cursor = cursor << 1;
        }
        cursor.ilog2()
    }
    fn is_full(&self) -> bool { self.mask & (1 << 31) == 1 }

    fn flip_last_h(&mut self) { self.bits = self.bits ^ 0b10; } // swap the second last bit
    fn flip_last_v(&mut self) { self.bits = self.bits ^ 0b01; } // swap the last bit

    fn ends_on_right(&self) -> bool { return self.bits & 0b10 == 1; } // second last bit is 1
    fn ends_on_bottom(&self) -> bool { return self.bits & 1 == 0; } // last bit is 0

    // top: _1_1_1
    // bottom:  _0_0_0
    // left: 0_0_0_
    // right: 1_1_1_ 
    fn has_only(&self, direction: Direction) -> bool {
        let target = match direction {
            Direction::Top | Direction::Right => 1,
            _ => 0
        };

        let mut bits = match direction {
            Direction::Left | Direction::Right => self.bits >> 1,
            _ => self.bits
        };

        while bits != 0 {
            if bits & 1 != target { return false; }
            bits = bits >> 2;
        }

        true
    }

    fn move_right(&mut self) {

        if !self.ends_on_right() { self.flip_last_h(); }
    
        else if self.has_only(Direction::Right) { // flip every x bit 
            let mut cursor = 0b10;
    
            while cursor & self.mask != 0 {
                self.bits = self.bits ^ cursor;
                cursor = cursor << 2;
            }
        }
    
        else { self.bits =  self.bits ^ 0b1010; } // flip last 2 x bits

    }

    fn move_down(&mut self) {

        if !self.ends_on_bottom() { self.flip_last_v(); }
    
        else if self.has_only(Direction::Bottom) { // flip every y bit 
            let mut cursor = 1;
    
            while cursor & self.mask == 1 {
                self.bits = self.bits ^ cursor;
                cursor = cursor << 2;
            }
        }
    
        else { self.bits =  self.bits ^ 0b0101; } // flip last 2 y bits

    }

    fn move_up(&mut self) {
        if self.mask == 0 { return; }
        self.bits = self.bits >> 2;
        self.mask = self.mask >> 2;
    }
}

impl Into<String> for Geohash {
    fn into(mut self) -> String {
        let mut v = vec![];
        while self.bits != 0 {
            v.push(char::from_digit(self.bits & 1, 2).unwrap());
            self.bits = self.bits >> 1;
        }

        v.iter().collect()
    }
}

struct GeoIterator {
    x: f32,
    y: f32,
    geohash: Geohash,
    bound: Rect
}

impl GeoIterator {
    fn from_pos(x: f32, y: f32) -> Self {
        GeoIterator {
            x, y,
            geohash: Geohash::default(),
            bound: Rect {}
        }
    }
}

impl Iterator for GeoIterator {
    type Item = (Geohash, Rect);

    fn next(&mut self) -> Option<Self::Item> {

        if self.geohash.is_full() { return None; }

        self.bound.width /= 2.0;
        self.bound.height /= 2.0;

        let mid_x = self.bound.x + self.bound.width;
        let mid_y = self.bound.y + self.bound.height;

        if self.x > mid_x {
            self.bound.x = mid_x;
            if self.y > mid_y {
                self.geohash.bits = (self.geohash.bits << 2) | Quadrant::TopRight as u32;
            }
            else {
                self.bound.y += self.bound.width;
                self.geohash.bits = (self.geohash.bits << 2) | Quadrant::BottomRight  as u32;
            }
        }
        else {
            if self.bound.y > mid_y {
                self.geohash.bits = (self.geohash.bits << 2) | Quadrant::TopLeft  as u32;
            }            
            else {
                self.bound.y += self.bound.width;
                self.geohash.bits = (self.geohash.bits << 2) | Quadrant::BottomLeft  as u32;
            }
        }

        Some((self.geohash.clone(), self.bound.clone()))
    }
}

#[derive(PartialEq, Default, Debug, Clone)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn get_rooms(view: Rect) -> Vec<Geohash> {
    let size = view.width.max(view.height);

    for (geohash, top_left_tile) in GeoIterator::from_pos(view.x, view.y) {
        if size > top_left_tile.width { 
            if geohash.is_full() { return vec![geohash]; }
            continue;
        }
        
        let tile_width = ((top_left_tile.x + top_left_tile.width - view.x) / top_left_tile.width).ceil() as usize;
        let tile_height = ((top_left_tile.y + top_left_tile.height - view.y) / top_left_tile.height).ceil() as usize;
        
        let mut res = Vec::with_capacity(tile_width * tile_height);
            
        for _ in 0..tile_height {
    
            let mut row_cursor = geohash.clone();
    
            for _ in 0..tile_width {
                res.push(row_cursor.clone());
                row_cursor.move_right();
            }
    
            row_cursor.move_down();
        }

        return res;
    }

    panic!("could not get rooms for {view:?}")
    
}
