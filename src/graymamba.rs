use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::sync::{Arc, RwLock};
// use tokio::sync::RwLock as OtherRwLock;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::{Duration, Instant};

use std::ops::Bound;
use std::os::unix::ffi::OsStrExt;
// use std::sync::atomic::AtomicU64;
use regex::Regex;
use chrono::{Local, DateTime};

use std::os::unix::fs::PermissionsExt;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
//use futures::future::ok;
// use intaglio::osstr::SymbolTable;
use tracing::debug;

use graymamba::blockchain_audit::BlockchainAudit;
//use graymamba::fs_util::*;
use graymamba::nfs::*;
use graymamba::nfs::nfsstat3;
use graymamba::tcp::{NFSTcp, NFSTcpListener};
use graymamba::vfs::{DirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};
use graymamba::channel_buffer;

use crate::channel_buffer::{ChannelBuffer, ActiveWrite};

use graymamba::data_store::{DataStore,DataStoreError,DataStoreResult};

use wasmtime::*;

extern crate secretsharing;
use secretsharing::{disassemble, reassemble};

use config::{Config, File as ConfigFile};
use base64::{Engine as _, engine::general_purpose::STANDARD};

lazy_static::lazy_static! {static ref USER_ID: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));}

lazy_static::lazy_static! {static ref HASH_TAG: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));}

#[derive(Debug, Clone)]
struct FileMetadata {

    // Common metadata fields
    ftype: u8,      // 0 for directory, 1 for file, 2 for Sybolic link
    size: u64,
    permissions: u32,
    access_time_secs: u32,
    access_time_nsecs: u32,
    change_time_secs: u32,
    change_time_nsecs: u32,
    modification_time_secs: u32,
    modification_time_nsecs: u32,
    #[allow(dead_code)]
    fileid: fileid3,

}

impl FileMetadata {
    // Constructor method to create FileMetadata for files
    #[allow(dead_code)]
    fn new(ftype: u8, permissions: u32, size: u64, access_time_secs: u32, access_time_nsecs: u32, change_time_secs: u32, change_time_nsecs: u32, modification_time_secs: u32, modification_time_nsecs: u32, fileid: u64) -> Self {
        FileMetadata {
            ftype,
            permissions,
            size,
            access_time_secs,
            access_time_nsecs,
            change_time_secs,
            change_time_nsecs,
            modification_time_secs,
            modification_time_nsecs,
            fileid,
        }
    }

    async fn mode_unmask(mode: u32) -> u32 {
        let mode = mode | 0x80;
        let permissions = std::fs::Permissions::from_mode(mode);
        permissions.mode() & 0x1FF
    }

    
    pub async fn metadata_to_fattr3(fid: fileid3, metadata: &FileMetadata) -> Result<fattr3, nfsstat3> {
        let size = metadata.size;
        let file_mode = Self::mode_unmask(metadata.permissions);
        let ftype = match metadata.ftype {
            0 => ftype3::NF3DIR,
            1 => ftype3::NF3REG,
            2 => ftype3::NF3LNK,
            _ => return Err(nfsstat3::NFS3ERR_INVAL),
        };
        
        Ok(fattr3 {
            ftype,
            mode: file_mode.await,
            nlink: 1,
            uid: 0,
            gid: 0,
            size,
            used: size,
            fsid: 0,
            fileid: fid,
            rdev: specdata3::default(),
            atime: nfstime3 {
                seconds: metadata.access_time_secs as u32,
                nseconds: metadata.access_time_nsecs as u32,
            },
            mtime: nfstime3 {
                seconds: metadata.modification_time_secs as u32,
                nseconds: metadata.modification_time_nsecs as u32,
            },
            ctime: nfstime3 {
                seconds: metadata.change_time_secs as u32,
                nseconds: metadata.change_time_nsecs as u32,
            },
        })
    }
}

//#[derive(Debug)]
// 
#[derive(Clone)]
pub struct MirrorFS {
    data_store: Arc<dyn DataStore>,
    blockchain_audit: Option<Arc<BlockchainAudit>>, // Add NFSModule wrapped in Arc
    active_writes: Arc<Mutex<HashMap<fileid3, ActiveWrite>>>,
    commit_semaphore: Arc<Semaphore>,
    
}

impl MirrorFS {
    pub fn new(data_store: Arc<dyn DataStore>, blockchain_audit: Option<Arc<BlockchainAudit>>) -> MirrorFS {
        // Create shared components for active writes
        let active_writes = Arc::new(Mutex::new(HashMap::new()));
        let commit_semaphore = Arc::new(Semaphore::new(10)); // Adjust based on your system's capabilities

        let mirror_fs = MirrorFS {
            data_store,
            blockchain_audit,
            active_writes: active_writes.clone(),
            commit_semaphore: commit_semaphore.clone(),
        };

        // Start the background task to monitor active writes
        let mirror_fs_clone = mirror_fs.clone();
        tokio::spawn(async move {
            mirror_fs_clone.monitor_active_writes().await;
        });

        mirror_fs
    }

    fn mode_unmask_setattr(mode: u32) -> u32 {
        let mode = mode | 0x80;
        let permissions = std::fs::Permissions::from_mode(mode);
        permissions.mode() & 0x1FF
    }

