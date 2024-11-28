// Key components of this module:
// The Poseidon permutation function
// The sponge construction for hashing
// Field arithmetic using ark-ff

use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use std::error::Error;
use rocksdb::{DB, ColumnFamily, Options};
use super::poseidon_hash::PoseidonHasher;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MerkleNode {
    pub hash: Vec<u8>,
    pub timestamp: i64,
    pub left_child: Option<Box<MerkleNode>>,
    pub right_child: Option<Box<MerkleNode>>,
    pub event_data: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct TimeWindowedMerkleTree {
    pub db: DB,
    pub current_window_start: DateTime<Utc>,
    pub window_duration_hours: i64,
}

impl MerkleNode {
    pub fn new_leaf(data: &[u8], timestamp: i64) -> Result<Self, Box<dyn Error>> {
        let hasher = PoseidonHasher::new()?;
        let hash = hasher.hash_leaf(data);

        Ok(MerkleNode {
            hash,
            timestamp,
            left_child: None,
            right_child: None,
            event_data: Some(data.to_vec()),
        })
    }

    pub fn new_internal(left: MerkleNode, right: MerkleNode) -> Result<Self, Box<dyn Error>> {
        let hasher = PoseidonHasher::new()?;
        let hash = hasher.hash_nodes(&left.hash, &right.hash);

        Ok(MerkleNode {
            hash,
            timestamp: std::cmp::max(left.timestamp, right.timestamp),
            left_child: Some(Box::new(left)),
            right_child: Some(Box::new(right)),
            event_data: None,
        })
    }
}

impl TimeWindowedMerkleTree {
    pub fn new(db_path: &str) -> Result<Self, Box<dyn Error>> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        
        // Create column families
        opts.create_missing_column_families(true);
        let cfs = vec![
            "current_tree",
            "historical_roots",
            "event_data",
            "time_indices"
        ];
        
        let db = DB::open_cf(&opts, db_path, cfs)?;

        Ok(Self {
            db,
            current_window_start: Utc::now(),
            window_duration_hours: 24,
        })
    }

    pub fn insert_event(&mut self, event_data: &[u8]) -> Result<(), Box<dyn Error>> {
        let now = Utc::now();
        
        // Check if we need to rotate the window
        if (now - self.current_window_start).num_hours() >= self.window_duration_hours {
            self.rotate_window()?;
        }

        // Get the current tree CF
        let cf_current = self.db.cf_handle("current_tree")
            .ok_or("Failed to get current_tree CF")?;

        // Create a new leaf node using the same timestamp
        let timestamp = now.timestamp_micros();
        let leaf = MerkleNode::new_leaf(event_data, timestamp)?;
        let serialized = bincode::serialize(&leaf)?;
        
        // Use the same timestamp for key ordering
        let key = format!("leaf:{}:{}", timestamp, hex::encode(&leaf.hash[..4]));
        self.db.put_cf(cf_current, key.as_bytes(), serialized)?;

        Ok(())
    }

    fn rotate_window(&mut self) -> Result<(), Box<dyn Error>> {
        // Get column family handles
        let cf_current = self.db.cf_handle("current_tree")
            .ok_or("Failed to get current_tree CF")?;
        let cf_historical = self.db.cf_handle("historical_roots")
            .ok_or("Failed to get historical_roots CF")?;

        // Build the final tree for the current window
        let root = self.build_tree_from_leaves(cf_current)?;

        // Store the root in historical roots
        let window_key = format!("window:{}", self.current_window_start.timestamp());
        let serialized_root = bincode::serialize(&root)?;
        self.db.put_cf(cf_historical, window_key.as_bytes(), serialized_root)?;

        // Clear the current tree
        // Note: In production, you might want to do this in batches
        let iter = self.db.iterator_cf(cf_current, rocksdb::IteratorMode::Start);
        for item in iter {
            let (key, _) = item?;
            self.db.delete_cf(cf_current, key)?;
        }

        // Update the window start time
        self.current_window_start = Utc::now();

        Ok(())
    }

    fn build_tree_from_leaves(&self, cf: &ColumnFamily) -> Result<MerkleNode, Box<dyn Error>> {
        let mut nodes = Vec::new();
        
        // Collect all leaves
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        for item in iter {
            let (_, value) = item?;
            let node: MerkleNode = bincode::deserialize(&value)?;
            nodes.push(node);
        }

        // If empty, return error
        if nodes.is_empty() {
            return Err("No leaves found in current window".into());
        }

        // Build tree bottom-up
        while nodes.len() > 1 {
            let mut new_nodes = Vec::new();
            
            for chunk in nodes.chunks(2) {
                match chunk {
                    [left, right] => {
                        new_nodes.push(MerkleNode::new_internal(left.clone(), right.clone())?);
                    }
                    [left] => {
                        // If odd number of nodes, promote the last one
                        new_nodes.push(left.clone());
                    }
                    _ => unreachable!(),
                }
            }
            
            nodes = new_nodes;
        }

        Ok(nodes.remove(0))
    }
} 