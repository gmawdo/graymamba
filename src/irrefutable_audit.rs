/*
Here's a detailed explanation of how the connection between send and process_event is established by the trait:

In the trait definition, the connection between send and process_event is established through the spawn_event_handler method
and the channel system. Here's the flow:

1. The trait requires get_sender() to provide a Sender<AuditEvent>
2. The default implementation of trigger_event() uses this sender to put events on the channel:
3. The trait requires spawn_event_handler(receiver: Receiver<AuditEvent>) which gets the receiving end of the channel
4. The implementation of spawn_event_handler should create a loop that:
    - Receives events from the channel (receiver.recv())
    - Calls process_event() for each received event

Even though the trait doesn't explicitly link them, the implementation has to connect them via the channel system
in spawn_event_handler, thus laying down the pattern for an irrfutable logegr.
*/
use std::error::Error;
use std::sync::mpsc::{Sender, Receiver};
use async_trait::async_trait;

/// Represents an audit event that must be recorded
#[derive(Clone, Debug)]
pub struct AuditEvent {
    pub creation_time: String,
    pub event_type: String,
    pub file_path: String,
    pub event_key: String,
}

/// Trait defining the interface for irrefutable audit systems
/// Implementations of this trait guarantee that events are permanently recorded
/// in a tamper-evident manner using a dedicated event handling thread
#[async_trait]
pub trait IrrefutableAudit: Send + Sync {
    /// Initialize a new instance of the audit system
    /// This must:
    /// 1. Create a channel for event communication
    /// 2. Spawn a dedicated event handling thread
    /// 3. Return a ready-to-use instance
    async fn new() -> Result<Self, Box<dyn Error>> where Self: Sized;

    /// Get the sender handle for dispatching events
    fn get_sender(&self) -> &Sender<AuditEvent>;

    /// Trigger a new audit event
    fn trigger_event(
        &self,
        creation_time: &str,
        event_type: &str,
        file_path: &str,
        event_key: &str,
    ) -> Result<(), Box<dyn Error>> {
        let event = AuditEvent {
            creation_time: creation_time.to_string(),
            event_type: event_type.to_string(),
            file_path: file_path.to_string(),
            event_key: event_key.to_string(),
        };
        self.get_sender()
            .send(event)
            .map_err(|e| Box::new(AuditError::EventProcessingError(e.to_string())) as Box<dyn Error>)
    }

    /// Start the event handler thread
    fn spawn_event_handler(receiver: Receiver<AuditEvent>) -> Result<(), Box<dyn Error>> where Self: Sized;

    /// Process a single event in the handler thread
    async fn process_event(&self, event: AuditEvent) -> Result<(), Box<dyn Error>>;

    /// Gracefully shutdown the audit system
    fn shutdown(&self) -> Result<(), Box<dyn Error>>;
}

/// Error types specific to audit operations
#[derive(Debug)]
pub enum AuditError {
    ConnectionError(String),
    TransactionError(String),
    ConfigurationError(String),
    EventProcessingError(String),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AuditError::ConnectionError(msg) => write!(f, "Audit connection error: {}", msg),
            AuditError::TransactionError(msg) => write!(f, "Audit transaction error: {}", msg),
            AuditError::ConfigurationError(msg) => write!(f, "Audit configuration error: {}", msg),
            AuditError::EventProcessingError(msg) => write!(f, "Audit event processing error: {}", msg),
        }
    }
}

impl Error for AuditError {}

/// Constants defining known event types
pub mod event_types {
    pub const DISASSEMBLED: &str = "disassembled";
    pub const REASSEMBLED: &str = "reassembled";
}