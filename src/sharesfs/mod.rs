mod metadata;

pub mod channel_buffer;

use std::collections::BTreeSet;
use std::ops::Bound;
use tokio::time::Instant;
use std::sync::{Arc, RwLock};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use crate::kernel::api::nfs::fileid3;
use std::collections::HashMap;
use tokio::sync::{Mutex, Semaphore};
use rayon::prelude::*;

use tokio::time::Duration;

use std::os::unix::fs::PermissionsExt;
use chrono::{Local, DateTime};

use crate::kernel::api::nfs::*;
use crate::kernel::api::nfs::nfsstat3;
use crate::kernel::api::nfs::sattr3;


use graymamba::file_metadata::FileMetadata;

use graymamba::backingstore::data_store::{DataStore,DataStoreError,DataStoreResult};

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use regex::Regex;

use channel_buffer::ActiveWrite;
use channel_buffer::ChannelBuffer;

use async_trait::async_trait;

use tracing::{debug, warn};

use crate::kernel::vfs::api::{DirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};

use lazy_static::lazy_static;
lazy_static! {
    pub static ref NAMESPACE_ID: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));
    pub static ref HASH_TAG: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));
}

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::secret_sharing::SecretSharingService;

#[cfg(feature = "irrefutable_audit")]
use crate::audit_adapters::irrefutable_audit::{AuditEvent, IrrefutableAudit};
#[cfg(feature = "irrefutable_audit")]
use crate::audit_adapters::irrefutable_audit::event_types::{DISASSEMBLED, REASSEMBLED};

#[derive(Clone)]
pub struct SharesFS {
    pub data_store: Arc<dyn DataStore>,
    pub irrefutable_audit: Option<Arc<dyn IrrefutableAudit>>, // Add NFSModule wrapped in Arc
    pub active_writes: Arc<Mutex<HashMap<fileid3, ActiveWrite>>>,
    pub commit_semaphore: Arc<Semaphore>,
    pub secret_sharing: Arc<SecretSharingService>,
    
}

