use rocksdb::DB;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use std::error::Error;

// Import the AuditEvent struct from the main library
use graymamba::irrefutable_audit::AuditEvent;

#[derive(Serialize, Deserialize, Debug)]
struct StoredEvent {
    event: AuditEvent,
    timestamp: u64,
}

fn main() -> Result<(), Box<dyn Error>> {
    // Open the same DB path as used in MerkleBasedAuditSystem
    let db = DB::open_default("../RocksDBs/audit_db")?;
    
    println!("Reading audit events from database...\n");
    
    // Iterate over all key-value pairs in the database
    let iterator = db.iterator(rocksdb::IteratorMode::Start);
    
    for item in iterator {
        let (key, value) = item?;
        let key_str = String::from_utf8(key.to_vec())?;
        
        let stored_event: StoredEvent = bincode::deserialize(&value)?;
        let timestamp = DateTime::<Utc>::from_timestamp(stored_event.timestamp as i64, 0)
            .unwrap()
            .to_rfc3339();
        
        println!("Event Key: {}", key_str);
        println!("Timestamp: {}", timestamp);
        println!("Event Type: {}", stored_event.event.event_type);
        println!("File Path: {}", stored_event.event.file_path);
        println!("Creation Time: {}", stored_event.event.creation_time);
        println!("-------------------\n");
    }
    
    Ok(())
}