use std::error::Error;
use super::Fattr3;

#[derive(Debug)]
pub struct LookupReply {
    pub status: u32,
    pub file_handle: Option<[u8; 16]>,
    pub attributes: Option<Fattr3>,
    pub dir_attributes: Option<Fattr3>,
}

impl LookupReply {
    pub fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        if data.len() < 28 {  // 24 (RPC header) + 4 (status)
            return Err("Reply too short".into());
        }

        let status = u32::from_be_bytes(data[24..28].try_into()?);
        
        if status != 0 {
            return Ok(LookupReply {
                status,
                file_handle: None,
                attributes: None,
                dir_attributes: None,
            });
        }

        // Parse successful reply
        let mut offset = 28;
        
        // Read file handle (skip length field since we know it's 16)
        offset += 4;  // Skip fh_len
        
        let mut file_handle = [0u8; 16];
        file_handle.copy_from_slice(&data[offset..offset+16]);
        offset += 16;
        
        // Parse object attributes
        let obj_attrs = Fattr3::from_bytes(&data[offset..])?;
        offset += 84; // Size of Fattr3
        
        // Parse dir attributes
        let dir_attrs = Fattr3::from_bytes(&data[offset..])?;
        
        Ok(LookupReply {
            status,
            file_handle: Some(file_handle),
            attributes: Some(obj_attrs),
            dir_attributes: Some(dir_attrs),
        })
    }
}

const NFS_PROC_LOOKUP: u32 = 3;
const NFS_PROGRAM: u32 = 100003;
const NFS_VERSION: u32 = 3;

pub fn build_lookup_call(xid: u32, file_handle: &[u8; 16], name: &str) -> Vec<u8> {
    let mut call = Vec::new();
    
    // Standard RPC header (matching pattern from getattr.rs)
    call.extend_from_slice(&xid.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());
    call.extend_from_slice(&2u32.to_be_bytes());
    call.extend_from_slice(&NFS_PROGRAM.to_be_bytes());
    call.extend_from_slice(&NFS_VERSION.to_be_bytes());
    call.extend_from_slice(&NFS_PROC_LOOKUP.to_be_bytes());
    
    // Auth UNIX (matching pattern from getattr.rs)
    call.extend_from_slice(&1u32.to_be_bytes());
    call.extend_from_slice(&84u32.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());
    call.extend_from_slice(&501u32.to_be_bytes());
    call.extend_from_slice(&20u32.to_be_bytes());
    call.extend_from_slice(&16u32.to_be_bytes());
    
    let aux_gids = [12, 20, 61, 79, 80, 81, 98, 102, 701, 33, 100, 204, 250, 395, 398, 101];
    for gid in aux_gids.iter() {
        call.extend_from_slice(&(*gid as u32).to_be_bytes());
    }
    
    // Verifier
    call.extend_from_slice(&0u32.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());
    
    // File handle
    call.extend_from_slice(&16u32.to_be_bytes());
    call.extend_from_slice(file_handle);
    
    // Name
    call.extend_from_slice(&(name.len() as u32).to_be_bytes());
    call.extend_from_slice(name.as_bytes());
    
    // Padding
    let padding = (4 - (name.len() % 4)) % 4;
    call.extend(std::iter::repeat(0).take(padding));
    
    call
}
