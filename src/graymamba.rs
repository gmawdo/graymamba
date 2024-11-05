use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::sync::Arc;
use tokio::time::Instant;

use std::ops::Bound;
use std::os::unix::ffi::OsStrExt;
use chrono::{Local, DateTime};

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use tracing::{debug, warn};

use graymamba::blockchain_audit::BlockchainAudit;
use graymamba::nfs::*;
use graymamba::nfs::nfsstat3;
use graymamba::tcp::{NFSTcp, NFSTcpListener};
use graymamba::vfs::{DirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};
use graymamba::channel_buffer;

use crate::channel_buffer::{ChannelBuffer, ActiveWrite};

use graymamba::data_store::DataStore;

use wasmtime::*;

extern crate secretsharing;

use config::{Config, File as ConfigFile};

mod file_metadata;
use file_metadata::FileMetadata;

mod sharesbased_fs;
use sharesbased_fs::SharesFS;
use crate::sharesbased_fs::{USER_ID, HASH_TAG};

#[async_trait]
impl NFSFileSystem for SharesFS {
    fn data_store(&self) -> &dyn DataStore {
        &*self.data_store
    }
    fn root_dir(&self) -> fileid3 {
        0
    }
    fn capabilities(&self) -> VFSCapabilities {
        VFSCapabilities::ReadWrite
    }
 
    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        let filename_str = OsStr::from_bytes(filename).to_str().ok_or(nfsstat3::NFS3ERR_IO)?;

        // Handle the root directory case
        if dirid == 0 {
            
            let child_path = format!("/{}", filename_str);
            
            if let Ok(child_id) = self.get_id_from_path(&child_path).await {
                //println!("ID--------{}", child_id);
                return Ok(child_id);
            }
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
    
        // Handle other directories
        let parent_path = self.get_path_from_id(dirid).await?;
        let child_path = format!("{}/{}", parent_path, filename_str);
    
        if let Ok(child_id) = self.get_id_from_path(&child_path).await {
            return Ok(child_id);
        }
    
        Err(nfsstat3::NFS3ERR_NOENT)
    }
    
    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {       
        warn!("graymamba getattr {:?}", id);
        let metadata = self.get_metadata_from_id(id).await?;
        let path = self.get_path_from_id(id).await?;
        debug!("Stat {:?}: {:?}", path, &metadata);
        let fattr = FileMetadata::metadata_to_fattr3(id, &metadata).await?;
        Ok(fattr)
        
    }

    async fn read(&self, id: fileid3, offset: u64, count: u32) -> Result<(Vec<u8>, bool), nfsstat3> {           
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;

        // Get file path from the share store
        let path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, user_id),
            &id.to_string()
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;
            //.unwrap_or_default();

        // Retrieve the current file content (Base64 encoded)
        // Retrieve the existing data from the share store
        let current_data= self.get_data(&path).await;
        // Check if the offset is beyond the current data length
        if offset as usize >= current_data.len() {
            return Ok((vec![], true)); // Return an empty vector and EOF as true
        }

        // Calculate the end of the data slice to return
        let end = std::cmp::min(current_data.len(), (offset + count as u64) as usize);

        // Slice the data from offset to the calculated end
        let data_slice = &current_data[offset as usize..end];

        // Determine if this slice reaches the end of the file
        let eof = end >= current_data.len();
        
        // Get the current local date and time
        let local_date_time: DateTime<Local> = Local::now();

        // Format the date and time using the specified pattern
        let creation_time = local_date_time.format("%b %d %H:%M:%S %Y").to_string();
        
        // Initialize extracted with an empty string or any default value
        
        let mut user = "";

        let parts: Vec<&str> = path.split('/').collect();

