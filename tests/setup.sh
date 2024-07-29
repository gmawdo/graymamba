#!/bin/bash

docker-compose up -d

docker exec redis-cluster-entry /bin/sh -c "echo yes > in.txt && /data/redis-trib.rb create --password myredis --replicas 1 127.0.0.1:8000 127.0.0.1:8001 127.0.0.1:8002 127.0.0.1:8003 127.0.0.1:8004 127.0.0.1:8005 < in.txt"

echo "Redis cluster started, try: \"redis-cli -c -p 8000 -a myredis\""
