use regex::Regex;

use crate::kernel::api::nfs::nfsstat3;

use super::SharesFS;

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use tracing::debug;
impl SharesFS {
pub async fn rename_directory_file(&self, from_path: &str, to_path: &str) -> Result<(), nfsstat3> { 
    let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
    //Rename the metadata hashkey
    let _ = self.data_store.rename(
        &format!("{}{}", hash_tag, from_path),
        &format!("{}{}", hash_tag, to_path)
    ).await.map_err(|_| nfsstat3::NFS3ERR_IO);
    //Rename entries in hashset
    debug!("rename_directory_file {:?} {:?}", from_path, to_path);

    // Create a pattern to match all keys under the old path
    let pattern = format!("{}{}{}", hash_tag, from_path, "/*");
    
    // Retrieve all keys matching the pattern
    let keys_result = self.data_store.keys(&pattern)
        .await
        .map_err(|_| nfsstat3::NFS3ERR_IO);
    debug!("Retrieve all keys matching the pattern {:?}", pattern);
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
    let key = format!("{}/{}_nodes", hash_tag, namespace_id);

    // Retrieve all members of the sorted set with their scores
    debug!("Retrieve all members of the sorted set with their scores {:?}", key);
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
            debug!("The entry is the directory itself, just replace it {:?}", directory_path);
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
    let path_to_id_key = format!("{}/{}_path_to_id", hash_tag, namespace_id);
    let id_to_path_key = format!("{}/{}_id_to_path", hash_tag, namespace_id);

    // Retrieve all the members of path_to_id hash
    debug!("Retrieve all the members of path_to_id hash {:?}", path_to_id_key);
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
}