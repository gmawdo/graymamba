const NFS_PROGRAM: u32 = 100003;
const NFS_VERSION: u32 = 3;

pub fn build_null_call(xid: u32) -> Vec<u8> {
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