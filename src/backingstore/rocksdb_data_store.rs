use rocksdb::{DB, Options};
use async_trait::async_trait;
use crate::backingstore::data_store::{DataStore, DataStoreError};

use crate::backingstore::data_store::KeyType;

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::debug;

use serde::{Serialize, Deserialize};
use std::collections::HashMap;

use graymamba::sharesfs::SharesFS;

impl fmt::Display for RocksDBDataStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RocksDBDataStore")
    }
}

pub struct RocksDBDataStore{
    db: DB,
}

#[derive(Serialize, Deserialize)]
struct AttributeFields {
    fields: HashMap<String, String>
}

impl RocksDBDataStore {
    pub fn new(path: &str) -> Result<Self, DataStoreError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, path).map_err(|_| DataStoreError::ConnectionError)?;
        Ok(RocksDBDataStore { db })
    }
}

#[async_trait]
impl DataStore for RocksDBDataStore {
    async fn authenticate_user(&self, username: &str) -> KeyType {
        // Create a key with the format "user:{username}"
        let user_key = format!("user:{}", username);

        // Check if the user exists in the database
        match self.db.get(user_key.as_bytes()) {
            Ok(Some(_)) => KeyType::Usual,  // User exists
            Ok(None) => KeyType::Usual,  // User does not exist - force existence!!!!
            Err(_) => KeyType::None,  // Operation failed
        }
    }

    async fn init_user_directory(&self, mount_path: &str) -> Result<(), DataStoreError> {
        let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
        debug!("namespace_id: {:?}", namespace_id);
        debug!("hash_tag: {:?}", hash_tag);
        let path = format!("/{}", namespace_id);
        let key = format!("{}{}", hash_tag, mount_path);
        debug!("===============rocksdb init_user_directory({})", key);       

        // Check if the directory already exists
        if let Ok(Some(_)) = self.db.get(key.as_bytes()) {
            debug!("rocksdb init_user_directory({}) already exists", key);
            return Ok(());
        }
        let node_type = "0";
        let size = 0;
        let permissions = 777;
        let score = if mount_path == "/" { 1.0 } else { 2.0 };

        let nodes = format!("{}/{}_nodes", hash_tag, namespace_id);
        debug!("===============rocksdb init_user_directory({}) nodes", nodes);
        let key_exists: bool = self.db.get(&nodes).map_err(|_| DataStoreError::OperationFailed)?.is_some();
        debug!("===============rocksdb init_user_directory({}) key_exists?", key_exists);

        let next_fileid_key = format!("{}/{}_next_fileid", hash_tag, namespace_id);
        let fileid = self.incr(&next_fileid_key).await?;

        let system_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos();

        // Add to sorted set (equivalent to Redis ZADD)
        let nodes_key = format!("{}/{}_nodes:{}", hash_tag, namespace_id, mount_path);
        self.db.put(nodes_key.as_bytes(), score.to_string().as_bytes())
            .map_err(|_| DataStoreError::OperationFailed)?;

        let size_str = size.to_string();
        let permissions_str = permissions.to_string();
        let epoch_seconds_str = epoch_seconds.to_string();
        let epoch_nseconds_str = epoch_nseconds.to_string();
        let fileid_str = fileid.to_string();

        // Now create the vector with references to our stored strings
        let hash_fields = vec![
            ("ftype", node_type),
            ("size", &size_str),
            ("permissions", &permissions_str),
            ("change_time_secs", &epoch_seconds_str),
            ("change_time_nsecs", &epoch_nseconds_str),
            ("modification_time_secs", &epoch_seconds_str),
            ("modification_time_nsecs", &epoch_nseconds_str),
            ("access_time_secs", &epoch_seconds_str),
            ("access_time_nsecs", &epoch_nseconds_str),
            ("birth_time_secs", &epoch_seconds_str),
            ("birth_time_nsecs", &epoch_nseconds_str),
            ("fileid", &fileid_str),
        ];

        // Use hset_multiple instead of individual puts
        let key = format!("{}{}", hash_tag, mount_path);
        self.hset_multiple(&key, &hash_fields).await?;

        // Set path to id mapping
        let path_to_id_key = format!("{}{}_path_to_id", hash_tag, path);
        self.db.put(
            format!("{}:{}", path_to_id_key, mount_path).as_bytes(),
            fileid_str.as_bytes()
        ).map_err(|_| DataStoreError::OperationFailed)?;

        // Set id to path mapping
        let id_to_path_key = format!("{}{}_id_to_path", hash_tag, path);
        self.db.put(
            format!("{}:{}", id_to_path_key, fileid_str).as_bytes(),
            mount_path.as_bytes()
        ).map_err(|_| DataStoreError::OperationFailed)?;

        if fileid == 1 {
            self.db.put(
                format!("{}{}_next_fileid", hash_tag, path).as_bytes(),
                b"1"
            ).map_err(|_| DataStoreError::OperationFailed)?;
        }

        Ok(())
    }

