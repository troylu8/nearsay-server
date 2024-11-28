import { io } from "socket.io-client";
import { drillWhile, Geohash, Bound, getRooms } from "./geohash";

const socket = io("http://localhost:5000");

type QTreeNode = { 
    time: number,
    nw: QTreeNode | null,
    ne: QTreeNode | null,
    sw: QTreeNode | null,
    se: QTreeNode | null,
};

function subTree(rect: Rect) {
    const size = Math.max(rect.width, rect.height);

    const [geohash, bound] = drillWhile(rect.x, rect.y, (_, bound: Bound) => rect.x + rect.width < bound.right && rect.y + rect.height < bound.bottom);
    

}

let timestamps: QTreeNode = {
    time: 10,
    nw: null,
    ne: null,
    sw: null,
    se: null
};

function onMove(prev: Rect, curr: Rect) {
    
    socket.emit("move", prev, curr, timestamps.rooted(curr));
    timestamps.update(getRooms(curr), Date.now());
}

type Rect = { x: number, y: number, width: number, height: number };

