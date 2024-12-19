use std::net::SocketAddr;
use tokio::net::TcpStream;
use std::error::Error;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::time::sleep;
use std::time::Duration;

const NFS_PROGRAM: u32 = 100003;
const NFS_VERSION: u32 = 3;
const MOUNT_PROGRAM: u32 = 100005;
const MOUNT_VERSION: u32 = 3;
const MOUNT_PROC_MNT: u32 = 1;  // MNT procedure number
const NFS_PROC_GETATTR: u32 = 1;  // GETATTR procedure number

async fn send_rpc_message(stream: &mut TcpStream, data: &[u8]) -> Result<(), Box<dyn Error>> {
    let record_marker = 0x80000000u32 | (data.len() as u32);
    
    // Send record marker
    stream.write_all(&record_marker.to_be_bytes()).await?;
    // Send data
    stream.write_all(data).await?;
    stream.flush().await?;
    Ok(())
}

#[derive(Debug)]
#[allow(dead_code)]
struct RpcReply {
    xid: u32,
    message_type: u32,
    reply_state: u32,
    verifier_flavor: u32,
    verifier_length: u32,
    accept_state: u32,
}

impl RpcReply {
    fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        if data.len() < 24 {
            return Err("Reply too short".into());
        }

        Ok(RpcReply {
            xid: u32::from_be_bytes(data[0..4].try_into()?),
            message_type: u32::from_be_bytes(data[4..8].try_into()?),
            reply_state: u32::from_be_bytes(data[8..12].try_into()?),
            verifier_flavor: u32::from_be_bytes(data[12..16].try_into()?),
            verifier_length: u32::from_be_bytes(data[16..20].try_into()?),
            accept_state: u32::from_be_bytes(data[20..24].try_into()?),
        })
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct MountReply {
    rpc: RpcReply,
    status: u32,          // Mount status (0 = success)
    file_handle_len: u32, // Should be 16 bytes for NFSv3
    file_handle: [u8; 16],
    auth_flavors: Vec<u32>, // List of supported auth flavors
}

impl MountReply {
    fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        if data.len() < 28 {  // 24 (RPC header) + 4 (status)
            return Err("Reply too short".into());
        }

        let rpc = RpcReply::from_bytes(&data[0..24])?;
        let status = u32::from_be_bytes(data[24..28].try_into()?);
        
        if status == 0 {  // Success
            let file_handle_len = u32::from_be_bytes(data[28..32].try_into()?);
            if file_handle_len != 16 {
                return Err(format!("Unexpected file handle length: {}", file_handle_len).into());
            }
            
            let mut file_handle = [0u8; 16];
            file_handle.copy_from_slice(&data[32..48]);
            
            // Read auth flavors count
            let auth_count = u32::from_be_bytes(data[48..52].try_into()?);
            let mut auth_flavors = Vec::new();
            
            // Read auth flavors
            for i in 0..auth_count as usize {
                let offset = 52 + (i * 4);
                auth_flavors.push(u32::from_be_bytes(data[offset..offset+4].try_into()?));
            }

            Ok(MountReply {
                rpc,
                status,
                file_handle_len,
                file_handle,
                auth_flavors,
            })
        } else {
            Ok(MountReply {
                rpc,
                status,
                file_handle_len: 0,
                file_handle: [0; 16],
                auth_flavors: vec![],
            })
        }
    }
}

