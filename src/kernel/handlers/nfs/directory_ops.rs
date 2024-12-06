#![allow(clippy::upper_case_acronyms)]
#![allow(dead_code)]
use crate::kernel::protocol::context::RPCContext;
use crate::kernel::api::nfs;
use crate::kernel::protocol::rpc::*;
use crate::kernel::vfs::vfs::VFSCapabilities;
use crate::kernel::protocol::xdr::*;
use std::io::{Read, Write};
use tracing::{debug, error, trace};

#[allow(non_camel_case_types)]
#[derive(Debug, Default)]
struct READDIRPLUS3args {
    dir: nfs::nfs_fh3,
    cookie: nfs::cookie3,
    cookieverf: nfs::cookieverf3,
    dircount: nfs::count3,
    maxcount: nfs::count3,
}
XDRStruct!(
    READDIRPLUS3args,
    dir,
    cookie,
    cookieverf,
    dircount,
    maxcount
);

#[allow(non_camel_case_types)]
#[derive(Debug, Default)]
struct entryplus3 {
    fileid: nfs::fileid3,
    name: nfs::filename3,
    cookie: nfs::cookie3,
    name_attributes: nfs::post_op_attr,
    name_handle: nfs::post_op_fh3,
}
XDRStruct!(
    entryplus3,
    fileid,
    name,
    cookie,
    name_attributes,
    name_handle
);

pub async fn nfsproc3_readdirplus(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut args = READDIRPLUS3args::default();
    args.deserialize(input)?;
    debug!("nfsproc3_readdirplus({:?},{:?}) ", xid, args);

    let dirid = context.vfs.fh_to_id(&args.dir);
    // fail if unable to convert file handle
    if let Err(stat) = dirid {
        make_success_reply(xid).serialize(output)?;
        stat.serialize(output)?;
        nfs::post_op_attr::Void.serialize(output)?;
        return Ok(());
    }
    let dirid = dirid.unwrap();
    let dir_attr_maybe = context.vfs.getattr(dirid).await;

    let dir_attr = match dir_attr_maybe {
        Ok(v) => nfs::post_op_attr::attributes(v),
        Err(_) => nfs::post_op_attr::Void,
    };

    let dirversion = if let Ok(ref dir_attr) = dir_attr_maybe {
        let cvf_version = (dir_attr.mtime.seconds as u64) << 32 | (dir_attr.mtime.nseconds as u64);
        cvf_version.to_be_bytes()
    } else {
        nfs::cookieverf3::default()
    };
    debug!(" -- Dir attr {:?}", dir_attr);
    debug!(" -- Dir version {:?}", dirversion);
    let has_version = args.cookieverf != nfs::cookieverf3::default();
    
    // subtract off the final entryplus* field (which must be false) and the eof
    let max_bytes_allowed = args.maxcount as usize - 128;
    // args.dircount is bytes of just fileid, name, cookie.
    // This is hard to ballpark, so we just divide it by 16
    let estimated_max_results = args.dircount / 16;
    let max_dircount_bytes = args.dircount as usize;
    let mut ctr = 0;
    match context
        .vfs
        .readdir(dirid, args.cookie, estimated_max_results as usize)
        .await
    {
        Ok(result) => {
            // we count dir_count seperately as it is just a subset of fields
            let mut accumulated_dircount: usize = 0;
            let mut all_entries_written = true;

            // this is a wrapper around a writer that also just counts the number of bytes
            // written
            let mut counting_output = crate::write_counter::WriteCounter::new(output);

            make_success_reply(xid).serialize(&mut counting_output)?;
            nfs::nfsstat3::NFS3_OK.serialize(&mut counting_output)?;
            dir_attr.serialize(&mut counting_output)?;
            dirversion.serialize(&mut counting_output)?;
            for entry in result.entries {
                let obj_attr = entry.attr;
                let handle = nfs::post_op_fh3::handle(context.vfs.id_to_fh(entry.fileid));

                let entry = entryplus3 {
                    fileid: entry.fileid,
                    name: entry.name,
                    cookie: entry.fileid,
                    name_attributes: nfs::post_op_attr::attributes(obj_attr),
                    name_handle: handle,
                };
                // write the entry into a buffer first
                let mut write_buf: Vec<u8> = Vec::new();
                let mut write_cursor = std::io::Cursor::new(&mut write_buf);
                // true flag for the entryplus3* to mark that this contains an entry
                true.serialize(&mut write_cursor)?;
                entry.serialize(&mut write_cursor)?;
                write_cursor.flush()?;
                let added_dircount = std::mem::size_of::<nfs::fileid3>()                   // fileid
                                    + std::mem::size_of::<u32>() + entry.name.len()  // name
                                    + std::mem::size_of::<nfs::cookie3>(); // cookie
                let added_output_bytes = write_buf.len();
                // check if we can write without hitting the limits
                if added_output_bytes + counting_output.bytes_written() < max_bytes_allowed
                    && added_dircount + accumulated_dircount < max_dircount_bytes
                {
                    trace!("  -- dirent {:?}", entry);
                    // commit the entry
                    ctr += 1;
                    counting_output.write_all(&write_buf)?;
                    accumulated_dircount += added_dircount;
                    trace!(
                        "  -- lengths: {:?} / {:?} {:?} / {:?}",
                        accumulated_dircount,
                        max_dircount_bytes,
                        counting_output.bytes_written(),
                        max_bytes_allowed
                    );
                } else {
                    trace!(" -- insufficient space. truncating");
                    all_entries_written = false;
                    break;
                }
            }
            // false flag for the final entryplus* linked list
            false.serialize(&mut counting_output)?;
            // eof flag is only valid here if we wrote everything
            if all_entries_written {
                debug!("  -- readdir eof {:?}", result.end);
                result.end.serialize(&mut counting_output)?;
            } else {
                debug!("  -- readdir eof {:?}", false);
                false.serialize(&mut counting_output)?;
            }
            debug!(
                "readir {}, has_version {},  start at {}, flushing {} entries, complete {}",
                dirid, has_version, args.cookie, ctr, all_entries_written
            );
        }
        Err(stat) => {
            error!("readdir error {:?} --> {:?} ", xid, stat);
            make_success_reply(xid).serialize(output)?;
            stat.serialize(output)?;
            dir_attr.serialize(output)?;
        }
    };
   
    Ok(())
}

