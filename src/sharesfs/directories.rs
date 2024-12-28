use regex::Regex;

use crate::kernel::api::nfs::nfsstat3;

use super::SharesFS;

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use crate::kernel::api::nfs::fileid3;   
use crate::kernel::api::nfs::filename3;
use crate::kernel::api::nfs::fattr3;
use graymamba::file_metadata::FileMetadata;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;


use tracing::debug;
impl SharesFS {
pub async fn rename_directory_file(&self, from_path: &str, to_path: &str) -> Result<(), nfsstat3> { 
    let (namespace_id, community) = SharesFS::get_namespace_id_and_community().await;
    //Rename the metadata hashkey
    let _ = self.data_store.rename(
        &format!("{}{}", community, from_path),
        &format!("{}{}", community, to_path)
    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
    //Rename entries in hashset
    debug!("rename_directory_file {:?} {:?}", from_path, to_path);

    // Create a pattern to match all keys under the old path
    let pattern = format!("{}{}{}", community, from_path, "/*");
    
    // RETRIEVEall keys matching the pattern
    debug!("Retrieve all keys matching the pattern {:?}", pattern);
    let keys_result = self.data_store.keys(&pattern)
        .await
        .map_err(|_| nfsstat3::NFS3ERR_IO);
    let keys: Vec<String> = match keys_result {
        Ok(k) => k,
        Err(_) => return Err(nfsstat3::NFS3ERR_IO),  // Replace with appropriate nfsstat3 error
    };
    debug!("keys matching the pattern {:?}", keys);
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
    let key = format!("{}/{}_nodes", community, namespace_id);

    // RETRIEVE all members of the sorted set with their scores
    debug!("Retrieve all members of the sorted set with their scores {:?}", key);
    let members_result = self.data_store.zrange_withscores(&key, 0, -1)
    .await
    .map_err(|_| nfsstat3::NFS3ERR_IO);
    debug!("Result for retrieve all members of the sorted set with their scores {:?}", members_result);

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
            debug!("If the entry is the directory itself, just replace it {:?}", directory_path);
            let zrem_result = self.data_store.zrem(&key, &directory_path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO);

            if zrem_result.is_err() {
                return Err(nfsstat3::NFS3ERR_IO);  // Replace with appropriate nfsstat3 error
            }
            debug!("Add the new path to the sorted set {:?}", to_path);
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
                debug!("If the new path doesn't exist, update it");
                let zrem_result = self.data_store.zrem(&key, &directory_path)
                .await
                .map_err(|_| nfsstat3::NFS3ERR_IO);

                if zrem_result.is_err() {
                    return Err(nfsstat3::NFS3ERR_IO);
                }
                debug!("Add the new path to the sorted set {:?}", new_directory_path);
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
    let path_to_id_key = format!("{}/{}_path_to_id", community, namespace_id);
    let id_to_path_key = format!("{}/{}_id_to_path", community, namespace_id);

    // Retrieve all the members of path_to_id hash
    debug!("Retrieve all the members of path_to_id hash for key {:?}", path_to_id_key);
    let fields_result = self.data_store.hgetall(&path_to_id_key)
    .await
    .map_err(|_| nfsstat3::NFS3ERR_IO);

    debug!("Result for retrieve all the members of path_to_id hash {:?}", fields_result);

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
            debug!("The entry is the directory itself, just replace it {:?}", directory_path);
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

            let _ = self.data_store.hset_multiple(&format!("{}{}", community, to_path),
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

                let _ = self.data_store.hset_multiple(&format!("{}{}", community, new_directory_path),
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

pub async fn remove_directory_file(&self, path: &str) -> Result<(), nfsstat3> {
            
    let (namespace_id, community) = SharesFS::get_namespace_id_and_community().await;
    let pattern = format!("{}/*", path);
    let sorted_set_key = format!("{}/{}_nodes", community, namespace_id);
    let match_found = self.get_member_keys(&pattern, &sorted_set_key).await?;
    if match_found {
        return Err(nfsstat3::NFS3ERR_NOTEMPTY);
    }
    debug!("remove_directory_file {:?}", path);
    let dir_id = self.data_store.hget(
        &format!("{}/{}_path_to_id", community, namespace_id),
        path
    ).await;
    
    let value: String = match dir_id {
        Ok(k) => k,
        Err(_) => return Err(nfsstat3::NFS3ERR_IO),
    };
    // Remove the directory         
    // Remove the node from the sorted set
    debug!("Remove the node from the sorted set {:?}", format!("{}/{}_nodes", community, namespace_id));
    let _ = self.data_store.zrem(
        &format!("{}/{}_nodes", community, namespace_id),
        path
    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
            
    // Delete the metadata hash associated with the node
    debug!("Delete the metadata hash associated with the node {:?}", format!("{}{}", community, path));
    let _ = self.data_store.delete(&format!("{}{}", community, path))
        .await
        .map_err(|_| nfsstat3::NFS3ERR_IO);
         
    // Remove the directory from the path-to-id mapping
    debug!("Remove the directory from the path-to-id mapping {:?}", format!("{}/{}_path_to_id", community, namespace_id));
    let _ = self.data_store.hdel(
        &format!("{}/{}_path_to_id", community, namespace_id),
        path
    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
    
    // Remove the directory from the id-to-path mapping
    debug!("Remove the directory from the id-to-path mapping {:?}", format!("{}/{}_id_to_path", community, namespace_id));
    let _ = self.data_store.hdel(
        &format!("{}/{}_id_to_path", community, namespace_id),
        &value
    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);             
     
    Ok(())

}

    pub async fn handle_mkdir(&self, dirid: fileid3, dirname: &filename3) -> Result<(fileid3, fattr3), nfsstat3> {
        let (namespace_id, community) = SharesFS::get_namespace_id_and_community().await;
        let key1 = format!("{}/{}_id_to_path", community, namespace_id);

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

        // let key2 = format!("{}/{}_path_to_id", community, namespace_id);

        // Check if directory already exists
        let exists: bool = match self.data_store.zscore(
            &format!("{}/{}_nodes", community, namespace_id),
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
        let key = format!("{}/{}_next_fileid", community, namespace_id);
    
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
}