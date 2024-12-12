Secure Provenance Tracking Filesystem - README
==============================================

Overview
========

The Secure Provenance Tracking Filesystem is a cutting-edge Network File System (NFS) developed in Rust. The filesystem incorporates advanced features to support the principle of Secure by Design, ensuring a highly secure IDE and provenance tracking. It employs innovative methods such as the Shamir Secret Sharing Algorithm, Merkle Trees, ZNP Git interception feature to deliver a secure, reliable, and traceable IDE based file storage solution.

Features
========

1. Shamir Secret Sharing Algorithm:
    - Uses Shamir Secret Sharing to disassemble file contents into secret shares upon creation.
    - Reassembles the secret shares to reconstruct the file contents when the file is read.
    - Enhances security by ensuring that file contents are only accessible when a threshold number of shares are combined.
    - The protocol of disassembly and reassembly os controlled by blockchain and provides audit hooks that are tamper-proof.
2. Provenance Tracking with Blockchain:
    - Tracks provenance by recording disassembly and reassembly events on Polkadot/Aleph Zero blockchain or similar (e.g. Merkle Tree and append only aidit with ZKPs).
    - Provides tamper-proof records ensuring the integrity and traceability of file operations.
3. A data store for Persistence that must adhere to the following requirements as embodied in a trait:
    - Store files and directories  persistently.
    - Ensure a high availability and fault tolerance.
    - Provide a set of operations that can be used to implement the file system.
    - Currently Redis and RocksDB are the only data stores that adhere to the trait.
4. Git Interception for Fast Cloning:
    - Enhances the cloning of large Git repositories by utilizing Shamir Secret Sharing.
    - Disassembles repository data into secret shares for faster and secure cloning processes.

Installation
============

Prerequisites
=============

 	- Rust (latest stable version)
 	- Polkadot/Aleph Zero node (optional)
  	- Redis Cluster or RocksDB

Steps
=====
1. Clone the Repository:

      `git clone https://github.com/gmawdo/secure-provenance-tracking-filesystem.git`<br>
      `cd secure-provenance-tracking-filesystem`

2. Install Dependencies:

      Ensure you have the necessary dependencies installed. You can use cargo for Rust dependencies.
      - $ `cargo build`

3. Configure Redis Cluster:
      Set up your Redis Cluster with 3 node with ports 6380,6381,6382.
            - Install Redis : 
                  https://redis.io/docs/latest/operate/oss_and_stack/install/install-redis/
		- To create redis cluster somewhere in system
			- $ `mkdir Redis Cluster`
			- $ `cd Redis Cluster`
 			- create three files with below config: redis-6380.conf, redis-6381.conf, redis-6382.conf
``` shell
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
                        requirepass 0rangerY
```
            - In Redis Cluster folder: 
                  - $ `mkdir Redis_database`
                  - $ `cd Redis_database`
                  - $ `mkdir redis-6380`
                  - $ `mkdir redis-6381`
                  - $ `mkdir redis-6382`
            - To Run Redis Cluster run below commands in 4 different terminals at Redis Cluster folder.
                  $ `redis-server redis-6380.conf`
                  $ `redis-server redis-6381.conf`
                  $ `redis-server redis-6382.conf`
                  $ `redis-cli --cluster create 127.0.0.1:6380 127.0.0.1:6381 127.0.0.1:6382 --cluster-replicas 0 -a 0rangerY --cluster-yes`
                  


4. Configure Blockchain Node:
      Set up your Polkadot/Aleph Zero node following below steps
            $ `git clone https://github.com/datasignals/aleph-node-pinkscorpion.git`
		$ `cd aleph-node-pinkscorpion`
 		$ `cargo build —release`
		$ `scripts/run_nodes.sh`

5. Now to Build the filesystem open terminal at secure-provenance-filesystem folder and run one of (note iirefutable_audit feature compiles in blockchain tech integration):

       `cargo build --bin graymamba --features="traceability" --release`
       `cargo build --bin graymamba --features="traceability,irrefutable_audit" --release`
       `cargo test --features irrefutable_audit -- --nocapture`
   
       `cargo build --bin audit_reader --features="traceability,irrefutable_audit" --release`
       `cargo build --bin qrocks --features="traceability,irrefutable_audit" --release`
   
       `cargo clippy --features="traceability,irrefutable_audit"`
   
       `cargo run --bin graymamba --features=traceability,irrefutable_audit`
       `cargo run --bin audit_reader --features=traceability,irrefutable_audit`

When the blockchain is compiled in and running then you will see the following type of output on the console when a file is created or read:
---------------
............................
Preparing to send event...
Disassembled call submitted, waiting for transaction to be finalized...
Disassembled event processed.
............................

Testing and performance commands
---------------

1. Linter:

      `cargo clippy`

2. Benchmarking:

      `cargo bench`

Docker Commands
---------------

1. Build the Docker Image:

      `sudo docker build -t graymamba_image .`

2. Run the Docker Image on a docker container:

      `sudo docker run -d --name graymamba_container --privileged --network host graymamba_image`


Usage
=====

Basic Commands
--------------

 - Create a File:<br>
 To create a file, use the standard NFS commands. The filesystem will automatically disassemble the file contents into secret shares.

- Read a File:<br>
To read a file, use the standard NFS commands. The filesystem will reassemble the secret shares to reconstruct the file contents.

Provenance Tracking
-------------------

 - Event Tracking:<br>
   Disassembly and reassembly events are automatically sent to the Polkadot/Aleph Zero blockchain.
    - Use blockchain explorer tools to view the provenance records.

Git Interception
----------------

 - Fast Cloning:  
   The filesystem intercepts Git clone operations to disassemble repository data into secret shares, enhancing the cloning speed and security. Despite continuous disassembly and reassembly, the Git interception feature ensures efficient cloning of git repositories.
    - Clone a repository as usual with Git. The filesystem will handle the disassembly and reassembly processes in the background.

Security
--------

 - Data Security:<br>
   File contents are secured using Shamir Secret Sharing, ensuring that data is split into multiple shares and requires a threshold number of shares to reconstruct.
 - Integrity and Traceability:<br>
   Provenance tracking using blockchain ensures that all file operations are recorded in a tamper-proof manner.
