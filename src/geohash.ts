const MAX = 180;

/** `[bits, mask]` */
export type Geohash = [number, number];

export type Bound = { top: number, bottom: number, left: number, right: number };

// a b
// c d


// 01 11
// 00 10
// xy
enum Quadrant { TOP_LEFT = 0b01, TOP_RIGHT = 0b11, BOTTOM_LEFT = 0b00, BOTTOM_RIGHT = 0b10 };


function flipLastH(q: Quadrant) { return q ^ 0b10; } // swap the second last bit
function flipLastV(q: Quadrant) { return q ^ 0b01; } // swap the last bit

function endsOnRight(q: Quadrant) { return (q >> 1) == 1; } // second last bit is 1
function endsOnBottom(q: Quadrant) { return (q & 1) == 0; } // last bit is 0

/** starting from the 2nd last bit, every other bit is 1: `_1_1_1_` */
function allRight(geohash: Geohash) {
    let cursor = 0b10;

    while ((geohash[1] & cursor) != 0) {
        if ((geohash[0] & cursor) == 0) return false;
        cursor = cursor << 2;
    }

    return true;
}
function allBottom(geohash: number) { // if last bit is 0, every other bit is 0: 010101010
    while (geohash != 0) {
        if ((geohash & 1) == 1) return false;
        geohash = geohash >> 2;
    }
    return true;
}

// function getAdjRight(geohash: Geohash): Geohash {

//     if (!endsOnRight(geohash)) return flipLastH(geohash);

//     if (allRight(geohash)) { // flip every x bit 
//         let cursor = 0b10;

//         while (cursor < geohash) {
//             geohash = geohash ^ cursor;
//             cursor << 2;
//         }

//         return geohash;
//     }

//     return geohash ^ 0b1010; // flip last 2 bits
// }

function geohashToStr(geohash: Geohash) {
    let mask = geohash[1];
    let ones = 0;
    while (mask != 0) {
        mask = mask >> 1;
        ones++;
    }
    return ((geohash[0] & geohash[1]) >>> 0).toString(2).padStart(ones, '0');
}
const geohash = drillWhile(0, 0, (geohash, bound) => {
    console.log(geohashToStr(geohash));
    console.log(bound);
    return true;
})[0];
// console.log(getAdjRight(0b0_01_01).toString(2).padStart(10, '0'));

export function getRooms(view: Bound): string[] {
    const size = Math.max(view.top - view.bottom, view.right - view.left);
    
    const [geohash, top_left_tile] = drillWhile(view.top, view.left, (_, bound: Bound) => size > bound.top - bound.bottom);
    const bound_size = top_left_tile.right - top_left_tile.left;

    const tile_width = Math.ceil((view.right - top_left_tile.left) / bound_size);
    const tile_height = Math.ceil((top_left_tile.top - view.bottom) / bound_size);
    

    return [];
}

export function drillWhile(x: number, y: number, cb: (geohash: Geohash, bound: Bound) => boolean): [Geohash, Bound] {
    const bound = {top: MAX, bottom: -MAX, left: -MAX, right: MAX};

    let geohash: Geohash = [0, 0];

    for (let i = 0; i < 16; i++) {
        const mid_x = (bound.left + bound.right) / 2;
        const mid_y = (bound.top + bound.bottom) / 2;

        if (x > mid_x) {
            bound.left = mid_x;
            if (y > mid_y) {
                bound.bottom = mid_y;
                geohash[0] = (geohash[0] << 2) | Quadrant.TOP_RIGHT;
            }
            else {
                bound.top = mid_y;
                geohash[0] = (geohash[0] << 2) | Quadrant.BOTTOM_RIGHT;
            }
        }
        else {
            bound.right = mid_x;
            if (y > mid_y) {
                bound.bottom = mid_y;
                geohash[0] = (geohash[0] << 2) | Quadrant.TOP_LEFT;
            }            
            else {
                bound.top = mid_y;
                geohash[0] = (geohash[0] << 2) | Quadrant.BOTTOM_LEFT;
            }
        }

        geohash[1] = (geohash[1] << 2) | 0b11;

        if (!cb(geohash, bound)) break;
    }

    return [geohash, bound];
}