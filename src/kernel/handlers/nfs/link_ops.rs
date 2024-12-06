use crate::kernel::protocol::context::RPCContext;
use crate::kernel::protocol::rpc::*;
use std::io::{Read, Write};
use tracing::{debug, error};
use crate::kernel::api::nfs;
use crate::kernel::vfs::api::VFSCapabilities;
use crate::kernel::protocol::xdr::*;

#[allow(non_camel_case_types)]
#[derive(Debug, Default)]
struct SYMLINK3args {
    dirops: nfs::diropargs3,
    symlink: nfs::symlinkdata3,
}
XDRStruct!(SYMLINK3args, dirops, symlink);

pub async fn nfsproc3_symlink(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    // if we do not have write capabilities
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        make_success_reply(xid).serialize(output)?;
        nfs::nfsstat3::NFS3ERR_ROFS.serialize(output)?;
        nfs::wcc_data::default().serialize(output)?;
        return Ok(());
    }
    let mut args = SYMLINK3args::default();
    args.deserialize(input)?;

    debug!("nfsproc3_symlink({:?}, {:?}) ", xid, args);

    // find the directory we are supposed to create the
    // new file in
    let dirid = context.vfs.fh_to_id(&args.dirops.dir);
    if let Err(stat) = dirid {
        // directory does not exist
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        nfs::wcc_data::default().serialize(output)?;
        error!("Directory does not exist");
        return Ok(());
    }
    // found the directory, get the attributes
    let dirid = dirid.unwrap();

    // get the object attributes before the write
    let pre_dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => {
            let wccattr = nfs::wcc_attr {
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime,
            };
            nfs::pre_op_attr::attributes(wccattr)
        }
        Err(stat) => {
            error!("Cannot stat directory");
            make_success_reply(xid).serialize(output)?;
            stat.serialize(output)?;
            nfs::wcc_data::default().serialize(output)?;
            return Ok(());
        }
    };

    let res = context
        .vfs
        .symlink(
            dirid,
            &args.dirops.name,
            &args.symlink.symlink_data,
            &args.symlink.symlink_attributes,
        )
        .await;

    // Re-read dir attributes for post op attr
    let post_dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => nfs::post_op_attr::attributes(v),
        Err(_) => nfs::post_op_attr::Void,
    };
    let wcc_res = nfs::wcc_data {
        before: pre_dir_attr,
        after: post_dir_attr,
    };

    match res {
        Ok((fid, fattr)) => {
            debug!("symlink success --> {:?}, {:?}", fid, fattr);
            make_success_reply(xid).serialize(output)?;
            nfs::nfsstat3::NFS3_OK.serialize(output)?;
            // serialize CREATE3resok
            let fh = context.vfs.id_to_fh(fid);
            nfs::post_op_fh3::handle(fh).serialize(output)?;
            nfs::post_op_attr::attributes(fattr).serialize(output)?;
            wcc_res.serialize(output)?;
        }
        Err(e) => {
            debug!("symlink error --> {:?}", e);
            // serialize CREATE3resfail
            make_success_reply(xid).serialize(output)?;
            e.serialize(output)?;
            wcc_res.serialize(output)?;
        }
    }

    Ok(())
}


pub async fn nfsproc3_readlink(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut handle = nfs::nfs_fh3::default();
    handle.deserialize(input)?;
    debug!("nfsproc3_readlink({:?},{:?}) ", xid, handle);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        return Ok(());
    }
    let id = id.unwrap();
    // if the id does not exist, we fail
    let symlink_attr = match context.vfs.getattr(id).await {
        Ok(v) => nfs::post_op_attr::attributes(v),
        Err(stat) => {
            make_success_reply(xid).serialize(output)?;
            stat.serialize(output)?;
            nfs::post_op_attr::Void.serialize(output)?;
            return Ok(());
        }
    };
    match context.vfs.readlink(id).await {
        Ok(path) => {
            debug!(" {:?} --> {:?}", xid, path);
            make_success_reply(xid).serialize(output)?;
            nfs::nfsstat3::NFS3_OK.serialize(output)?;
            symlink_attr.serialize(output)?;
            path.serialize(output)?;
        }
        Err(stat) => {
            // failed to read link
            // retry with failure and the post_op_attr
            make_success_reply(xid).serialize(output)?;
            stat.serialize(output)?;
            symlink_attr.serialize(output)?;
        }
    }
    Ok(())
}