    async fn get(&self, key: &str) -> Result<String, DataStoreError> {
        match self.db.get(key) {
            Ok(Some(value)) => Ok(String::from_utf8(value).map_err(|_| DataStoreError::OperationFailed)?),
            Ok(None) => Err(DataStoreError::KeyNotFound),
            Err(_) => Err(DataStoreError::OperationFailed),
        }
    }
    
    async fn set(&self, key: &str, value: &str) -> Result<(), DataStoreError> {
        self.db.put(key.as_bytes(), value.as_bytes())
            .map_err(|_| DataStoreError::OperationFailed)
    }

    async fn delete(&self, key: &str) -> Result<(), DataStoreError> {
        // Delete the main key
        debug!("rocksdb delete({})", key);
        self.db.delete(key.as_bytes())
            .map_err(|_| DataStoreError::OperationFailed)?;
        
        // Also delete the data entry if it exists
        let data_key = format!("{}:data", key);
        debug!("rocksdb delete data({})", data_key);
        let _ = self.db.delete(data_key.as_bytes());
        
        Ok(())
    }

    //This is a modified version of the hget function from the RedisDataStore
    //In redis file attributes are stored in a redis hash holding field/values andis querable by field
    //in rocksdb we store the fields as a json string in the value of the key
    //and for single values as a direct k,v store
    //therefore if we singularly query for a value and don't find it we need to query the entire hash for the key
    //ftype and size are the two attributes asked for independently
    async fn hget(&self, key: &str, field: &str) -> Result<String, DataStoreError> {
        let full_key = format!("{}:{}", key, field);
        match self.get(&full_key).await {
            Ok(value) => Ok(value),
            Err(DataStoreError::KeyNotFound) => {
                // If the direct key lookup failed, try hgetall
                match self.hgetall(key).await {
                    Ok(fields) => {
                        // Look for the field in the hash map
                        fields.into_iter()
                            .find(|(k, _)| k == field)
                            .map(|(_, v)| v)
                            .ok_or(DataStoreError::KeyNotFound)
                    },
                    Err(e) => Err(e),
                }
            },
            Err(e) => Err(e),
        }
    }

    async fn hset(&self, key: &str, field: &str, value: &str) -> Result<(), DataStoreError> {
        let full_key = format!("{}:{}", key, field);
        self.set(&full_key, value).await
    }

    async fn hdel(&self, key: &str, field: &str) -> Result<(), DataStoreError> {
        let full_key = format!("{}:{}", key, field);
        self.delete(&full_key).await
    }

    async fn incr(&self, key: &str) -> Result<i64, DataStoreError> {
        let max_retries = 10; // Maximum number of retry attempts
        let mut attempts = 0;
        debug!("===============rocksdb0 incr({})", key);
        loop {
            // Get the current value
            let current = self.get(key).await.unwrap_or("0".to_string());
            let value = current.parse::<i64>().map_err(|_| DataStoreError::OperationFailed)?;
            let new_value = value + 1;

            debug!("===============rocksdb1 incr({}) current", current);
            debug!("===============rocksdb2 incr({}) Ok(_)", key);
            debug!("===============rocksdb3 incr({}) new_value", new_value);
            // Attempt to update atomically
            match self.db.put(
                key.as_bytes(),
                new_value.to_string().as_bytes()
            ) {
                Ok(_) => return Ok(new_value),
                Err(_) => {
                    attempts += 1;
                    if attempts >= max_retries {
                        return Err(DataStoreError::OperationFailed);
                    }
                    // Small backoff to reduce contention
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                    continue;
                }
            }
        }
    }

