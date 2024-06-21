Secure Provenance Tracking Filesystem - README
==============================================

Overview
========

The Secure Provenance Tracking Filesystem is a cutting-edge Network File System (NFS) developed in Rust. The filesystem incorporates advanced features to ensure security and provenance tracking. It employs innovative methods such as the Shamir Secret Sharing Algorithm, blockchain (Polkadot/Aleph Zero), Redis Cluster, and a Git interception feature to deliver a secure, reliable, and traceable file storage solution.

Features
========

1. Shamir Secret Sharing Algorithm:
 - Uses Shamir Secret Sharing to disassemble file contents into secret shares upon creation.
 - Reassembles the secret shares to reconstruct the file contents when the file is read.
 - Enhances security by ensuring that file contents are only accessible when a threshold number of shares are combined.
2. Provenance Tracking with Blockchain:
 - Tracks provenance by recording disassembly and reassembly events on Polkadot/Aleph Zero blockchain.
 - Provides tamper-proof records ensuring the integrity and traceability of file operations.
3. Redis Cluster for Persistence:
 - Utilizes Redis Cluster to store files and directories  persistently.
 - Ensures high availability and fault tolerance.
4. Git Interception for Fast Cloning:
 - Enhances the cloning of large Git repositories by utilizing Shamir Secret Sharing.
 - Disassembles repository data into secret shares for faster and secure cloning processes.

Installation
============

Prerequisites
=============
 	- Rust (latest stable version)
 	- Redis Cluster
 	- Kafka Broker
 	- Polkadot/Aleph Zero node

Steps
=====
1. Clone the Repository:

      `git clone https://github.com/datasignals/secure-provenance-tracking-filesystem.git`<br>
      `cd secure-provenance-tracking-filesystem`

2. Install Dependencies:

      Ensure you have the necessary dependencies installed. You can use cargo for Rust dependencies.

3. Configure Redis Cluster:

      Set up your Redis Cluster and update the configuration in the filesystem code as required.

4. Configure Blockchain Node:

      Set up your Polkadot/Aleph Zero node and update the configuration in the filesystem code as required.

5. Build the filesystem using:

      `cargo build --bin lockular_nfs --features="demo" --release`

6. Run the FileSystem:

      `mkdir /mnt/nfs`<br>
      `./target/release/lockular_nfs /mnt/nfs`

7. Mount the FileSystem:

      `mkdir mount_point`<br>
      `mount_nfs -o nolocks,vers=3,tcp,rsize=131072,actimeo=120,port=2049,mountport=2049 localhost:/  / mount_point`

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
   The filesystem intercepts Git clone operations to disassemble repository data into secret shares, enhancing the cloning speed and security.
    - Clone a large repository as usual with Git. The filesystem will handle the disassembly and reassembly processes in the background.

Security
--------

 - Data Security:<br>
   File contents are secured using Shamir Secret Sharing, ensuring that data is split into multiple shares and requires a threshold number of shares to reconstruct.
 - Integrity and Traceability:<br>
   Provenance tracking using blockchain ensures that all file operations are recorded in a tamper-proof manner.

Contact
-------

For any questions or support, please open an issue on GitHub or contact us at https://www.lockular.com/contact-us.html.

Thank you for using Secure Provenance Tracking Filesystem. We are committed to providing a secure and reliable filesystem solution.
