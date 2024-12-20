use super::auth::AUTH_NULL;
use super::RpcReply;
use std::error::Error;

const MOUNT_PROGRAM: u32 = 100005;
const MOUNT_VERSION: u32 = 3;
const MOUNT_PROC_MNT: u32 = 1;

#[derive(Debug)]
#[allow(dead_code)]
pub struct MountAuth {
    stamp: u32,
    machine_name: String,
    uid: u32,
    gid: u32,
    aux_gids: Vec<u32>,
}

impl Default for MountAuth {
    fn default() -> Self {
        Self {
            stamp: 0,
            machine_name: String::new(),
            uid: 0,
            gid: 0,
            aux_gids: vec![0],  // Mount only needs one auxiliary GID
        }
    }
}

impl MountAuth {
    pub fn write_to_vec(&self, call: &mut Vec<u8>) {
        // Auth UNIX (flavor = 1)
        call.extend_from_slice(&1u32.to_be_bytes());  // AUTH_UNIX
        call.extend_from_slice(&24u32.to_be_bytes()); // Length of auth data
        call.extend_from_slice(&self.stamp.to_be_bytes());
        call.extend_from_slice(&0u32.to_be_bytes());  // Machine name length (0)
        call.extend_from_slice(&self.uid.to_be_bytes());
        call.extend_from_slice(&self.gid.to_be_bytes());
        call.extend_from_slice(&1u32.to_be_bytes());  // 1 auxiliary GID
        call.extend_from_slice(&0u32.to_be_bytes());  // Auxiliary GID value
        
        // Verifier (AUTH_NULL)
        call.extend_from_slice(&AUTH_NULL.to_be_bytes());
        call.extend_from_slice(&0u32.to_be_bytes());
    }
}

pub fn build_mount_call(xid: u32, name: &str) -> Vec<u8> {
    let path = format!("{}'s drive", name);
    let path_len = path.len() as u32;
    
    let mut call = Vec::new();
    call.extend_from_slice(&xid.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());  // call type = 0
    call.extend_from_slice(&2u32.to_be_bytes());  // RPC version = 2
    call.extend_from_slice(&MOUNT_PROGRAM.to_be_bytes());
    call.extend_from_slice(&MOUNT_VERSION.to_be_bytes());
    call.extend_from_slice(&MOUNT_PROC_MNT.to_be_bytes());
    
    // Add mount-specific authentication
    MountAuth::default().write_to_vec(&mut call);
    
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
#[allow(dead_code)]
pub struct MountReply {
    pub rpc: RpcReply,
    pub status: u32,          // Mount status (0 = success)
    pub file_handle_len: u32, // Should be 16 bytes for NFSv3
    pub file_handle: [u8; 16],
    pub auth_flavors: Vec<u32>, // List of supported auth flavors
}

impl MountReply {
    pub fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
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