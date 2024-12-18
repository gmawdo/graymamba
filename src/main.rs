use serde_xdr::{to_bytes, from_bytes};
use tokio::net::TcpStream;
use std::error::Error;
use tokio::io::{AsyncWriteExt, AsyncReadExt};

use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor, Read};
use anyhow::Result;

// Define the RPC MOUNT request
#[derive(Debug, serde::Serialize)]
struct MountRequest {
    size: u32,
    xid: u32,             // Transaction ID
    message_type: u32,    // 0 = CALL
    rpc_version: u32,     // Must be 2
    program: u32,         // MOUNT program number (100005)
    version: u32,         // MOUNT protocol version (3)
    procedure: u32,       // Procedure 1 = MNT
    credentials: (u32, u32), // Null auth
    verifier: (u32, u32),    // Null verifier
    path: String,         // Path to mount
}

#[derive(Debug, serde::Deserialize)]
struct MountResponse {
    xid: u32,
    message_type: u32,
    reply_state: u32,
    verifier: (u32, u32),
    accept_state: u32,
    status: u32,
    file_handle: [u8; 16],  // Changed from Vec<u8> to fixed-size array of 16 bytes
    auth_flavors: Vec<u32>, // Added auth_flavors field
}

// Add this implementation for MountResponse
impl MountResponse {
    fn from_be_bytes(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let mut cursor = Cursor::new(bytes);
        
        Ok(MountResponse {
            xid: ReadBytesExt::read_u32::<BigEndian>(&mut cursor)?,
            message_type: ReadBytesExt::read_u32::<BigEndian>(&mut cursor)?,
            reply_state: ReadBytesExt::read_u32::<BigEndian>(&mut cursor)?,
            verifier: (
                ReadBytesExt::read_u32::<BigEndian>(&mut cursor)?,
                ReadBytesExt::read_u32::<BigEndian>(&mut cursor)?,
            ),
            accept_state: ReadBytesExt::read_u32::<BigEndian>(&mut cursor)?,
            status: ReadBytesExt::read_u32::<BigEndian>(&mut cursor)?,
            file_handle: {
                let mut fh = [0u8; 16];
                std::io::Read::read_exact(&mut cursor, &mut fh)?;
                fh
            },
            auth_flavors: {
                let count = ReadBytesExt::read_u32::<BigEndian>(&mut cursor)?;
                let mut flavors = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    flavors.push(ReadBytesExt::read_u32::<BigEndian>(&mut cursor)?);
                }
                flavors
            },
        })
    }
}

// Define the NULL procedure request (NFSv3 Procedure 0)
#[derive(Debug, serde::Serialize)]
struct NullRequest {
    size: u32,
    xid: u32,
    message_type: u32,  // 0 = CALL
    rpc_version: u32,   // RPC version 2
    program: u32,       // NFS program number (100003)
    version: u32,       // NFS version 3
    procedure: u32,     // NULL procedure = 0
    credentials: (u32, u32),
    verifier: (u32, u32),
}

// Define a generic response structure
#[derive(Debug, serde::Deserialize)]
struct RpcReply {
    xid: u32,
    message_type: u32, // 1 = REPLY
    reply_state: u32,  // 0 = MSG_ACCEPTED
    verifier: (u32, u32),
    accept_state: u32, // 0 = SUCCESS
}

fn encode_last_flag(length: u32, is_last: bool) -> u32 {
    if is_last {
        length | 0x80000000 // Set the highest bit (Bit 31) if `is_last` is true
    } else {
        length & 0x7FFFFFFF // Ensure the highest bit is cleared
    }
}

pub async fn read_rpc_response<T>(
    socket: &mut TcpStream,
    expected_size: Option<usize>,
) -> Result<T, anyhow::Error>
where
    T: for<'de> serde::Deserialize<'de>, // Deserialize the response into any type
{
    let mut buf = Vec::new();
    let mut fragment_count = 0;

    loop {
        // Read the fragment header (4 bytes for length and last fragment flag)
        let mut header_buf = [0_u8; 4];
        socket.read_exact(&mut header_buf).await?;
        let fragment_header = u32::from_be_bytes(header_buf);
        let is_last = (fragment_header & (1 << 31)) > 0;
        let fragment_size = (fragment_header & ((1 << 31) - 1)) as usize;
        println!("Fragment header: {:?}", fragment_header);
        println!("Is last: {:?}", is_last);
        println!("Fragment size: {:?}", fragment_size);

        // Read the fragment data
        let mut fragment = vec![0u8; fragment_size];
        socket.read_exact(&mut fragment).await?;

        // Append fragment data to the buffer
        buf.extend_from_slice(&fragment);

        // If it's the last fragment, break the loop
        if is_last {
            break;
        }

        fragment_count += 1;
        // If you know the expected size, you can break early if too many fragments are read.
        if let Some(size) = expected_size {
            if buf.len() >= size {
                break;
            }
        }
    }
    println!("Buffer length: {:?}", buf.len());
    // Deserialize the combined response data
    let cursor = Cursor::new(buf);
    println!("Cursor position: {:?}", cursor.position());
    let response: T = bincode::deserialize_from(cursor)?;
    
    Ok(response)
}

