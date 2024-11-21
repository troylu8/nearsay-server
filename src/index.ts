import { Server, Socket } from "socket.io";
import { drillWhile, getRooms, Bound } from "./geohash";

const io = new Server(5001, {cors: {origin: "*"}});

// io.on("connection", (client: Socket) => {
//     console.log("connection");

//     client.on("move", (prev: Bound, curr: Bound, timestamps: {[key: string]: number} ) => {
//         const newRooms = getRooms(curr);
//         for (const room of client.rooms) {
//             if (!newRooms.includes(room)) client.leave(room);
//         }
//         client.join(newRooms);

//         // for all pois in curr & not in prev:
//         //      if timestamps[poi] < poi.timestamp: send it
//     });

    
//     client.on("post", (post: any) => {
        
//         drillWhile(post.loc[0], post.loc[1], (geohash: string) => {
//             if (geohash.length < 3) return true;
            
//             io.to(geohash).emit("new-post", post);

//             return true;
//         });
//     });

// });