impl SharesFS {
    pub async fn create_test_entry(&self, _parent_id: u64, path: &str, id: u64) -> Result<(), nfsstat3> {
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
        
        // Store path to id mapping
        self.data_store.hset(
            &format!("{}/{}_path_to_id", hash_tag, namespace_id),
            path,
            &id.to_string()
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;

        // Store id to path mapping
        self.data_store.hset(
            &format!("{}/{}_id_to_path", hash_tag, namespace_id),
            &id.to_string(),
            path
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;

        // Add to nodes set
        let score = path.split("/").count() as f64;
        self.data_store.zadd(
            &format!("{}/{}_nodes", hash_tag, namespace_id),
            path,
            score
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;

        Ok(())
    }

    pub fn new(data_store: Arc<dyn DataStore>, irrefutable_audit: Option<Arc<dyn IrrefutableAudit>>) -> SharesFS {
        let active_writes = Arc::new(Mutex::new(HashMap::new()));
        let commit_semaphore = Arc::new(Semaphore::new(10));
        let secret_sharing = Arc::new(SecretSharingService::new().expect("Failed to initialize SecretSharingService"));

        SharesFS {
            data_store,
            irrefutable_audit,
            active_writes,
            commit_semaphore,
            secret_sharing,
        }
    }
    // New method to start monitoring
    pub async fn start_monitoring(&self) {
        self.monitor_active_writes().await;
    }

    pub fn mode_unmask_setattr(mode: u32) -> u32 {
        let mode = mode | 0x80;
        let permissions = std::fs::Permissions::from_mode(mode);
        permissions.mode() & 0x1FF
    }

    pub async fn get_path_from_id(&self, id: fileid3) -> Result<String, nfsstat3> {

        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;

        let key = format!("{}/{}_id_to_path", hash_tag, namespace_id);
          
        let path: String = {
            self.data_store
                .hget(&key, &id.to_string())
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO)?
        };

        Ok(path)
    }

    /// Get the ID for a given file/directory path
    pub async fn get_id_from_path(&self, path: &str) -> Result<fileid3, nfsstat3> {
       
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;

        let key = format!("{}/{}_path_to_id", hash_tag, namespace_id);

        let id_str: String = self.data_store
        .hget(&key, path)
        .await
        .map_err(|_| nfsstat3::NFS3ERR_IO)?;
    
        let id: fileid3 = id_str.parse()
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        Ok(id)
    }

    /// Get the metadata for a given file/directory ID
    pub async fn get_metadata_from_id(&self, id: fileid3) -> Result<FileMetadata, nfsstat3> {
        //warn!("SharesFS::get_metadata_from_id");
        let path = self.get_path_from_id(id).await?;
        
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

    pub async fn get_direct_children(&self, path: &str) -> Result<Vec<fileid3>, nfsstat3> {
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
        
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;

        let key = format!("{}/{}_nodes", hash_tag, namespace_id);

        // let pool_guard = shared_pool.lock().unwrap();
        // let mut conn = pool_guard.get_connection();
        
        if subpath == "/" {
            // If the subpath is the root, return nodes with a score of ROOT_DIRECTORY_SCORE
            // Await the async operation and then transform the Ok value
            let nodes_result: Vec<String> = self.data_store.zrangebyscore(&key, 2.0, 2.0).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;
            Ok(nodes_result)
            
        } else {
            // Calculate start and end scores based on the hierarchy level
            let start_score = subpath.split("/").count() as f64;
            let end_score = start_score + 1.0;

            // Retrieve nodes at the specified hierarchy level
            // Await the async operation and then transform the Ok value
            let nodes_result: Vec<String> = self.data_store.zrangebyscore(&key, end_score, end_score).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;
            Ok(nodes_result)
            
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

    pub async fn get_last_path_element(&self, input: String) -> String {
        let elements: Vec<&str> = input.split("/").collect();
        
        if elements.is_empty() {
            return String::from("");
        }
        
        let last_element = elements[elements.len() - 1];
        if last_element == "/" {
            String::from("/")
        } else {
            last_element.to_string()
        }
    }

    pub async fn create_node(&self, node_type: &str, fileid: fileid3, path: &str) -> DataStoreResult<()> {
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
      
        let size = 0;
        let permissions = 777;
        let score: f64 = path.matches('/').count() as f64 + 1.0;  
        
        let system_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos(); // Capture nanoseconds part
        
        self.data_store.zadd(
            &format!("{}/{}_nodes", hash_tag, namespace_id),
            path,
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
        
            self.data_store.hset(&format!("{}/{}_path_to_id", hash_tag, namespace_id), path, &fileid.to_string()).await?;
            self.data_store.hset(&format!("{}/{}_id_to_path", hash_tag, namespace_id), &fileid.to_string(), path).await?;
            
        Ok(())
    }
    
    pub async fn create_file_node(&self, node_type: &str, fileid: fileid3, path: &str, setattr: sattr3,) -> DataStoreResult<()> {
       
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
      
        let size = 0;
        let score: f64 = path.matches('/').count() as f64 + 1.0;  
        
        let system_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos(); // Capture nanoseconds part
        
        let _ = self.data_store.zadd(
            &format!("{}/{}_nodes", hash_tag, namespace_id),
            path,
            score
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

        let permissions = if let set_mode3::mode(mode) = setattr.mode {
            debug!(" -- set permissions {:?} {:?}", path, mode);
            Self::mode_unmask_setattr(mode).to_string()
        } else {
            "777".to_string() // Default permissions if none specified
        };

        let _ = self.data_store.hset_multiple(&format!("{}{}", hash_tag, path), 
    &[
            ("ftype", node_type),
            ("size", &size.to_string()),
            ("permissions", &permissions),
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

        let _ = self.data_store.hset(&format!("{}/{}_path_to_id", hash_tag, namespace_id), path, &fileid.to_string()).await.map_err(|_| nfsstat3::NFS3ERR_IO);
        let _ = self.data_store.hset(&format!("{}/{}_id_to_path", hash_tag, namespace_id), &fileid.to_string(), path).await.map_err(|_| nfsstat3::NFS3ERR_IO);

        
        Ok(())
            
    }
    
    pub async fn get_ftype(&self, path: String) -> Result<String, nfsstat3> {
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
                .collect();
        
                if !matching_keys.is_empty() {
                    return Ok(true);
                }
            }
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),
        }
    
        Ok(false)
    }
    
    pub async fn remove_directory_file(&self, path: &str) -> Result<(), nfsstat3> {
            
            let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
            let pattern = format!("{}/*", path);
            let sorted_set_key = format!("{}/{}_nodes", hash_tag, namespace_id);
            let match_found = self.get_member_keys(&pattern, &sorted_set_key).await?;
            if match_found {
                return Err(nfsstat3::NFS3ERR_NOTEMPTY);
            }

            let dir_id = self.data_store.hget(
                &format!("{}/{}_path_to_id", hash_tag, namespace_id),
                path
            ).await;
            
            let value: String = match dir_id {
                Ok(k) => k,
                Err(_) => return Err(nfsstat3::NFS3ERR_IO),
            };
            // Remove the directory         
            // Remove the node from the sorted set
            let _ = self.data_store.zrem(
                &format!("{}/{}_nodes", hash_tag, namespace_id),
                path
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
                    
            // Delete the metadata hash associated with the node
            let _ = self.data_store.delete(&format!("{}{}", hash_tag, path))
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO);
                 
            // Remove the directory from the path-to-id mapping
            let _ = self.data_store.hdel(
                &format!("{}/{}_path_to_id", hash_tag, namespace_id),
                path
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            
            // Remove the directory from the id-to-path mapping
            let _ = self.data_store.hdel(
                &format!("{}/{}_id_to_path", hash_tag, namespace_id),
                &value
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);             
             
            Ok(())
        
    }
    
    pub async fn rename_directory_file(&self, from_path: &str, to_path: &str) -> Result<(), nfsstat3> { 
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
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
        let key = format!("{}/{}_nodes", hash_tag, namespace_id);

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
        let path_to_id_key = format!("{}/{}_path_to_id", hash_tag, namespace_id);
        let id_to_path_key = format!("{}/{}_id_to_path", hash_tag, namespace_id);

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

    pub async fn get_namespace_id_and_hash_tag() -> (String, String) {
        let namespace_id = NAMESPACE_ID.read().unwrap().clone();
        let hash_tag = HASH_TAG.read().unwrap().clone();
        (namespace_id, hash_tag)
    }

    pub async fn get_data(&self, path: &str) -> Vec<u8> {
     
        let (_namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;

        // Retrieve the current file content (Base64 encoded) from store
        let store_value: String = (self.data_store.hget(&format!("{}{}", hash_tag, path),"data").await).unwrap_or_default();
    if !store_value.is_empty() {
        match self.secret_sharing.reassemble(&store_value).await {
            Ok(reconstructed_secret) => {
                    // Decode the base64 string to a byte array
                    STANDARD.decode(&reconstructed_secret).unwrap_or_default()
                }
                Err(_) => Vec::new(), // Handle the re_assembly error by returning an empty Vec<u8>
            }
        } else {
            Vec::new() // Return an empty Vec<u8> if no data is found
        }
    }

    pub async fn load_existing_content(&self, id: fileid3, channel: &Arc<ChannelBuffer>) -> Result<(), nfsstat3> {
        
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
        warn!("Starting active writes monitor");
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            
            let to_commit: Vec<fileid3> = {
                let active_writes = self.active_writes.lock().await;
                let mut to_commit = Vec::new();
                
                for (&id, write) in active_writes.iter() {
                    // Get the path for this file id
                    if let Ok(path) = self.get_path_from_id(id).await {
                        // Don't auto-commit pack files
                        if !path.contains("/objects/pack/") && write.channel.is_write_complete() {
                            to_commit.push(id);
                        }
                    }
                }
                to_commit
            };
    
            for id in to_commit {
                debug!("Auto-committing write for id: {}", id);
                let _ = self.commit_write(id).await;
            }
        }
    }

    async fn commit_write(&self, id: fileid3) -> Result<(), DataStoreError> {

        debug!("Starting commit process for file ID: {}", id);

        let _permit = self.commit_semaphore.acquire().await.map_err(|_| DataStoreError::OperationFailed);

        let (_namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;


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


        let base64_contents = STANDARD.encode(&contents); // Convert byte array (Vec<u8>) to a base64 encoded string
        

        match self.secret_sharing.disassemble(&base64_contents).await {
            Ok(shares) => {
                // Attempt to write the shares to the data store
                debug!("Writing shares to data store");
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
        let (_namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;

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

    pub async fn is_likely_last_write(&self, id: fileid3, offset: u64, data_len: usize) -> Result<bool, nfsstat3> {

        let (_namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;

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

    pub async fn mark_write_as_complete(&self, id: fileid3) -> Result<(), nfsstat3> {
        let mut active_writes = self.active_writes.lock().await;
        if let Some(write) = active_writes.get_mut(&id) {
            write.channel.set_complete();
        }
        Ok(())
    }
}

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
        debug!("lookup: {:?}", filename);
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
        self.get_attribute(id).await
    }

    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        let (_namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
        let path = self.get_path_from_id(id).await?;

        debug!("write: {:?}", path);
    
        let channel = {
            let mut active_writes = self.active_writes.lock().await;
            if let Some(write) = active_writes.get_mut(&id) {
                write.last_activity = Instant::now();
                write.channel.clone()
            } else {
                let channel = ChannelBuffer::new();
                active_writes.insert(id, ActiveWrite::new(channel.clone()));
                channel
            }
        };
    
        channel.write(offset, data).await;
        
        let total_size = channel.total_size();
        self.data_store.hset_multiple(
            &format!("{}{}", hash_tag, path),
            &[
                ("size",&total_size.to_string())
            ]
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;

        if self.is_likely_last_write(id, offset, data.len()).await? {
            self.mark_write_as_complete(id).await?;
        }
    
        if !path.contains("/objects/pack/") && (path.contains("/.git/") || path.ends_with(".git")) {
            self.commit_write(id).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;
        }

        let local_date_time: DateTime<Local> = Local::now();
        let creation_time = local_date_time.format("%b %d %H:%M:%S %Y").to_string();
    
        let mut user = "";
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() > 2 {
            user = parts[1];
        }

        if let Some(irrefutable_audit) = &self.irrefutable_audit {
            debug!("Triggering disassembled event");
            let event = AuditEvent {
                creation_time: creation_time.clone(),
                event_type: DISASSEMBLED.to_string(),
                file_path: path.clone(),
                event_key: user.to_string(),
            };
            if let Err(e) = irrefutable_audit.trigger_event(event).await {
                warn!("Failed to trigger audit event: {}", e);
            }
        }
    
        let metadata = self.get_metadata_from_id(id).await?;
        FileMetadata::metadata_to_fattr3(id, &metadata).await
    }

    async fn read(&self, id: fileid3, offset: u64, count: u32) -> Result<(Vec<u8>, bool), nfsstat3> {
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
    
        let path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, namespace_id),
            &id.to_string()
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;

        debug!("read: {:?}", path);
        
        // For pack files, read from active writes if present
        if path.contains("/objects/pack/") {
            let channel = {
                let active_writes = self.active_writes.lock().await;
                if let Some(write) = active_writes.get(&id) {
                    write.channel.clone()
                } else {
                    let channel = ChannelBuffer::new();
                    drop(active_writes);
                    self.load_existing_content(id, &channel).await?;
                    let mut active_writes = self.active_writes.lock().await;
                    active_writes.insert(id, ActiveWrite::new(channel.clone()));
                    channel
                }
            };
    
            // Calculate chunk boundaries
            let chunk_start = (offset / 32768) * 32768;
            let chunk_end = ((offset + count as u64 + 32767) / 32768) * 32768;
            
            // Read all needed chunks
            let mut full_buffer = Vec::new();
            for chunk_offset in (chunk_start..chunk_end).step_by(32768) {
                let chunk = channel.read_range(chunk_offset, 32768).await;
                full_buffer.extend_from_slice(&chunk);
            }
    
            // Extract the requested range
            let start = (offset - chunk_start) as usize;
            let end = std::cmp::min(start + count as usize, full_buffer.len());
            let buffer = full_buffer[start..end].to_vec();
            
            let total_size = channel.total_size();
            let eof = offset + buffer.len() as u64 >= total_size;
            
            return Ok((buffer, eof));
        } else if path.contains("/.git/") || path.ends_with(".git") {
            let active_writes = self.active_writes.lock().await;
            if let Some(_write) = active_writes.get(&id) {
                drop(active_writes);
                self.commit_write(id).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;
            }
        }
    
        let current_data = self.get_data(&path).await;
        
        if offset as usize >= current_data.len() {
            return Ok((vec![], true));
        }
    
        let end = std::cmp::min(current_data.len(), (offset + count as u64) as usize);
        let data_slice = &current_data[offset as usize..end];
        let eof = end >= current_data.len();
    
        let local_date_time: DateTime<Local> = Local::now();
        let creation_time = local_date_time.format("%b %d %H:%M:%S %Y").to_string();
    
        let mut user = "";
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() > 2 {
            user = parts[1];
        }
    
        if let Some(irrefutable_audit) = &self.irrefutable_audit {
            let event = AuditEvent {
                creation_time: creation_time.clone(),
                event_type: REASSEMBLED.to_string(),
                file_path: path.clone(),
                event_key: user.to_string(),
            };
            if let Err(e) = irrefutable_audit.trigger_event(event).await {
                warn!("Failed to trigger audit event: {}", e);
            }
        }
    
        Ok((data_slice.to_vec(), eof))
    }

    async fn readdir_sequential(&self, dirid: fileid3, start_after: fileid3, max_entries: usize) -> Result<ReadDirResult, nfsstat3> {
        let path = self.get_path_from_id(dirid).await?;
        //println!("path: {:?}", path);

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
        debug!("children len: {:?}", children.len());
        debug!("remaining_len : {:?}", remaining_length);
        for child_id in children.range((range_start, Bound::Unbounded)) {
            let child_path = self.get_path_from_id(*child_id).await?;
            let child_name = self.get_last_path_element(child_path).await;
            let child_metadata = self.get_metadata_from_id(*child_id).await?;

            //println!("\t --- {:?} {:?}", child_id, child_name);
            
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

        //println!("readdir_result:{:?}", ret);

        Ok(ret)
    }

    async fn readdir(
        &self,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirResult, nfsstat3> {
        let path = self.get_path_from_id(dirid).await?;
        let children = self.get_direct_children(&path).await?;

        debug!("readdir: {:?}", path);
        
        let children_set: BTreeSet<_> = children.into_iter().collect();
        
        let range = if start_after > 0 {
            children_set.range((Bound::Excluded(start_after), Bound::Unbounded))
        } else {
            children_set.range(..)
        };

        let entries: Vec<_> = range
        .take(max_entries)
        .collect::<Vec<_>>()
        .par_iter()
        .filter_map(|&child_id| {
            let metadata = futures::executor::block_on(self.get_metadata_from_id(*child_id)).ok()?;
            let path = futures::executor::block_on(self.get_path_from_id(*child_id)).ok()?;
            let name = futures::executor::block_on(self.get_last_path_element(path));
            let attr = futures::executor::block_on(FileMetadata::metadata_to_fattr3(*child_id, &metadata)).ok()?;
            
            Some(DirEntry {
                fileid: *child_id,
                name: name.as_bytes().into(),
                attr,
            })
        })
        .collect();

        let cnt = entries.len();
        Ok(ReadDirResult {
            entries,
            end: cnt < max_entries,
        })
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {       
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;

        // Get file path from the share store
        let path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, namespace_id),
            &id.to_string()
        ).await
            .unwrap_or_else(|_| String::new());

        debug!("setattr: {:?}", path);

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
            let _ = self.data_store.hset_multiple(
                &format!("{}{}", hash_tag, path),
                &[
                ("permissions",&mode_value.to_string())
                ],
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            
        }

        if let set_size3::size(size3) = setattr.size {
            debug!(" -- set size {:?} {:?}", path, size3);
    
            // Update the size metadata of the file in the share store
            let _hset_result = self.data_store.hset_multiple(
                &format!("{}{}", hash_tag, path),
                &[
                ("size",&size3.to_string())
                ],
            ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
        }
        
        
        let metadata = self.get_metadata_from_id(id).await?;

        //FileMetadata::metadata_to_fattr3(id, &metadata)
        let fattr = FileMetadata::metadata_to_fattr3(id, &metadata).await?;

        Ok(fattr)
    }

    async fn create(&self, dirid: fileid3, filename: &filename3, setattr: sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
                
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
        
        // Get parent directory path from the share store
        let parent_path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, namespace_id),
            &dirid.to_string()
        ).await
            .unwrap_or_else(|_| String::new());
        
        //warn!("graymamba create {:?}", parent_path);
        if parent_path.is_empty() {
            return Err(nfsstat3::NFS3ERR_NOENT); // No such directory id exists
        }

        let objectname_osstr = OsStr::from_bytes(filename).to_os_string();
        
        let new_file_path: String = if parent_path == "/" {
            format!("/{}", objectname_osstr.to_str().unwrap_or(""))
        } else {
            format!("{}/{}", parent_path, objectname_osstr.to_str().unwrap_or(""))
        };

        debug!("create: {:?}", new_file_path);

        // Check if file already exists
        let exists: bool = match self.data_store.zscore(
            &format!("{}/{}_nodes", hash_tag, namespace_id),
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
            &format!("{}/{}_next_fileid", hash_tag, namespace_id)
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

            //let (namespace_id, hash_tag, new_file_path,new_file_id ) = {
                
                let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
                
                // Get parent directory path from the share store
                let parent_path: String = self.data_store.hget(
                    &format!("{}/{}_id_to_path", hash_tag, namespace_id),
                    &dirid.to_string()
                ).await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            
                // if parent_path.is_empty() {
                //     return Err(nfsstat3::NFS3ERR_NOENT); // No such directory id exists
                // }
        
                let objectname_osstr = OsStr::from_bytes(filename).to_os_string();
                
                let new_file_path: String = if parent_path == "/" {
                    format!("/{}", objectname_osstr.to_str().unwrap_or(""))
                } else {
                    format!("{}/{}", parent_path, objectname_osstr.to_str().unwrap_or(""))
                };

                let exists: bool = match self.data_store.zscore(
                    &format!("{}/{}_nodes", hash_tag, namespace_id),
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
                        &format!("{}/{}_path_to_id", hash_tag, namespace_id),
                        &new_file_path
                    ).await;
                    match fields_result {
                        Ok(value) => {
                            // File already exists, return the existing file ID
                            return value.parse::<u64>().map_err(|_| nfsstat3::NFS3ERR_IO);
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
                    &format!("{}/{}_next_fileid", hash_tag, namespace_id)
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
        let parent_path = (self.get_path_from_id(dirid).await?).to_string();
        let objectname_osstr = OsStr::from_bytes(filename).to_os_string();           
        // Construct the full path of the file/directory
        let new_dir_path: String = if parent_path == "/" {
            format!("/{}", objectname_osstr.to_str().unwrap_or(""))
        } else {
            format!("{}/{}", parent_path, objectname_osstr.to_str().unwrap_or(""))
        };

        debug!("remove: {:?}", new_dir_path);

        let ftype_result = self.get_ftype(new_dir_path.clone()).await;
        
        match ftype_result {
        Ok(ftype) => {
            if ftype == "0" || ftype == "1" || ftype == "2" {
                self.remove_directory_file(&new_dir_path).await?;
            } else {
                return Err(nfsstat3::NFS3ERR_IO);
            }
        },
        Err(_) => return Err(nfsstat3::NFS3ERR_IO),
        }
            
        Ok(())
    }

    async fn rename(&self, from_dirid: fileid3, from_filename: &filename3, to_dirid: fileid3, to_filename: &filename3) -> Result<(), nfsstat3> {
        //warn!("graymamba rename {:?} {:?} {:?} {:?}", from_dirid, from_filename, to_dirid, to_filename);
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
        
        let from_path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, namespace_id),
            &from_dirid.to_string()
        ).await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
        
        let objectname_osstr = OsStr::from_bytes(from_filename).to_os_string();
        // Construct the full path of the file/directory    
        let new_from_path: String = if from_path == "/" {
            format!("/{}", objectname_osstr.to_str().unwrap_or(""))
        } else {
            format!("{}/{}", from_path, objectname_osstr.to_str().unwrap_or(""))
        };

        debug!("rename: {:?} {:?}", from_path, new_from_path);

            // Check if the source file exists in the share store
            let from_exists: bool = match self.data_store.zscore(
                &format!("{}/{}_nodes", hash_tag, namespace_id),
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
            &format!("{}/{}_id_to_path", hash_tag, namespace_id),
            &to_dirid.to_string()
        ).await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
        
        let objectname_osstr = OsStr::from_bytes(to_filename).to_os_string();
    
        // Construct the full path of the file/directory
        let new_to_path: String = if to_path == "/" {
            format!("/{}", objectname_osstr.to_str().unwrap_or(""))
        } else {
            format!("{}/{}", to_path, objectname_osstr.to_str().unwrap_or(""))
        };
            
        let ftype_result = self.get_ftype(new_from_path.clone()).await;
        match ftype_result {
            Ok(ftype) => {
                if ftype == "0" || ftype == "1" || ftype == "2" {
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
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
        let key1 = format!("{}/{}_id_to_path", hash_tag, namespace_id);

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
        let new_dir_path: String = if parent_path == "/" {
            format!("/{}", objectname_osstr.to_str().unwrap_or(""))
        } else {
            format!("{}/{}", parent_path, objectname_osstr.to_str().unwrap_or(""))
        };

        debug!("mkdir: {:?}", new_dir_path);

        // let key2 = format!("{}/{}_path_to_id", hash_tag, namespace_id);

        // Check if directory already exists
        let exists: bool = match self.data_store.zscore(
            &format!("{}/{}_nodes", hash_tag, namespace_id),
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
        let key = format!("{}/{}_next_fileid", hash_tag, namespace_id);
    
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

        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;   

        // Get the current system time for metadata timestamps
        let system_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap();
            let epoch_seconds = system_time.as_secs();
            let epoch_nseconds = system_time.subsec_nanos(); // Capture nanoseconds part
            

        // Get the directory path from the directory ID
        let dir_path: String = self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, namespace_id),
            &dirid.to_string()
        ).await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        //Convert symlink to string
        let symlink_osstr = OsStr::from_bytes(symlink).to_os_string();

        debug!("symlink: {:?}", symlink_osstr);
        // Construct the full symlink path
        let objectname_osstr = OsStr::from_bytes(linkname).to_os_string();
                    
            let symlink_path: String = if dir_path == "/" {
                format!("/{}", objectname_osstr.to_str().unwrap_or(""))
            } else {
                format!("{}/{}", dir_path, objectname_osstr.to_str().unwrap_or(""))
            };

            let symlink_exists: bool = match self.data_store.zscore(
                &format!("{}/{}_nodes", hash_tag, namespace_id),
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
            &format!("{}/{}_next_fileid", hash_tag, namespace_id)
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
            &format!("{}/{}_nodes", hash_tag, namespace_id),
            &symlink_path,
            score
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
        
        // First calculate the permissions
        let permissions = if let set_mode3::mode(mode) = attr.mode {
            Self::mode_unmask_setattr(mode).to_string()
        } else {
            "777".to_string() // Default permissions if none specified
        };

        // Include permissions in the initial hset_multiple
        let _ = self.data_store.hset_multiple(
            &format!("{}{}", hash_tag, &symlink_path),
            &[
                ("ftype", "2"),
                ("size", &symlink.len().to_string()),
                ("permissions", &permissions),
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

        let _ = self.data_store.hset(
            &format!("{}{}", hash_tag, symlink_path),
            "symlink_target",
            symlink_osstr.to_str().unwrap_or_default()
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
        
        let _ = self.data_store.hset(
            &format!("{}/{}_path_to_id", hash_tag, namespace_id),
            &symlink_path,
            &symlink_id.to_string()
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

        let _ = self.data_store.hset(
            &format!("{}/{}_id_to_path", hash_tag, namespace_id),
            &symlink_id.to_string(),
            &symlink_path
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO);

        let metadata = self.get_metadata_from_id(symlink_id).await?;

        Ok((symlink_id, FileMetadata::metadata_to_fattr3(symlink_id, &metadata).await?))
        
    }

    async fn readlink(&self, id: fileid3) -> Result<nfsstring, nfsstat3> {
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
    
        // Retrieve the path from the file ID
        let path: String = match self.data_store.hget(
            &format!("{}/{}_id_to_path", hash_tag, namespace_id),
            &id.to_string()
        ).await {
            Ok(path) => path,
            Err(e) => {
                eprintln!("Error retrieving path for ID {}: {:?}", id, e);
                return Err(nfsstat3::NFS3ERR_STALE);
            }
        };

        debug!("readlink: {:?}", path);
    
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