sequenceDiagram
    participant Client
    participant TCP as kernel::protocol::tcp
    participant RPC as kernel::protocol::rpc
    participant Handler as kernel::handlers::nfs_handlers
    participant API as kernel::api::nfs
    participant VFS as kernel::vfs
    participant SharesFS

    Client->>TCP: NFS Request
    Note over TCP: NFSTcpListener handles connection
    
    TCP->>RPC: Parse RPC Message
    Note over RPC: Decode XDR format
    
    RPC->>Handler: Route NFS Procedure
    Note over Handler: Match NFS Program
    
    Handler->>API: Use NFS Types/Constants
    Note over API: RFC 1813 Definitions
    
    Handler->>VFS: Execute Filesystem Operation
    Note over VFS: Abstract FS Interface
    
    VFS->>SharesFS: Implement Operation
    Note over SharesFS: Actual Storage Logic
    
    SharesFS-->>Handler: Operation Result
    Handler-->>RPC: Format Response
    RPC-->>TCP: Encode Response
    TCP-->>Client: Send Response