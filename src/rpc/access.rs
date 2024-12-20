use std::error::Error;

#[derive(Debug)]
#[allow(dead_code)]
pub struct AccessReply {
    pub status: u32,
    pub attributes: Option<super::Fattr3>,
    pub access: u32,
}

const NFS_PROC_ACCESS: u32 = 4;
const NFS_PROGRAM: u32 = 100003;
const NFS_VERSION: u32 = 3;

// Access rights bits
#[allow(dead_code)]
pub const ACCESS_READ: u32    = 0x0001;
#[allow(dead_code)]
pub const ACCESS_LOOKUP: u32  = 0x0002;
#[allow(dead_code)]
pub const ACCESS_MODIFY: u32  = 0x0004;
#[allow(dead_code)]
pub const ACCESS_EXTEND: u32  = 0x0008;
#[allow(dead_code)]
pub const ACCESS_DELETE: u32  = 0x0010;
#[allow(dead_code)]
pub const ACCESS_EXECUTE: u32 = 0x0020;

pub fn build_access_call(xid: u32, file_handle: &[u8; 16], check_access: u32) -> Vec<u8> {
    let mut call = Vec::new();
    
    // Standard RPC header
    call.extend_from_slice(&xid.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());  // call type = 0
    call.extend_from_slice(&2u32.to_be_bytes());  // RPC version = 2
    call.extend_from_slice(&NFS_PROGRAM.to_be_bytes());
    call.extend_from_slice(&NFS_VERSION.to_be_bytes());
    call.extend_from_slice(&NFS_PROC_ACCESS.to_be_bytes());
    
    // Add authentication
    super::auth::AuthUnix::default().write_to_vec(&mut call);
    
    // File handle (first length, then data)
    call.extend_from_slice(&16u32.to_be_bytes());  // Fixed length of 16
    println!("File handle used by access: {:02x?}", file_handle);
    call.extend_from_slice(file_handle);
    
    // Access to check
    call.extend_from_slice(&check_access.to_be_bytes());
    
    call
}

impl AccessReply {
    pub fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        if data.len() < 28 {  // RPC header (24) + status (4)
            return Err("Reply too short".into());
        }

        let status = u32::from_be_bytes(data[24..28].try_into()?);
        if status != 0 {
            return Ok(AccessReply {
                status,
                attributes: None,
                access: 0,
            });
        }

        let mut offset = 28;
        
        // Read attributes presence flag
        let has_attrs = u32::from_be_bytes(data[offset..offset+4].try_into()?) == 1;
        offset += 4;
        
        // Parse attributes if present
        let attributes = if has_attrs {
            let attrs = super::Fattr3::from_bytes(&data[offset..])?;
            offset += 84;  // Size of Fattr3
            Some(attrs)
        } else {
            None
        };

        // Read access rights
        let access = u32::from_be_bytes(data[offset..offset+4].try_into()?);
        
        Ok(AccessReply {
            status,
            attributes,
            access,
        })
    }
}