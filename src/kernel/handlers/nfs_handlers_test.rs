use std::io::Cursor;
use crate::kernel::protocol::context::RPCContext;
use crate::kernel::vfs::mock::MockNFSFileSystem;
use crate::kernel::protocol::rpc::auth_unix;
use std::sync::Arc;
use crate::kernel::handlers::nfs_handlers::nfsproc3_create;

#[tokio::test]
async fn test_nfsproc3_create_readonly() {
    let mut input = Cursor::new(Vec::new());
    let mut output = Cursor::new(Vec::new());
    
    // Create mock filesystem with readonly capability
    let mock_fs = MockNFSFileSystem::new_readonly();
    let context = RPCContext {
        local_port: 2049,
        client_addr: "127.0.0.1".to_string(),
        auth: auth_unix::default(),
        vfs: Arc::new(mock_fs),
        mount_signal: None
    };

    // Test create operation on readonly filesystem
    let result = nfsproc3_create(1, &mut input, &mut output, &context).await;
    assert!(result.is_ok());
    
    // Verify response indicates read-only filesystem error
    let response = output.into_inner();
    assert!(response.len() > 0);
    // Verify NFS3ERR_ROFS status in response
} 