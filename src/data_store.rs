use async_trait::async_trait;

#[async_trait]
pub trait DataStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<String, DataStoreError>;
    async fn set(&self, key: &str, value: &str) -> Result<(), DataStoreError>;
    async fn delete(&self, key: &str) -> Result<(), DataStoreError>;
    async fn hget(&self, key: &str, field: &str) -> Result<String, DataStoreError>;
    async fn hset(&self, key: &str, field: &str, value: &str) -> Result<(), DataStoreError>;
    async fn hdel(&self, key: &str, field: &str) -> Result<(), DataStoreError>;
    async fn hgetall(&self, key: &str) -> Result<Vec<(String, String)>, DataStoreError>;
    async fn incr(&self, key: &str) -> Result<i64, DataStoreError>;
    async fn rename(&self, old_key: &str, new_key: &str) -> Result<(), DataStoreError>;
    async fn keys(&self, pattern: &str) -> Result<Vec<String>, DataStoreError>;
    async fn zrange_withscores(&self, key: &str, start: isize, stop: isize) -> Result<Vec<(String, f64)>, DataStoreError>;
    async fn zadd(&self, key: &str, member: &str, score: f64) -> Result<(), DataStoreError>;
    async fn zrem(&self, key: &str, member: &str) -> Result<(), DataStoreError>;
    async fn zrangebyscore(&self, key: &str, min: f64, max: f64) -> Result<Vec<String>, DataStoreError>;
    async fn hset_multiple(&self, key: &str, fields: &[(&str, &str)]) -> Result<(), DataStoreError>;

}

#[derive(Debug)]
pub enum DataStoreError {
    ConnectionError,
    KeyNotFound,
    OperationFailed,
}