#[allow(non_camel_case_types)]
#[derive(Debug, Default)]
struct MKDIR3args {
    dirops: nfs::diropargs3,
    attributes: nfs::sattr3,
}
XDRStruct!(MKDIR3args, dirops, attributes);

pub async fn nfsproc3_mkdir(
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
    let mut args = MKDIR3args::default();
    args.deserialize(input)?;

    debug!("nfsproc3_mkdir({:?}, {:?}) ", xid, args);
   

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

    let res = context.vfs.mkdir(dirid, &args.dirops.name).await;

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
            debug!("mkdir success --> {:?}, {:?}", fid, fattr);
            make_success_reply(xid).serialize(output)?;
            nfs::nfsstat3::NFS3_OK.serialize(output)?;
            // serialize CREATE3resok
            let fh = context.vfs.id_to_fh(fid);
            nfs::post_op_fh3::handle(fh).serialize(output)?;
            nfs::post_op_attr::attributes(fattr).serialize(output)?;
            wcc_res.serialize(output)?;
        }
        Err(e) => {
            debug!("mkdir error {:?} --> {:?}", xid, e);
            // serialize CREATE3resfail
            make_success_reply(xid).serialize(output)?;
            e.serialize(output)?;
            wcc_res.serialize(output)?;
        }
    }

    /* 
    if let Ok((_fid, _fattr)) = res {
        if let Ok(name_str) = std::str::from_utf8(&args.dirops.name) {
            if name_str.contains("sdk") {

                let datetime = Utc::now();

                // Format the DateTime to a string with the specified format
                let formatted_datetime = datetime.format("%b %d %H:%M:%S %Y").to_string();
                let event_type = "Directory Created";
                let userid = std::env::var("THEIA_CLOUD_SESSION_USER").unwrap_or_else(|_| "BMFSBO".to_string());
                // Prepare log message
                //let log_message = format!("SDK Created: {:?}", args.dirops.name);
                //String.format("%s, %s, %s, %s", creationTime, eventType, filePath, userId)
                
                let name_str = String::from_utf8_lossy(&args.dirops.name);
            }
        }
    } */

    Ok(())
}