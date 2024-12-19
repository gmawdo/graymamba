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
    println!("File handle used by getattr: {:02x?}", file_handle);
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