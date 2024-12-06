#![allow(clippy::upper_case_acronyms)]
#![allow(dead_code)]
use crate::kernel::protocol::context::RPCContext;
use crate::kernel::api::nfs;
use crate::kernel::protocol::rpc::*;
use crate::kernel::vfs::vfs::VFSCapabilities;
use crate::kernel::protocol::xdr::*;
use std::io::{Read, Write};
use tracing::debug;

#[allow(non_camel_case_types)]
#[derive(Debug, Default)]
struct FSINFO3resok {
    obj_attributes: nfs::post_op_attr,
    rtmax: u32,
    rtpref: u32,
    rtmult: u32,
    wtmax: u32,
    wtpref: u32,
    wtmult: u32,
    dtpref: u32,
    maxfilesize: nfs::size3,
    time_delta: nfs::nfstime3,
    properties: u32,
}
XDRStruct!(
    FSINFO3resok,
    obj_attributes,
    rtmax,
    rtpref,
    rtmult,
    wtmax,
    wtpref,
    wtmult,
    dtpref,
    maxfilesize,
    time_delta,
    properties
);

const FSF_LINK: u32 = 0x0001;
const FSF_SYMLINK: u32 = 0x0002;
const FSF_HOMOGENEOUS: u32 = 0x0008;
const FSF_CANSETTIME: u32 = 0x0010;

pub async fn nfsproc3_fsinfo(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut handle = nfs::nfs_fh3::default();
    handle.deserialize(input)?;
    debug!("nfsproc3_fsinfo({:?},{:?}) ", xid, handle);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        nfs::post_op_attr::Void.serialize(output)?;
        return Ok(());
    }
    let id = id.unwrap();
    //println!("nfsproc3_fsinfo-before");
    let dir_attr = match context.vfs.getattr(id).await {
        Ok(v) => nfs::post_op_attr::attributes(v),
        Err(_) => nfs::post_op_attr::Void,
    };

    let res = FSINFO3resok {
        obj_attributes: dir_attr,
        rtmax: 1024 * 1024,
        rtpref: 1024 * 124,
        rtmult: 1024 * 1024,
        wtmax: 1024 * 1024,
        wtpref: 1024 * 1024,
        wtmult: 1024 * 1024,
        dtpref: 1024 * 1024,
        maxfilesize: 128 * 1024 * 1024 * 1024,
        time_delta: nfs::nfstime3 {
            seconds: 0,
            nseconds: 1000000,
        },
        properties: FSF_SYMLINK | FSF_HOMOGENEOUS | FSF_CANSETTIME,
    };

    make_success_reply(xid).serialize(output)?;
    nfs::nfsstat3::NFS3_OK.serialize(output)?;
    debug!(" {:?} ---> {:?}", xid, res);
    res.serialize(output)?;
    Ok(())
}

const ACCESS3_READ: u32 = 0x0001;
const ACCESS3_LOOKUP: u32 = 0x0002;
const ACCESS3_MODIFY: u32 = 0x0004;
const ACCESS3_EXTEND: u32 = 0x0008;
const ACCESS3_DELETE: u32 = 0x0010;
const ACCESS3_EXECUTE: u32 = 0x0020;

pub async fn nfsproc3_access(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut handle = nfs::nfs_fh3::default();
    handle.deserialize(input)?;
    let mut access: u32 = 0;
    access.deserialize(input)?;
    debug!("nfsproc3_access({:?},{:?},{:?})", xid, handle, access);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        nfs::post_op_attr::Void.serialize(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    let obj_attr = match context.vfs.getattr(id).await {
        Ok(v) => nfs::post_op_attr::attributes(v),
        Err(_) => nfs::post_op_attr::Void,
    };
    // TODO better checks here
    if !matches!(context.vfs.capabilities(), VFSCapabilities::ReadWrite) {
        access &= ACCESS3_READ | ACCESS3_LOOKUP;
    }
    debug!(" {:?} ---> {:?}", xid, access);
    make_success_reply(xid).serialize(output)?;
    nfs::nfsstat3::NFS3_OK.serialize(output)?;
    obj_attr.serialize(output)?;
    access.serialize(output)?;
    Ok(())
}

#[allow(non_camel_case_types)]
#[derive(Debug, Default)]
struct PATHCONF3resok {
    obj_attributes: nfs::post_op_attr,
    linkmax: u32,
    name_max: u32,
    no_trunc: bool,
    chown_restricted: bool,
    case_insensitive: bool,
    case_preserving: bool,
}
XDRStruct!(
    PATHCONF3resok,
    obj_attributes,
    linkmax,
    name_max,
    no_trunc,
    chown_restricted,
    case_insensitive,
    case_preserving
);

pub async fn nfsproc3_pathconf(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut handle = nfs::nfs_fh3::default();
    handle.deserialize(input)?;
    debug!("nfsproc3_pathconf({:?},{:?})", xid, handle);

    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        nfs::post_op_attr::Void.serialize(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    let obj_attr = match context.vfs.getattr(id).await {
        Ok(v) => nfs::post_op_attr::attributes(v),
        Err(_) => nfs::post_op_attr::Void,
    };
    let res = PATHCONF3resok {
        obj_attributes: obj_attr,
        linkmax: 0,
        name_max: 32768,
        no_trunc: true,
        chown_restricted: true,
        case_insensitive: false,
        case_preserving: true,
    };
    debug!(" {:?} ---> {:?}", xid, res);
    make_success_reply(xid).serialize(output)?;
    nfs::nfsstat3::NFS3_OK.serialize(output)?;
    res.serialize(output)?;
    Ok(())
}

#[allow(non_camel_case_types)]
#[derive(Debug, Default)]
struct FSSTAT3resok {
    obj_attributes: nfs::post_op_attr,
    tbytes: nfs::size3,
    fbytes: nfs::size3,
    abytes: nfs::size3,
    tfiles: nfs::size3,
    ffiles: nfs::size3,
    afiles: nfs::size3,
    invarsec: u32,
}
XDRStruct!(
    FSSTAT3resok,
    obj_attributes,
    tbytes,
    fbytes,
    abytes,
    tfiles,
    ffiles,
    afiles,
    invarsec
);


pub async fn nfsproc3_fsstat(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut handle = nfs::nfs_fh3::default();
    handle.deserialize(input)?;
    debug!("nfsproc3_fsstat({:?},{:?}) ", xid, handle);
    let id = context.vfs.fh_to_id(&handle);
    // fail if unable to convert file handle
    if let Err(stat) = id {
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        nfs::post_op_attr::Void.serialize(output)?;
        return Ok(());
    }
    let id = id.unwrap();

    let obj_attr = match context.vfs.getattr(id).await {
        Ok(v) => nfs::post_op_attr::attributes(v),
        Err(_) => nfs::post_op_attr::Void,
    };
    let res = FSSTAT3resok {
        obj_attributes: obj_attr,
        tbytes: 1024 * 1024 * 1024 * 1024,
        fbytes: 1024 * 1024 * 1024 * 1024,
        abytes: 1024 * 1024 * 1024 * 1024,
        tfiles: 1024 * 1024 * 1024,
        ffiles: 1024 * 1024 * 1024,
        afiles: 1024 * 1024 * 1024,
        invarsec: u32::MAX,
    };
    make_success_reply(xid).serialize(output)?;
    nfs::nfsstat3::NFS3_OK.serialize(output)?;
    debug!(" {:?} ---> {:?}", xid, res);
    res.serialize(output)?;
    Ok(())
}