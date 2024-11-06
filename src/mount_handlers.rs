use crate::context::RPCContext;
use crate::mount::*;
use crate::rpc::*;
use crate::xdr::*;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::cast::{FromPrimitive, ToPrimitive};
use std::io::{Read, Write};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tracing::debug;

use anyhow::Result;

use crate::data_store::KeyType;

#[allow(non_camel_case_types)]
#[allow(clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug, FromPrimitive, ToPrimitive)]
enum MountProgram {
    MOUNTPROC3_NULL = 0,
    MOUNTPROC3_MNT = 1,
    MOUNTPROC3_DUMP = 2,
    MOUNTPROC3_UMNT = 3,
    MOUNTPROC3_UMNTALL = 4,
    MOUNTPROC3_EXPORT = 5,
    INVALID,
}

pub async fn handle_mount(
    xid: u32,
    call: call_body,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let prog = MountProgram::from_u32(call.proc).unwrap_or(MountProgram::INVALID);

    match prog {
        MountProgram::MOUNTPROC3_NULL => mountproc3_null(xid, input, output)?,
        MountProgram::MOUNTPROC3_MNT => mountproc3_mnt(xid, input, output, context).await?,
        MountProgram::MOUNTPROC3_UMNT => mountproc3_umnt(xid, input, output, context).await?,
        MountProgram::MOUNTPROC3_UMNTALL => {
            mountproc3_umnt_all(xid, input, output, context).await?
        }
        MountProgram::MOUNTPROC3_EXPORT => mountproc3_export(xid, input, output)?,
        _ => {
            proc_unavail_reply_message(xid).serialize(output)?;
        }
    }
    Ok(())
}

pub fn mountproc3_null(
    xid: u32,
    _: &mut impl Read,
    output: &mut impl Write,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_null({:?}) ", xid);
    // build an RPC reply
    let msg = make_success_reply(xid);
    debug!("\t{:?} --> {:?}", xid, msg);
    msg.serialize(output)?;
    Ok(())
}

#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
struct mountres3_ok {
    fhandle: fhandle3, // really same thing as nfs::nfs_fh3
    auth_flavors: Vec<u32>,
}
XDRStruct!(mountres3_ok, fhandle, auth_flavors);

pub async fn mountproc3_mnt(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut path = dirpath::new();
    path.deserialize(input)?;

    // Parse options from the input stream
    let path = std::str::from_utf8(&path).unwrap_or_default();
    // println!("path: {:?}", path);

    // Parse user_key from the input stream
    let mut user_key = None;
    let options: Vec<&str> = path.split('/').collect();

    for option in options {
        if option.ends_with("'s drive") {
            user_key = Some(option.trim_end_matches("'s drive").to_string());
        }
    }

    // Authenticate user
    // let mut utf8path = String::new();
    let utf8path: String = if let Some(ref user_key) = user_key {
        match context.vfs.data_store().authenticate_user(user_key).await {
            KeyType::Usual => {
                println!("Authenticated as a standard user: {}", user_key);
                // Set the default mount directory
                format!("/{}", user_key)
                
            }
            KeyType::Special => {
                println!("Authenticated as a superuser: {}", user_key);
                // Set the default mount directory for special key
                String::from("/")
                
            }
            KeyType::None => {
                make_failure_reply(xid).serialize(output)?;
                return Err(anyhow::anyhow!("Authentication failed"));
            }
        }
    } else {
        make_failure_reply(xid).serialize(output)?;
        return Err(anyhow::anyhow!("User key not provided"));
    };

    

    // Initialize the mount directory
    let _ = init_user_directory(&utf8path);
    
    {
        
        debug!("mountproc3_mnt({:?},{:?}) ", xid, utf8path);
        if let Ok(fileid) = context.vfs.get_id_from_path(&utf8path, context.vfs.data_store()).await {
            let response = mountres3_ok {
                fhandle: context.vfs.id_to_fh(fileid).data,
                auth_flavors: vec![
                    auth_flavor::AUTH_NULL.to_u32().unwrap(),
                    auth_flavor::AUTH_UNIX.to_u32().unwrap(),
                ],
            };
            debug!("{:?} --> {:?}", xid, response);
            
            if let Some(ref chan) = context.mount_signal {
                let _ = chan.send(true).await;
            }
            make_success_reply(xid).serialize(output)?;
            mountstat3::MNT3_OK.serialize(output)?;
            response.serialize(output)?;
        } else {
            debug!("{:?} --> MNT3ERR_NOENT", xid);
            make_success_reply(xid).serialize(output)?;
            mountstat3::MNT3ERR_NOENT.serialize(output)?;
        }
        Ok(())
    }
}


/*

DESCRIPTION

  Procedure EXPORT returns a list of all the exported file
  systems and which clients are allowed to mount each one.
  The names in the group list are implementation-specific
  and cannot be directly interpreted by clients. These names
  can represent hosts or groups of hosts.

IMPLEMENTATION

  This procedure generally returns the contents of a list of
  shared or exported file systems. These are the file
  systems which are made available to NFS version 3 protocol
  clients.
 */

