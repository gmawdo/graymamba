use chrono::{DateTime, Local};
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::kernel::api::nfs::{fattr3, fileid3, nfsstat3};

use crate::audit_adapters::irrefutable_audit::AuditEvent;
use crate::audit_adapters::irrefutable_audit::event_types::DISASSEMBLED;

use crate::graymamba::file_metadata::FileMetadata;
use super::{SharesFS, ActiveWrite};

use crate::sharesfs::ChannelBuffer;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use graymamba::backingstore::data_store::DataStoreError;

use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tokio::time::Duration;

impl SharesFS {
    pub(super) async fn handle_write(
        &self,
        id: fileid3,
        offset: u64,
        data: &[u8]
    ) -> Result<fattr3, nfsstat3> {
        let (_namespace_id, community) = SharesFS::get_namespace_id_and_community().await;
        let path = self.get_path_from_id(id).await?;

        debug!("write: {:?}", path);
    
        let channel = {
            let mut active_writes = self.active_writes.lock().await;
            if let Some(write) = active_writes.get_mut(&id) {
                write.last_activity = Instant::now();
                write.channel.clone()
            } else {
                let channel = ChannelBuffer::new();
                active_writes.insert(id, ActiveWrite::new(channel.clone()));
                channel
            }
        };
    
        channel.write(offset, data).await;

        debug!("channel write complete");
        
        let total_size = channel.total_size();
        debug!("total_size: {:?}", total_size);
        debug!("community: {:?}", community);
        debug!("path: {:?}", path);
        self.data_store.hset_multiple(
            &format!("{}{}", community, path),
            &[
                ("size",&total_size.to_string())
            ]
        ).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;

        debug!("hset_multiple complete");

        if self.is_likely_last_write(id, offset, data.len()).await? {
            self.mark_write_as_complete(id).await?;
        }

        if !path.contains("/objects/pack/") && (path.contains("/.git/") || path.ends_with(".git")) {
            debug!("commit_write due to detection of git repo");
            self.commit_write(id).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;
        }

        let local_date_time: DateTime<Local> = Local::now();
        let creation_time = local_date_time.format("%b %d %H:%M:%S %Y").to_string();
    
        let mut user = "";
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() > 2 {
            user = parts[1];
        }

        debug!("Triggering disassembled event");
        let event = AuditEvent {
            creation_time: creation_time.clone(),
            event_type: DISASSEMBLED.to_string(),
            file_path: path.clone(),
            event_key: user.to_string(),
        };
        if let Err(e) = self.irrefutable_audit.trigger_event(event).await {
            warn!("Failed to trigger audit event: {}", e);
        }
    
        let metadata = self.get_metadata_from_id(id).await?;
        FileMetadata::metadata_to_fattr3(id, &metadata).await
    }

    pub async fn commit_write(&self, id: fileid3) -> Result<(), DataStoreError> {

        debug!("Starting commit process for file ID: {}", id);

        let _permit = self.commit_semaphore.acquire().await.map_err(|_| DataStoreError::OperationFailed);

        let (_namespace_id, community) = SharesFS::get_namespace_id_and_community().await;


        let channel = {

            let mut active_writes = self.active_writes.lock().await;

            match active_writes.remove(&id) {

                Some(write) => write.channel,

                None => {

                    debug!("Commit called for non-existent write ID: {}", id);

                    return Ok(());
                }
            }
        };

        let path_result = self.get_path_from_id(id).await;
        let path: String = match path_result {
            Ok(k) => k,
            Err(_) => return Err(DataStoreError::OperationFailed),  // Replace with appropriate nfsstat3 error
        };   

        let contents = channel.read_all().await;


        let base64_contents = STANDARD.encode(&contents); // Convert byte array (Vec<u8>) to a base64 encoded string
        

        match self.secret_sharing.disassemble(&base64_contents).await {
            Ok(shares) => {
                // Attempt to write the shares to the data store
                debug!("Writing shares to data store under key: {:?}", format!("{}{}", community, path));
                match self
                    .data_store
                    .hset(&format!("{}{}", community, path), "data", &shares)
                    .await
                {
                    Ok(_) => {
                        // Update file metadata upon successful storage
                        self.update_file_metadata(&path).await?;

                        // Clear the buffer contents after a successful commit
                        channel.clear().await;

                        Ok(())
                    }
                    Err(_e) => {
                        debug!("Error setting data in DataStore");
                        Err(DataStoreError::OperationFailed)
                    }
                }
            }
            Err(e) => {
                debug!("Shamir disassembly failed: {:?}", e);
                Err(DataStoreError::OperationFailed)
            }
        }

            
       
    }

    pub async fn monitor_active_writes(&self) {
        warn!("Starting active writes monitor");
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            
            let to_commit: Vec<fileid3> = {
                let active_writes = self.active_writes.lock().await;
                let mut to_commit = Vec::new();
                
                for (&id, write) in active_writes.iter() {
                    // Get the path for this file id
                    if let Ok(path) = self.get_path_from_id(id).await {
                        // Don't auto-commit pack files
                        if !path.contains("/objects/pack/") && write.channel.is_write_complete() {
                            to_commit.push(id);
                        }
                    }
                }
                to_commit
            };
    
            for id in to_commit {
                debug!("Auto-committing write for id: {}", id);
                let _ = self.commit_write(id).await;
            }
        }
    }

    async fn update_file_metadata(&self, path: &str) -> Result<(), DataStoreError> {
        let system_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let epoch_seconds = system_time.as_secs();
        let epoch_nseconds = system_time.subsec_nanos();
        let (_namespace_id, community) = SharesFS::get_namespace_id_and_community().await;

        debug!("Updating file metadata for path: {:?}", path);

        let update_result = self.data_store.hset_multiple(&format!("{}{}", community, path),
            &[
                ("change_time_secs", &epoch_seconds.to_string()),
                ("change_time_nsecs", &epoch_nseconds.to_string()),
                ("modification_time_secs", &epoch_seconds.to_string()),
                ("modification_time_nsecs", &epoch_nseconds.to_string()),
                ("access_time_secs", &epoch_seconds.to_string()),
                ("access_time_nsecs", &epoch_nseconds.to_string()),
            ]).await;
            
            match update_result {
                Ok(_) => Ok(()),
                Err(e) => {
                    eprintln!("Error updating file metadata in DataStore: {:?}", e);
                    Err(DataStoreError::OperationFailed)
                }
            }
            
    }

    pub async fn is_likely_last_write(&self, id: fileid3, offset: u64, data_len: usize) -> Result<bool, nfsstat3> {

        debug!("Checking if last write for id: {:?}", id);

        let (_namespace_id, community) = SharesFS::get_namespace_id_and_community().await;

        let path_result = self.get_path_from_id(id).await;
        let path: String = match path_result {
            Ok(k) => k,
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),  // Replace with appropriate nfsstat3 error
        };

        let current_size_result = self.data_store.hget(&format!("{}{}", community, path), "size").await;
        let current_size: u64 = match current_size_result {
            Ok(k) => k.parse::<u64>().map_err(|_| nfsstat3::NFS3ERR_IO)?,
            Err(_) => return Err(nfsstat3::NFS3ERR_IO),  // Replace with appropriate nfsstat3 error
        };
            
        Ok(offset + data_len as u64 >= current_size)
    }

    pub async fn mark_write_as_complete(&self, id: fileid3) -> Result<(), nfsstat3> {
        debug!("Marking write as complete for id: {:?}", id);
        let mut active_writes = self.active_writes.lock().await;
        if let Some(write) = active_writes.get_mut(&id) {
            write.channel.set_complete();
        }
        Ok(())
    }
} 