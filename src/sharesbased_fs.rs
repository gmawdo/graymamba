use std::sync::{Arc, RwLock};
use graymamba::nfs::fileid3;
use graymamba::blockchain_audit::BlockchainAudit;
use std::collections::HashMap;
use tokio::sync::{Mutex, Semaphore};
use graymamba::channel_buffer::ActiveWrite;

use tokio::time::Duration;

use std::os::unix::fs::PermissionsExt;

use graymamba::nfs::*;
use graymamba::nfs::nfsstat3;
use graymamba::nfs::sattr3;


use crate::FileMetadata;

use graymamba::data_store::{DataStore,DataStoreError,DataStoreResult};

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use regex::Regex;

use crate::channel_buffer::ChannelBuffer;

use tracing::{debug, warn};

use lazy_static::lazy_static;
lazy_static! {
    pub static ref USER_ID: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));
    pub static ref HASH_TAG: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));
}

use base64::{Engine as _, engine::general_purpose::STANDARD};

use secretsharing::{disassemble, reassemble};

#[derive(Clone)]
pub struct SharesFS {
    pub data_store: Arc<dyn DataStore>,
    pub blockchain_audit: Option<Arc<BlockchainAudit>>, // Add NFSModule wrapped in Arc
    pub active_writes: Arc<Mutex<HashMap<fileid3, ActiveWrite>>>,
    pub commit_semaphore: Arc<Semaphore>,
    
}

impl SharesFS {
    pub fn new(data_store: Arc<dyn DataStore>, blockchain_audit: Option<Arc<BlockchainAudit>>) -> SharesFS {
        // Create shared components for active writes
        warn!("SharesFS::new");
        let active_writes = Arc::new(Mutex::new(HashMap::new()));
        let commit_semaphore = Arc::new(Semaphore::new(10)); // Adjust based on your system's capabilities

        let shares_fs = SharesFS {
            data_store,
            blockchain_audit,
            active_writes: active_writes.clone(),
            commit_semaphore: commit_semaphore.clone(),
        };

        // Start the background task to monitor active writes
        let shares_fs_clone = shares_fs.clone();
        tokio::spawn(async move {
            shares_fs_clone.monitor_active_writes().await;
        });

        shares_fs
    }
    pub fn mode_unmask_setattr(mode: u32) -> u32 {
        let mode = mode | 0x80;
        let permissions = std::fs::Permissions::from_mode(mode);
        permissions.mode() & 0x1FF
    }

    pub async fn get_path_from_id(&self, id: fileid3) -> Result<String, nfsstat3> {

        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;

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
    pub async fn get_id_from_path(&self, path: &str) -> Result<fileid3, nfsstat3> {
       
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;

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
    pub async fn get_metadata_from_id(&self, id: fileid3) -> Result<FileMetadata, nfsstat3> {
        warn!("SharesFS::get_metadata_from_id");
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
        
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;

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

    pub async fn get_last_path_element(&self, input: String) -> String {
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

    pub async fn create_node(&self, node_type: &str, fileid: fileid3, path: &str) -> DataStoreResult<()> {
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;
      
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
    
    pub async fn create_file_node(&self, node_type: &str, fileid: fileid3, path: &str, setattr: sattr3,) -> DataStoreResult<()> {
       
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;
      
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
    
    pub async fn get_ftype(&self, path: String) -> Result<String, nfsstat3> {
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
    
    pub async fn remove_directory_file(&self, path: &str) -> Result<(), nfsstat3> {
            
            let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;
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
    
    pub async fn rename_directory_file(&self, from_path: &str, to_path: &str) -> Result<(), nfsstat3> { 
        let (user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;
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

    pub async fn get_user_id_and_hash_tag() -> (String, String) {
        let user_id = USER_ID.read().unwrap().clone();
        let hash_tag = HASH_TAG.read().unwrap().clone();
        (user_id, hash_tag)
    }

    pub async fn get_data(&self, path: &str) -> Vec<u8> {
     
        let (_user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;

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

        let (_user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;


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
        let (_user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;

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

        let (_user_id, hash_tag) = SharesFS::get_user_id_and_hash_tag().await;

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