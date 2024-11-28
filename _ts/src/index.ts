import { Server, Socket } from "socket.io";
import { drillWhile, getRooms, Bound, Geohash, geohashToStr } from "./geohash";

const io = new Server(5001, {cors: {origin: "*"}});

io.on("connection", (client: Socket) => {
    console.log("connection");

    client.on("move", (prev: Bound, curr: Bound, timestamps: QuadTree ) => {
        const newRooms = getRooms(curr);
        for (const room of client.rooms) {
            if (!newRooms.includes(room)) client.leave(room);
        }
        client.join(newRooms);

        // for all pois in curr_snapped & not in prev_view:
        //      if timestamps.get(poi.loc) < poi.timestamp: send it
    });

    
    client.on("post", (post: any) => {
        
        drillWhile(post.loc[0], post.loc[1], (geohash: Geohash) => {
            if ((geohash[1] & 0b100000000) != 0) return true;
            
            io.to(geohashToStr(geohash)).emit("new-post", post);

            return true;
        });
    });

});

