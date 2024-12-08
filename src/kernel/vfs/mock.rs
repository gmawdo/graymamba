use super::api::*;
use crate::kernel::api::nfs::*;
use crate::backingstore::data_store::{DataStore, DataStoreError, KeyType};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MockDataStore {}

#[async_trait]
impl DataStore for MockDataStore {
    async fn get(&self, _key: &str) -> Result<String, DataStoreError> {
        todo!("MockDataStore::get not implemented")
    }

    async fn set(&self, _key: &str, _value: &str) -> Result<(), DataStoreError> {
        todo!("MockDataStore::set not implemented")
    }

    async fn delete(&self, _key: &str) -> Result<(), DataStoreError> {
        todo!("MockDataStore::delete not implemented")
    }

    async fn hget(&self, _key: &str, _field: &str) -> Result<String, DataStoreError> {
        todo!("MockDataStore::hget not implemented")
    }

    async fn hset(&self, _key: &str, _field: &str, _value: &str) -> Result<(), DataStoreError> {
        todo!("MockDataStore::hset not implemented")
    }

    async fn hdel(&self, _key: &str, _field: &str) -> Result<(), DataStoreError> {
        todo!("MockDataStore::hdel not implemented")
    }

    async fn hgetall(&self, _key: &str) -> Result<Vec<(String, String)>, DataStoreError> {
        todo!("MockDataStore::hgetall not implemented")
    }

    async fn incr(&self, _key: &str) -> Result<i64, DataStoreError> {
        todo!("MockDataStore::incr not implemented")
    }

    async fn rename(&self, _old_key: &str, _new_key: &str) -> Result<(), DataStoreError> {
        todo!("MockDataStore::rename not implemented")
    }

    async fn keys(&self, _pattern: &str) -> Result<Vec<String>, DataStoreError> {
        todo!("MockDataStore::keys not implemented")
    }

    async fn zrange_withscores(&self, _key: &str, _start: isize, _stop: isize) -> Result<Vec<(String, f64)>, DataStoreError> {
        todo!("MockDataStore::zrange_withscores not implemented")
    }

    async fn zadd(&self, _key: &str, _member: &str, _score: f64) -> Result<(), DataStoreError> {
        todo!("MockDataStore::zadd not implemented")
    }

    async fn zrem(&self, _key: &str, _member: &str) -> Result<(), DataStoreError> {
        todo!("MockDataStore::zrem not implemented")
    }

    async fn zrangebyscore(&self, _key: &str, _min: f64, _max: f64) -> Result<Vec<String>, DataStoreError> {
        todo!("MockDataStore::zrangebyscore not implemented")
    }

    async fn hset_multiple(&self, _key: &str, _fields: &[(&str, &str)]) -> Result<(), DataStoreError> {
        todo!("MockDataStore::hset_multiple not implemented")
    }

    async fn zscan_match(&self, _key: &str, _pattern: &str) -> Result<Vec<String>, DataStoreError> {
        todo!("MockDataStore::zscan_match not implemented")
    }

    async fn zscore(&self, _key: &str, _member: &str) -> Result<Option<f64>, DataStoreError> {
        todo!("MockDataStore::zscore not implemented")
    }

    async fn authenticate_user(&self, _userkey: &str) -> KeyType {
        todo!("MockDataStore::authenticate_user not implemented")
    }

    async fn init_user_directory(&self, _mount_path: &str) -> Result<(), DataStoreError> {
        todo!("MockDataStore::init_user_directory not implemented")
    }
}

pub struct MockNFSFileSystem {
    capabilities: super::api::VFSCapabilities,
    files: Arc<RwLock<HashMap<fileid3, fattr3>>>,
    next_fileid: Arc<RwLock<u64>>,
    data_store: MockDataStore
}

impl MockNFSFileSystem {
    pub fn new_readonly() -> Self {
        Self {
            capabilities: super::api::VFSCapabilities::ReadOnly,
            files: Arc::new(RwLock::new(HashMap::new())),
            next_fileid: Arc::new(RwLock::new(1)),
            data_store: MockDataStore {}
        }
    }
    
    pub fn new_readwrite() -> Self {
        Self {
            capabilities: super::api::VFSCapabilities::ReadWrite,
            files: Arc::new(RwLock::new(HashMap::new())),
            next_fileid: Arc::new(RwLock::new(1)),
            data_store: MockDataStore {}
        }
    }
}

#[async_trait]
impl NFSFileSystem for MockNFSFileSystem {
    fn data_store(&self) -> &dyn DataStore {
        &self.data_store
    }

    fn capabilities(&self) -> VFSCapabilities {
        self.capabilities.clone()
    }

    fn root_dir(&self) -> fileid3 {
        1
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        let files = self.files.read().await;
        match files.get(&id) {
            Some(attr) => Ok(*attr),
            None => Err(nfsstat3::NFS3ERR_NOENT)
        }
    }

    async fn lookup(&self, _dirid: fileid3, _filename: &filename3) -> Result<fileid3, nfsstat3> {
        let next_id = {
            let mut id = self.next_fileid.write().await;
            *id += 1;
            *id
        };
        Ok(next_id)
    }

    // Implement other required methods with mock behavior
    async fn setattr(&self, _id: fileid3, _setattr: sattr3) -> Result<fattr3, nfsstat3> {
        Ok(fattr3::default())
    }

    async fn read(&self, _id: fileid3, _offset: u64, _count: u32) -> Result<(Vec<u8>, bool), nfsstat3> {
        Ok((Vec::new(), true))
    }

    async fn write(&self, _id: fileid3, _offset: u64, _data: &[u8]) -> Result<fattr3, nfsstat3> {
        todo!("MockNFSFileSystem::write not implemented")
    }

    async fn create(&self, _dirid: fileid3, _filename: &filename3, _attr: sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
        todo!("MockNFSFileSystem::create not implemented")
    }

    async fn create_exclusive(&self, _dirid: fileid3, _filename: &filename3) -> Result<fileid3, nfsstat3> {
        todo!("MockNFSFileSystem::create_exclusive not implemented")
    }

    async fn mkdir(&self, _dirid: fileid3, _dirname: &filename3) -> Result<(fileid3, fattr3), nfsstat3> {
        todo!("MockNFSFileSystem::mkdir not implemented")
    }

    async fn remove(&self, _dirid: fileid3, _filename: &filename3) -> Result<(), nfsstat3> {
        todo!("MockNFSFileSystem::remove not implemented")
    }

    async fn rename(&self, _from_dirid: fileid3, _from_filename: &filename3, _to_dirid: fileid3, _to_filename: &filename3) -> Result<(), nfsstat3> {
        todo!("MockNFSFileSystem::rename not implemented")
    }

    async fn readdir(&self, _dirid: fileid3, _start_after: fileid3, _max_entries: usize) -> Result<ReadDirResult, nfsstat3> {
        todo!("MockNFSFileSystem::readdir not implemented")
    }

    async fn symlink(&self, _dirid: fileid3, _linkname: &filename3, _symlink: &nfspath3, _attr: &sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
        todo!("MockNFSFileSystem::symlink not implemented")
    }

    async fn readlink(&self, _id: fileid3) -> Result<nfspath3, nfsstat3> {
        todo!("MockNFSFileSystem::readlink not implemented")
    }

    // ... implement remaining required methods
} 