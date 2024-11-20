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
        
        let db = Arc::new(DB::open_default("../RocksDBs/audit_db")?);
        
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
        
        let serialized = bincode::serialize(&stored_event)?;
        self.db.put(event.event_key.as_bytes(), serialized)?;
        
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
    fn test_event_storage() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let audit_system = MerkleBasedAuditSystem::new().await.unwrap();
            let event = AuditEvent {
                creation_time: "2023-10-01T12:00:00Z".to_string(),
                event_type: "test_event".to_string(),
                file_path: "/test/path".to_string(),
                event_key: "test_key".to_string(),
            };
            
            audit_system.process_event(event.clone()).await.unwrap();
            
            let stored_data = audit_system.db.get(event.event_key.as_bytes()).unwrap().unwrap();
            let stored_event: StoredEvent = bincode::deserialize(&stored_data).unwrap();
            
            assert_eq!(stored_event.event.event_key, event.event_key);
        });
    }
}