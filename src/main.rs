use serde_xdr::{to_bytes, from_bytes};
use tokio::net::TcpStream;
use std::error::Error;
use tokio::io::{AsyncWriteExt, AsyncReadExt};

use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;
use anyhow::Result;
use tokio::sync::mpsc;
use std::str::FromStr;

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

async fn mount_nfs(stream: &mut TcpStream, mount_path: &str) -> Result<MountResponse, Box<dyn Error>> {
    let req_len = 64;
    let req_len_with_last_flag = encode_last_flag(req_len, true);

    // Create and send MOUNT request
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
        path: mount_path.to_string(),
    };

    let request_data = to_bytes(&mount_request)?;
    println!("Sending MOUNT request...");
    stream.write_all(&request_data).await?;
    stream.flush().await?;

    // Send NULL procedure
    let null_request = NullRequest {
        size: req_len_with_last_flag,
        xid: 0x87654321,
        message_type: 0,
        rpc_version: 2,
        program: 100003,
        version: 3,
        procedure: 0,
        credentials: (0, 0),
        verifier: (0, 0),
    };

    let null_data = to_bytes(&null_request)?;
    println!("Sending NFS NULL procedure...");
    stream.write_all(&null_data).await?;
    stream.flush().await?;

    // Receive response
    let mut size_buffer = [0u8; 4];
    stream.read_exact(&mut size_buffer).await?;
    let response_size = u32::from_be_bytes(size_buffer) & 0x7FFFFFFF;
    
    let mut buffer = vec![0u8; response_size as usize];
    stream.read_exact(&mut buffer).await?;
    println!("Received response from NFS server! Size: {}", response_size);

    let mount_reply = MountResponse::from_be_bytes(&buffer)?;
    println!("Mount successful! File handle received: {:02x?}", mount_reply.file_handle);
    Ok(mount_reply)
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

// Define available commands
#[derive(Debug)]
enum NfsCommand {
    ListDir,
    GetAttr,
    ReadFile(String),
    Quit,
}

// Implement parsing for commands
impl FromStr for NfsCommand {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.trim().split_whitespace().collect();
        if parts.is_empty() {
            return Err("Empty command".to_string());
        }
        
        match parts[0].to_lowercase().as_str() {
            "ls" | "dir" => Ok(NfsCommand::ListDir),
            "attr" => Ok(NfsCommand::GetAttr),
            "read" if parts.len() > 1 => Ok(NfsCommand::ReadFile(parts[1].to_string())),
            "quit" | "exit" => Ok(NfsCommand::Quit),
            _ => Err(format!("Unknown command: {}", parts[0])),
        }
    }
}