pub fn mountproc3_export(
    xid: u32,
    _: &mut impl Read,
    output: &mut impl Write,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_export({:?}) ", xid);
    make_success_reply(xid).serialize(output)?;
    true.serialize(output)?;
    // dirpath
    "/".as_bytes().to_vec().serialize(output)?;
    // groups
    false.serialize(output)?;
    // next exports
    false.serialize(output)?;
    Ok(())
}

pub async fn mountproc3_umnt(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut path = dirpath::new();
    path.deserialize(input)?;
    let utf8path = std::str::from_utf8(&path).unwrap_or_default();
    debug!("mountproc3_umnt({:?},{:?}) ", xid, utf8path);
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    make_success_reply(xid).serialize(output)?;
    mountstat3::MNT3_OK.serialize(output)?;
    Ok(())
}

pub async fn mountproc3_umnt_all(
    xid: u32,
    _input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_umnt_all({:?}) ", xid);
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    make_success_reply(xid).serialize(output)?;
    mountstat3::MNT3_OK.serialize(output)?;
    Ok(())
}

//the following code should not be visible at this high level
//it is specific to a storage type and should be behind a trait
use r2d2_redis_cluster::Commands; 
use r2d2_redis_cluster::redis_cluster_rs::redis;
use redis::RedisError;

impl From<RedisError> for crate::nfs::nfsstat3 {
    fn from(_: RedisError) -> Self {
        crate::nfs::nfsstat3::NFS3ERR_IO // or another appropriate nfsstat3 variant
    }
}
//must move these to a separate module
fn init_user_directory(mount_path: &str) -> Result<(), crate::nfs::nfsstat3> {
    let pool = crate::redis_data_store::get_redis_cluster_pool().unwrap();
    let mut conn = pool.get().map_err(|_| crate::nfs::nfsstat3::NFS3ERR_IO)?;

    let hash_tag = "{graymamba}";
    let path = format!("/{}", "graymamba");
    let key = format!("{}:{}", hash_tag, mount_path);
    let mut pipeline = redis::pipe();
    let exists_response: bool = conn.exists(key).unwrap_or(false);
    
    if exists_response {
        return Ok(());
    } else {

    let node_type = "0";
    let size = 0;
    let permissions = 777;
    let score = if mount_path == "/" { 1.0 } else { 2.0 };

    let nodes = format!("{}:/{}_nodes", hash_tag, "graymamba");
    let key_exists: bool = conn.exists(nodes).unwrap_or(false);

    let fileid: u64 = if key_exists {
        match conn.incr(format!("{}:/{}_next_fileid", hash_tag, "graymamba"), 1) {
            Ok(id) => id,
            Err(_) => {
                return Err(crate::nfs::nfsstat3::NFS3ERR_IO);
            }
        }
    } else {
        1
    };

    let system_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let epoch_seconds = system_time.as_secs();
    let epoch_nseconds = system_time.subsec_nanos(); // Capture nanoseconds part
 
    pipeline
        .cmd("ZADD")
        .arg(format!("{}:/{}_nodes", hash_tag, "graymamba"))
        .arg(score.to_string())
        //.arg(path.clone())
        .arg(mount_path)
        .cmd("HMSET")
        //.arg(format!("{}:{}", hash_tag, path))
        .arg(format!("{}:{}", hash_tag, mount_path))
        .arg("ftype")
        .arg(node_type)
        .arg("size")
        .arg(size.to_string())
        .arg("permissions")
        .arg(permissions.to_string())
        .arg("change_time_secs")
        .arg(epoch_seconds.to_string())
        .arg("change_time_nsecs")
        .arg(epoch_nseconds.to_string())
        .arg("modification_time_secs")
        .arg(epoch_seconds.to_string())
        .arg("modification_time_nsecs")
        .arg(epoch_nseconds.to_string())
        .arg("access_time_secs")
        .arg(epoch_seconds.to_string())
        .arg("access_time_nsecs")
        .arg(epoch_nseconds.to_string())
        .arg("birth_time_secs")
        .arg(epoch_seconds.to_string())
        .arg("birth_time_nsecs")
        .arg(epoch_nseconds.to_string())
        .arg("fileid")
        .arg(fileid)
        .cmd("HMSET")
        .arg(format!("{}:{}_path_to_id", hash_tag, path))
        //.arg(path.clone())
        .arg(mount_path)
        .arg(fileid)
        .cmd("HMSET")
        .arg(format!("{}:{}_id_to_path", hash_tag, path))
        .arg(fileid)
        .arg(mount_path);
        if fileid == 1 {
            pipeline
                .cmd("SET")
                .arg(format!("{}:{}_next_fileid", hash_tag, path))
                .arg(1);
        }
        
        pipeline.query(&mut *conn)?;
        
        Ok(())
    }
}