async fn receive_rpc_reply(stream: &mut TcpStream) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut marker_buf = [0u8; 4];
    stream.read_exact(&mut marker_buf).await?;
    let marker = u32::from_be_bytes(marker_buf);
    
    let size = marker & 0x7fffffff;
    let is_last = (marker & 0x80000000) != 0;
    
    println!("Receiving reply: size={}, last={}", size, is_last);
    
    let mut response = vec![0u8; size as usize];
    stream.read_exact(&mut response).await?;
    
    println!("Received reply data: {:02x?}", response);
    
    if response.len() >= 32 {  // RPC header + status
        match Fattr3::from_bytes(&response) {
            Ok(attrs) => {
                println!("File attributes:");
                println!("  Type: {}", match attrs.file_type {
                    1 => "Regular File",
                    2 => "Directory",
                    3 => "Block Device",
                    4 => "Character Device",
                    5 => "Symbolic Link",
                    6 => "Socket",
                    7 => "FIFO",
                    _ => "Unknown",
                });
                println!("  Mode: {:o}", attrs.mode);
                println!("  Links: {}", attrs.nlink);
                println!("  UID: {}", attrs.uid);
                println!("  GID: {}", attrs.gid);
                println!("  Size: {} bytes", attrs.size);
                println!("  Used: {} bytes", attrs.used);
            },
            Err(e) => println!("Error parsing attributes: {}", e),
        }
    }
    
    Ok(response)
}

fn build_null_call(xid: u32) -> Vec<u8> {
    let mut call = Vec::new();
    // Note: We no longer need the size prefix in the RPC message itself
    call.extend_from_slice(&xid.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());  // call type = 0
    call.extend_from_slice(&2u32.to_be_bytes());  // RPC version = 2
    call.extend_from_slice(&NFS_PROGRAM.to_be_bytes());
    call.extend_from_slice(&NFS_VERSION.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());  // procedure = 0 (NULL)
    
    // Auth UNIX (flavor = 1)
    call.extend_from_slice(&1u32.to_be_bytes());  // AUTH_UNIX
    call.extend_from_slice(&24u32.to_be_bytes()); // Length of auth data
    call.extend_from_slice(&0u32.to_be_bytes());  // Stamp
    call.extend_from_slice(&0u32.to_be_bytes());  // Machine name length (0)
    call.extend_from_slice(&0u32.to_be_bytes());  // UID
    call.extend_from_slice(&0u32.to_be_bytes());  // GID
    call.extend_from_slice(&1u32.to_be_bytes());  // 1 auxiliary GID
    call.extend_from_slice(&0u32.to_be_bytes());  // Auxiliary GID value
    
    // Verifier (AUTH_NULL)
    call.extend_from_slice(&0u32.to_be_bytes());  // AUTH_NULL
    call.extend_from_slice(&0u32.to_be_bytes());  // Length 0
    
    println!("Call buffer ({} bytes): {:02x?}", call.len(), call);
    
    call
}

fn build_mount_call(xid: u32, name: &str) -> Vec<u8> {
    let path = format!("{}'s drive", name);
    let path_len = path.len() as u32;
    
    let mut call = Vec::new();
    call.extend_from_slice(&xid.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());  // call type = 0
    call.extend_from_slice(&2u32.to_be_bytes());  // RPC version = 2
    call.extend_from_slice(&MOUNT_PROGRAM.to_be_bytes());
    call.extend_from_slice(&MOUNT_VERSION.to_be_bytes());
    call.extend_from_slice(&MOUNT_PROC_MNT.to_be_bytes());
    
    // Auth UNIX (flavor = 1)
    call.extend_from_slice(&1u32.to_be_bytes());  // AUTH_UNIX
    call.extend_from_slice(&24u32.to_be_bytes()); // Length of auth data
    call.extend_from_slice(&0u32.to_be_bytes());  // Stamp
    call.extend_from_slice(&0u32.to_be_bytes());  // Machine name length (0)
    call.extend_from_slice(&0u32.to_be_bytes());  // UID
    call.extend_from_slice(&0u32.to_be_bytes());  // GID
    call.extend_from_slice(&1u32.to_be_bytes());  // 1 auxiliary GID
    call.extend_from_slice(&0u32.to_be_bytes());  // Auxiliary GID value
    
    // Verifier (AUTH_NULL)
    call.extend_from_slice(&0u32.to_be_bytes());  // AUTH_NULL
    call.extend_from_slice(&0u32.to_be_bytes());  // Length 0
    
    // Path
    call.extend_from_slice(&path_len.to_be_bytes());
    call.extend_from_slice(path.as_bytes());
    
    // Pad to 4-byte boundary if needed
    let padding = (4 - (path.len() % 4)) % 4;
    call.extend(std::iter::repeat(0).take(padding));
    
    println!("Mount call buffer ({} bytes): {:02x?}", call.len(), call);
    
    call
}

