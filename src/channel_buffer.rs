
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};
use bytes::{BytesMut, Bytes};
use std::sync::Arc;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
pub struct ActiveWrite {
    pub channel: Arc<ChannelBuffer>,
    pub last_activity: Instant,
}
pub struct ChannelBuffer {
    //buffer: Mutex<BytesMut>,
    buffer: Mutex<BTreeMap<u64, Bytes>>,
    total_size: AtomicU64,
    last_write: Mutex<Instant>,
    is_complete: AtomicBool,
}

impl ChannelBuffer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            buffer: Mutex::new(BTreeMap::new()),
            total_size: AtomicU64::new(0),
            last_write: Mutex::new(Instant::now()),
            is_complete: AtomicBool::new(false),
        })
    }

    pub async fn write(&self, offset: u64, data: &[u8]) {
        let mut buffer = self.buffer.lock().await;
        buffer.insert(offset, Bytes::copy_from_slice(data));
        
        let new_size = offset + data.len() as u64;
        self.total_size.fetch_max(new_size, Ordering::SeqCst);

        *self.last_write.lock().await = Instant::now();
    }

    pub async fn read_all(&self) -> Bytes {
        let buffer = self.buffer.lock().await;
        let mut result = BytesMut::with_capacity(self.total_size.load(Ordering::SeqCst) as usize);
        
        let mut expected_offset = 0;
        for (&offset, chunk) in buffer.iter() {
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
        Instant::now().duration_since(*self.last_write.lock().await)
    }

    pub async fn clear(&self) {
        let mut buffer = self.buffer.lock().await;
        buffer.clear();
    }

    pub async fn is_empty(&self) -> bool {
        let buffer = self.buffer.lock().await;
        buffer.is_empty()
    }
}