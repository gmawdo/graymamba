use crate::kernel::protocol::context::RPCContext;
use crate::kernel::api::nfs::*;
use crate::kernel::protocol::rpc::*;
use crate::kernel::vfs::vfs::NFSFileSystem;
use anyhow::Error;
use std::io::{Read, Write};
use tracing::{debug, warn};

// Common helper functions used across handlers
pub(crate) fn make_success_reply(xid: u32) -> reply_message {
    reply_message {
        xid,
        stat: msg_stat::MSG_ACCEPTED,
        contents: reply_body::success(accept_stat::SUCCESS),
    }
}
