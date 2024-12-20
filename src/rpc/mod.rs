pub mod getattr;
pub mod null;
pub mod mount;
pub mod lookup;
pub mod readdirplus;
pub mod read;
pub mod access;
pub mod auth;
use std::error::Error;
use tokio::net::TcpStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};

#[derive(Debug)]
#[allow(dead_code)]
pub struct Fattr3 {
    pub file_type: u32,    // type (directory, file, etc)
    pub mode: u32,         // protection mode bits
    pub nlink: u32,        // number of hard links
    pub uid: u32,          // user ID of owner
    pub gid: u32,          // group ID of owner
    pub size: u64,         // file size in bytes
    pub used: u64,         // bytes actually used
    pub rdev: Rdev3,       // device info
    pub fsid: u64,         // filesystem id
    pub fileid: u64,       // file id
    pub atime: Nfstime3,   // last access time
    pub mtime: Nfstime3,   // last modified time
    pub ctime: Nfstime3,   // last status change time
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Rdev3 {
    pub specdata1: u32,
    pub specdata2: u32,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Nfstime3 {
    pub seconds: u32,
    pub nseconds: u32,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct RpcReply {
    pub xid: u32,
    pub message_type: u32,
    pub reply_state: u32,
    pub verifier_flavor: u32,
    pub verifier_length: u32,
    pub accept_state: u32,
}

impl Fattr3 {
    pub fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
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

impl RpcReply {
    pub fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
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

pub async fn send_rpc_message(stream: &mut TcpStream, data: &[u8]) -> Result<(), Box<dyn Error>> {
    let record_marker = 0x80000000u32 | (data.len() as u32);
    
    // Send record marker
    stream.write_all(&record_marker.to_be_bytes()).await?;
    // Send data
    stream.write_all(data).await?;
    stream.flush().await?;
    Ok(())
}

pub async fn receive_rpc_reply(stream: &mut TcpStream) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut complete_response = Vec::new();
    
    loop {
        let mut marker_buf = [0u8; 4];
        stream.read_exact(&mut marker_buf).await?;
        let marker = u32::from_be_bytes(marker_buf);
        
        let size = marker & 0x7fffffff;
        let is_last = (marker & 0x80000000) != 0;
        
        println!("Receiving fragment: size={}, last={}", size, is_last);
        
        let mut fragment = vec![0u8; size as usize];
        stream.read_exact(&mut fragment).await?;
        
        println!("Received fragment data: {:02x?}", fragment);
        complete_response.extend_from_slice(&fragment);
        
        if is_last {
            break;
        }
    }
    parse_rpc_reply(&complete_response);
    
    Ok(complete_response)
}

fn parse_rpc_reply(complete_response: &[u8]) {
    if complete_response.len() >= 32 {  // RPC header + status
        match Fattr3::from_bytes(&complete_response) {
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
}