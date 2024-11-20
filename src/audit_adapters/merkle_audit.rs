use std::error::Error;
use tokio::sync::mpsc as tokio_mpsc;
use async_trait::async_trait;
use std::sync::Arc;
use crate::irrefutable_audit::{AuditEvent, IrrefutableAudit};

use tracing::debug;

use rocksdb::DB;
use serde::{Serialize, Deserialize};

/// Implementation of the IrrefutableAudit trait
#[derive(Serialize, Deserialize)]
struct StoredEvent {
    event: AuditEvent,
    timestamp: u64,
}

pub struct MerkleBasedAuditSystem {
    sender: tokio_mpsc::Sender<AuditEvent>,
    db: Arc<DB>,
}

#[async_trait]
impl IrrefutableAudit for MerkleBasedAuditSystem {
    async fn new() -> Result<Self, Box<dyn Error>> {
        println!("Initialising Merkle based audit system");
        let (sender, receiver) = tokio_mpsc::channel(100);
        
        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.set_max_background_jobs(4);
        opts.set_use_fsync(true);
        opts.set_keep_log_file_num(10);
        opts.set_allow_concurrent_memtable_write(true);
        
        let db = Arc::new(DB::open(&opts, "../RocksDBs/audit_db")?);
        
        let audit = Arc::new(MerkleBasedAuditSystem { sender, db });
        MerkleBasedAuditSystem::spawn_event_handler(audit.clone(), receiver)?;
        Ok(MerkleBasedAuditSystem { sender: audit.get_sender().clone(), db: audit.db.clone() })
    }

    fn get_sender(&self) -> &tokio_mpsc::Sender<AuditEvent> {
        &self.sender
    }

    fn spawn_event_handler(
        audit: Arc<dyn IrrefutableAudit>, 
        mut receiver: tokio_mpsc::Receiver<AuditEvent>
    ) -> Result<(), Box<dyn Error>> {
        println!("Spawning event handler");
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                debug!("Received event: {:?}", event);
                if let Err(e) = audit.process_event(event).await {
                    eprintln!("Error processing event: {}", e);
                }
            }
        });
        Ok(())
    }

    async fn process_event(&self, event: AuditEvent) -> Result<(), Box<dyn Error>> {
        debug!("Processing event: {:?}", event);
        
        let stored_event = StoredEvent {
            event: event.clone(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        
        // Create a composite key: event_key:creation_time:file_path
        let composite_key = format!(
            "{}:{}:{}",
            event.event_key,
            event.creation_time,
            event.file_path
        );
        
        let serialized = bincode::serialize(&stored_event)?;
        self.db.put(composite_key.as_bytes(), serialized)?;
        
        Ok(())
    }

    fn shutdown(&self) -> Result<(), Box<dyn Error>> {
        println!("Shutting down audit system.");
        Ok(())
    }
}
#[cfg(test)]
pub mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn test_multiple_event_storage() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let audit_system = MerkleBasedAuditSystem::new().await.unwrap();
            
            // Create two events with same event_key but different times/paths
            let event1 = AuditEvent {
                creation_time: "2023-10-01T12:00:00Z".to_string(),
                event_type: "test_event".to_string(),
                file_path: "/test/path1".to_string(),
                event_key: "Martha".to_string(),
            };
            
            let event2 = AuditEvent {
                creation_time: "2023-10-01T12:01:00Z".to_string(),
                event_type: "test_event".to_string(),
                file_path: "/test/path2".to_string(),
                event_key: "Martha".to_string(),
            };
            
            audit_system.process_event(event1.clone()).await.unwrap();
            audit_system.process_event(event2.clone()).await.unwrap();
            
            // Verify both events are stored
            let key1 = format!("{}:{}:{}", event1.event_key, event1.creation_time, event1.file_path);
            let key2 = format!("{}:{}:{}", event2.event_key, event2.creation_time, event2.file_path);
            
            let stored1 = audit_system.db.get(key1.as_bytes()).unwrap().unwrap();
            let stored2 = audit_system.db.get(key2.as_bytes()).unwrap().unwrap();
            
            let stored_event1: StoredEvent = bincode::deserialize(&stored1).unwrap();
            let stored_event2: StoredEvent = bincode::deserialize(&stored2).unwrap();
            
            assert_eq!(stored_event1.event.file_path, event1.file_path);
            assert_eq!(stored_event2.event.file_path, event2.file_path);
        });
    }
}