use rocksdb::{DB, Options};
use async_trait::async_trait;
use crate::data_store::{DataStore, DataStoreError};

use crate::data_store::KeyType;

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
    async fn authenticate_user(&self, userkey: &str) -> KeyType {
        // Implement user authentication logic
        unimplemented!("User authentication not implemented for RocksDB")
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
        // For RocksDB, you'll need to implement atomic increment
        // This is a basic implementation, not atomic
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

    async fn zrange_withscores(&self, key: &str, start: isize, stop: isize) 
        -> Result<Vec<(String, f64)>, DataStoreError> {
        // Implement sorted set operations
        unimplemented!("Sorted set operations not implemented for RocksDB")
    }

    async fn zadd(&self, key: &str, member: &str, score: f64) -> Result<(), DataStoreError> {
        unimplemented!("Sorted set operations not implemented for RocksDB")
    }

    async fn zrem(&self, key: &str, member: &str) -> Result<(), DataStoreError> {
        unimplemented!("Sorted set operations not implemented for RocksDB")
    }

    //used by get_nodes_in_subpath > get_direct_children > readdir
    async fn zrangebyscore(&self, key: &str, min: f64, max: f64) 
        -> Result<Vec<String>, DataStoreError> {
        unimplemented!("Sorted set operations not implemented for RocksDB")
    }

    async fn hset_multiple(&self, key: &str, fields: &[(&str, &str)]) 
        -> Result<(), DataStoreError> {
        for (field, value) in fields {
            self.hset(key, field, value).await?;
        }
        Ok(())
    }

    //not sure this is used
    async fn zscan_match(&self, key: &str, pattern: &str) 
        -> Result<Vec<String>, DataStoreError> {
        unimplemented!("Sorted set operations not implemented for RocksDB")
    }
    //not sure this is used
    async fn zscore(&self, key: &str, member: &str) -> Result<Option<f64>, DataStoreError> {
        unimplemented!("Sorted set operations not implemented for RocksDB")
    }

    async fn init_user_directory(&self, mount_path: &str) -> Result<(), DataStoreError> {
        // Implement directory initialization logic
        unimplemented!("Directory initialization not implemented for RocksDB")
    }
}