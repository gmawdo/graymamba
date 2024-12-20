pub mod getattr;
pub mod null;
pub mod mount;
pub mod lookup;
pub mod readdirplus;
pub mod read;
pub mod access;
pub mod auth;
use std::error::Error;

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