    async fn rename(&self, old_key: &str, new_key: &str) -> Result<(), DataStoreError> {
        debug!("rocksdb rename({}) = {}", old_key, new_key);
        
        let old_data_key = format!("{}:data", old_key);
        let new_data_key = format!("{}:data", new_key);
        
        debug!("BOWIE rocksdb rename data({}) = {}", old_data_key, new_data_key);
        if let Ok(data_value) = self.get(&old_data_key).await {
            debug!("BOWIE rocksdb rename data value found");
            self.set(&new_data_key, &data_value).await?;
            debug!("BOWIE rocksdb delete data({})", old_data_key);
            self.delete(&old_data_key).await?;
        }

        // Handle the main key
        if let Ok(value) = self.get(old_key).await {
            self.set(new_key, &value).await?;
            self.delete(old_key).await?;
        }
        
        Ok(())
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>, DataStoreError> {
        //debug!("rocksdb keys pattern matching for: {}", pattern);
        let mut results = Vec::new();
        
        // Convert pattern to parts for matching, handling both / and : separators
        let pattern_without_wildcard = if pattern.ends_with('*') {
            let base = pattern.trim_end_matches('*');
            base.trim_end_matches('/') // Remove trailing slash if present
        } else {
            pattern
        };
        
        //debug!("Using pattern prefix: {}", pattern_without_wildcard);
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);

        for item in iter {
            let (key, _) = item.map_err(|_| DataStoreError::OperationFailed)?;
            let key_str = String::from_utf8(key.to_vec())
                .map_err(|_| DataStoreError::OperationFailed)?;
            
            //debug!("Checking key: {} against pattern prefix: {}", key_str, pattern_without_wildcard);
            
            if pattern.ends_with('*') {
                // For wildcard patterns, match if the key starts with the pattern prefix
                // and the next character (if any) is either '/' or ':'
                let matches = key_str.starts_with(pattern_without_wildcard) && 
                    key_str[pattern_without_wildcard.len()..]
                        .chars()
                        .next()
                        .map(|c| c == '/' || c == ':')
                        .unwrap_or(true);
                
                //debug!("Wildcard match result: {} for key: {}", matches, key_str);
                if matches {
                    results.push(key_str);
                }
            } else {
                // For exact patterns, match the entire string
                let matches = key_str == pattern;
                //debug!("Exact match result: {} for key: {}", matches, key_str);
                if matches {
                    results.push(key_str);
                }
            }
        }

        //debug!("Found {} matching keys: {:?}", results.len(), results);
        Ok(results)
    }

    //Note that this is not as efficient as Redis's native sorted set implementation,
    //as it needs to load all members into memory and sort them. For large sets,
    //we'll consider using a more sophisticated storage scheme
    //(like storing scores as sortable byte strings in the key)