async fn run_nfs_client(mut stream: TcpStream, file_handle: [u8; 16]) -> Result<(), Box<dyn Error>> {
    println!("NFS client running with file handle: {:?}", file_handle);
    
    // Create a channel for handling shutdown signals
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel(1);
    
    // Set up Ctrl-C handler
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            println!("\nReceived Ctrl-C, shutting down...");
            let _ = shutdown_tx_clone.send(()).await;
        }
    });

    // Main client loop
    loop {
        tokio::select! {
            // Check for shutdown signal
            _ = shutdown_rx.recv() => {
                println!("Shutting down NFS client...");
                break;
            }
            
            // Handle NFS operations
            result = handle_nfs_operations(&mut stream, &file_handle) => {
                match result {
                    Ok(_) => continue,
                    Err(e) => {
                        eprintln!("Error in NFS operation: {}", e);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_nfs_operations(stream: &mut TcpStream, file_handle: &[u8; 16]) -> Result<(), Box<dyn Error>> {
    // Here you would implement various NFS operations like:
    // - GETATTR (get file attributes)
    // - LOOKUP (look up file names)
    // - READ (read file contents)
    // - WRITE (write to files)
    // - READDIR (read directory contents)
    // etc.
    
    // For example, you might want to periodically check attributes:
    // send_getattr_request(stream, file_handle).await?;
    
    // Sleep to prevent tight loop
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mount_addr = "127.0.0.1:2049";
    let mut stream = TcpStream::connect(mount_addr).await?;
    println!("Connected to NFS MOUNT service on {}", mount_addr);

    // Step 1: Send MOUNT Request
    let req_len = 64;
    let req_len_with_last_flag = encode_last_flag(req_len, true);

    let mount_request = MountRequest {
        size: req_len_with_last_flag,
        xid: 0x12345678,
        message_type: 0,   // CALL
        rpc_version: 2,
        program: 100005,   // MOUNT program
        version: 3,        // MOUNT protocol version
        procedure: 1,      // MNT procedure
        credentials: (0, 0),
        verifier: (0, 0),
        path: "/joseph's drive".to_string(), // Directory to mount
    };

    let request_data = to_bytes(&mount_request)?;
    println!("First four bytes of serialized request: {:?}", &request_data[0..4]);
    println!("Size of the serialized request: {}", request_data.len());
    println!("Sending MOUNT request...");
    stream.write_all(&request_data).await?;
    stream.flush().await?;

    // Step 2: Send NFS NULL Procedure
    let null_request = NullRequest {
        size: req_len_with_last_flag,
        xid: 0x87654321,  // A different transaction ID
        message_type: 0,  // CALL
        rpc_version: 2,
        program: 100003,  // NFS program
        version: 3,       // NFS version 3
        procedure: 0,     // NULL procedure
        credentials: (0, 0),
        verifier: (0, 0),
    };

    let null_data = to_bytes(&null_request)?;
    println!("Sending NFS NULL procedure...");
    stream.write_all(&null_data).await?;
    stream.flush().await?;

    // Step 3: Receive Response
    let mut size_buffer = [0u8; 4];
    stream.read_exact(&mut size_buffer).await?;
    let response_size = u32::from_be_bytes(size_buffer) & 0x7FFFFFFF; // Remove last fragment bit
    
    let mut buffer = vec![0u8; response_size as usize];

    stream.read_exact(&mut buffer).await?;
    println!("Received response from NFS server! Size: {}", response_size);

    // Debug: Log raw response
    println!("Raw response: {:?}", &buffer);

    // Deserialize the response
    let mount_reply = MountResponse::from_be_bytes(&buffer);

    match mount_reply {
        Ok(reply) => {
            if reply.reply_state == 0 && reply.accept_state == 0 && reply.status == 0 {
                println!("Mount successful!");
                println!("File handle: {:?}", reply.file_handle);
                
                // Start the client loop with the obtained file handle
                run_nfs_client(stream, reply.file_handle).await?;
            } else {
                println!("Mount failed with status: {}", reply.status);
            }
        }
        Err(e) => {
            println!("Failed to parse response: {:?}", e);
        }
    }

    Ok(())
}