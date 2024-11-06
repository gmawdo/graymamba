use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::data_store::{DataStore, DataStoreError, DataStoreResult};
use crate::data_store::KeyType;

pub struct TestDataStore {
    data: Arc<RwLock<HashMap<String, String>>>,
    sets: Arc<RwLock<HashMap<String, HashMap<String, f64>>>>
}

#[async_trait]
impl DataStore for TestDataStore {
    async fn authenticate_user(&self, userkey: &str) -> KeyType {
        if userkey.ends_with("-su") {
            KeyType::Special
        } else {
            KeyType::Usual
        }
    }

    async fn get(&self, key: &str) -> DataStoreResult<String> {
        let data = self.data.read().await;
        data.get(key).cloned().ok_or(DataStoreError::KeyNotFound)
    }

    async fn set(&self, key: &str, value: &str) -> DataStoreResult<()> {
        let mut data = self.data.write().await;
        data.insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn delete(&self, key: &str) -> DataStoreResult<()> {
        let mut data = self.data.write().await;
        data.remove(key);
        Ok(())
    }

    async fn hset(&self, key: &str, field: &str, value: &str) -> DataStoreResult<()> {
        let mut data = self.data.write().await;
        data.insert(format!("{}:{}", key, field), value.to_string());
        Ok(())
    }

    async fn hget(&self, key: &str, field: &str) -> DataStoreResult<String> {
        let data = self.data.read().await;
        data.get(&format!("{}:{}", key, field))
            .cloned()
            .ok_or(DataStoreError::KeyNotFound)
    }

    async fn hdel(&self, key: &str, field: &str) -> DataStoreResult<()> {
        let mut data = self.data.write().await;
        data.remove(&format!("{}:{}", key, field));
        Ok(())
    }

    async fn hgetall(&self, key: &str) -> DataStoreResult<Vec<(String, String)>> {
        let data = self.data.read().await;
        let prefix = format!("{}:", key);
        let result: Vec<(String, String)> = data.iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(k, v)| (k[prefix.len()..].to_string(), v.clone()))
            .collect();
        Ok(result)
    }

    async fn incr(&self, key: &str) -> DataStoreResult<i64> {
        let mut data = self.data.write().await;
        let current = data.get(key)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let new_value = current + 1;
        data.insert(key.to_string(), new_value.to_string());
        Ok(new_value)
    }

    async fn rename(&self, old_key: &str, new_key: &str) -> DataStoreResult<()> {
        let mut data = self.data.write().await;
        if let Some(value) = data.remove(old_key) {
            data.insert(new_key.to_string(), value);
        }
        Ok(())
    }

    async fn keys(&self, pattern: &str) -> DataStoreResult<Vec<String>> {
        let data = self.data.read().await;
        Ok(data.keys()
            .filter(|k| k.contains(pattern))
            .cloned()
            .collect())
    }

    async fn zrange_withscores(&self, key: &str, start: isize, stop: isize) -> DataStoreResult<Vec<(String, f64)>> {
        let sets = self.sets.read().await;
        Ok(sets.get(key)
            .map(|set| set.iter()
                .skip(start as usize)
                .take((stop - start) as usize)
                .map(|(member, score)| (member.clone(), *score))
                .collect())
            .unwrap_or_default())
    }

    async fn zadd(&self, key: &str, member: &str, score: f64) -> DataStoreResult<()> {
        let mut sets = self.sets.write().await;
        sets.entry(key.to_string())
            .or_default()
            .insert(member.to_string(), score);
        Ok(())
    }

    async fn zrem(&self, key: &str, member: &str) -> DataStoreResult<()> {
        let mut sets = self.sets.write().await;
        if let Some(set) = sets.get_mut(key) {
            set.remove(member);
        }
        Ok(())
    }

    async fn zrangebyscore(&self, key: &str, min: f64, max: f64) -> DataStoreResult<Vec<String>> {
        let sets = self.sets.read().await;
        Ok(sets.get(key)
            .map(|set| set.iter()
                .filter(|(_, score)| **score >= min && **score <= max)
                .map(|(member, _)| member.clone())
                .collect())
            .unwrap_or_default())
    }

    async fn hset_multiple(&self, key: &str, fields: &[(&str, &str)]) -> DataStoreResult<()> {
        let mut data = self.data.write().await;
        for (field, value) in fields {
            data.insert(format!("{}:{}", key, field), value.to_string());
        }
        Ok(())
    }

    async fn zscan_match(&self, key: &str, pattern: &str) -> DataStoreResult<Vec<String>> {
        let sets = self.sets.read().await;
        Ok(sets.get(key)
            .map(|set| set.iter()
                .filter(|(member, _)| member.contains(pattern))
                .map(|(member, _)| member.clone())
                .collect())
            .unwrap_or_default())
    }

    async fn zscore(&self, key: &str, member: &str) -> DataStoreResult<Option<f64>> {
        let sets = self.sets.read().await;
        Ok(sets.get(key)
            .and_then(|set| set.get(member).copied()))
    }
}

impl TestDataStore {
    pub fn new() -> Self {
        TestDataStore {
            data: Arc::new(RwLock::new(HashMap::new())),
            sets: Arc::new(RwLock::new(HashMap::new()))
        }
    }
}