    async fn zrange_withscores(&self, key: &str, _start: isize, _stop: isize) 
        -> Result<Vec<(String, f64)>, DataStoreError> 
    {
        let mut results = Vec::new();
        
        // The key format should match what we use in zadd
        // In zadd we use: format!("{}/{}_nodes:{}", hash_tag, namespace_id, mount_path)
        let prefix = format!("{}", key);

        // Iterate over all entries with this prefix
        let iter = self.db.prefix_iterator(prefix.as_bytes());
        for item in iter {
            let (key_bytes, value_bytes) = item.map_err(|_| DataStoreError::OperationFailed)?;
            
            // Convert key and value to strings
            let full_key = String::from_utf8(key_bytes.to_vec())
                .map_err(|_| DataStoreError::OperationFailed)?;
            let score_str = String::from_utf8(value_bytes.to_vec())
                .map_err(|_| DataStoreError::OperationFailed)?;

            // Extract member from the key (remove prefix)
            if let Some(member) = full_key.strip_prefix(&format!("{}:", prefix)) {
                let score = score_str.parse::<f64>()
                    .map_err(|_| DataStoreError::OperationFailed)?;
                
                results.push((member.to_string(), score));
            }
        }
        // Sort results by score (to match Redis behavior)
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Apply range limits if needed
        let len = results.len() as isize;
        let start = if _start < 0 { (len + _start).max(0) } else { _start.min(len) } as usize;
        let stop = if _stop < 0 { (len + _stop + 1).max(0) } else { (_stop + 1).min(len) } as usize;

        Ok(results[start..stop].to_vec())
    }

/*
1. Key Format: The key is formatted as zset:{key}:{member}. This allows us to use
prefix iteration to retrieve all members of a sorted set.
Score Storage: The score is stored as the value associated with the key. This allows
us to retrieve and parse the score when needed.
3. Error Handling: Any errors during the put operation are mapped to
DataStoreError::OperationFailed.
This implementation allows us to add members to a sorted set in RocksDB. When
combined with the zrange_withscores function, you can retrieve and sort these members
by their scores.
*/
    async fn zadd(&self, key: &str, member: &str, score: f64) -> Result<(), DataStoreError> {
        // Create a key with the format "{key}:{member}"
        let member_key = format!("{}:{}", key, member);
        
        // Convert the score to a string
        let score_str = score.to_string();
        
        // Store the score as the value
        self.db.put(member_key.as_bytes(), score_str.as_bytes())
            .map_err(|_| DataStoreError::OperationFailed)
    }

/*
1. Key Format: The key is formatted as zset:{key}:{member}, consistent with the zadd function.
2. Delete Operation: The delete method is used to remove the key from the database.
3. Error Handling: Any errors during the delete operation are mapped to DataStoreError::OperationFailed.
This implementation allows us to remove a member from a sorted set in RocksDB.
 */
    async fn zrem(&self, key: &str, member: &str) -> Result<(), DataStoreError> {
        // Create a key with the format "{key}:{member}"
        let member_key = format!("{}:{}", key, member);
        
        // Delete the key from the database
        self.db.delete(member_key.as_bytes())
            .map_err(|_| DataStoreError::OperationFailed)
    }

    /*
    1. Key Format: The function uses the key format zset:{key}:{member} to iterate over
    all members of the sorted set.
    2. Score Filtering: It parses the score from the value and checks if it falls
    within the specified min and max range.
    3. Error Handling: Any errors during iteration or parsing are mapped
    to DataStoreError::OperationFailed.
    This implementation allows us to retrieve members of a sorted set whose scores 
    all within a specified range.
     */
    async fn zrangebyscore(&self, key: &str, min: f64, max: f64) -> Result<Vec<String>, DataStoreError> {
        let mut results = Vec::new();
        let prefix = format!("{}:", key);

        // Collect all members and scores
        let iter = self.db.prefix_iterator(prefix.as_bytes());
        for item in iter {
            let (key_bytes, value_bytes) = item.map_err(|_| DataStoreError::OperationFailed)?;

            // Convert key and value to strings
            let full_key = String::from_utf8(key_bytes.to_vec())
                .map_err(|_| DataStoreError::OperationFailed)?;
            let score_str = String::from_utf8(value_bytes.to_vec())
                .map_err(|_| DataStoreError::OperationFailed)?;

            // Extract member from the key (remove prefix)
            if let Some(member) = full_key.strip_prefix(&prefix) {
                let score = score_str.parse::<f64>()
                    .map_err(|_| DataStoreError::OperationFailed)?;

                // Check if the score is within the specified range
                if score >= min && score <= max {
                    results.push(member.to_string());
                }
            }
        }

        Ok(results)
    }