#[derive(Debug)]
struct NfsSession {
    file_handle: [u8; 16],
}

fn build_getattr_call(xid: u32, file_handle: &[u8; 16]) -> Vec<u8> {
    let mut call = Vec::new();
    
    // Standard RPC header
    call.extend_from_slice(&xid.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());  // call type = 0
    call.extend_from_slice(&2u32.to_be_bytes());  // RPC version = 2
    call.extend_from_slice(&NFS_PROGRAM.to_be_bytes());
    call.extend_from_slice(&NFS_VERSION.to_be_bytes());
    call.extend_from_slice(&NFS_PROC_GETATTR.to_be_bytes());
    
    // Auth UNIX (flavor = 1)
    call.extend_from_slice(&1u32.to_be_bytes());  // AUTH_UNIX
    call.extend_from_slice(&84u32.to_be_bytes()); // Length of auth data (matches Finder)
    call.extend_from_slice(&0u32.to_be_bytes());  // Stamp
    call.extend_from_slice(&0u32.to_be_bytes());  // Machine name length (0)
    call.extend_from_slice(&501u32.to_be_bytes()); // UID (matching Finder)
    call.extend_from_slice(&20u32.to_be_bytes());  // GID (matching Finder)
    call.extend_from_slice(&16u32.to_be_bytes());  // 16 auxiliary GIDs
    
    // Auxiliary GIDs from Finder
    let aux_gids = [12, 20, 61, 79, 80, 81, 98, 102, 701, 33, 100, 204, 250, 395, 398, 101];
    for gid in aux_gids.iter() {
        call.extend_from_slice(&(*gid as u32).to_be_bytes());
    }
    
    // Verifier (AUTH_NULL)
    call.extend_from_slice(&0u32.to_be_bytes());  // AUTH_NULL
    call.extend_from_slice(&0u32.to_be_bytes());  // Length 0
    
    // File handle length
    call.extend_from_slice(&16u32.to_be_bytes());
    
    // File handle
    call.extend_from_slice(file_handle);
    
    println!("GETATTR call components:");
    println!("  XID: {}", xid);
    println!("  Program: {}", NFS_PROGRAM);
    println!("  Version: {}", NFS_VERSION);
    println!("  Procedure: {}", NFS_PROC_GETATTR);
    println!("  Auth length: 84");
    println!("  UID: 501");
    println!("  GID: 20");
    println!("  Aux GIDs: {:?}", aux_gids);
    println!("  File handle: {:02x?}", file_handle);
    println!("  Total length: {}", call.len());
    println!("  Raw bytes: {:02x?}", call);
    
    call
}

#[derive(Debug)]
#[allow(dead_code)]
struct Fattr3 {
    file_type: u32,    // type (directory, file, etc)
    mode: u32,         // protection mode bits
    nlink: u32,        // number of hard links
    uid: u32,          // user ID of owner
    gid: u32,          // group ID of owner
    size: u64,         // file size in bytes
    used: u64,         // bytes actually used
    rdev: Rdev3,       // device info
    fsid: u64,         // filesystem id
    fileid: u64,       // file id
    atime: Nfstime3,   // last access time
    mtime: Nfstime3,   // last modified time
    ctime: Nfstime3,   // last status change time
}

#[derive(Debug)]
#[allow(dead_code)]
struct Rdev3 {
    specdata1: u32,
    specdata2: u32,
}

#[derive(Debug)]
#[allow(dead_code)]
struct Nfstime3 {
    seconds: u32,
    nseconds: u32,
}

impl Fattr3 {
    fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        // First 32 bytes are RPC header + status
        let pos = 32;  // Starting position of fattr3 data
        
        // We need at least 32 + 52 = 84 bytes for the basic attributes
        if data.len() < pos + 52 {  // Reduced from 84 to 52
            return Err(format!("Reply too short for fattr3: {} bytes", data.len()).into());
        }

