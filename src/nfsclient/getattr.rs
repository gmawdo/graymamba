const NFS_PROC_GETATTR: u32 = 1;  // GETATTR procedure number
const NFS_PROGRAM: u32 = 100003;
const NFS_VERSION: u32 = 3;

pub fn build_getattr_call(xid: u32, file_handle: &[u8; 16]) -> Vec<u8> {
    let mut call = Vec::new();
    
    // Standard RPC header
    call.extend_from_slice(&xid.to_be_bytes());
    call.extend_from_slice(&0u32.to_be_bytes());  // call type = 0
    call.extend_from_slice(&2u32.to_be_bytes());  // RPC version = 2
    call.extend_from_slice(&NFS_PROGRAM.to_be_bytes());
    call.extend_from_slice(&NFS_VERSION.to_be_bytes());
    call.extend_from_slice(&NFS_PROC_GETATTR.to_be_bytes());
    
    // Add authentication
    super::auth::AuthUnix::default().write_to_vec(&mut call);
    
    // File handle length
    call.extend_from_slice(&16u32.to_be_bytes());
    
    // File handle
    println!("File handle used by getattr: {:02x?}", file_handle);
    call.extend_from_slice(file_handle);
    
    println!("GETATTR call components:");
    println!("  XID: {}", xid);
    println!("  Program: {}", NFS_PROGRAM);
    println!("  Version: {}", NFS_VERSION);
    println!("  Procedure: {}", NFS_PROC_GETATTR);
    println!("  File handle: {:02x?}", file_handle);
    println!("  Total length: {}", call.len());
    println!("  Raw bytes: {:02x?}", call);
    
    call
}