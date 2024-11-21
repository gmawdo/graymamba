use rocksdb::{DB, Options};
use chrono::{DateTime, Utc};
use std::error::Error;

#[cfg(feature = "irrefutable_audit")]
use graymamba::audit_adapters::merkle_tree::MerkleNode;
use graymamba::irrefutable_audit::AuditEvent;

fn main() -> Result<(), Box<dyn Error>> {
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    
    // Define column families
    let cfs = vec![
        "current_tree",
        "historical_roots",
        "event_data",
        "time_indices"
    ];
    
    // Open the database in read-only mode
    let db = DB::open_cf_for_read_only(&opts, "../RocksDBs/audit_merkle_db", &cfs, false)?;
    
    println!("Reading audit events from Merkle tree database...\n");
    println!("{:<24} {:<12} {:<40} {:<32}", "TIMESTAMP", "TYPE", "PATH", "MERKLE HASH");
    println!("{}", "-".repeat(108));
    
    // Read from current tree
    let cf_current = db.cf_handle("current_tree")
        .ok_or("Failed to get current_tree column family")?;
    
    // Read from historical roots
    let cf_historical = db.cf_handle("historical_roots")
        .ok_or("Failed to get historical_roots column family")?;
    
    // Function to print events from a column family
    let print_events = |cf: &rocksdb::ColumnFamily| -> Result<(), Box<dyn Error>> {
        let iter = db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        
        for item in iter {
            let (_, value) = item?;
            let node: MerkleNode = bincode::deserialize(&value)?;
            
            if let Some(event_data) = node.event_data {
                let event: AuditEvent = bincode::deserialize(&event_data)?;
                let timestamp = DateTime::<Utc>::from_timestamp(node.timestamp, 0)
                    .unwrap()
                    .to_rfc3339();
                
                let hash_preview = hex::encode(&node.hash[..4]);
                
                println!("{:<24} {:<12} {:<40} {:<32}", 
                    timestamp,
                    event.event_type.to_uppercase(),
                    event.file_path,
                    format!("{}...", hash_preview)
                );
            }
        }
        Ok(())
    };
    
    println!("\nCurrent Window Events:");
    print_events(cf_current)?;
    
    println!("\nHistorical Root Hashes:");
    let hist_iter = db.iterator_cf(cf_historical, rocksdb::IteratorMode::Start);
    for item in hist_iter {
        let (key, value) = item?;
        let window_key = String::from_utf8(key.to_vec())?;
        let root: MerkleNode = bincode::deserialize(&value)?;
        println!("Window: {}, Root Hash: {}", 
            window_key,
            hex::encode(&root.hash)
        );
    }
    
    Ok(())
}