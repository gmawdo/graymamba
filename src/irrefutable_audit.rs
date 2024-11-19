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

Goals of the IrrefutableAudit Trait:
Initialization (new):
    - Create a new instance of the audit system.
    - This includes creating a channel for event communication and spawning a dedicated event handling thread.

Event Dispatching (get_sender):
    - Provide a sender handle for dispatching events to the audit system.

Event Handling (spawn_event_handler):
    - Start the event handler thread that processes events received on the channel.
    - This method takes an Arc<dyn IrrefutableAudit> and a tokio_mpsc::Receiver<AuditEvent>
        to handle incoming events.

Event Processing (process_event):
    - Define how individual audit events should be processed.
    - This method is called by the event handler thread for each received event.

Shutdown (shutdown):
    - Cleanly shut down the audit system, ensuring that all resources are properly released.

    The trait ensures that any implementation will have a consistent interface for handling audit events in a reliable and tamper-evident manner.
*/
use std::error::Error;
use tokio::sync::mpsc as tokio_mpsc;
use async_trait::async_trait;
use std::sync::Arc;

use tracing::debug;

/// Represents an audit event that must be recorded
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct AuditEvent {
    pub creation_time: String,
    pub event_type: String,
    pub file_path: String,
    pub event_key: String,
}

/// Trait defining the interface for irrefutable audit systems
#[async_trait]
pub trait IrrefutableAudit: Send + Sync {
    async fn new() -> Result<Self, Box<dyn Error>> where Self: Sized;
    fn get_sender(&self) -> &tokio_mpsc::Sender<AuditEvent>;
    fn spawn_event_handler(
        audit: Arc<dyn IrrefutableAudit>, 
        receiver: tokio_mpsc::Receiver<AuditEvent>
    ) -> Result<(), Box<dyn Error>> where Self: Sized;
    async fn process_event(&self, event: AuditEvent) -> Result<(), Box<dyn Error>>;
    fn shutdown(&self) -> Result<(), Box<dyn Error>>;

    /// Trigger a new audit event
    async fn trigger_event(&self, event: AuditEvent) -> Result<(), Box<dyn Error>> {
        debug!("Triggering event about to get_sender etc");

        match self.get_sender().send(event.clone()).await {
            Ok(_) => {
                debug!("Successfully sent audit event: {:?}", event);
                Ok(())
            }
            Err(e) => {
                println!("Failed to send audit event: {:?}, error: {}", event, e);
                Err(Box::new(AuditError::EventProcessingError(e.to_string())))
            }
        }
    }
}

/// Error types specific to audit operations
#[allow(dead_code)]
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