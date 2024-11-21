use super::merkle_tree::TimeWindowedMerkleTree;
use crate::irrefutable_audit::{AuditEvent, IrrefutableAudit};
use async_trait::async_trait;
use std::error::Error;
use tokio::sync::mpsc as tokio_mpsc;
use std::sync::Arc;
use tracing::debug;

pub struct MerkleBasedAuditSystem {
    sender: tokio_mpsc::Sender<AuditEvent>,
    merkle_tree: Arc<parking_lot::RwLock<TimeWindowedMerkleTree>>,
}

#[async_trait]
impl IrrefutableAudit for MerkleBasedAuditSystem {
    async fn new() -> Result<Self, Box<dyn Error>> {
        println!("Attaching and initialising a Merkle based audit system");
        let (sender, receiver) = tokio_mpsc::channel(100);
        let merkle_tree = Arc::new(parking_lot::RwLock::new(
            TimeWindowedMerkleTree::new("../RocksDBs/audit_merkle_db")?
        ));
        
        let audit = Arc::new(Self { 
            sender, 
            merkle_tree 
        });
        
        Self::spawn_event_handler(audit.clone(), receiver)?;
        
        Ok(Self { 
            sender: audit.get_sender().clone(),
            merkle_tree: audit.merkle_tree.clone()
        })
    }

    fn get_sender(&self) -> &tokio_mpsc::Sender<AuditEvent> {
        &self.sender
    }

    async fn process_event(&self, event: AuditEvent) -> Result<(), Box<dyn Error>> {
        debug!("Processing event: {:?}", event);
        
        // Serialize the event
        let event_bytes = bincode::serialize(&event)?;
        
        // Insert into Merkle tree
        self.merkle_tree.write().insert_event(&event_bytes)?;
        
        Ok(())
    }

    fn spawn_event_handler(
        audit: Arc<dyn IrrefutableAudit>,
        mut receiver: tokio_mpsc::Receiver<AuditEvent>
    ) -> Result<(), Box<dyn Error>> {
        println!("Spawning handler for audit events");
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

    fn shutdown(&self) -> Result<(), Box<dyn Error>> {
        println!("Shutting down Merkle based audit system.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;
    use crate::irrefutable_audit::AuditEvent;
    use crate::audit_adapters::merkle_tree::MerkleNode;
    use rocksdb;
    use std::fs;

    fn cleanup_test_db() {
        let _ = fs::remove_dir_all("../RocksDBs/audit_merkle_db");
    }

    #[test]
    fn test_multiple_event_storage() -> Result<(), Box<dyn Error>> {
        println!("Starting test_multiple_event_storage");
        cleanup_test_db();

        let rt = Runtime::new()?;
        let result = rt.block_on(async {
            println!("Creating new audit system...");
            let audit_system = MerkleBasedAuditSystem::new().await?;
            
            // Create two events with same event_key but different times/paths
            let event1 = AuditEvent {
                creation_time: "2023-10-01T12:00:00Z".to_string(),
                event_type: "test_event".to_string(),
                file_path: "/test/path1".to_string(),
                event_key: "Martha".to_string(),
            };
            
            let event2 = AuditEvent {
                creation_time: "2023-10-01T12:01:00Z".to_string(),
                event_type: "test_event2".to_string(),
                file_path: "/test/path2".to_string(),
                event_key: "Martha".to_string(),
            };
            
            // Process both events
            println!("Processing event 1: {:?}", event1);
            audit_system.process_event(event1.clone()).await?;
            println!("Successfully processed event 1");

            println!("Processing event 2: {:?}", event2);
            audit_system.process_event(event2.clone()).await?;
            println!("Successfully processed event 2");
            
            // Get the merkle tree and verify both events are stored
            println!("Reading from Merkle tree...");
            let tree = audit_system.merkle_tree.read();
            let cf_current = tree.db.cf_handle("current_tree")
                .ok_or("Failed to get current_tree column family")?;
            
            // Verify events exist in the tree
            let iter = tree.db.iterator_cf(cf_current, rocksdb::IteratorMode::Start);
            let mut found_events = 0;
            
            println!("Iterating over events in the tree");
            for item in iter {
                let (key, value) = item?;
                println!("Found key: {}", String::from_utf8_lossy(&key));
                let node: MerkleNode = bincode::deserialize(&value)?;
                if let Some(event_data) = &node.event_data {
                    let stored_event: AuditEvent = bincode::deserialize(event_data)?;
                    println!("Found event: {:?}", stored_event);
                    if stored_event.event_key == "Martha" {
                        found_events += 1;
                    }
                }
            }
            
            println!("Found {} events", found_events);
            if found_events != 2 {
                return Err("Both events should be stored in the Merkle tree".into());
            }
            Ok(())
        });

        cleanup_test_db();
        println!("Test completed");
        result
    }
}