    async fn get_path_from_id(&self, id: fileid3) -> Result<String, nfsstat3> {

        let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;

        let key = format!("{}/{}_id_to_path", hash_tag, user_id);
          
        let path: String = {
            self.data_store
                .hget(&key, &id.to_string())
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO)?
        };

        Ok(path)
    }

    /// Get the ID for a given file/directory path
    async fn get_id_from_path(&self, path: &str) -> Result<fileid3, nfsstat3> {
       
        let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;

        let key = format!("{}/{}_path_to_id", hash_tag, user_id);

        let id_str: String = self.data_store
        .hget(&key, path)
        .await
        .map_err(|_| nfsstat3::NFS3ERR_IO)?;
    
        let id: fileid3 = id_str.parse()
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        Ok(id)
    }

    /// Get the metadata for a given file/directory ID
    async fn get_metadata_from_id(&self, id: fileid3) -> Result<FileMetadata, nfsstat3> {
        let path = self.get_path_from_id(id).await?;
        
        // Acquire lock on HASH_TAG
        //let hash_tag = HASH_TAG.lock().unwrap();
        let hash_tag = HASH_TAG.read().unwrap().clone();
        
        // Construct the share store key for metadata
        let metadata_key = format!("{}{}", hash_tag, path);

        let metadata_vec = self.data_store.hgetall(&metadata_key).await
        .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        if metadata_vec.is_empty() {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }

        let metadata: HashMap<String, String> = metadata_vec.into_iter().collect();
        // Parse metadata fields and construct FileMetadata object
        let file_metadata = FileMetadata {
            // Extract metadata fields from the HashMap
            ftype: metadata.get("ftype").and_then(|s| s.parse::<u8>().ok()).unwrap_or(0),
            permissions: metadata.get("permissions").and_then(|s| s.parse().ok()).unwrap_or(0), // Assuming permissions are stored as integer
            size: metadata.get("size").and_then(|s| s.parse().ok()).unwrap_or(0), // Assuming size is stored as integer
           
            access_time_secs: metadata.get("access_time_secs").and_then(|s| s.parse().ok()).unwrap_or(0),
            access_time_nsecs: metadata.get("access_time_nsecs").and_then(|s| s.parse().ok()).unwrap_or(0),
            
            
            modification_time_secs: metadata.get("modification_time_secs").and_then(|s| s.parse().ok()).unwrap_or(0),
            modification_time_nsecs: metadata.get("modification_time_nsecs").and_then(|s| s.parse().ok()).unwrap_or(0),
            
            change_time_secs: metadata.get("change_time_secs").and_then(|s| s.parse().ok()).unwrap_or(0),
            change_time_nsecs: metadata.get("change_time_nsecs").and_then(|s| s.parse().ok()).unwrap_or(0),
           

            fileid: metadata.get("fileid").and_then(|s| s.parse().ok()).unwrap_or(0), // Assuming fileid is stored as integer
        };

        Ok(file_metadata)
        
    }

    async fn get_direct_children(&self, path: &str) -> Result<Vec<fileid3>, nfsstat3> {
        // Get nodes in the specified subpath

        let nodes_in_subpath: Vec<String> = self.get_nodes_in_subpath(path).await?;

        let mut direct_children = Vec::new();
        for node in nodes_in_subpath {
            if self.is_direct_child(&node, path).await {
                let id_result = self.get_id_from_path(&node).await;
                match id_result {
                    Ok(id) => direct_children.push(id),
                    Err(_) => return Err(nfsstat3::NFS3ERR_IO),
                }
            }
        }
        Ok(direct_children)
    }

    async fn get_nodes_in_subpath(&self, subpath: &str) -> Result<Vec<String>, nfsstat3> {
        
        let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;

        let key = format!("{}/{}_nodes", hash_tag, user_id);

        // let pool_guard = shared_pool.lock().unwrap();
        // let mut conn = pool_guard.get_connection();
        
        if subpath == "/" {
            // If the subpath is the root, return nodes with a score of ROOT_DIRECTORY_SCORE
            // Await the async operation and then transform the Ok value
            let nodes_result: Vec<String> = self.data_store.zrangebyscore(&key, 2.0, 2.0).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;
            Ok(nodes_result.into())
            
        } else {
            // Calculate start and end scores based on the hierarchy level
            let start_score = subpath.split("/").count() as f64;
            let end_score = start_score + 1.0;

            // Retrieve nodes at the specified hierarchy level
            // Await the async operation and then transform the Ok value
            let nodes_result: Vec<String> = self.data_store.zrangebyscore(&key, end_score, end_score).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;
            Ok(nodes_result.into())
            
        }
    }

    async fn is_direct_child(&self, node: &str, path: &str) -> bool {
        // Check if the node is a direct child of the specified path
        // For example, if path is "/asif", check if node is "/asif/something"
        if path == "/" {
            node.contains("/")
        } else {
            node.starts_with(&format!("{}{}", path, "/")) && !node[(path.len() + 1)..].contains("/")
        }
    }

    async fn get_last_path_element(&self, input: String) -> String {
        let elements: Vec<&str> = input.split("/").collect();
        
        if elements.is_empty() {
            return String::from("");
        }
        
        let last_element = elements[elements.len() - 1];
        if last_element == "/" {
            return String::from("/");
        } else {
            return last_element.to_string();
        }
    }

    async fn create_node(&self, node_type: &str, fileid: fileid3, path: &str) -> DataStoreResult<()> {
        let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;
      
        let size = 0;
        let permissions = 777;
        let score: f64 = path.matches('/').count() as f64 + 1.0;  
        
        let system_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos(); // Capture nanoseconds part
        
        self.data_store.zadd(
            &format!("{}/{}_nodes", hash_tag, user_id),
            &path,
            score
        ).await?;
    
        self.data_store.hset_multiple(&format!("{}{}", hash_tag, path), &[
            ("ftype", node_type),
            ("size", &size.to_string()),
            ("permissions", &permissions.to_string()),
            ("change_time_secs", &epoch_seconds.to_string()),
            ("change_time_nsecs", &epoch_nseconds.to_string()),
            ("modification_time_secs", &epoch_seconds.to_string()),
            ("modification_time_nsecs", &epoch_nseconds.to_string()),
            ("access_time_secs", &epoch_seconds.to_string()),
            ("access_time_nsecs", &epoch_nseconds.to_string()),
            ("birth_time_secs", &epoch_seconds.to_string()),
            ("birth_time_nsecs", &epoch_nseconds.to_string()),
            ("fileid", &fileid.to_string())
            ]).await?;
        
            self.data_store.hset(&format!("{}/{}_path_to_id", hash_tag, user_id), path, &fileid.to_string()).await?;
            self.data_store.hset(&format!("{}/{}_id_to_path", hash_tag, user_id), &fileid.to_string(), path).await?;
            
        Ok(())
    }
    
    async fn create_file_node(&self, node_type: &str, fileid: fileid3, path: &str, setattr: sattr3,) -> DataStoreResult<()> {
       
        let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;
      
        let size = 0;
        let score: f64 = path.matches('/').count() as f64 + 1.0;  
        
        let system_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos(); // Capture nanoseconds part
        
        let _ = self.data_store.zadd(
            &format!("{}/{}_nodes", hash_tag, user_id),
            &path,
            score
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

        let _ = self.data_store.hset_multiple(&format!("{}{}", hash_tag, path), 
    &[
            ("ftype", node_type),
            ("size", &size.to_string()),
            //("permissions", &permissions.to_string()),
            ("change_time_secs", &epoch_seconds.to_string()),
            ("change_time_nsecs", &epoch_nseconds.to_string()),
            ("modification_time_secs", &epoch_seconds.to_string()),
            ("modification_time_nsecs", &epoch_nseconds.to_string()),
            ("access_time_secs", &epoch_seconds.to_string()),
            ("access_time_nsecs", &epoch_nseconds.to_string()),
            ("birth_time_secs", &epoch_seconds.to_string()),
            ("birth_time_nsecs", &epoch_nseconds.to_string()),
            ("fileid", &fileid.to_string())
            ]).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            if let set_mode3::mode(mode) = setattr.mode {
                debug!(" -- set permissions {:?} {:?}", path, mode);
                let mode_value = Self::mode_unmask_setattr(mode);
    
                // Update the permissions metadata of the file
                let _ = self.data_store.hset(
                    &format!("{}{}", hash_tag, path),
                    "permissions",
                    &mode_value.to_string()
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
                
            }
        // let _ = self.data_store.hset(&format!("{}{}", hash_tag, path), "data", "").await.map_err(|_| nfsstat3::NFS3ERR_IO);
        let _ = self.data_store.hset(&format!("{}/{}_path_to_id", hash_tag, user_id), path, &fileid.to_string()).await.map_err(|_| nfsstat3::NFS3ERR_IO);
        let _ = self.data_store.hset(&format!("{}/{}_id_to_path", hash_tag, user_id), &fileid.to_string(), path).await.map_err(|_| nfsstat3::NFS3ERR_IO);

        
        Ok(())
            
    }
    
    async fn get_ftype(&self, path: String) -> Result<String, nfsstat3> {
        // Acquire lock on HASH_TAG
        let hash_tag = HASH_TAG.read().unwrap().clone();
        let key = format!("{}{}", hash_tag, path.clone());

        let ftype_result = self.data_store.hget(&key, "ftype").await.map_err(|_| nfsstat3::NFS3ERR_IO);
        let ftype: String = match ftype_result {
            Ok(k) => k,
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),  // Replace with appropriate nfsstat3 error
        };   
        Ok(ftype)
    }

    async fn get_member_keys(&self, pattern: &str, sorted_set_key: &str) -> Result<bool, nfsstat3> {

        match self.data_store.zscan_match(sorted_set_key, pattern).await {
            Ok(iter) => {
                // Process the iterator
                // This part depends on what type your zscan_match returns
                let matching_keys: Vec<String> = iter
                .into_iter()
                .map(|key| key)
                .collect();
        
                if !matching_keys.is_empty() {
                    return Ok(true);
                }
            }
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),
        }
    
        Ok(false)
    }
    
    async fn remove_directory_file(&self, path: &str) -> Result<(), nfsstat3> {
            
            let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;
            let pattern = format!("{}/*", path);
            let sorted_set_key = format!("{}/{}_nodes", hash_tag, user_id);
            let match_found = self.get_member_keys(&pattern, &sorted_set_key).await?;
            if match_found {
                return Err(nfsstat3::NFS3ERR_NOTEMPTY);
            }

            let dir_id = self.data_store.hget(
                &format!("{}/{}_path_to_id", hash_tag, user_id),
                path
            ).await;
            
            let value: String = match dir_id {
                Ok(k) => k,
                Err(_) => return Err(nfsstat3::NFS3ERR_IO),
            };
            // Remove the directory         
            // Remove the node from the sorted set
            let _ = self.data_store.zrem(
                &format!("{}/{}_nodes", hash_tag, user_id),
                path
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
                    
            // Delete the metadata hash associated with the node
            let _ = self.data_store.delete(&format!("{}{}", hash_tag, path))
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO);
                 
            // Remove the directory from the path-to-id mapping
            let _ = self.data_store.hdel(
                &format!("{}/{}_path_to_id", hash_tag, user_id),
                path
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            
            // Remove the directory from the id-to-path mapping
            let _ = self.data_store.hdel(
                &format!("{}/{}_id_to_path", hash_tag, user_id),
                &value
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);             
             
            Ok(())
        
    }
    
    async fn rename_directory_file(&self, from_path: &str, to_path: &str) -> Result<(), nfsstat3> { 
        let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;
        //Rename the metadata hashkey
        let _ = self.data_store.rename(
            &format!("{}{}", hash_tag, from_path),
            &format!("{}{}", hash_tag, to_path)
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
        //Rename entries in hashset

        // Create a pattern to match all keys under the old path
        let pattern = format!("{}{}{}", hash_tag, from_path, "/*");
        
        // Retrieve all keys matching the pattern
        let keys_result = self.data_store.keys(&pattern)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO);

        let keys: Vec<String> = match keys_result {
            Ok(k) => k,
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),  // Replace with appropriate nfsstat3 error
        };

        // Compile a regex from the old path to replace only the first occurrence safely
        let re = Regex::new(&regex::escape(from_path)).unwrap();

        for key in keys {
            // Replace only the first occurrence of old_path with new_path
            let new_key = re.replace(&key, to_path).to_string();
            // Rename the key in the share store
            let _ = self.data_store.rename(&key, &new_key)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO);
        } 
        //Rename entries in sorted set (_nodes)
        let key = format!("{}/{}_nodes", hash_tag, user_id);

        // Retrieve all members of the sorted set with their scores
        let members_result = self.data_store.zrange_withscores(&key, 0, -1)
        .await
        .map_err(|_| nfsstat3::NFS3ERR_IO);

        let members: Vec<(String, f64)> = match members_result {
            Ok(k) => k,
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),
        };

        // Regex to ensure only the first occurrence of old_path is replaced
        let re = Regex::new(&regex::escape(from_path)).unwrap();

        for (directory_path, _score) in members {
            
            if directory_path == from_path {
                
                //Get the score from the path
                let new_score: f64 = to_path.matches('/').count() as f64 + 1.0; 

                // The entry is the directory itself, just replace it
                let zrem_result = self.data_store.zrem(&key, &directory_path)
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO);

                if zrem_result.is_err() {
                    return Err(nfsstat3::NFS3ERR_IO);  // Replace with appropriate nfsstat3 error
                }
                let zadd_result = self.data_store.zadd(
                    &key,
                    to_path,
                    new_score
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                if zadd_result.is_err() {
                    return Err(nfsstat3::NFS3ERR_IO);  // Replace with appropriate nfsstat3 error
                }
                
            } else if directory_path.starts_with(&(from_path.to_owned() + "/")) {
                
                // The entry is a subdirectory or file
                let new_directory_path = re.replace(&directory_path, to_path).to_string();

                //Get the score from the path
                let new_score: f64 = new_directory_path.matches('/').count() as f64 + 1.0; 

                // Check if the new path already exists in the sorted set
                if new_directory_path != directory_path {
                    
                    // If the new path doesn't exist, update it
                    let zrem_result = self.data_store.zrem(&key, &directory_path)
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO);

                    if zrem_result.is_err() {
                        return Err(nfsstat3::NFS3ERR_IO);
                    }
                    let zadd_result = self.data_store.zadd(
                        &key,
                        &new_directory_path,
                        new_score
                    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                    if zadd_result.is_err() {
                        return Err(nfsstat3::NFS3ERR_IO);  
                    }
                }
            }
        }
        //Rename entries in path_to_id and id_to_path hash
        let path_to_id_key = format!("{}/{}_path_to_id", hash_tag, user_id);
        let id_to_path_key = format!("{}/{}_id_to_path", hash_tag, user_id);

        // Retrieve all the members of path_to_id hash
        let fields_result = self.data_store.hgetall(&path_to_id_key)
        .await
        .map_err(|_| nfsstat3::NFS3ERR_IO);

        let fields: Vec<(String, String)> = match fields_result {
            Ok(k) => k,
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),  
        };


        // Regex to ensure only the first occurrence of old_path is replaced
        let re = Regex::new(&regex::escape(from_path)).unwrap();

        for (directory_path, value) in fields {
            let system_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap();
            let epoch_seconds = system_time.as_secs();
            let epoch_nseconds = system_time.subsec_nanos(); // Capture nanoseconds part

            if directory_path == to_path {
                let hdel_result_id_to_path = self.data_store.hdel(
                    &id_to_path_key,
                    &value
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                if hdel_result_id_to_path.is_err() {
                    return Err(nfsstat3::NFS3ERR_IO);
                }
            }

            if directory_path == from_path {
                // The entry is the directory itself, just replace it      
                let hdel_result_path_to_id = self.data_store.hdel(
                    &path_to_id_key,
                    &directory_path
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                if hdel_result_path_to_id.is_err() {
                    return Err(nfsstat3::NFS3ERR_IO); 
                }

                let hset_result_path_to_id = self.data_store.hset(
                    &path_to_id_key,
                    to_path,
                    &value.to_string()
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                if hset_result_path_to_id.is_err() {
                    return Err(nfsstat3::NFS3ERR_IO); 
                }

                let hdel_result_id_to_path = self.data_store.hdel(
                    &id_to_path_key,
                    &value
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
                if hdel_result_id_to_path.is_err() {
                    return Err(nfsstat3::NFS3ERR_IO); 
                }

                let hset_result_id_to_path = self.data_store.hset(
                    &id_to_path_key,
                    &value.to_string(),
                    to_path
                ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                if hset_result_id_to_path.is_err() {
                    return Err(nfsstat3::NFS3ERR_IO);
                }

                let _ = self.data_store.hset_multiple(&format!("{}{}", hash_tag, to_path),
                    &[
                        ("change_time_secs", &epoch_seconds.to_string()),
                        ("change_time_nsecs", &epoch_nseconds.to_string()),
                        ("modification_time_secs", &epoch_seconds.to_string()),
                        ("modification_time_nsecs", &epoch_nseconds.to_string()),
                        ("access_time_secs", &epoch_seconds.to_string()),
                        ("access_time_nsecs", &epoch_nseconds.to_string()),
                        // ("fileid", &new_file_id.to_string())
                        ("fileid", &value.to_string())
                    ])
                .await.map_err(|_| nfsstat3::NFS3ERR_IO);
                
            } else if directory_path.starts_with(&(from_path.to_owned() + "/")) {            
                // The entry is a subdirectory or file
                let new_directory_path = re.replace(&directory_path, to_path).to_string();

                // Check if the new path already exists in the path_to_id hash
                if new_directory_path != directory_path {
                    
                    // If the new path doesn't exist, update it
                    let hdel_result_path_to_id = self.data_store.hdel(
                        &path_to_id_key,
                        &directory_path
                    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                    if hdel_result_path_to_id.is_err() {
                        return Err(nfsstat3::NFS3ERR_IO);  // Replace with appropriate nfsstat3 error
                    }

                    let hset_result_path_to_id = self.data_store.hset(
                        &path_to_id_key,
                        &new_directory_path,
                        &value.to_string()
                    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                    if hset_result_path_to_id.is_err() {
                        return Err(nfsstat3::NFS3ERR_IO);  // Replace with appropriate nfsstat3 error
                    }

                    let hdel_result_id_to_path = self.data_store.hdel(
                        &id_to_path_key,
                        &value
                    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                    if hdel_result_id_to_path.is_err() {
                        return Err(nfsstat3::NFS3ERR_IO);  // Replace with appropriate nfsstat3 error
                    }

                    let hset_result_id_to_path = self.data_store.hset(
                        &id_to_path_key,
                        &value.to_string(),
                        &new_directory_path
                    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

                    if hset_result_id_to_path.is_err() {
                        return Err(nfsstat3::NFS3ERR_IO);  // Replace with appropriate nfsstat3 error
                    }

                    let _ = self.data_store.hset_multiple(&format!("{}{}", hash_tag, new_directory_path),
                        &[
                            ("change_time_secs", &epoch_seconds.to_string()),
                            ("change_time_nsecs", &epoch_nseconds.to_string()),
                            ("modification_time_secs", &epoch_seconds.to_string()),
                            ("modification_time_nsecs", &epoch_nseconds.to_string()),
                            ("access_time_secs", &epoch_seconds.to_string()),
                            ("access_time_nsecs", &epoch_nseconds.to_string()),
                            // ("fileid", &new_file_id.to_string())
                            ("fileid", &value.to_string())
                        ])
                    .await.map_err(|_| nfsstat3::NFS3ERR_IO);
                }
            }
        }
        Ok(())   
    }

    // async fn get_data(&self, path: &str) -> Vec<u8> {

    //     // Acquire lock on HASH_TAG 
    //     let hash_tag = HASH_TAG.read().unwrap().clone();

    //     if path.contains(".git/objects/pack/tmp_pack") {
    //         // Retrieve the existing data from the in-memory hashmap
    //         let hashmap = self.in_memory_hashmap.read().await;
    //         let data = hashmap.get(path).cloned().unwrap_or_default(); // This is initially a String
    //         if !data.is_empty() {
    //             // Directly decode the base64 string to a byte array
    //             match STANDARD.decode(&data) {
    //                 Ok(byte_array) => byte_array, // Use the Vec<u8> byte array as needed
    //                 Err(_) => Vec::new(), // Handle decoding error by returning an empty Vec<u8>
    //             }
    //         } else {
    //             Vec::new() // Return an empty Vec<u8> if no data is found
    //         }
    //     } else {
    //         // Retrieve the current file content (Base64 encoded) from the share store
    //         let sharestore_value: String = match self.data_store.hget(
    //             &format!("{}{}", hash_tag, path),
    //             "data"
    //         ).await {
    //             Ok(value) => value,
    //             Err(_) => String::default(), // or handle the error differently
    //         };
    //         if !sharestore_value.is_empty() {
    //                match reassemble(&sharestore_value).await {
    //                 Ok(reconstructed_secret) => {
    //                     // Decode the base64 string to a byte array
    //                     match STANDARD.decode(&reconstructed_secret) {
    //                         Ok(byte_array) => byte_array, // Use the Vec<u8> byte array as needed
    //                         Err(_) => Vec::new(), // Handle decoding error by returning an empty Vec<u8>
    //                     }
    //                 }
    //                 Err(_) => Vec::new(), // Handle the re_assembly error by returning an empty Vec<u8>
    //             }
    //         } else {
    //             Vec::new() // Return an empty Vec<u8> if no data is found
    //         }
    //     }
        
    // }

    // async fn write_data(&self, path: &str, data: Vec<u8>) -> Result<bool, DataStoreError> {
        
    //     // Acquire lock on HASH_TAG 
    //     let hash_tag = HASH_TAG.read().unwrap().clone();

    //     let base64_string = STANDARD.encode(&data); // Convert byte array (Vec<u8>) to a base64 encoded string

    //     if path.contains(".git/objects/pack/tmp_pack") {
    //         // Retrieve the existing data from the in-memory hashmap
    //         let mut data_block = self.in_memory_hashmap.write().await;
    //         data_block.insert(path.to_owned(), base64_string);
    //         Ok(true) // Operation successful
    //     } else {

    //         // Apply secret sharing to the secret
    //         match disassemble(&base64_string).await {
    //             Ok(shares) => {               
    //                 // Write the shares to share store
    //                     self.data_store.hset(
    //                         &format!("{}{}", hash_tag, path),
    //                         "data",
    //                         &shares
    //                     ).await
    //                     .map(|_| true)
    //                     .map_err(|_e| {
    //                         //eprintln!("Error writing to DataStore: {}", e);
    //                         DataStoreError::OperationFailed // Or use an appropriate error type
    //                     })
    //             },
    //             Err(e) => {
    //                 eprintln!("Error during disassembly: {}", e);
    //                 Err(DataStoreError::OperationFailed)
    //             }
    //         }
    //     }
        
    // }

    async fn get_user_id_and_hash_tag() -> (String, String) {
        let user_id = USER_ID.read().unwrap().clone();
        let hash_tag = HASH_TAG.read().unwrap().clone();
        (user_id, hash_tag)
    }

    async fn get_data(&self, path: &str) -> Vec<u8> {
     
        let (_user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;

        // Retrieve the current file content (Base64 encoded) from store
        let store_value: String = match self.data_store.hget(
            &format!("{}{}", hash_tag, path),
            "data"
        ).await {
            Ok(value) => value,
            Err(_) => String::default(), // or handle the error differently
        };
    if !store_value.is_empty() {
        match reassemble(&store_value).await {
            Ok(reconstructed_secret) => {
                    // Decode the base64 string to a byte array
                    match STANDARD.decode(&reconstructed_secret) {
                        Ok(byte_array) => byte_array, // Use the Vec<u8> byte array as needed
                        Err(_) => Vec::new(), // Handle decoding error by returning an empty Vec<u8>
                    }
                }
                Err(_) => Vec::new(), // Handle the re_assembly error by returning an empty Vec<u8>
            }
        } else {
            Vec::new() // Return an empty Vec<u8> if no data is found
        }
    }

    async fn load_existing_content(&self, id: fileid3, channel: &Arc<ChannelBuffer>) -> Result<(), nfsstat3> {
        
        if channel.is_empty().await {
            
            let path_result = self.get_path_from_id(id).await;
            let path: String = match path_result {
                Ok(k) => k,
                Err(_) => return Err(nfsstat3::NFS3ERR_IO),  // Replace with appropriate nfsstat3 error
            };

            let contents = self.get_data(&path).await;

            
            channel.write(0, &contents).await;
        }

        Ok(())
    }


    async fn monitor_active_writes(&self) {
        println!("Starting active writes monitor");
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            
            let to_commit: Vec<fileid3> = {
                let active_writes = self.active_writes.lock().await;
                let mut to_commit = Vec::new();
    
                for (&id, write) in active_writes.iter() {
                    if write.channel.is_write_complete() && write.channel.time_since_last_write().await > Duration::from_secs(10) {
                        to_commit.push(id);
                    }
                }
    
                to_commit
            };

            for id in to_commit {
                // println!("Attempting to commit write for file ID: {}", id);
                match self.commit_write(id).await {
                    Ok(()) => {
                        debug!("Successfully committed write for file ID: {}", id);
                    },
                    Err(e) => {
                        debug!("Error committing write for file ID: {}: {:?}", id, e);
                    }
                }
            }
        }
    }

    async fn commit_write(&self, id: fileid3) -> Result<(), DataStoreError> {

        debug!("Starting commit process for file ID: {}", id);

        let _permit = self.commit_semaphore.acquire().await.map_err(|_| DataStoreError::OperationFailed);

        let (_user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;


        let channel = {

            let mut active_writes = self.active_writes.lock().await;

            match active_writes.remove(&id) {

                Some(write) => write.channel,

                None => {

                    debug!("Commit called for non-existent write ID: {}", id);

                    return Ok(());
                }
            }
        };

        let path_result = self.get_path_from_id(id).await;
        let path: String = match path_result {
            Ok(k) => k,
            Err(_) => return Err(DataStoreError::OperationFailed),  // Replace with appropriate nfsstat3 error
        };   

        let contents = channel.read_all().await;


        let base64_contents = STANDARD.encode(&contents.to_vec()); // Convert byte array (Vec<u8>) to a base64 encoded string
        

        match disassemble(&base64_contents).await {
            Ok(shares) => {
                // Attempt to write the shares to the data store
                match self
                    .data_store
                    .hset(&format!("{}{}", hash_tag, path), "data", &shares)
                    .await
                {
                    Ok(_) => {
                        // Update file metadata upon successful storage
                        self.update_file_metadata(&path).await?;

                        // Clear the buffer contents after a successful commit
                        channel.clear().await;

                        Ok(())
                    }
                    Err(_e) => {
                        debug!("Error setting data in DataStore");
                        Err(DataStoreError::OperationFailed)
                    }
                }
            }
            Err(e) => {
                debug!("Shamir disassembly failed: {:?}", e);
                Err(DataStoreError::OperationFailed)
            }
        }

            
       
    }

    async fn update_file_metadata(&self, path: &str) -> Result<(), DataStoreError> {
        let system_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos();
        let (_user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;

        let update_result = self.data_store.hset_multiple(&format!("{}{}", hash_tag, path),
            &[
                ("change_time_secs", &epoch_seconds.to_string()),
                ("change_time_nsecs", &epoch_nseconds.to_string()),
                ("modification_time_secs", &epoch_seconds.to_string()),
                ("modification_time_nsecs", &epoch_nseconds.to_string()),
                ("access_time_secs", &epoch_seconds.to_string()),
                ("access_time_nsecs", &epoch_nseconds.to_string()),
            ]).await;
            
            match update_result {
                Ok(_) => Ok(()),
                Err(e) => {
                    eprintln!("Error updating file metadata in DataStore: {:?}", e);
                    Err(DataStoreError::OperationFailed)
                }
            }
            
    }

    async fn is_likely_last_write(&self, id: fileid3, offset: u64, data_len: usize) -> Result<bool, nfsstat3> {

        let (_user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;

        let path_result = self.get_path_from_id(id).await;
        let path: String = match path_result {
            Ok(k) => k,
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),  // Replace with appropriate nfsstat3 error
        };

        let current_size_result = self.data_store.hget(&format!("{}{}", hash_tag, path), "size").await;
        let current_size: u64 = match current_size_result {
            Ok(k) => k.parse::<u64>().map_err(|_| nfsstat3::NFS3ERR_IO)?,
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),  // Replace with appropriate nfsstat3 error
        };
            
        Ok(offset + data_len as u64 >= current_size)
    }

    async fn mark_write_as_complete(&self, id: fileid3) -> Result<(), nfsstat3> {
        let mut active_writes = self.active_writes.lock().await;
        if let Some(write) = active_writes.get_mut(&id) {
            write.channel.set_complete();
        }
        Ok(())
    }

}

#[async_trait]
impl NFSFileSystem for MirrorFS {
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
        
        {           

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
    }
    
    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {

      

        {         
       
        let metadata = self.get_metadata_from_id(id).await?;
       
        let path = self.get_path_from_id(id).await?;

        debug!("Stat {:?}: {:?}", path, &metadata);

        let fattr = FileMetadata::metadata_to_fattr3(id, &metadata).await?;
        
        Ok(fattr)

        }
        
    }

    async fn read(&self, id: fileid3, offset: u64, count: u32) -> Result<(Vec<u8>, bool), nfsstat3> {

        
        {
            //let mut conn = self.pool.get_connection();             

            let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;


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
            //Ok((contents, eof))
        }
   
    }

    async fn readdir(&self, dirid: fileid3, start_after: fileid3, max_entries: usize) -> Result<ReadDirResult, nfsstat3> {

        
        {            

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
       
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {

       
            {

            //let mut conn = self.pool.get_connection();             

            let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;

            {

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
            


        }
            
            let metadata = self.get_metadata_from_id(id).await?;

            //FileMetadata::metadata_to_fattr3(id, &metadata)
            let fattr = FileMetadata::metadata_to_fattr3(id, &metadata).await?;

            Ok(fattr)

            }
    }

    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {

        // println!("Starting write operation for file ID: {}, offset: {}, data length: {}", id, offset, data.len());

        let (_user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;

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


        {
            
            //let mut conn = self.pool.get_connection();             

    //        let (user_id, hash_tag, new_file_path,new_file_id ) = {
                
                let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;
                
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

//                (user_id, hash_tag, new_file_path, new_file_id) // Return the values needed outside the scope
//            };

            // if new_file_path.contains(".git/objects/pack/tmp_pack") {
            //     // Redirect to HashMap
            //     let mut hashmap = self.in_memory_hashmap.write().await;
            //     hashmap.clear();
            //     hashmap.insert(new_file_path.clone(), String::new());
                
            // }


            let _ = self.create_file_node("1", new_file_id, &new_file_path, setattr).await;

            let metadata = self.get_metadata_from_id(new_file_id).await?;
            
            // Get the current local date and time
            //let local_date_time: DateTime<Local> = Local::now();

            // Format the date and time using the specified pattern
            //let creation_time = local_date_time.format("%b %d %H:%M:%S %Y").to_string();

            // Send the event with the formatted creation time, event type, path, and user ID
            
            Ok((new_file_id, FileMetadata::metadata_to_fattr3(new_file_id, &metadata).await?))

        }
        
    }

    async fn create_exclusive(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {


        {

            //let mut conn = self.pool.get_connection();             

            //let (user_id, hash_tag, new_file_path,new_file_id ) = {
                
                let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;
                
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

            //    (user_id, hash_tag, new_file_path, new_file_id) // Return the values needed outside the scope
            //};

            // if new_file_path.contains(".git/objects/pack/tmp_pack") {
            //     // Redirect to HashMap
            //     let mut hashmap = self.in_memory_hashmap.write().await;
            //     hashmap.clear();
            //     hashmap.insert(new_file_path.clone(), String::new());
                
            // }

            
            let _ = self.create_node("1", new_file_id, &new_file_path).await;
            
            // Get the current local date and time
            //let local_date_time: DateTime<Local> = Local::now();

            // Format the date and time using the specified pattern
            //let creation_time = local_date_time.format("%b %d %H:%M:%S %Y").to_string();

            // Send the event with the formatted creation time, event type, path, and user ID

            Ok(new_file_id)
            
        }
        
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {
        {           
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
    }

    async fn rename(&self, from_dirid: fileid3, from_filename: &filename3, to_dirid: fileid3, to_filename: &filename3) -> Result<(), nfsstat3> {
      
        
        {
           
            //let mut conn = self.pool.get_connection();             

//            let (user_id, hash_tag, new_from_path, new_to_path) = {
                
                let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;
                
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

                    
                
    //            (user_id, hash_tag, new_from_path, new_to_path)
    //        };


            // if new_from_path.contains(".git/objects/pack/tmp_pack") {
            //     // Redirect to HashMap
            //     let hashmap = self.in_memory_hashmap.read().await;
             
            //     if let Some(key) = hashmap.keys().next() {
            //         let hash_path = key.to_string();  // Store the key in a String variable
            //         let data = hashmap.get(&hash_path).cloned().unwrap_or_default();  // This is initially a String
            
            //         // Disassemble the data
            //         match disassemble(&data).await {
            //             Ok(shares) => {
            //                 // Use shares if dis_assembly was successful
            //                 let result = self.data_store.hset(
            //                     &format!("{}{}", hash_tag, hash_path),
            //                     "data",
            //                     &shares
            //                 ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

            //                 if let Err(_e) = result {
            //                     //eprintln!("Error setting data in the share store: {}", e);
            //                 }
            //             }
            //             Err(e) => {
            //                 // Handle the error from dis_assembly
            //                 eprintln!("Error disassembling data: {}", e);
            //             }
            //         }
            //     }
            //     drop(hashmap);
            //     let mut hashmap = self.in_memory_hashmap.write().await;
            //     hashmap.clear();
            // }
            
            

            let ftype_result = self.get_ftype(new_from_path.clone()).await;
            
            // Get the current local date and time
            //let local_date_time: DateTime<Local> = Local::now();

            // Format the date and time using the specified pattern
            //let creation_time = local_date_time.format("%b %d %H:%M:%S %Y").to_string();

            // Send the event with the formatted creation time, event type, path, and user ID
            
            
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
    }

    async fn mkdir(&self, dirid: fileid3, dirname: &filename3) -> Result<(fileid3, fattr3), nfsstat3> {
    
        {
        
            //let mut conn = self.pool.get_connection();             

        //    let (user_id, hash_tag, parent_path, new_dir_path, new_dir_id) = {
                let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;
        
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

//                (user_id, hash_tag, parent_path, new_dir_path, new_dir_id) // Return the values needed outside the scope
//            };
            
            let _ = self.create_node("0", new_dir_id, &new_dir_path).await;

            let metadata = self.get_metadata_from_id(new_dir_id).await?;

            Ok((new_dir_id, FileMetadata::metadata_to_fattr3(new_dir_id, &metadata).await?))
            
        }
        
    }

    async fn symlink(&self, dirid: fileid3, linkname: &filename3, symlink: &nfspath3, attr: &sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
        // Validate input parameters
    if linkname.is_empty() || symlink.is_empty() {
        return Err(nfsstat3::NFS3ERR_INVAL);
    }

    
    //let mut conn = self.pool.get_connection();  

    let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;   

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
        let (user_id, hash_tag) = MirrorFS::get_user_id_and_hash_tag().await;
    
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

async fn start_ipfs_server() -> Result<(), Box<dyn std::error::Error>> {
    // Initialization and startup logic for the IPFS server.
    println!("graymamba NFS server started successfully.");
    Ok(())
}

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
    
    // Start the IPFS server
    if let Err(e) = start_ipfs_server().await {
        eprintln!("Failed to start NFS server: {}", e);
        return;
    }
    
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
    let fs = MirrorFS::new(data_store, blockchain_audit);

    let listener = NFSTcpListener::bind(&format!("0.0.0.0:{HOSTPORT}"), fs)
        .await
        .unwrap();
    listener.handle_forever().await.unwrap();
}    //Initialize NFSModule