            if parts.len() > 2 {
                user = parts[1];
            }
        if let Some(blockchain_audit) = &self.blockchain_audit {
            let _ = blockchain_audit.trigger_event(&creation_time, "reassembled", &path, &user);
        }
        Ok((data_slice.to_vec(), eof))
    }

    async fn readdir(&self, dirid: fileid3, start_after: fileid3, max_entries: usize) -> Result<ReadDirResult, nfsstat3> {
        let path = self.get_path_from_id(dirid).await?;

        let children_vec = self.get_direct_children(&path).await?;
        let children: BTreeSet<u64> = children_vec.into_iter().collect();
        
        //println!("Children: {:?}", children.iter().collect::<Vec<_>>());
        
        let mut ret = ReadDirResult {
            entries: Vec::new(),
            end: false,
        };

        let range_start = if start_after > 0 {
            Bound::Excluded(start_after)
        } else {
            Bound::Unbounded
        };

        let remaining_length = children.range((range_start, Bound::Unbounded)).count();
        debug!("path: {:?}", path);
        debug!("children len: {:?}", children.len());
        debug!("remaining_len : {:?}", remaining_length);
        for child_id in children.range((range_start, Bound::Unbounded)) {
            //println!("Child_Id-------{}", *child_id);
            let child_path = self.get_path_from_id(*child_id).await?;
            let child_name = self.get_last_path_element(child_path).await;
            let child_metadata = self.get_metadata_from_id(*child_id).await?;

            debug!("\t --- {:?} {:?}", child_id, child_name);
            
            ret.entries.push(DirEntry {
                fileid: *child_id,
                name: child_name.as_bytes().into(),
                attr: FileMetadata::metadata_to_fattr3(*child_id, &child_metadata).await.expect(""),
            });
            

            if ret.entries.len() >= max_entries {
                break;
            }
        }

        if ret.entries.len() == remaining_length {
            ret.end = true;
        }

        debug!("readdir_result:{:?}", ret);

        Ok(ret)
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {       
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;

        // Get file path from the share store
        let path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, user_id),
            &id.to_string()
        ).await
            .unwrap_or_else(|_| String::new());

        match setattr.atime {
            set_atime::SET_TO_SERVER_TIME => {
                let system_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap();
                let epoch_seconds = system_time.as_secs();
                let epoch_nseconds = system_time.subsec_nanos();
        
                // Update the atime metadata of the file
                let _ = self.data_store.hset_multiple(
                    &format!("{}{}", hash_tag, path),
                    &[
                        ("access_time_secs", &epoch_seconds.to_string()),
                        ("access_time_nsecs", &epoch_nseconds.to_string()),
                    ]
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            }
            set_atime::SET_TO_CLIENT_TIME(nfstime3 { seconds, nseconds }) => {
                // Update the atime metadata of the file with client-provided time
                let _ = self.data_store.hset_multiple(
                    &format!("{}{}", hash_tag, path),
                    &[
                        ("access_time_secs", &seconds.to_string()),
                        ("access_time_nsecs", &nseconds.to_string()),
                    ]
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            }
            _ => {}
        };

        match setattr.mtime {
            set_mtime::SET_TO_SERVER_TIME => {
                let system_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap();
                let epoch_seconds = system_time.as_secs();
                let epoch_nseconds = system_time.subsec_nanos();
        
                // Update the atime metadata of the file
                let _ = self.data_store.hset_multiple(
                    &format!("{}{}", hash_tag, path),
                    &[
                        ("modification_time_secs", &epoch_seconds.to_string()),
                        ("modification_time_nsecs", &epoch_nseconds.to_string()),
                    ],
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            }
            set_mtime::SET_TO_CLIENT_TIME(nfstime3 { seconds, nseconds }) => {
                // Update the atime metadata of the file with client-provided time
                let _ = self.data_store.hset_multiple(
                    &format!("{}{}", hash_tag, path),
                    &[
                        ("modification_time_secs", &seconds.to_string()),
                        ("modification_time_nsecs", &nseconds.to_string()),
                    ],
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            }
            _ => {}
        };

        if let set_mode3::mode(mode) = setattr.mode {
            debug!(" -- set permissions {:?} {:?}", path, mode);
            let mode_value = Self::mode_unmask_setattr(mode);

            // Update the permissions metadata of the file in the share store
            let _ = self.data_store.hset(
                &format!("{}{}", hash_tag, path),
                "permissions",
                &mode_value.to_string()
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            
        }

        if let set_size3::size(size3) = setattr.size {
            debug!(" -- set size {:?} {:?}", path, size3);
    
            // Update the size metadata of the file in the share store
            let _hset_result = self.data_store.hset(
                &format!("{}{}", hash_tag, path),
                "size",
                &size3.to_string()
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
        }
        
        
        let metadata = self.get_metadata_from_id(id).await?;

        //FileMetadata::metadata_to_fattr3(id, &metadata)
        let fattr = FileMetadata::metadata_to_fattr3(id, &metadata).await?;

        Ok(fattr)
    }

    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {

        // println!("Starting write operation for file ID: {}, offset: {}, data length: {}", id, offset, data.len());

        let (_user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;

        let path_result = self.get_path_from_id(id).await;
        let path: String = match path_result {
            Ok(k) => k,
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),  // Replace with appropriate nfsstat3 error
        };

        let _is_complete = {

        let mut active_writes = self.active_writes.lock().await;
        let write = active_writes.entry(id).or_insert_with(|| ActiveWrite {
            channel: ChannelBuffer::new(),
            last_activity: Instant::now(),
        });
    
        

        if write.channel.is_empty().await && offset > 0 {
            self.load_existing_content(id, &write.channel).await?;
        }

        //Perform the write operation
        write.channel.write(offset, data).await;

        write.last_activity = Instant::now();
        // write.last_activity = std::time::Instant::now().into();
        
        let total_size = write.channel.total_size();
       

       let _  = self.data_store.hset(&format!("{}{}", hash_tag, path), "size", &total_size.to_string()).await.map_err(|_| nfsstat3::NFS3ERR_IO);

        };

        
        // Check if this might be the last write
        if self.is_likely_last_write(id, offset, data.len()).await? {

            self.mark_write_as_complete(id).await?;

        }

        // Get the current local date and time
        let local_date_time: DateTime<Local> = Local::now();

        // Format the date and time using the specified pattern
        let creation_time = local_date_time.format("%b %d %H:%M:%S %Y").to_string();

        let mut user = "";

            let parts: Vec<&str> = path.split('/').collect();

                if parts.len() > 2 {
                    user = parts[1];
                }

        if let Some(blockchain_audit) = &self.blockchain_audit {
            let _ = blockchain_audit.trigger_event(&creation_time, "disassembled", &path, &user);
        }

        let metadata = self.get_metadata_from_id(id).await?;

        let fattr = FileMetadata::metadata_to_fattr3(id, &metadata).await?;

        Ok(fattr)
    }

    async fn create(&self, dirid: fileid3, filename: &filename3, setattr: sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
                
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;
        
        // Get parent directory path from the share store
        let parent_path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, user_id),
            &dirid.to_string()
        ).await
            .unwrap_or_else(|_| String::new());
        

        if parent_path.is_empty() {
            return Err(nfsstat3::NFS3ERR_NOENT); // No such directory id exists
        }

        let objectname_osstr = OsStr::from_bytes(filename).to_os_string();
        
        let new_file_path: String;
        
        if parent_path == "/" {
            new_file_path = format!("/{}", objectname_osstr.to_str().unwrap_or(""));
        } else {
            new_file_path = format!("{}/{}", parent_path, objectname_osstr.to_str().unwrap_or(""));
        }

        // Check if file already exists
        let exists: bool = match self.data_store.zscore(
            &format!("{}/{}_nodes", hash_tag, user_id),
            &new_file_path
        ).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                eprintln!("Error checking if file exists: {:?}", e);
                false
            }
        };
    
        
        if exists {
            return Err(nfsstat3::NFS3ERR_EXIST);
        }

        // Create new file ID
        let new_file_id: fileid3 = match self.data_store.incr(
            &format!("{}/{}_next_fileid", hash_tag, user_id)
        ).await {
            Ok(id) => id.try_into().unwrap(),
            Err(e) => {
                eprintln!("Error incrementing file ID: {:?}", e);
                return Err(nfsstat3::NFS3ERR_IO);
            }
        };

        let _ = self.create_file_node("1", new_file_id, &new_file_path, setattr).await;
        let metadata = self.get_metadata_from_id(new_file_id).await?;
        Ok((new_file_id, FileMetadata::metadata_to_fattr3(new_file_id, &metadata).await?))
        
    }

    async fn create_exclusive(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {


        {

            //let mut conn = self.pool.get_connection();             

            //let (user_id, hash_tag, new_file_path,new_file_id ) = {
                
                let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;
                
                // Get parent directory path from the share store
                let parent_path: String = self.data_store.hget(
                    &format!("{}/{}_id_to_path", hash_tag, user_id),
                    &dirid.to_string()
                ).await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            
                // if parent_path.is_empty() {
                //     return Err(nfsstat3::NFS3ERR_NOENT); // No such directory id exists
                // }
        
                let objectname_osstr = OsStr::from_bytes(filename).to_os_string();
                
                let new_file_path: String;
                
                if parent_path == "/" {
                    new_file_path = format!("/{}", objectname_osstr.to_str().unwrap_or(""));
                } else {
                    new_file_path = format!("{}/{}", parent_path, objectname_osstr.to_str().unwrap_or(""));
                }

                let exists: bool = match self.data_store.zscore(
                    &format!("{}/{}_nodes", hash_tag, user_id),
                    &new_file_path
                ).await {
                    Ok(Some(_)) => true,
                    Ok(None) => false,
                    Err(e) => {
                        eprintln!("Error checking if file exists: {:?}", e);
                        false
                    }
                };
                
                if exists {

                    let fields_result = self.data_store.hget(
                        &format!("{}/{}_path_to_id", hash_tag, user_id),
                        &new_file_path
                    ).await;
                    match fields_result {
                        Ok(value) => {
                            // File already exists, return the existing file ID
                            return Ok(value.parse::<u64>().map_err(|_| nfsstat3::NFS3ERR_IO)?);
                        }
                        Err(_) => {
                            // Handle the error case, e.g., return NFS3ERR_IO or any other appropriate error
                            return Err(nfsstat3::NFS3ERR_IO);
                        }
                    }
                    //return Err(nfsstat3::NFS3ERR_NOENT);
                }

                // Create new file ID
                let new_file_id: fileid3 = match self.data_store.incr(
                    &format!("{}/{}_next_fileid", hash_tag, user_id)
                ).await {
                    Ok(id) => id.try_into().unwrap(),
                    Err(e) => {
                        eprintln!("Error incrementing file ID: {:?}", e);
                        return Err(nfsstat3::NFS3ERR_IO);
                    }
                };
            
            let _ = self.create_node("1", new_file_id, &new_file_path).await;

            Ok(new_file_id)
            
        }
        
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {       
        let parent_path = format!("{}", self.get_path_from_id(dirid).await?);
        let objectname_osstr = OsStr::from_bytes(filename).to_os_string();           
        let new_dir_path: String;

        // Construct the full path of the file/directory
        if parent_path == "/" {
            new_dir_path = format!("/{}", objectname_osstr.to_str().unwrap_or(""));
        } else {
            new_dir_path = format!("{}/{}", parent_path, objectname_osstr.to_str().unwrap_or(""));
        }

        let ftype_result = self.get_ftype(new_dir_path.clone()).await;
        
        match ftype_result {
        Ok(ftype) => {
            if ftype == "0" {
                self.remove_directory_file(&new_dir_path).await?;
            } else if ftype == "1" || ftype == "2"{
                self.remove_directory_file(&new_dir_path).await?;
            } else if ftype == "2"{
                self.remove_directory_file(&new_dir_path).await?;
            }
            else {
                return Err(nfsstat3::NFS3ERR_IO);
            }
        },
        Err(_) => return Err(nfsstat3::NFS3ERR_IO),
        }
            
        Ok(())
    }

    async fn rename(&self, from_dirid: fileid3, from_filename: &filename3, to_dirid: fileid3, to_filename: &filename3) -> Result<(), nfsstat3> {
                
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;
        
        let from_path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, user_id),
            &from_dirid.to_string()
        ).await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
        
        let objectname_osstr = OsStr::from_bytes(&from_filename).to_os_string();
    
        let new_from_path: String;

        // Construct the full path of the file/directory
        if from_path == "/" {
            new_from_path = format!("/{}", objectname_osstr.to_str().unwrap_or(""));
        } else {
            new_from_path = format!("{}/{}", from_path, objectname_osstr.to_str().unwrap_or(""));
        }

            // Check if the source file exists in the share store
            let from_exists: bool = match self.data_store.zscore(
                &format!("{}/{}_nodes", hash_tag, user_id),
                &new_from_path
            ).await {
                Ok(Some(_)) => true,
                Ok(None) => false,
                Err(e) => {
                    eprintln!("Error checking if source file exists: {:?}", e);
                    false
                }
            };
            
            if !from_exists {
                return Err(nfsstat3::NFS3ERR_NOENT);
            }

        let to_path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, user_id),
            &to_dirid.to_string()
        ).await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
        
        let objectname_osstr = OsStr::from_bytes(&to_filename).to_os_string();
    
        let new_to_path: String;

        // Construct the full path of the file/directory
        if to_path == "/" {
            new_to_path = format!("/{}", objectname_osstr.to_str().unwrap_or(""));
        } else {
            new_to_path = format!("{}/{}", to_path, objectname_osstr.to_str().unwrap_or(""));
        }
            
        let ftype_result = self.get_ftype(new_from_path.clone()).await;
        match ftype_result {
            Ok(ftype) => {
                if ftype == "0" {
                    self.rename_directory_file(&new_from_path, &new_to_path).await?;
                } else if ftype == "1" || ftype == "2"{
                    self.rename_directory_file(&new_from_path, &new_to_path).await?;
                } else {
                    return Err(nfsstat3::NFS3ERR_IO);
                }
            },
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),
            }
            
        Ok(())
    }

    async fn mkdir(&self, dirid: fileid3, dirname: &filename3) -> Result<(fileid3, fattr3), nfsstat3> {
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;
        let key1 = format!("{}/{}_id_to_path", hash_tag, user_id);

        // Get parent directory path from the share store
        let parent_path: String = self.data_store.hget(
            &key1,
            &dirid.to_string()
        ).await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        if parent_path.is_empty() {
            return Err(nfsstat3::NFS3ERR_NOENT); // No such directory id exists
        }


        let objectname_osstr = OsStr::from_bytes(dirname).to_os_string();
        let new_dir_path: String;
        
        if parent_path == "/" {
            new_dir_path = format!("/{}", objectname_osstr.to_str().unwrap_or(""));
        } else {
            new_dir_path = format!("{}/{}", parent_path, objectname_osstr.to_str().unwrap_or(""));
        }

        // let key2 = format!("{}/{}_path_to_id", hash_tag, user_id);

        // Check if directory already exists
        let exists: bool = match self.data_store.zscore(
            &format!("{}/{}_nodes", hash_tag, user_id),
            &new_dir_path
        ).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                eprintln!("Error checking if directory exists: {:?}", e);
                false
            }
        };
        
        if exists {
            return Err(nfsstat3::NFS3ERR_EXIST);
        }

        // Create new directory ID
        let key = format!("{}/{}_next_fileid", hash_tag, user_id);
    
        let new_dir_id: fileid3 = match self.data_store.incr(&key).await {
            Ok(id) => {
                //println!("New directory ID: {}", id);
                id.try_into().unwrap()
            }
            Err(e) => {
                eprintln!("Error incrementing directory ID: {:?}", e);
                return Err(nfsstat3::NFS3ERR_IO);
            }
        };

        let _ = self.create_node("0", new_dir_id, &new_dir_path).await;
        let metadata = self.get_metadata_from_id(new_dir_id).await?;
        Ok((new_dir_id, FileMetadata::metadata_to_fattr3(new_dir_id, &metadata).await?))
        
    }

    async fn symlink(&self, dirid: fileid3, linkname: &filename3, symlink: &nfspath3, attr: &sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
        // Validate input parameters
    if linkname.is_empty() || symlink.is_empty() {
        return Err(nfsstat3::NFS3ERR_INVAL);
    }

    
    //let mut conn = self.pool.get_connection();  

    let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;   

    // Get the current system time for metadata timestamps
    let system_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos(); // Capture nanoseconds part
        

    // Get the directory path from the directory ID
    let dir_path: String = self.data_store.hget(
        &format!("{}/{}_id_to_path", hash_tag, user_id),
        &dirid.to_string()
    ).await
        .map_err(|_| nfsstat3::NFS3ERR_IO)?;

    //Convert symlink to string
    let symlink_osstr = OsStr::from_bytes(symlink).to_os_string();

    // Construct the full symlink path
    let objectname_osstr = OsStr::from_bytes(linkname).to_os_string();
                
        let symlink_path: String;
                
        if dir_path == "/" {
            symlink_path = format!("/{}", objectname_osstr.to_str().unwrap_or(""));
        } else {
            symlink_path = format!("{}/{}", dir_path, objectname_osstr.to_str().unwrap_or(""));
        }

        let symlink_exists: bool = match self.data_store.zscore(
            &format!("{}/{}_nodes", hash_tag, user_id),
            &symlink_path
        ).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                eprintln!("Error checking if symlink exists: {:?}", e);
                false
            }
        };

    if symlink_exists {
        return Err(nfsstat3::NFS3ERR_EXIST);
    }

    // Generate a new file ID for the symlink
    let symlink_id: fileid3 = match self.data_store.incr(
        &format!("{}/{}_next_fileid", hash_tag, user_id)
    ).await {
        Ok(id) => id.try_into().unwrap(),
        Err(e) => {
            eprintln!("Error incrementing symlink ID: {:?}", e);
            return Err(nfsstat3::NFS3ERR_IO);
        }
    };

    // Begin a share store transaction to ensure atomicity

    let score = (symlink_path.matches('/').count() as f64) + 1.0;
    let _ = self.data_store.zadd(
        &format!("{}/{}_nodes", hash_tag, user_id),
        &symlink_path,
        score
    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
    
        let _ = self.data_store.hset_multiple(
            &format!("{}{}", hash_tag, &symlink_path),
            &[
                ("ftype", "2"),
                ("size", &symlink.len().to_string()),
                //("permissions", attr.mode as u64),
                ("change_time_secs", &epoch_seconds.to_string()),
                ("change_time_nsecs", &epoch_nseconds.to_string()),
                ("modification_time_secs", &epoch_seconds.to_string()),
                ("modification_time_nsecs", &epoch_nseconds.to_string()),
                ("access_time_secs", &epoch_seconds.to_string()),
                ("access_time_nsecs", &epoch_nseconds.to_string()),
                ("birth_time_secs", &epoch_seconds.to_string()),
                ("birth_time_nsecs", &epoch_nseconds.to_string()),
                ("fileid", &symlink_id.to_string())
            ]
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

            if let set_mode3::mode(mode) = attr.mode {
                //debug!(" -- set permissions {:?} {:?}", symlink_path, mode);
                let mode_value = Self::mode_unmask_setattr(mode);
    
                // Update the permissions metadata of the file in the share store
                let _ = self.data_store.hset(
                    &format!("{}{}", hash_tag, symlink_path),
                    "permissions",
                    &mode_value.to_string()
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
                
            }

        let _ = self.data_store.hset(
            &format!("{}{}", hash_tag, symlink_path),
            "symlink_target",
            symlink_osstr.to_str().unwrap_or_default()
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
        
        let _ = self.data_store.hset(
            &format!("{}/{}_path_to_id", hash_tag, user_id),
            &symlink_path,
            &symlink_id.to_string()
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

        let _ = self.data_store.hset(
            &format!("{}/{}_id_to_path", hash_tag, user_id),
            &symlink_id.to_string(),
            &symlink_path
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

        let metadata = self.get_metadata_from_id(symlink_id).await?;

        Ok((symlink_id, FileMetadata::metadata_to_fattr3(symlink_id, &metadata).await?))
        
    }

    async fn readlink(&self, id: fileid3) -> Result<nfsstring, nfsstat3> {
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;
    
        // Retrieve the path from the file ID
        let path: String = match self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, user_id),
            &id.to_string()
        ).await {
            Ok(path) => path,
            Err(e) => {
                eprintln!("Error retrieving path for ID {}: {:?}", id, e);
                return Err(nfsstat3::NFS3ERR_STALE);
            }
        };
    
        // Retrieve the symlink target using the path
        let symlink_target: String = match self.data_store.hget(
            &format!("{}{}", hash_tag, path),
            "symlink_target"
        ).await {
            Ok(target) => target,
            Err(e) => {
                eprintln!("Error retrieving symlink target: {:?}", e);
                return Err(nfsstat3::NFS3ERR_IO);
            }
        };
    
        if symlink_target.is_empty() {
            Err(nfsstat3::NFS3ERR_INVAL) // Path exists but isn't a symlink (or missing target)
        } else {
            Ok(nfsstring::from(symlink_target.into_bytes()))
        }
    }
}
const HOSTPORT: u32 = 2049;

async fn set_user_id_and_hashtag() {
    let mut user_id = USER_ID.write().unwrap();
    *user_id = "graymamba".to_string();

    let mut hash_tag = HASH_TAG.write().unwrap();
    hash_tag.clear(); // Clear the previous content
    hash_tag.push_str(&format!("{{{}}}:", user_id));
}

fn other_function() {
    // Acquire lock on USER_ID
    let user_id = USER_ID.read().unwrap();

    // Acquire lock on HASH_TAG
    let hash_tag = HASH_TAG.read().unwrap();

    // Use the values stored in USER_ID and HASH_TAG
    println!("User ID: {}", *user_id);
    println!("Hash Tag: {}", *hash_tag);
}

#[tokio::main]
async fn main() {
    // Load settings from the configuration file
    let mut settings = Config::default();
    settings
        .merge(ConfigFile::with_name("config/settings.toml"))
        .expect("Failed to load configuration");

    // Retrieve log level from the configuration
    let log_level = settings
        .get::<String>("logging.level")
        .unwrap_or_else(|_| "warn".to_string());

    // Convert string to Level
    let level = match log_level.to_lowercase().as_str() {
        "error" => tracing::Level::ERROR,
        "warn" => tracing::Level::WARN,
        "info" => tracing::Level::INFO,
        "debug" => tracing::Level::DEBUG,
        "trace" => tracing::Level::TRACE,
        _ => tracing::Level::WARN, // Default to WARN if invalid
    };

    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .init();

    set_user_id_and_hashtag().await;
    other_function();
    
    use graymamba::redis_data_store::RedisDataStore;
    let data_store = Arc::new(RedisDataStore::new().expect("Failed to create a data store"));

    let blockchain_audit = if settings.get("enable_blockchain").unwrap_or(false) {
        match BlockchainAudit::new().await {
            Ok(module) => Some(Arc::new(module)),
            Err(e) => {
                eprintln!("Failed to create BlockchainAudit: {}", e);
                None
            }
        }
    } else {
        None
    };
    let fs = SharesFS::new(data_store, blockchain_audit);
    warn!("Created new SharesFS with data_store");
    let listener = NFSTcpListener::bind(&format!("0.0.0.0:{HOSTPORT}"), fs)
        .await
        .unwrap();
    listener.handle_forever().await.unwrap();
}    //Initialize NFSModule

