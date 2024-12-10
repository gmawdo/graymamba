use rocksdb::{DB, Options};
use async_trait::async_trait;
use crate::backingstore::data_store::{DataStore, DataStoreError};

use crate::backingstore::data_store::KeyType;

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::debug;

impl fmt::Display for RocksDBDataStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RocksDBDataStore")
    }
}

pub struct RocksDBDataStore{
    db: DB,
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
        let hash_tag = "{graymamba}";
        let path = format!("/{}", "graymamba");
        let key = format!("{}:{}", hash_tag, mount_path);

        // Check if the directory already exists
        if let Ok(Some(_)) = self.db.get(key.as_bytes()) {
            return Ok(());
        }
        let node_type = "0";
        let size = 0;
        let permissions = 777;
        let score = if mount_path == "/" { 1.0 } else { 2.0 };

        let nodes = format!("{}:/{}_nodes", hash_tag, "graymamba");
        let key_exists: bool = self.db.get(&nodes).map_err(|_| DataStoreError::OperationFailed)?.is_some();

        let fileid: u64 = if key_exists {
            // Get and increment the next file ID atomically
            let next_fileid_key = format!("{}:/{}^_next_fileid", hash_tag, "graymamba");
            let current_id = self.db.get(next_fileid_key.as_bytes())
                .map_err(|_| DataStoreError::OperationFailed)?
                .and_then(|v| String::from_utf8(v).ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let new_id = current_id + 1;
            // Save the incremented value
            self.db.put(next_fileid_key.as_bytes(), new_id.to_string().as_bytes())
                .map_err(|_| DataStoreError::OperationFailed)?;
            new_id
        } else {
            1
        };

        let system_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos();

        // Add to sorted set (equivalent to Redis ZADD)
        let nodes_key = format!("zset:{}:/{}_nodes:{}", hash_tag, "graymamba", mount_path);
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

        // Store hash fields (equivalent to Redis HSET)
        for (field, value) in hash_fields {
            let hash_key = format!("{}:{}:{}", hash_tag, mount_path, field);
            self.db.put(hash_key.as_bytes(), value.as_bytes())
                .map_err(|_| DataStoreError::OperationFailed)?;
        }

        // Set path to id mapping
        let path_to_id_key = format!("{}:{}_path_to_id", hash_tag, path);
        self.db.put(
            format!("{}:{}", path_to_id_key, mount_path).as_bytes(),
            fileid_str.as_bytes()
        ).map_err(|_| DataStoreError::OperationFailed)?;

        // Set id to path mapping
        let id_to_path_key = format!("{}:{}_id_to_path", hash_tag, path);
        self.db.put(
            format!("{}:{}", id_to_path_key, fileid_str).as_bytes(),
            mount_path.as_bytes()
        ).map_err(|_| DataStoreError::OperationFailed)?;

        if fileid == 1 {
            self.db.put(
                format!("{}:{}_next_fileid", hash_tag, path).as_bytes(),
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
        self.db.delete(key.as_bytes())
            .map_err(|_| DataStoreError::OperationFailed)
    }

    async fn hget(&self, key: &str, field: &str) -> Result<String, DataStoreError> {
        let full_key = format!("{}:{}", key, field);
        self.get(&full_key).await
    }

    async fn hset(&self, key: &str, field: &str, value: &str) -> Result<(), DataStoreError> {
        let full_key = format!("{}:{}", key, field);
        self.set(&full_key, value).await
    }

    async fn hdel(&self, key: &str, field: &str) -> Result<(), DataStoreError> {
        let full_key = format!("{}:{}", key, field);
        self.delete(&full_key).await
    }

    async fn hgetall(&self, key: &str) -> Result<Vec<(String, String)>, DataStoreError> {
        let mut results = Vec::new();
        let iter = self.db.prefix_iterator(key);
        for item in iter {
            let (full_key, value) = item.map_err(|_| DataStoreError::OperationFailed)?;
            let full_key_str = String::from_utf8(full_key.to_vec()).map_err(|_| DataStoreError::OperationFailed)?;
            let value_str = String::from_utf8(value.to_vec()).map_err(|_| DataStoreError::OperationFailed)?;
            if let Some(field) = full_key_str.strip_prefix(&format!("{}:", key)) {
                results.push((field.to_string(), value_str));
            }
        }
        Ok(results)
    }

    async fn incr(&self, key: &str) -> Result<i64, DataStoreError> {
        // For RocksDB, we need to implement atomic increment
        // This is a basic implementation, not atomic!!!!!
        let current = self.get(key).await.unwrap_or("0".to_string());
        let value = current.parse::<i64>().map_err(|_| DataStoreError::OperationFailed)?;
        let new_value = value + 1;
        self.set(key, &new_value.to_string()).await?;
        Ok(new_value)
    }

    async fn rename(&self, old_key: &str, new_key: &str) -> Result<(), DataStoreError> {
        let value = self.get(old_key).await?;
        self.set(new_key, &value).await?;
        self.delete(old_key).await
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>, DataStoreError> {
        // RocksDB doesn't have direct pattern matching, you'll need to implement scanning
        let mut results = Vec::new();
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        for item in iter {
            let (key, _) = item.map_err(|_| DataStoreError::OperationFailed)?;
            let key_str = String::from_utf8(key.to_vec())
                .map_err(|_| DataStoreError::OperationFailed)?;
            if key_str.contains(pattern) {
                results.push(key_str);
            }
        }
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
        // In zadd we use: format!("zset:{}:/{}_nodes:{}", hash_tag, "graymamba", mount_path)
        let prefix = format!("zset:{}", key);

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
        // Create a key with the format "zset:{key}:{member}"
        let member_key = format!("zset:{}:{}", key, member);
        
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
        // Create a key with the format "zset:{key}:{member}"
        let member_key = format!("zset:{}:{}", key, member);
        
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
        let prefix = format!("zset:{}:", key);

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

    async fn hset_multiple(&self, key: &str, fields: &[(&str, &str)]) 
        -> Result<(), DataStoreError> {
        for (field, value) in fields {
            self.hset(key, field, value).await?;
        }
        Ok(())
    }

    // This function is intended to scan through a sorted set and return members that match a specific pattern. 
    // It uses prefix iteration to retrieve all members of the sorted set.
    // It then checks if the member matches the specified pattern and collects the results.
    // Any errors during iteration or parsing are mapped to DataStoreError::OperationFailed.
    // This implementation allows us to retrieve members of a sorted set that match a specific pattern.
    async fn zscan_match(&self, key: &str, pattern: &str) -> Result<Vec<String>, DataStoreError> {
        let mut results = Vec::new();
        let prefix = format!("zset:{}:", key);

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
        // Create a key with the format "zset:{key}:{member}"
        let member_key = format!("zset:{}:{}", key, member);

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