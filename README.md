# A Highly Secure Provenance Tracking Filesystem with immutable decentralized traceability

A pluggable NFS that integrates seamlessly into design environments—or any machine, including PCs, IoT devices, and cars. This system provides an immutable, trackable audit that remains tamper-proof, even under a system admin attack.

It leverages blockchain and zero-knowledge proofs (ZKP) to ensure security and transparency. Complementary to Git, it excels in traceability, offering capabilities that go beyond what Git alone can provide.

- A secure filesystem that intrinsically tracks the provenance of files and directories.
- Entirely written in Rust for memory safety and performance.
- Supporting "Secure by Design" approach as articulated by gov [CISA Secure by Design](https://www.cisa.gov/securebydesign) and with reference to aspects of the SSDF by [NIST SP 800-218](https://csrc.nist.gov/publications/detail/sp/800-218/final) extending to principles of "capabilities" and memory safety as per [Cheri](https://cheri-alliance.org)
- Built on an NFS protocol layer and very loosely seeded with [NFSServe](https://github.com/NetApp/nfsserve), promoting ease of integration into existing systems and workflows. We restructured the NFSServe core under our src/kernel.
- Can be utilised in both local and distributed environments, with the ability to scale out to multiple nodes.
- Deployable to Mac and Linux devices (Windows if you must)
- Useful for secure capture and data transmission in the IoT space.
- Useful for secure collaboration and data sharing in sensitive environments.
- Can be used as a secure data store for AI models and data.
- Can be used to underpin many aspects of a "Secure by Design" approach.
- Can be backed by a variety of data stores, including RocksDB, Redis, and we'd like to add Cassandra in the future. We are really interested in the ability to use a distributed database for the backing store.
- For a specific chip design use case with the US Department of Defense, the system was integrated with Eclipse-Theia and Kubernetes to deliver a secure, traceable IDE at scale. This was part of Codasip’s cloud-based RISC-V chip design toolchain.
- Now also tested with vscode server based IDE leveraging Kubernetes resource management from the Theia project.

## Overview

## Building

### Features (determines what is built into the binary)

- Mandatory Backing Store, choose one of [ `rocksdb_store` | `redis_store` ]: Enables RocksDB or Redis as backing store for data shares (one of the two options must be chosen)
- Mandatory irrefutable_audit, choose one of [ `merkle_audit` | `az_audit` ]: Enables irrefutable audit logs for files and directories. Merkle audit writes to a merkle tree in a RocksDB, AZ audit writes to Aleph Zero custom blockchain. Custom blockchain rather than a smart contract based solution leads to lower gas fees, but requires hosting own nodes.
- Optional `compressed_store`: Enables compressed shares (if not specified then works uncompresed with reduced performance but greater traceability

RocksDB is built-in to the filesystem if chosen. If Redis is the store of choice, then it will need to be installed and running on the machine.

### Build and Run Commands

 - `To run or test the filesystem`: 🚀

       cargo build --bin graymamba --features="merkle_audit,compressed_store,rocksdb_store" --release
       cargo run --bin graymamba --features="merkle_audit,compressed_store,rocksdb_store" --release
       cargo test --features merkle_audit -- --nocapture

      --features="merkle_audit,compressed_store,rocksdb_store" is the default feature set
   
 - `To build and run the audit_reader, qrocks, and data-room` (see below for more details on these binaries): 🚀

       cargo run --bin audit_reader --features="merkle_audit" --release (this is only for use with the merkle audit option currently)
       cargo run --bin qrocks --release
       cargo run --bin data-room --release

 - `To run the Linter` : 🚀
   
       cargo clippy --features="merkle_audit,compressed_store"

- `To run bench marking` : 🚀
   
       cargo bench --features="merkle_audit,compressed_store"

- `To enable metrics` : 🚀
   
       cargo run --bin graymamba --features="metrics"
        metrics server runs on localhost:9091, configure the Prometheus server to scrape metrics from this address
      
## Cross compiling for x86_64-unknown-linux-gnu on Silicon Mac
### Brew
```
brew install gcc
brew install SergioBenitez/osxct/x86_64-unknown-linux-gnu
brew install zstd
```
### Compile all the binaries
You will also need to ensure you have the linux target defined: rustup target add x86_64-unknown-linux-gnu
```
export CC_x86_64_unknown_linux_gnu=/opt/homebrew/bin/x86_64-unknown-linux-gnu-gcc
export CXX_x86_64_unknown_linux_gnu=/opt/homebrew/bin/x86_64-unknown-linux-gnu-g++
TARGET_CC=x86_64-unknown-linux-gnu cargo build --features="merkle_audit,compressed_store,rocksdb_store" --release --target x86_64-unknown-linux-gnu
```

### Ensure project level .cargo/config.toml is correct
```
[target.x86_64-unknown-linux-gnu]
linker = "/opt/homebrew/bin/x86_64-unknown-linux-gnu-gcc"
```

### Build the docker image for amd64
This uses the project's Dockerfile and pulls in pre-builtbinaries for the linux/amd64 platform. See cross compiling above.
```
docker buildx build --platform linux/amd64 -t graymamba:v1.0.0 .
```

### Running from linux (debian) containers
As a quick start to test on the mac and connect to the NFS docker container from Finder
It assumes a TESTDATA directory on your mac (the host machine) with config and RocksDBs sub directories.
That way your filesystem data is persistent and can be shared between test runs.
```
docker network create graymamba-network
docker run -d -v /Users/mymac/GRAY/TESTDATA/config:/config -v /Users/mymac/GRAY/TESTDATA/RocksDBs:/RocksDBs  -p 2049:2049 --rm --network graymamba-network --platform linux/amd64 --entrypoint=/bin/bash graymamba:v1.0.1
```

### Running a docker network with a vscode server and the graymamba filesystem mounted
A general docker network runtime config comprises
-  a docker compose script
-  a container running the graymamba NFS with internal merkle audit and rocksdb backing store
-  a container running a client, here a vscode server with the graymamba filesystem mounted

```
version: '3.8'

services:
  graymamba:
    image: graymamba:v1.0.1
    container_name: graymamba
    ports:
      - "2049:2049"  # Expose NFS port
    networks:
      - graymamba-network

  client:
    image: vscodeide:v1.0.0
    depends_on:
      - graymamba
    networks:
      - graymamba-network
    command: >
      sh -c "mount -o nolock graymamba:/dolphin\'s\ drive /mnt && tail -f /dev/null"
networks:
  graymamba-network:
    driver: bridge
```

## Explanation of the project's binaries and their purpose
- `graymamba`: The filesystem itself, which can be mounted as an NFS server. The `main man`.
- `audit_reader`: Reads the audit logs and allows exploration, verification and proof generation.
- `qrocks`: A tool for querying the RocksDB database as there seems not to be one in wide circulation
- `data-room`: An experimental tool for providing a data sandbox for file sharing and collaboration in sensitive environments. An alternate but similar use case to the trackable cloud based vscode server IDE. See above.


## Logging and Tracing

The project uses a sophisticated logging system based on `tracing` and `tracing_subscriber` that provides structured, contextual logging with runtime configuration.

### Configuration
- **Primary**: `config/settings.toml` for base configuration
- **Secondary**: Environment variables for runtime overrides
- **Default Level**: "warn" if not explicitly configured

### Example Configuration

toml
[logging]
level = "info" # Base logging level
module_filter = [
    "graymamba::sharesfs::channel_buffer=debug",
    "graymamba::sharesfs::writing=debug",
    "graymamba::sharesfs::directories=debug",
    "graymamba::sharesfs::rename=debug",
    #"graymamba::kernel=debug",
    "graymamba::kernel::vfs::api=debug",
    #"data_room=debug",
    "graymamba::backingstore::rocksdb_data_store=debug"
    ]

### Features of the build and runtimes (a lot made possible due to Rust magic 😄)
- **Structured Logging**: Maintains context across async boundaries
- **Zero-Cost Abstractions**: Disabled log levels have no runtime overhead
- **Flexible Output**: 
  - Log level
  - Source file and line numbers
  - Compact formatting
  - Writes to stderr for container compatibility
- **Runtime Filtering**: Adjust log levels without recompilation
- **Module-Level Control**: Fine-grained logging for different components

### Usage in Code
rust
// Examples of usage in our code
- tracing::info!("Operation started");
- tracing::debug!("Detailed info: {:?}", data);
- tracing::error!("Error occurred: {}", error);
- modularization of the codebase naturally leads to a much greater granularity of logging system, essentially to see the trees!


### Architecture
Uses the Observer pattern through subscribers and layers:
1. Application code emits events
2. Events pass through configurable filters
3. Subscribers process and format events
4. Output is written to configured destination (stderr)

This approach provides robust logging suitable for both development and production environments, with the flexibility to adapt to different deployment scenarios.

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

                  


### Configure Blockchain Node: (if using Polkadot/Aleph Zero for irrefutable audit rather than the inbuilt merkle audit)
      Set up a Polkadot/Aleph Zero node following below steps
      $ git clone https://github.com/gmawdo/grayscorpion (based on an aleph-zero fork)
      $ cd grayscorpion
      $ cargo build —release
      $ scripts/run_nodes.sh

