use async_trait::async_trait;
use r2d2_redis_cluster::{redis::Commands, RedisClusterConnectionManager};
use r2d2::Pool;
use crate::data_store::{DataStore, DataStoreError};

pub struct RedisDataStore {
    pool: Pool<RedisClusterConnectionManager>,
}

impl RedisDataStore {
    pub fn new(pool: Pool<RedisClusterConnectionManager>) -> Self {
        RedisDataStore { pool }
    }
}

#[async_trait]
impl DataStore for RedisDataStore {
    async fn get(&self, key: &str) -> Result<String, DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.get(key).map_err(|_| DataStoreError::KeyNotFound)
    }

    async fn set(&self, key: &str, value: &str) -> Result<(), DataStoreError> {
        let mut conn = self.pool.get().map_err(|_| DataStoreError::ConnectionError)?;
        conn.set(key, value).map_err(|_| DataStoreError::OperationFailed)
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
        conn.hset(key, field, value).map_err(|_| DataStoreError::OperationFailed)
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
}