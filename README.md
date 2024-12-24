# A Highly Secure Provenance Tracking Filesystem

A secure filesystem that tracks the provenance of files and directories. Entirely written in Rust for memory safety and performance.
Built with an NFS protocol layer loosely based on NFSServe, promoting ease of integration into existing systems and workflows.

Can be utilised in both local and distributed environments, with the ability to scale out to multiple nodes. Also deployable to small Linux based IoT devices for secure capture and data transmission.

## Overview

## Building

### Features (determines what is built into the binary)

- Mandatory Backing Store, choose one of [ `rocksdb` | `redis` ]: Enables RocksDB or Redis as backing store for data shares (one of the two options must be chosen)
- Optional `irrefutable_audit`: Enables irrefutable audit logs for files and directories. (if not specified then no audit logs are created)
- Optional `compressed_store`: Enables compressed shares (if not specified then works uncompresed with reduced performance but greater traceability

RocksDB is built-in to the filesystem if chosen. If Redis is the store of choice, then it will need to be installed and running on the machine.

### Build and Run Commands

 - `To run or test the filesystem`: 🚀

       cargo build --bin graymamba --features="irrefutable_audit,compressed_store,rocksdb" --release
       cargo run --bin graymamba --features="irrefutable_audit,compressed_store,rocksdb" --release
       cargo test --features irrefutable_audit -- --nocapture
   
 - `To build and run the audit_reader, qrocks, and data-room` (see below for more details on these binaries): 🚀

       cargo run --bin audit_reader --features="irrefutable_audit" --release
       cargo run --bin qrocks --features="irrefutable_audit" --release
       cargo run --bin data-room --features="irrefutable_audit" --release

 - `To run the Linter` : 🚀
   
       cargo clippy --features="irrefutable_audit,compressed_store"

- `To run bench marking` : 🚀
   
       cargo bench --features="irrefutable_audit,compressed_store"


## Explanation of the project's binaries and their purpose
- `graymamba`: The filesystem itself, which can be mounted as an NFS server. The `main man`.
- `audit_reader`: Reads the audit logs and allows exploration, verification and proof generation.
- `qrocks`: A tool for querying the RocksDB database as there seems not to be one in wide circulation
- `data-room`: An experimental tool for providing a data sandbox for file sharing and collaboration in sensitive environments.

## Useful references

### Configure a Redis Cluster:
      Set up a Redis Cluster with 3 node with ports 6380,6381,6382.
       - Install Redis : https://redis.io/docs/latest/operate/oss_and_stack/install/install-redis/

 	- This will require a config file for each node: e.g. redis-6380.conf, redis-6381.conf, redis-6382.conf of the format:

                        # Change port 
                        port 6380 
                        #Expose the port
                        bind 0.0.0.0
                        #Mode
                        protected-mode no
                        # Data directory location
                        dir ./Redis_database/redis-6380
                        # Enable clustering  
                        cluster-enabled yes
                        # Set Password
                        requirepass password
                                     
                                                  
             redis-cli --cluster create 127.0.0.1:6380 127.0.0.1:6381 127.0.0.1:6382 --cluster-replicas 0 -a password --cluster-yes

             nohup redis-server redis-6380.conf > redis0.log 2>&1 &
             nohup redis-server redis-6381.conf > redis1.log 2>&1 &
             nohup redis-server redis-6382.conf > redis2.log 2>&1 &

                  


### Configure Blockchain Node: (if using Polkadot/Aleph Zero for irrfutable audit)
      Set up a Polkadot/Aleph Zero node following below steps
      $ git clone https://github.com/datasignals/aleph-node-pinkscorpion.git (based on aleph-zero fork)
      $ cd aleph-node-pinkscorpion
      $ cargo build —release
      $ scripts/run_nodes.sh

