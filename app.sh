#!/bin/bash

handle_exit() {
    echo "shutting down mongod"
    mongod --dbpath /nearsay_volume/db --shutdown

    echo "shutting down redis instances"
    redis-cli -p 6000 shutdown
    redis-cli -p 6001 shutdown

    echo "exiting!"
    exit 0
}
trap handle_exit EXIT

echo "starting mongod"
mkdir -p /nearsay_volume/db
mongod --dbpath /nearsay_volume/db --quiet --logpath /app/mongod.log --logappend --fork

echo "starting redis instances"
redis-server --port 6000 --daemonize yes --save "" --appendonly no
redis-server --port 6001 --daemonize yes --save "" --appendonly no

echo "starting server"
/app/target/release/nearsay-server