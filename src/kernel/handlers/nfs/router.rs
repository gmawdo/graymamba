#![allow(clippy::upper_case_acronyms)]
#![allow(dead_code)]
use crate::kernel::protocol::context::RPCContext;
use crate::kernel::api::nfs;
use crate::kernel::protocol::rpc::*;
use crate::kernel::protocol::xdr::*;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::cast::FromPrimitive;
use std::io::{Read, Write};
use tracing::warn;
use crate::kernel::handlers::nfs::directory_ops::*;
use crate::kernel::handlers::nfs::file_ops::*;
use crate::kernel::handlers::nfs::link_ops::*;
use crate::kernel::handlers::nfs::fs_ops::*;
use crate::kernel::handlers::nfs::basic_ops::*;

use crate::kernel::vfs::api::VFSCapabilities;


#[allow(non_camel_case_types)]
#[allow(clippy::upper_case_acronyms)]

#[derive(Copy, Clone, Debug, FromPrimitive, ToPrimitive)]
pub(crate) enum NFSProgram {
    NFSPROC3_NULL = 0,
    NFSPROC3_GETATTR = 1,
    NFSPROC3_SETATTR = 2,
    NFSPROC3_LOOKUP = 3,
    NFSPROC3_ACCESS = 4,
    NFSPROC3_READLINK = 5,
    NFSPROC3_READ = 6,
    NFSPROC3_WRITE = 7,
    NFSPROC3_CREATE = 8,
    NFSPROC3_MKDIR = 9,
    NFSPROC3_SYMLINK = 10,
    NFSPROC3_MKNOD = 11,
    NFSPROC3_REMOVE = 12,
    NFSPROC3_RMDIR = 13,
    NFSPROC3_RENAME = 14,
    NFSPROC3_LINK = 15,
    NFSPROC3_READDIR = 16,
    NFSPROC3_READDIRPLUS = 17,
    NFSPROC3_FSSTAT = 18,
    NFSPROC3_FSINFO = 19,
    NFSPROC3_PATHCONF = 20,
    NFSPROC3_COMMIT = 21,
    INVALID = 22,
}

pub async fn handle_nfs(
    xid: u32,
    call: call_body,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    if call.vers != nfs::VERSION {
        warn!(
            "Invalid NFS Version number {} != {}",
            call.vers,
            nfs::VERSION
        );
        prog_mismatch_reply_message(xid, nfs::VERSION).serialize(output)?;
        return Ok(());
    }
    let prog = NFSProgram::from_u32(call.proc).unwrap_or(NFSProgram::INVALID);

    // Check for write operations on read-only filesystem
    match prog {
        NFSProgram::NFSPROC3_WRITE | 
        NFSProgram::NFSPROC3_CREATE | 
        NFSProgram::NFSPROC3_SETATTR |
        NFSProgram::NFSPROC3_REMOVE |
        NFSProgram::NFSPROC3_RMDIR |
        NFSProgram::NFSPROC3_RENAME |
        NFSProgram::NFSPROC3_MKDIR |
        NFSProgram::NFSPROC3_SYMLINK => {
            if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
                make_success_reply(xid).serialize(output)?;
                nfs::nfsstat3::NFS3ERR_ROFS.serialize(output)?;
                nfs::wcc_data::default().serialize(output)?;
                return Ok(());
            }
        }
        _ => {}
    }

    match prog {
        NFSProgram::NFSPROC3_NULL => nfsproc3_null(xid, input, output)?,
        NFSProgram::NFSPROC3_GETATTR => nfsproc3_getattr(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_LOOKUP => nfsproc3_lookup(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_READ => nfsproc3_read(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_FSINFO => nfsproc3_fsinfo(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_ACCESS => nfsproc3_access(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_PATHCONF => nfsproc3_pathconf(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_FSSTAT => nfsproc3_fsstat(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_READDIRPLUS => {
            nfsproc3_readdirplus(xid, input, output, context).await?
        }
        NFSProgram::NFSPROC3_WRITE => nfsproc3_write(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_CREATE => nfsproc3_create(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_SETATTR => nfsproc3_setattr(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_REMOVE => nfsproc3_remove(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_RMDIR => nfsproc3_remove(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_RENAME => nfsproc3_rename(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_MKDIR => nfsproc3_mkdir(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_SYMLINK => nfsproc3_symlink(xid, input, output, context).await?,
        NFSProgram::NFSPROC3_READLINK => nfsproc3_readlink(xid, input, output, context).await?,
        _ => {
            //warn!("Unimplemented message {:?}", prog);
            proc_unavail_reply_message(xid).serialize(output)?;
        } /*
          NFSPROC3_MKNOD,
          NFSPROC3_LINK,
          NFSPROC3_READDIR,
          NFSPROC3_COMMIT,
          INVALID*/
    }
    Ok(())
}