    async fn hset_multiple(&self, key: &str, fields: &[(&str, &str)]) -> Result<(), DataStoreError> {
        // Initialize hash_map with existing values if present
        let mut hash_map = match self.db.get(key.as_bytes()) {
            Ok(Some(existing_bytes)) => {
                let existing_str = String::from_utf8(existing_bytes)
                    .map_err(|_| DataStoreError::OperationFailed)?;
                let existing_fields: AttributeFields = serde_json::from_str(&existing_str)
                    .map_err(|_| DataStoreError::OperationFailed)?;
                existing_fields.fields
            },
            Ok(None) => HashMap::new(),
            Err(_) => return Err(DataStoreError::OperationFailed),
        };

        // Update with new values
        for (field, value) in fields {
            hash_map.insert(field.to_string(), value.to_string());
        }
        
        let hash_fields = AttributeFields { fields: hash_map };
        let serialized = serde_json::to_string(&hash_fields)
            .map_err(|_| DataStoreError::OperationFailed)?;
        
        self.db.put(key.as_bytes(), serialized.as_bytes())
            .map_err(|_| DataStoreError::OperationFailed)
    }

    async fn hgetall(&self, key: &str) -> Result<Vec<(String, String)>, DataStoreError> {
        let mut results = Vec::new();
        let prefix = format!("{}:", key);

        // First check for exact key match (for backward compatibility)
        if let Ok(Some(value)) = self.db.get(key.as_bytes()) {
            if let Ok(value_str) = String::from_utf8(value) {
                if let Ok(hash_fields) = serde_json::from_str::<AttributeFields>(&value_str) {
                    return Ok(hash_fields.fields.into_iter().collect());
                }
            }
        }

        // Then check for prefix matches
        let iter = self.db.prefix_iterator(prefix.as_bytes());
        for item in iter {
            let (key_bytes, value_bytes) = item.map_err(|_| DataStoreError::OperationFailed)?;
            
            // Convert key and value to strings
            let full_key = String::from_utf8(key_bytes.to_vec())
                .map_err(|_| DataStoreError::OperationFailed)?;
            let value = String::from_utf8(value_bytes.to_vec())
                .map_err(|_| DataStoreError::OperationFailed)?;

            // Extract field name from the key (remove prefix)
            if let Some(field) = full_key.strip_prefix(&prefix) {
                results.push((field.to_string(), value));
            }
        }

        Ok(results)
    }

    // This function is intended to scan through a sorted set and return members that match a specific pattern. 
    // It uses prefix iteration to retrieve all members of the sorted set.
    // It then checks if the member matches the specified pattern and collects the results.
    // Any errors during iteration or parsing are mapped to DataStoreError::OperationFailed.
    // This implementation allows us to retrieve members of a sorted set that match a specific pattern.
    async fn zscan_match(&self, key: &str, pattern: &str) -> Result<Vec<String>, DataStoreError> {
        let mut results = Vec::new();
        let prefix = format!("{}:", key);

        // Iterate over all members of the sorted set
        let iter = self.db.prefix_iterator(prefix.as_bytes());
        for item in iter {
            let (key_bytes, _) = item.map_err(|_| DataStoreError::OperationFailed)?;

            // Convert key to string
            let full_key = String::from_utf8(key_bytes.to_vec())
                .map_err(|_| DataStoreError::OperationFailed)?;

            // Extract member from the key (remove prefix)
            if let Some(member) = full_key.strip_prefix(&prefix) {
                // Check if the member matches the pattern
                if member.contains(pattern) {
                    results.push(member.to_string());
                }
            }
        }

        Ok(results)
    }

    //not sure this is used
    async fn zscore(&self, key: &str, member: &str) -> Result<Option<f64>, DataStoreError> {
        // Create a key with the format "{key}:{member}"
        let member_key = format!("{}:{}", key, member);

        // Retrieve the score from the database
        match self.db.get(member_key.as_bytes()) {
            Ok(Some(value_bytes)) => {
                let score_str = String::from_utf8(value_bytes.to_vec())
                    .map_err(|_| DataStoreError::OperationFailed)?;
                let score = score_str.parse::<f64>()
                    .map_err(|_| DataStoreError::OperationFailed)?;
                Ok(Some(score))
            }
            Ok(None) => Ok(None), // Member not found
            Err(_) => Err(DataStoreError::OperationFailed),
        }
    }
}