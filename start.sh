#!/usr/bin/env bash

handle_exit() {
    
    echo "shutting down redis instances"
    
    # stop redis instances
    redis-cli -p 6000 shutdown
    redis-cli -p 6001 shutdown
    
    exit 0 
}
trap handle_exit EXIT


# start redis instances
redis-server --port 6000 --daemonize yes --save "" --appendonly no
redis-server --port 6001 --daemonize yes --save "" --appendonly no

# start program
# cargo run
read -p "running"