// Function to handle user input in a separate task
async fn handle_user_input(command_tx: mpsc::Sender<NfsCommand>) {
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut buffer = String::new();

    loop {
        buffer.clear();
        print!("> ");
        if let Ok(_) = tokio::io::AsyncWriteExt::flush(&mut tokio::io::stdout()).await {
            if let Ok(_) = tokio::io::AsyncBufReadExt::read_line(&mut stdin, &mut buffer).await {
                match NfsCommand::from_str(&buffer) {
                    Ok(cmd) => {
                        if let NfsCommand::Quit = cmd {
                            let _ = command_tx.send(cmd).await;
                            break;
                        }
                        if let Err(e) = command_tx.send(cmd).await {
                            eprintln!("Failed to send command: {}", e);
                            break;
                        }
                    }
                    Err(e) => eprintln!("Invalid command: {}", e),
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mount_addr = "127.0.0.1:2049";
    let mut stream = TcpStream::connect(mount_addr).await?;
    println!("Connected to NFS MOUNT service on {}", mount_addr);

    let mount_reply = mount_nfs(&mut stream, "/joseph's drive").await?;
    
    if mount_reply.reply_state == 0 && mount_reply.accept_state == 0 && mount_reply.status == 0 {
        println!("Mount successful!");
        println!("Using file handle: {:02x?}", mount_reply.file_handle);
        
        // Now use this exact file handle for GETATTR
        initialize_nfs(&mut stream, &mount_reply.file_handle).await?;
        
        // Channel for shutdown signals
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        
        // Channel for user commands
        let (command_tx, mut command_rx) = mpsc::channel(32);
        
        // Set up Ctrl-C handler
        let shutdown_tx_clone = shutdown_tx.clone();
        tokio::spawn(async move {
            if let Ok(()) = tokio::signal::ctrl_c().await {
                println!("\nReceived Ctrl-C, shutting down...");
                let _ = shutdown_tx_clone.send(()).await;
            }
        });

        // Spawn user input handler
        let command_tx_clone = command_tx.clone();
        tokio::spawn(handle_user_input(command_tx_clone));

        println!("NFS Client ready. Available commands:");
        println!("  ls/dir        - List directory");
        println!("  attr          - Get attributes");
        println!("  read <file>   - Read file");
        println!("  quit/exit     - Exit client");
        
        // Main client loop
        loop {
            tokio::select! {
                // Check for shutdown signal
                _ = shutdown_rx.recv() => {
                    println!("Shutting down NFS client...");
                    break;
                }
                
                // Check for user commands
                Some(cmd) = command_rx.recv() => {
                    match cmd {
                        NfsCommand::ListDir => {
                            println!("Listing directory...");
                            match list_directory(&mut stream, &mount_reply.file_handle).await {
                                Ok(_) => println!("Directory listing complete"),
                                Err(e) => eprintln!("Error listing directory: {}", e),
                            }
                        },
                        NfsCommand::GetAttr => {
                            println!("Getting attributes...");
                            match get_attributes(&mut stream, &mount_reply.file_handle).await {
                                Ok(_) => println!("Got attributes"),
                                Err(e) => eprintln!("Error getting attributes: {}", e),
                            }
                        },
                        NfsCommand::ReadFile(path) => {
                            println!("Reading file: {}", path);
                            match read_file(&mut stream, &mount_reply.file_handle, &path).await {
                                Ok(_) => println!("File read complete"),
                                Err(e) => eprintln!("Error reading file: {}", e),
                            }
                        },
                        NfsCommand::Quit => {
                            println!("Quitting...");
                            break;
                        }
                    }
                }
                
                // Handle background NFS operations if needed
                result = handle_nfs_operations(&mut stream, &mount_reply.file_handle) => {
                    if let Err(e) = result {
                        eprintln!("Error in NFS operation: {}", e);
                        break;
                    }
                }
            }
        }
    } else {
        println!("Mount failed with status: {}", mount_reply.status);
    }

    Ok(())
}

// You'll need to implement these functions:
async fn list_directory(stream: &mut TcpStream, file_handle: &[u8; 16]) -> Result<(), Box<dyn Error>> {
    // Initialize NFS connection first
    initialize_nfs(stream, file_handle).await?;
    
    // Then proceed with READDIR
    send_readdir_request(stream, file_handle).await
}

async fn get_attributes(stream: &mut TcpStream, file_handle: &[u8; 16]) -> Result<(), Box<dyn Error>> {
    // Implement GETATTR (procedure 1) here
    todo!("Implement get attributes")
}

async fn read_file(stream: &mut TcpStream, file_handle: &[u8; 16], path: &str) -> Result<(), Box<dyn Error>> {
    // Implement LOOKUP (procedure 3) followed by READ (procedure 6)
    todo!("Implement file reading")
}

#[derive(Debug, serde::Serialize)]
struct ReaddirRequest {
    size: u32,
    xid: u32,
    message_type: u32,    // CALL (0)
    rpc_version: u32,     // 2
    program: u32,         // NFS program (100003)
    version: u32,         // NFSv3 (3)
    procedure: u32,       // READDIR (16)
    credentials: (u32, u32),
    verifier: (u32, u32),
    dir_handle: [u8; 16], // The file handle we got from mount
    cookie: u64,          // Directory cookie (0 for first call)
    cookie_verf: [u8; 8], // Cookie verifier (8 bytes for NFSv3)
    count: u32,          // Maximum size of response
}

async fn send_readdir_request(stream: &mut TcpStream, dir_handle: &[u8; 16]) -> Result<(), Box<dyn Error>> {
    println!("Using file handle from mount: {:?}", dir_handle);
    
    let readdir_request = ReaddirRequest {
        size: 0,          // Will be filled in after serialization
        xid: 0x12345679,  // Unique transaction ID
        message_type: 0,   // CALL
        rpc_version: 2,
        program: 100003,   // NFS
        version: 3,        // NFSv3
        procedure: 16,     // READDIR
        credentials: (0, 0),
        verifier: (0, 0),
        dir_handle: *dir_handle,  // Using the mount-provided file handle
        cookie: 0,
        cookie_verf: [0; 8],
        count: 8192,
    };

    let mut request_data = to_bytes(&readdir_request)?;
    
    // Calculate and set the size with last fragment flag
    let size = request_data.len() as u32;
    let size_with_flag = encode_last_flag(size, true);
    request_data[0..4].copy_from_slice(&size_with_flag.to_be_bytes());

    println!("Sending READDIR request (size: {})", size);
    println!("First 32 bytes: {:?}", &request_data[..32.min(request_data.len())]);
    
    // Use the existing stream from the mount
    stream.write_all(&request_data).await?;
    stream.flush().await?;

    // Read response size
    let mut size_buffer = [0u8; 4];
    stream.read_exact(&mut size_buffer).await?;
    let response_size = u32::from_be_bytes(size_buffer) & 0x7FFFFFFF;
    
    println!("Response size indicator: {}", response_size);
    
    // Read response data
    let mut buffer = vec![0u8; response_size as usize];
    stream.read_exact(&mut buffer).await?;
    println!("Received READDIR response! Size: {}", response_size);
    println!("First 32 bytes of response: {:?}", &buffer[..32.min(buffer.len())]);

    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct GetAttrRequest {
    xid: u32,
    message_type: u32,    // CALL (0)
    rpc_version: u32,     // 2
    program: u32,         // NFS program (100003)
    version: u32,         // NFSv3 (3)
    procedure: u32,       // GETATTR (1)
    credentials: (u32, u32),
    verifier: (u32, u32),
    file_handle: [u8; 16],
}

#[derive(Debug, serde::Serialize)]
struct FsInfoRequest {
    xid: u32,
    message_type: u32,    // CALL (0)
    rpc_version: u32,     // 2
    program: u32,         // NFS program (100003)
    version: u32,         // NFSv3 (3)
    procedure: u32,       // FSINFO (19)
    credentials: (u32, u32),
    verifier: (u32, u32),
    file_handle: [u8; 16],
}

async fn send_getattr_request(stream: &mut TcpStream, file_handle: &[u8; 16]) -> Result<(), Box<dyn Error>> {
    // Add TCP stream state checks
    println!("TCP Stream state before GETATTR:");
    println!("Peer address: {:?}", stream.peer_addr()?);
    println!("Local address: {:?}", stream.local_addr()?);
    
    let mut message = Vec::new();
    
    // 1. Record marker (4 bytes)
    let fragment_header = 0x80000000u32; // Last fragment flag set
    message.extend_from_slice(&fragment_header.to_be_bytes());
    
    // 2. RPC Call Header
    message.extend_from_slice(&0x12345680u32.to_be_bytes()); // XID
    message.extend_from_slice(&0u32.to_be_bytes());          // Message Type (CALL)
    message.extend_from_slice(&2u32.to_be_bytes());          // RPC Version
    message.extend_from_slice(&100003u32.to_be_bytes());     // NFS Program
    message.extend_from_slice(&3u32.to_be_bytes());          // NFS Version 3
    message.extend_from_slice(&1u32.to_be_bytes());          // GETATTR Procedure
    
    // 3. NULL Authentication (simplest case)
    message.extend_from_slice(&0u32.to_be_bytes());          // Auth Flavor (AUTH_NONE)
    message.extend_from_slice(&0u32.to_be_bytes());          // Auth Length
    message.extend_from_slice(&0u32.to_be_bytes());          // Verifier Flavor
    message.extend_from_slice(&0u32.to_be_bytes());          // Verifier Length

    // 4. GETATTR arguments - file handle with XDR formatting
    message.extend_from_slice(&16u32.to_be_bytes());         // File Handle length (fixed 16 bytes)
    message.extend_from_slice(file_handle);                  // File Handle content
    
    // Update record marker with actual size
    let msg_len = (message.len() - 4) as u32;
    message[0..4].copy_from_slice(&(0x80000000u32 | msg_len).to_be_bytes());
    
    println!("Sending GETATTR request:");
    println!("Total message length: {}", message.len());
    println!("Record marker: {:02x?}", &message[0..4]);
    println!("XID: {:02x?}", &message[4..8]);
    println!("File handle length: 16");
    println!("File handle: {:02x?}", file_handle);
    println!("Full message: {:02x?}", &message);
    
    // Send the message
    stream.write_all(&message).await?;
    stream.flush().await?;
    
    // Wait for response
    let mut size_buffer = [0u8; 4];
    match stream.read_exact(&mut size_buffer).await {
        Ok(_) => {
            let response_size = u32::from_be_bytes(size_buffer) & 0x7FFFFFFF;
            println!("Got response marker: {:02x?}, size: {}", size_buffer, response_size);
            
            let mut buffer = vec![0u8; response_size as usize];
            match stream.read_exact(&mut buffer).await {
                Ok(_) => println!("Got response: {:02x?}", &buffer),
                Err(e) => println!("Failed to read response: {}", e),
            }
        },
        Err(e) => println!("Failed to read response marker: {}", e),
    }
    
    Ok(())
}

async fn initialize_nfs(stream: &mut TcpStream, file_handle: &[u8; 16]) -> Result<(), Box<dyn Error>> {
    println!("File handle from mount: {:02x?}", file_handle);
    send_getattr_request(stream, file_handle).await
}