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
    let mut opts = rocksdb::Options::default();
    opts.set_max_background_jobs(4);
    opts.set_allow_concurrent_memtable_write(true);
    // Open in read-only mode since we're just reading
    let db = DB::open_for_read_only(&opts, "../RocksDBs/audit_db", false)?;
    
    println!("Reading audit events from database...\n");
    
    // Iterate over all key-value pairs in the database
    let iterator = db.iterator(rocksdb::IteratorMode::Start);
    
    for item in iterator {
        let (key, value) = item?;
        let key_str = String::from_utf8(key.to_vec())?;
        
        let stored_event: StoredEvent = bincode::deserialize(&value)?;
        let _timestamp = DateTime::<Utc>::from_timestamp(stored_event.timestamp as i64, 0)
            .unwrap()
            .to_rfc3339();
        
        println!("{:>12.12}, {}", stored_event.event.event_type.to_uppercase(), key_str.replace(":", " :: "));
    }
    
    Ok(())
}