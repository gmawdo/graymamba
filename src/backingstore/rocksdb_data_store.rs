use rocksdb::{DB, Options};
use async_trait::async_trait;
use crate::backingstore::data_store::{DataStore, DataStoreError};

use crate::backingstore::data_store::KeyType;

use std::fmt;

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
    async fn authenticate_user(&self, _userkey: &str) -> KeyType {
        // Implement user authentication logic
        unimplemented!("User authentication not implemented for RocksDB")
    }

    async fn init_user_directory(&self, _mount_path: &str) -> Result<(), DataStoreError> {
        // Implement directory initialization logic
        unimplemented!("Directory initialization not implemented for RocksDB")
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

    async fn zrange_withscores(&self, key: &str, start: isize, stop: isize) 
        -> Result<Vec<(String, f64)>, DataStoreError> {
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
                results.push((member.to_string(), score));
            }
        }

        // Sort by score
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Apply range
        let len = results.len() as isize;
        let normalized_start = if start < 0 { len + start } else { start };
        let normalized_stop = if stop < 0 { len + stop } else { stop };
        
        let start_idx = normalized_start.clamp(0, len) as usize;
        let stop_idx = (normalized_stop + 1).clamp(0, len) as usize;

        Ok(results[start_idx..stop_idx].to_vec())
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