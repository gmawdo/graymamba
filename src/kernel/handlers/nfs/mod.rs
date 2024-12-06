pub mod basic_ops;  // NULL, GETATTR, ACCESS, etc.
pub mod directory_ops;  // LOOKUP, READDIR, etc. 
pub mod file_ops;  // READ, WRITE, CREATE, etc.
pub mod fs_ops;  // FSSTAT, FSINFO, etc.
pub mod link_ops;  // SYMLINK, READLINK, etc.
pub mod router;  // Main handler router

// Re-export main handler to make for simple import a la use crate::kernel::handlers::handle_nfs;
pub use router::handle_nfs;