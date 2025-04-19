#!/usr/bin/env bash

handle_exit() {
    
    echo "shutting down mongod"
    mongod --dbpath db --shutdown
    
    echo "shutting down redis instances"
    redis-cli -p 6000 shutdown
    redis-cli -p 6001 shutdown
    
    echo "exiting!"
    exit 0 
}
trap handle_exit EXIT

echo "starting mongod"
mongod --dbpath db --quiet --logpath mongod.log --logappend --fork

echo "starting redis instances"
redis-server --port 6000 --daemonize yes --save "" --appendonly no
redis-server --port 6001 --daemonize yes --save "" --appendonly no

echo "starting server"
read -p "running"
# ./nearsay-server.exe