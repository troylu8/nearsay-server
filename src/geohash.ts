const MAX = 180;


export type Bound = { top: number, bottom: number, left: number, right: number };

// a b
// c d

enum Quadrant { TOP_LEFT = 'a', TOP_RIGHT = 'b', BOTTOM_LEFT = 'c', BOTTOM_RIGHT = 'd' };


export function getRooms(view: Bound): string[] {
    const size = Math.max(view.top - view.bottom, view.right - view.left);
    
    const [geohash, bound] = drillWhile(view.top, view.left, (_, bound: Bound) => size > bound.top - bound.bottom);

    

    return [];
}

export function drillWhile(x: number, y: number, cb: (geohash: string, bound: Bound) => boolean): [string, Bound] {
    const bound = {top: MAX, bottom: -MAX, left: -MAX, right: MAX}

    let geohash: string = "";

    for (let i = 0; i < 16; i++) {
        const mid_x = (bound.left + bound.right) / 2;
        const mid_y = (bound.top + bound.bottom) / 2;

        if (x > mid_x) {
            bound.left = mid_x;
            if (y > mid_y) {
                bound.bottom = mid_y;
                geohash += Quadrant.TOP_RIGHT;
            }            
            else {
                bound.top = mid_y;
                geohash += Quadrant.BOTTOM_RIGHT;
            }
        }
        else {
            bound.right = mid_x;
            if (y > mid_y) {
                bound.bottom = mid_y;
                geohash += Quadrant.TOP_LEFT;
            }            
            else {
                bound.top = mid_y;
                geohash += Quadrant.BOTTOM_LEFT;
            }
        }

        if (!cb(geohash, bound)) break;
    }

    return [geohash, bound];
}