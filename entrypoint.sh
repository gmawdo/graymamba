#!/bin/sh

# Start Redis servers
#redis-server /app/redis/redis-6380.conf &
#redis-server /app/redis/redis-6381.conf &
#redis-server /app/redis/redis-6382.conf &

# Wait for Redis servers to start
#sleep 5

# Create the Redis cluster
#yes | redis-cli --cluster create 127.0.0.1:6380 127.0.0.1:6381 127.0.0.1:6382 --cluster-replicas 0 --cluster-yes
#yes | redis-cli --cluster create 127.0.0.1:6380 127.0.0.1:6381 127.0.0.1:6382 --cluster-replicas 0 -a 0rangerY --cluster-yes


# Edit the /etc/exports file to configure the NFS exports and add an entry to allow access from the client IP or all IPs
echo "/mnt/nfs *(rw,sync,no_subtree_check)" >> /etc/exports

# Start the NFS server
/usr/local/bin/lockular_nfs /mnt/nfs &

# Wait for the NFS server to start
sleep 5


# Mount the NFS filesystem
mount -t nfs -o nolocks,tcp,rsize=131072,actimeo=120,port=2049,mountport=2049 localhost:/ ../mount_point
# mount -t nfs -o tcp,rsize=131072,port=2049,mountport=2049 localhost:/ /mount_point

# Check if the mount was successful
if mountpoint -q /mount_point; then
    echo "NFS filesystem mounted successfully."

    # Perform basic functionality tests
    echo "Running basic functionality tests..."

    # Test 1: Create a directory
    mkdir -p /mount_point/test_dir && echo "Directory creation test: PASSED" || echo "Directory creation test: FAILED"

    # Test 2: Create a file
    echo "Hello, NFS!" > /mount_point/test_dir/test_file && echo "File creation test: PASSED" || echo "File creation test: FAILED"

    # Test 3: Read the file
    cat /mount_point/test_dir/test_file && echo "File read test: PASSED" || echo "File read test: FAILED"

    # Test 4: Delete the file
    rm /mount_point/test_dir/test_file && echo "File deletion test: PASSED" || echo "File deletion test: FAILED"

    # Test 5: Delete the directory
    rmdir /mount_point/test_dir && echo "Directory deletion test: PASSED" || echo "Directory deletion test: FAILED"

    else
        echo "Failed to mount NFS filesystem."
    fi

# Keep the container running
tail -f /dev/null
