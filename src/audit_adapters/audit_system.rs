use std::error::Error;
use tokio::sync::mpsc as tokio_mpsc;
use async_trait::async_trait;
use std::sync::Arc;
use crate::irrefutable_audit::{AuditEvent, IrrefutableAudit};

/// Implementation of the IrrefutableAudit trait
pub struct AuditSystem {
    sender: tokio_mpsc::Sender<AuditEvent>,
}

#[async_trait]
impl IrrefutableAudit for AuditSystem {
    async fn new() -> Result<Self, Box<dyn Error>> {
        println!("Initialising audit system");
        let (sender, receiver) = tokio_mpsc::channel(100);
        let audit = Arc::new(AuditSystem { sender });
        AuditSystem::spawn_event_handler(audit.clone(), receiver)?;
        Ok(AuditSystem { sender: audit.get_sender().clone() })
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
                println!("Received event: {:?}", event);
                if let Err(e) = audit.process_event(event).await {
                    eprintln!("Error processing event: {}", e);
                }
            }
        });
        Ok(())
    }

    async fn process_event(&self, event: AuditEvent) -> Result<(), Box<dyn Error>> {
        println!("Processing event: {:?}", event);
        Ok(())
    }

    fn shutdown(&self) -> Result<(), Box<dyn Error>> {
        println!("Shutting down audit system.");
        Ok(())
    }
}