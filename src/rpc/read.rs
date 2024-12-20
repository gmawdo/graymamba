use std::error::Error;

#[derive(Debug)]
#[allow(dead_code)]
pub struct ReadReply {
    pub status: u32,
    pub attributes: Option<super::Fattr3>,
    pub count: u32,
    pub eof: bool,
    pub data: Vec<u8>,
}

#[allow(dead_code)]
const NFS_PROC_READ: u32 = 6;
#[allow(dead_code)]
const NFS_PROGRAM: u32 = 100003;
#[allow(dead_code)]
const NFS_VERSION: u32 = 3;

#[allow(dead_code)]
pub fn build_read_call(xid: u32, file_handle: &[u8; 16], offset: u64, count: u32) -> Vec<u8> {
    let mut call = Vec::new();
    
    // Standard RPC header
    call.extend_from_slice(&xid.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());  // call type = 0
    call.extend_from_slice(&2u32.to_be_bytes());  // RPC version = 2
    call.extend_from_slice(&NFS_PROGRAM.to_be_bytes());
    call.extend_from_slice(&NFS_VERSION.to_be_bytes());
    call.extend_from_slice(&NFS_PROC_READ.to_be_bytes());
    
    // Add authentication
    super::auth::AuthUnix::default().write_to_vec(&mut call);
    
    // File handle
    call.extend_from_slice(&16u32.to_be_bytes());
    call.extend_from_slice(file_handle);
    
    // Offset and count
    call.extend_from_slice(&offset.to_be_bytes());
    call.extend_from_slice(&count.to_be_bytes());
    
    call
}

#[allow(dead_code)]
impl ReadReply {
    pub fn from_bytes(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        if data.len() < 28 {
            return Err("Reply too short".into());
        }

        let status = u32::from_be_bytes(data[24..28].try_into()?);
        
        if status != 0 {
            return Ok(ReadReply {
                status,
                attributes: None,
                count: 0,
                eof: false,
                data: vec![],
            });
        }

        let mut offset = 28;
        
        // Parse attributes if present
        let has_attrs = u32::from_be_bytes(data[offset..offset+4].try_into()?) == 1;
        offset += 4;
        
        let attributes = if has_attrs {
            let attrs = super::Fattr3::from_bytes(&data[offset..])?;
            offset += 84;
            Some(attrs)
        } else {
            None
        };

        // Read count
        let count = u32::from_be_bytes(data[offset..offset+4].try_into()?);
        offset += 4;

        // Read EOF
        let eof = u32::from_be_bytes(data[offset..offset+4].try_into()?) == 1;
        offset += 4;

        // Read data length and actual data
        let data_length = u32::from_be_bytes(data[offset..offset+4].try_into()?);
        offset += 4;
        
        // Extract just the actual data bytes, skipping the length prefix
        let data = data[offset..offset+data_length as usize].to_vec();

        Ok(ReadReply {
            status,
            attributes,
            count,
            eof,
            data,
        })
    }
}