        let attrs = Fattr3 {
            file_type: u32::from_be_bytes(data[pos..pos+4].try_into()?),
            mode: u32::from_be_bytes(data[pos+4..pos+8].try_into()?),
            nlink: u32::from_be_bytes(data[pos+8..pos+12].try_into()?),
            uid: u32::from_be_bytes(data[pos+12..pos+16].try_into()?),
            gid: u32::from_be_bytes(data[pos+16..pos+20].try_into()?),
            size: u64::from_be_bytes(data[pos+20..pos+28].try_into()?),
            used: u64::from_be_bytes(data[pos+28..pos+36].try_into()?),
            rdev: Rdev3 {
                specdata1: u32::from_be_bytes(data[pos+36..pos+40].try_into()?),
                specdata2: u32::from_be_bytes(data[pos+40..pos+44].try_into()?),
            },
            fsid: u64::from_be_bytes(data[pos+44..pos+52].try_into()?),
            // Use default values for the rest since they might not be present
            fileid: 0,
            atime: Nfstime3 { seconds: 0, nseconds: 0 },
            mtime: Nfstime3 { seconds: 0, nseconds: 0 },
            ctime: Nfstime3 { seconds: 0, nseconds: 0 },
        };

        println!("File attributes:");
        println!("  Type: {}", match attrs.file_type {
            1 => "Regular File",
            2 => "Directory",
            3 => "Block Device",
            4 => "Character Device",
            5 => "Symbolic Link",
            6 => "Socket",
            7 => "FIFO",
            _ => "Unknown",
        });
        println!("  Mode: {:o}", attrs.mode);
        println!("  Links: {}", attrs.nlink);
        println!("  UID: {}", attrs.uid);
        println!("  GID: {}", attrs.gid);
        println!("  Size: {} bytes", attrs.size);
        println!("  Used: {} bytes", attrs.used);

        Ok(attrs)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr: SocketAddr = "127.0.0.1:2049".parse()?;
    let mut stream = TcpStream::connect(addr).await?;
    
    println!("Connected to NFS server");
    
    // First do NULL call
    let null_call = build_null_call(1);
    println!("Sending NULL call");
    send_rpc_message(&mut stream, &null_call).await?;
    
    match receive_rpc_reply(&mut stream).await {
        Ok(reply) => {
            println!("Received NULL reply: {:02x?}", reply);
        },
        Err(e) => {
            println!("Error receiving reply: {}", e);
            return Err(e);
        }
    }
    
    // Then do MOUNT call
    let mount_call = build_mount_call(2, "joseph");
    println!("Sending MOUNT call");
    send_rpc_message(&mut stream, &mount_call).await?;
    
    let session = match receive_rpc_reply(&mut stream).await {
        Ok(reply) => {
            println!("Received MOUNT reply: {:02x?}", reply);
            if let Ok(mount_reply) = MountReply::from_bytes(&reply) {
                if mount_reply.status == 0 {
                    Some(NfsSession {
                        file_handle: mount_reply.file_handle,
                    })
                } else {
                    println!("Mount failed with status: {}", mount_reply.status);
                    None
                }
            } else {
                None
            }
        },
        Err(e) => {
            println!("Error receiving reply: {}", e);
            None
        }
    };

    // Now do GETATTR call if we have a valid session
    if let Some(session) = session {
        println!("Got file handle: {:02x?}", session.file_handle);
        
        let getattr_call = build_getattr_call(3, &session.file_handle);
        println!("Sending GETATTR call");
        send_rpc_message(&mut stream, &getattr_call).await?;
        
        // Add small delay
        sleep(Duration::from_millis(100)).await;
        
        match receive_rpc_reply(&mut stream).await {
            Ok(reply) => {
                println!("Received GETATTR reply: {:02x?}", reply);
            },
            Err(e) => {
                println!("Error receiving reply: {}", e);
            }
        }
    }
    
    Ok(())
}