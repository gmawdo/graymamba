use std::error::Error;
use super::Fattr3;

#[derive(Debug)]
pub struct EntryPlus3 {
    pub fileid: u64,
    pub name: String,
    pub cookie: u64,
    pub name_attributes: Option<Fattr3>,
    pub name_handle: Option<[u8; 16]>,
}

#[derive(Debug)]
pub struct ReaddirplusReply {
    pub status: u32,
    pub dir_attributes: Option<Fattr3>,
    pub cookieverf: u64,
    pub entries: Vec<EntryPlus3>,
    pub eof: bool,
}

const NFS_PROC_READDIRPLUS: u32 = 17;
const NFS_PROGRAM: u32 = 100003;
const NFS_VERSION: u32 = 3;

pub fn build_readdirplus_call(xid: u32, file_handle: &[u8; 16], cookie: u64, cookieverf: u64, dircount: u32, maxcount: u32) -> Vec<u8> {
    let mut call = Vec::new();
    
    // Standard RPC header
    call.extend_from_slice(&xid.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());
    call.extend_from_slice(&2u32.to_be_bytes());
    call.extend_from_slice(&NFS_PROGRAM.to_be_bytes());
    call.extend_from_slice(&NFS_VERSION.to_be_bytes());
    call.extend_from_slice(&NFS_PROC_READDIRPLUS.to_be_bytes());
    
    // Auth UNIX (same as other calls)
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
    
    // Cookie
    call.extend_from_slice(&cookie.to_be_bytes());
    
    // Cookieverf
    call.extend_from_slice(&cookieverf.to_be_bytes());
    
    // Dircount (max bytes for names)
    call.extend_from_slice(&dircount.to_be_bytes());
    
    // Maxcount (max bytes total)
    call.extend_from_slice(&maxcount.to_be_bytes());
    
    call
}

impl ReaddirplusReply {
    pub fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        if data.len() < 28 {  // 24 (RPC header) + 4 (status)
            return Err("Reply too short".into());
        }

        let status = u32::from_be_bytes(data[24..28].try_into()?);
        
        if status != 0 {
            return Ok(ReaddirplusReply {
                status,
                dir_attributes: None,
                cookieverf: 0,
                entries: vec![],
                eof: false,
            });
        }

        let mut offset = 28;
        
        // Parse dir attributes if present (indicated by value 1)
        let has_attrs = u32::from_be_bytes(data[offset..offset+4].try_into()?) == 1;
        offset += 4;
        
        let dir_attributes = if has_attrs {
            let attrs = Fattr3::from_bytes(&data[offset..])?;
            offset += 84; // Size of Fattr3
            Some(attrs)
        } else {
            None
        };

        // Read cookieverf
        let cookieverf = u64::from_be_bytes(data[offset..offset+8].try_into()?);
        offset += 8;

        // Read entries
        let mut entries = Vec::new();
        loop {
            // Check for more entries
            let has_entry = u32::from_be_bytes(data[offset..offset+4].try_into()?) == 1;
            offset += 4;
            
            if !has_entry {
                break;
            }

            // Read fileid
            let fileid = u64::from_be_bytes(data[offset..offset+8].try_into()?);
            offset += 8;

            // Read name
            let name_len = u32::from_be_bytes(data[offset..offset+4].try_into()?) as usize;
            offset += 4;
            let name = String::from_utf8(data[offset..offset+name_len].to_vec())?;
            offset += name_len;
            // Skip padding
            offset += (4 - (name_len % 4)) % 4;

            // Read cookie
            let cookie = u64::from_be_bytes(data[offset..offset+8].try_into()?);
            offset += 8;

            // Read attributes if present
            let has_attrs = u32::from_be_bytes(data[offset..offset+4].try_into()?) == 1;
            offset += 4;
            
            let name_attributes = if has_attrs {
                let attrs = Fattr3::from_bytes(&data[offset..])?;
                offset += 84;
                Some(attrs)
            } else {
                None
            };

            // Read handle if present
            let has_handle = u32::from_be_bytes(data[offset..offset+4].try_into()?) == 1;
            offset += 4;
            
            let name_handle = if has_handle {
                offset += 4; // Skip handle length
                let mut handle = [0u8; 16];
                handle.copy_from_slice(&data[offset..offset+16]);
                offset += 16;
                Some(handle)
            } else {
                None
            };

            entries.push(EntryPlus3 {
                fileid,
                name,
                cookie,
                name_attributes,
                name_handle,
            });
        }

        // Read EOF
        let eof = u32::from_be_bytes(data[offset..offset+4].try_into()?) == 1;

        Ok(ReaddirplusReply {
            status,
            dir_attributes,
            cookieverf,
            entries,
            eof,
        })
    }
}