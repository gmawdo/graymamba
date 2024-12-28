use std::sync::Arc;
use r2d2::Pool;
use std::error::Error as StdError;
use r2d2_redis_cluster2::Commands;
use async_trait::async_trait;
use crate::backingstore::data_store::{DataStore, DataStoreError};
use config::{Config, File as ConfigFile, ConfigError};

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use r2d2_redis_cluster2::{r2d2, RedisClusterConnectionManager};

use tracing::warn;
use crate::backingstore::data_store::KeyType;

use graymamba::sharesfs::SharesFS;

pub fn get_redis_cluster_pool() -> Result<Pool<RedisClusterConnectionManager>, Box<dyn StdError>> {
    RedisClusterPool::get_redis_cluster_pool()
}

pub struct RedisClusterPool {
    pub pool: r2d2::Pool<RedisClusterConnectionManager>,
}

impl RedisClusterPool {
    pub fn new(_redis_urls: Vec<&str>, _max_size: u32) -> Result<RedisClusterPool, Box<dyn std::error::Error>> {
        // Instead of creating a new pool directly, use the existing function
        let pool = Self::get_redis_cluster_pool()?;
        Ok(RedisClusterPool { pool })
    }

    // Add a new function to get the pool:
    pub fn get_redis_cluster_pool() -> Result<Pool<RedisClusterConnectionManager>, Box<dyn StdError>> {
        let mut settings = Config::default();
        settings.merge(ConfigFile::with_name("config/settings.toml"))?;

        let storage_nodes: Vec<String> = settings.get::<Vec<String>>("cluster_nodes")?;
        let storage_nodes: Vec<&str> = storage_nodes.iter().map(|s| s.as_str()).collect();

        println!("🛠️ Creating Redis cluster pool with nodes: {:?}", storage_nodes);
        
        let manager = RedisClusterConnectionManager::new(storage_nodes)
            .map_err(|e| Box::new(e) as Box<dyn StdError>)?;

        r2d2::Pool::builder()
            .max_size(2)
            .build(manager)
            .map_err(|e| Box::new(e) as Box<dyn StdError>)
    }

    pub fn from_config_file() -> Result<RedisClusterPool, ConfigError> {
        let mut settings = Config::default();
        settings.merge(ConfigFile::with_name("config/settings.toml"))?;
        
        let redis_nodes: Vec<String> = settings.get::<Vec<String>>("cluster_nodes")?;
        let redis_nodes: Vec<&str> = redis_nodes.iter().map(|s| s.as_str()).collect();
        
        let max_size: u32 = settings.get("redis_pool_max_size").unwrap_or(1000);

        RedisClusterPool::new(redis_nodes, max_size).map_err(|e| {
            ConfigError::Message(e.to_string())
        })
    }
}

pub struct RedisDataStore {
    pool: Arc<r2d2::Pool<r2d2_redis_cluster2::RedisClusterConnectionManager>>,
}

impl RedisDataStore {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let redis_pool = RedisClusterPool::from_config_file()?;
        let inner_pool = Arc::new(redis_pool.pool);
        Ok(RedisDataStore { pool: inner_pool })
    }
}

#[async_trait]
impl DataStore for RedisDataStore {
    async fn authenticate_user(&self, userkey: &str) -> KeyType {
        let mut conn = self.pool.get().unwrap();

        let user_exists: Result<bool, _> = conn.sismember("GRAYMAMBAWALLETS", userkey);
        if let Ok(exists) = user_exists {
            if exists {
                warn!("Wallet exists: {}", userkey);
                return KeyType::Usual;
            }
        }

        // Check if userkey variant exists for special access
        let special_key = format!("{}-su", userkey);
        let special_exists: Result<bool, _> = conn.sismember("GRAYMAMBAWALLETS", &special_key);
        if let Ok(exists) = special_exists {
            if exists {
                return KeyType::Special;
            }
        }

        KeyType::None
    }

