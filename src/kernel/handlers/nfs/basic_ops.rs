#![allow(clippy::upper_case_acronyms)]
#![allow(dead_code)]
use crate::kernel::protocol::context::RPCContext;
use crate::kernel::api::nfs;
use crate::kernel::protocol::rpc::*;
use crate::kernel::vfs::vfs::VFSCapabilities;
use crate::kernel::protocol::xdr::*;
use byteorder::{ReadBytesExt, WriteBytesExt};
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::cast::FromPrimitive;
use std::io::{Read, Write};
use tracing::{debug, error, trace, warn};

pub async fn nfsproc3_getattr(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut handle = nfs::nfs_fh3::default();
    handle.deserialize(input)?;
    debug!("nfsproc3_getattr({:?},{:?}) ", xid, handle);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        return Ok(());
    }
    let id = id.unwrap();
    match context.vfs.getattr(id).await {
        Ok(fh) => {
            debug!("nfsproc3_getattr ({:?} --> {:?})", xid, fh);
            make_success_reply(xid).serialize(output)?;
            nfs::nfsstat3::NFS3_OK.serialize(output)?;
            fh.serialize(output)?;
        }
        Err(stat) => {
            error!("getattr error {:?} --> {:?}", xid, stat);
            make_success_reply(xid).serialize(output)?;
            stat.serialize(output)?;
        }
    }

    Ok(())
}

#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, Default)]
#[repr(u32)]
pub enum sattrguard3 {
    #[default]
    Void,
    obj_ctime(nfs::nfstime3),
}
XDRBoolUnion!(sattrguard3, obj_ctime, nfs::nfstime3);

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Default)]
struct SETATTR3args {
    object: nfs::nfs_fh3,
    new_attribute: nfs::sattr3,
    guard: sattrguard3,
}
XDRStruct!(SETATTR3args, object, new_attribute, guard);



pub async fn nfsproc3_setattr(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        make_success_reply(xid).serialize(output)?;
        nfs::nfsstat3::NFS3ERR_ROFS.serialize(output)?;
        nfs::wcc_data::default().serialize(output)?;
        return Ok(());
    }
    let mut args = SETATTR3args::default();
    args.deserialize(input)?;
    debug!("nfsproc3_setattr({:?},{:?}) ", xid, args);

    let id = context.vfs.fh_to_id(&args.object);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    let ctime;

    let pre_op_attr = match context.vfs.getattr(id).await {
        Ok(v) => {
            let wccattr = nfs::wcc_attr {
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime,
            };
            ctime = v.ctime;
            nfs::pre_op_attr::attributes(wccattr)
        }
        Err(stat) => {
            make_success_reply(xid).serialize(output)?;
            stat.serialize(output)?;
            nfs::wcc_data::default().serialize(output)?;
            return Ok(());
        }
    };
    // handle the guard
    match args.guard {
        sattrguard3::Void => {}
        sattrguard3::obj_ctime(c) => {
            if c.seconds != ctime.seconds || c.nseconds != ctime.nseconds {
                make_success_reply(xid).serialize(output)?;
                nfs::nfsstat3::NFS3ERR_NOT_SYNC.serialize(output)?;
                nfs::wcc_data::default().serialize(output)?;
            }
        }
    }

    match context.vfs.setattr(id, args.new_attribute).await {
        Ok(post_op_attr) => {
            debug!(" setattr success {:?} --> {:?}", xid, post_op_attr);
            let wcc_res = nfs::wcc_data {
                before: pre_op_attr,
                after: nfs::post_op_attr::attributes(post_op_attr),
            };
            make_success_reply(xid).serialize(output)?;
            nfs::nfsstat3::NFS3_OK.serialize(output)?;
            wcc_res.serialize(output)?;
        }
        Err(stat) => {
            error!("setattr error {:?} --> {:?}", xid, stat);
            make_success_reply(xid).serialize(output)?;
            stat.serialize(output)?;
            nfs::wcc_data::default().serialize(output)?;
        }
    }
    Ok(())
}


pub async fn nfsproc3_lookup(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut dirops = nfs::diropargs3::default();
    dirops.deserialize(input)?;
    debug!("nfsproc3_lookup({:?},{:?}) ", xid, dirops);

    let dirid = context.vfs.fh_to_id(&dirops.dir);
    // fail if unable to convert file handle
    if let Err(stat) = dirid {
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        nfs::post_op_attr::Void.serialize(output)?;
        return Ok(());
    }
    let dirid = dirid.unwrap();
    let dir_attr = match context.vfs.getattr(dirid).await {
        Ok(v) => nfs::post_op_attr::attributes(v),
        Err(_) => nfs::post_op_attr::Void,
    };
    match context.vfs.lookup(dirid, &dirops.name).await {
        Ok(fid) => {
            let obj_attr = match context.vfs.getattr(fid).await {
                Ok(v) => nfs::post_op_attr::attributes(v),
                Err(_) => nfs::post_op_attr::Void,
            };
            debug!("lookup success {:?} --> {:?}", xid, obj_attr);
            make_success_reply(xid).serialize(output)?;
            nfs::nfsstat3::NFS3_OK.serialize(output)?;
            context.vfs.id_to_fh(fid).serialize(output)?;
            obj_attr.serialize(output)?;
            dir_attr.serialize(output)?;
        }
        Err(stat) => {
            debug!("lookup error {:?}({:?}) --> {:?}", xid, dirops.name, stat);
            make_success_reply(xid).serialize(output)?;
            stat.serialize(output)?;
            dir_attr.serialize(output)?;
        }
    }
    Ok(())
}
