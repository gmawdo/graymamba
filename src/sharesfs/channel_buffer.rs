use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use bytes::{BytesMut, Bytes};
use std::sync::Arc;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tracing::debug;
pub struct ActiveWrite {
    pub channel: Arc<ChannelBuffer>,
    pub last_activity: Instant,
}

impl ActiveWrite {
    pub fn new(channel: Arc<ChannelBuffer>) -> Self {
        ActiveWrite {
            channel,
            last_activity: Instant::now(),
        }
    }
}
pub struct ChannelBuffer {
    buffer: RwLock<BTreeMap<u64, Bytes>>,
    total_size: AtomicU64,
    last_write: RwLock<Instant>,
    is_complete: AtomicBool,
}

impl ChannelBuffer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            buffer: RwLock::new(BTreeMap::new()),
            total_size: AtomicU64::new(0),
            last_write: RwLock::new(Instant::now()),
            is_complete: AtomicBool::new(false),
        })
    }

    pub async fn read_range(&self, offset: u64, count: u32) -> Vec<u8> {
        let buffer = self.buffer.read().await;
        let mut result = Vec::with_capacity(count as usize);
        let end_offset = offset + count as u64;
        let total_size = self.total_size.load(Ordering::SeqCst);
        
        // If reading past total size, adjust end_offset
        let end_offset = std::cmp::min(end_offset, total_size);
        
        // Find all chunks that overlap with the requested range
        let mut current_offset = offset;
        while current_offset < end_offset {
            if let Some(bytes) = buffer.get(&current_offset) {
                let available_bytes = bytes.len() as u64;
                let bytes_to_copy = std::cmp::min(
                    available_bytes,
                    end_offset - current_offset
                ) as usize;
                
                result.extend_from_slice(&bytes[..bytes_to_copy]);
                current_offset += bytes_to_copy as u64;
            } else {
                break;
            }
        }
        
        result
    }

    pub async fn write(&self, offset: u64, data: &[u8]) {
        debug!("write: {:?}", offset);
        let mut buffer = self.buffer.write().await; // Acquire write lock

        buffer.insert(offset, Bytes::copy_from_slice(data));

        // Update total size if this write extends it
        let end_offset = offset + data.len() as u64;
        let current_size = self.total_size.load(Ordering::SeqCst);
        if end_offset > current_size {
            self.total_size.store(end_offset, Ordering::SeqCst);
        }

        // Update last write time
        *self.last_write.write().await = Instant::now(); // Acquire write lock for last_write
    }

    pub async fn read_all(&self) -> Bytes {
        debug!("read_all");
        let buffer = self.buffer.read().await;
        let mut result = BytesMut::with_capacity(self.total_size.load(Ordering::SeqCst) as usize);
        
        let mut expected_offset = 0;
        for (&offset, chunk) in buffer.iter() {
            // Print the key (offset) and value (chunk) being processed
            println!("Processing offset: {}, chunk: {:?}", offset, chunk); //this revealed an ordering issue

            if offset != expected_offset {
                result.resize(offset as usize, 0);
            }
            result.extend_from_slice(chunk);
            expected_offset = offset + chunk.len() as u64;
        }

        result.freeze()
    }

    pub fn total_size(&self) -> u64 {
        self.total_size.load(Ordering::SeqCst)
    }

    pub fn is_write_complete(&self) -> bool {
        self.is_complete.load(Ordering::SeqCst)
    }

    pub fn set_complete(&self) {
        self.is_complete.store(true, Ordering::SeqCst);
    }

    pub async fn time_since_last_write(&self) -> Duration {
        Instant::now().duration_since(*self.last_write.read().await)
    }

    pub async fn clear(&self) {
        let mut buffer = self.buffer.write().await;
        buffer.clear();
    }

    pub async fn is_empty(&self) -> bool {
        let buffer = self.buffer.read().await;
        buffer.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;
    use rand::Rng;
    use tokio::task;
    use std::convert::TryInto;

    #[test]
    fn test_multiple_writes_aggregate() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            // Step 1: Create a new ChannelBuffer instance
            let channel = ChannelBuffer::new();

            // Step 2: Define multiple blocks of bytes to write
            let block_a = b"Hello, ";
            let block_b = b"world!";
            let block_c = b" This is a test.";

            // Step 3: Write the blocks to the channel buffer
            channel.write(0, block_a).await;
            channel.write(7, block_b).await; // Write at offset 7
            channel.write(13, block_c).await; // Write at offset 13

            // Step 4: Read back the data
            let result = channel.read_all().await;

            // Step 5: Verify that the data matches the expected output
            let expected = b"Hello, world! This is a test.";
            assert_eq!(result.as_ref(), expected);
        });
    }

    #[test]
    fn test_multiple_large_writes_aggregate() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let channel = ChannelBuffer::new();
            let num_blocks = 10000; // Adjust this number for larger tests
            let mut expected_data = Vec::new();

            for i in 0..num_blocks {
                // Varying block size based on the index
                let block = format!("Block {}", i).into_bytes();
                let offset = expected_data.len() as u64; // Use the current length of expected_data for offset
                expected_data.extend_from_slice(&block);
                
                // Debugging output
                println!("Writing block {} (size: {}) at offset {}", i, block.len(), offset);
                channel.write(offset, &block).await;
            }

            let result = channel.read_all().await;

            // Debugging output
            println!("Expected data: {:?}", expected_data);
            println!("Actual result: {:?}", result);

            assert_eq!(result.as_ref(), expected_data.as_slice());
        });
    }

    #[test]
    fn test_non_sequential_writes() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let channel = ChannelBuffer::new();
            let num_blocks = 10; // Number of blocks to write
            let mut expected_data = Vec::new();
            let mut rng = rand::thread_rng();
            let mut max_offset = 0; // Track the maximum offset used

            for _ in 0..num_blocks {
                let block_size = rng.gen_range(1..=20); // Random block size between 1 and 20
                let block = vec![b'A'; block_size]; // Create a block of 'A's
                let offset = max_offset; // Use the current max_offset for writing
                max_offset += block_size; // Update max_offset for the next write

                // Print the block and offset for debugging
                println!("Writing block of size {} at offset {}", block_size, offset);
                println!("Block content: {:?}", String::from_utf8_lossy(&block));

                // Introduce a delay of 1 second
                //sleep(Duration::from_secs(1)).await; // Ensure this is in an async context

                // Ensure expected_data is large enough to accommodate the offset
                if offset as usize + block_size > expected_data.len() {
                    expected_data.resize(offset as usize + block_size, 0); // Resize with padding
                }

                // Directly set the expected data at the specified offset
                expected_data[offset as usize..(offset as usize + block_size)].copy_from_slice(&block);

                // Write the block to the channel buffer
                channel.write(offset.try_into().unwrap(), &block).await;
            }

            // Read back the data
            let result = channel.read_all().await;

            // Print expected and actual results for debugging
            println!("Expected data: {:?}", String::from_utf8_lossy(&expected_data));
            println!("Actual result: {:?}", String::from_utf8_lossy(&result));

            // Verify that the data matches the expected output
            assert_eq!(result.as_ref(), expected_data.as_slice());
        });
    }

    #[test]
    fn test_concurrent_writes() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let channel = ChannelBuffer::new();
            let num_tasks = 10; // Number of concurrent tasks
            let mut handles = vec![];

            for i in 0..num_tasks {
                let channel_clone = channel.clone();
                let handle = task::spawn(async move {
                    let block = format!("Block from task {}", i).into_bytes();
                    let offset = (i * block.len()) as u64; // Sequential offsets for simplicity
                    channel_clone.write(offset, &block).await;
                });
                handles.push(handle);
            }

            // Wait for all tasks to complete
            for handle in handles {
                let _ = handle.await;
            }

            // Read back the data
            let result = channel.read_all().await;

            // Verify that the data matches the expected output
            let expected_data: Vec<u8> = (0..num_tasks)
                .flat_map(|i| format!("Block from task {}", i).into_bytes())
                .collect();

            assert_eq!(result.as_ref(), expected_data.as_slice());
        });
    }

    #[test]
    fn test_non_sequential_overlapping_writes() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let channel = ChannelBuffer::new();
            let num_blocks = 5; // Number of blocks to write
            let mut expected_data = Vec::new();
            let mut rng = rand::thread_rng();

            for i in 0..num_blocks {
                //println!("expected data length: {:?}", expected_data.len());
                let block_size = rng.gen_range(1..=15); // Random block size between 1 and 20
                let block_char = (b'A' + (i % 26)) as char; // Cycle through A-Z
                let block = vec![block_char as u8; block_size]; // Create a block filled with the character

                let offset = rng.gen_range(0..=100); // Random offset between 0 and 100

                // Print the block and offset for debugging
                println!("Writing block of size {} at offset {}, with content: {:?}", block_size, offset, String::from_utf8_lossy(&block));

                // Ensure expected_data is large enough to accommodate the offset
                if offset as usize + block_size > expected_data.len() {
                    expected_data.resize(offset as usize + block_size, 0); // Resize with padding
                }

                // Update expected data at the specified offset
                expected_data[offset as usize..(offset as usize + block_size)].copy_from_slice(&block);

                // Write the block to the channel buffer
                channel.write(offset.try_into().unwrap(), &block).await;
                println!("Expected data: {:?}", String::from_utf8_lossy(&expected_data).replace("\0", "-"));
            }

            // Read back the data
            let result = channel.read_all().await;

            // Print actual results for debugging
            println!("Actual result: {:?}", String::from_utf8_lossy(&result).replace("\0", "-"));

            // Verify that the data matches the expected output
            assert_eq!(result.as_ref(), expected_data.as_slice());
        });
    }
}