    async fn get(&self, key: &str) -> Result<String, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.get(key).map_err(|_| DataStoreError::KeyNotFound)
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        let _: () = conn.set(key, value).map_err(|_| DataStoreError::OperationFailed)?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.del(key).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn hget(&self, key: &str, field: &str) -> Result<String, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.hget(key, field).map_err(|_| DataStoreError::KeyNotFound)
    }

    async fn hset(&self, key: &str, field: &str, value: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        let _: () = conn.hset(key, field, value).map_err(|_| DataStoreError::OperationFailed)?;
        Ok(())
    }

    async fn hdel(&self, key: &str, field: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.hdel(key, field).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn hgetall(&self, key: &str) -> Result<Vec<(String, String)>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.hgetall(key).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn incr(&self, key: &str) -> Result<i64, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.incr(key, 1).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn rename(&self, old_key: &str, new_key: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.rename(old_key, new_key).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.keys(pattern).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn zrange_withscores(&self, key: &str, start: isize, stop: isize) -> Result<Vec<(String, f64)>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zrange_withscores(key, start, stop).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn zadd(&self, key: &str, member: &str, score: f64) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zadd(key, member, score).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn zrem(&self, key: &str, member: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zrem(key, member).map_err(|_| DataStoreError::OperationFailed)
    }
    async fn zrangebyscore(&self, key: &str, min: f64, max: f64) -> Result<Vec<String>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zrangebyscore(key, min, max).map_err(|_| DataStoreError::OperationFailed)
    }
    async fn hset_multiple(&self, key: &str, fields: &[(&str, &str)]) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        let _: () = conn.hset_multiple::<_, _, _, ()>(key, fields).map_err(|_| DataStoreError::OperationFailed)?;
        Ok(())
    }
    async fn zscan_match(&self, key: &str, pattern: &str) -> Result<Vec<String>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        let results: Vec<(String, f64)> = conn.zscan_match(key, pattern)
            .map_err(|_| DataStoreError::OperationFailed)?
            .collect();
        Ok(results.into_iter().map(|(member, _)| member).collect())
    }

    async fn zscore(&self, key: &str, member: &str) -> Result<Option<f64>, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.zscore(key, member).map_err(|_| DataStoreError::OperationFailed)
    }

    async fn init_user_directory(&self, mount_path: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        let (namespace_id, community) = SharesFS::get_namespace_id_and_community().await;
        let path = format!("/{}", namespace_id);
        let key = format!("{}{}", community, mount_path);
        let exists_response: bool = conn.exists(&key).map_err(|_| DataStoreError::OperationFailed)?;
    
        if exists_response {
            return Ok(());
        }
    
        let node_type = "0";
        let size = 0;
        let permissions = 777;
        let score = if mount_path == "/" { 1.0 } else { 2.0 };
    
        let nodes = format!("{}/{}_nodes", community, namespace_id);
        let key_exists: bool = conn.exists(&nodes).map_err(|_| DataStoreError::OperationFailed)?;
    
        let fileid: u64 = if key_exists {
            conn.incr(format!("{}/{}_next_fileid", community, namespace_id), 1)
                .map_err(|_| DataStoreError::OperationFailed)?
        } else {
            1
        };
    
        let system_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos();
    
        // Instead of using pipeline, execute commands individually
        let _: () = conn.zadd(
            format!("{}/{}_nodes", community, namespace_id),
            mount_path,
            score
        ).map_err(|_| DataStoreError::OperationFailed)?;
    
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

        // In the init_user_directory function, modify the hset_multiple call:
        let _: () = conn.hset_multiple(
            format!("{}{}", community, mount_path),
            &hash_fields
        ).map_err(|_| DataStoreError::OperationFailed)?;
    
        // Set path to id mapping
        let _: () = conn.hset(
            format!("{}{}_path_to_id", community, path),
            mount_path,
            fileid
        ).map_err(|_| DataStoreError::OperationFailed)?;
    
        // Set id to path mapping
        let _: () = conn.hset(
            format!("{}{}_id_to_path", community, path),
            fileid.to_string(),
            mount_path
        ).map_err(|_| DataStoreError::OperationFailed)?;
    
        if fileid == 1 {
            let _: () = conn.set(
                format!("{}{}_next_fileid", community, path),
                1
            ).map_err(|_| DataStoreError::OperationFailed)?;
        }
    
        Ok(())
    }
}