use crate::audit_adapters::irrefutable_audit::{IrrefutableAudit, AuditEvent, AuditError};
use async_trait::async_trait;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::mpsc::{self as tokio_mpsc};

use config::{Config, File};
use subxt::backend::legacy::LegacyRpcMethods;
use subxt::backend::rpc::RpcClient;
use subxt::OnlineClient;
use subxt::PolkadotConfig;
use subxt_signer::sr25519::dev;
use subxt_signer::sr25519::Keypair;

#[subxt::subxt(runtime_metadata_path = "metadata.scale")]
pub mod pallet_template {}

use tracing::debug;

#[derive(Clone)]
pub struct SubstrateBasedAudit {
    api: OnlineClient<PolkadotConfig>,
    signer: Keypair,
    tx_sender: tokio_mpsc::Sender<AuditEvent>,
}


#[derive(Debug)]
pub enum SubstrateError {
    ConnectionFailed(String),
    ConfigError(String),
}

impl std::fmt::Display for SubstrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubstrateError::ConnectionFailed(msg) => write!(f, "Substrate Connection Error: {}", msg),
            SubstrateError::ConfigError(msg) => write!(f, "Substrate Config Error: {}", msg),
        }
    }
}

impl std::error::Error for SubstrateError {}

#[async_trait]
impl IrrefutableAudit for SubstrateBasedAudit {
    async fn new() -> Result<Self, Box<dyn Error>> {
        let mut settings = Config::default();
        settings.merge(File::with_name("config/settings.toml"))
            .map_err(|e| SubstrateError::ConfigError(e.to_string()))?;

        let ws_url: String = settings.get("substrate.ws_url")
            .map_err(|e| SubstrateError::ConfigError(e.to_string()))?;
        
        // Attempt to create RPC client
        let rpc_client = RpcClient::from_url(&ws_url).await
            .map_err(|e| SubstrateError::ConnectionFailed(
                format!("Failed to connect to Substrate at {}: {}. Please ensure the node is running.", ws_url, e)
            ))?;

        // Create the API client
        let api = OnlineClient::<PolkadotConfig>::from_rpc_client(rpc_client.clone()).await
            .map_err(|e| SubstrateError::ConnectionFailed(
                format!("Failed to establish substrate connection: {}", e)
            ))?;

        let _rpc = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client);
        println!("✅ Connection with Substrate/AZ Node established.");

        //let account_id: AccountId32 = dev::alice().public_key().into();
        let signer = dev::alice();
        println!("🔑 Using account: {}", hex::encode(signer.public_key().0));
        
        let (tx_sender, rx) = tokio_mpsc::channel(100);
        
        let audit = SubstrateBasedAudit {
            api,
            signer,
            tx_sender,
        };
        
        // Spawn the event handler
        Self::spawn_event_handler(Arc::new(audit.clone()), rx)?;
        
        Ok(audit)
    }

    fn get_sender(&self) -> &tokio_mpsc::Sender<AuditEvent> {
        &self.tx_sender
    }

    fn spawn_event_handler(
        audit: Arc<dyn IrrefutableAudit>,
        mut receiver: tokio_mpsc::Receiver<AuditEvent>
    ) -> Result<(), Box<dyn Error>> {
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                debug!("Processing event: {:?}", event);
                
                if let Err(e) = audit.process_event(event).await {
                    eprintln!("Error processing event: {}", e);
                }
            }
        });
        Ok(())
    }

    async fn process_event(&self, event: AuditEvent) -> Result<(), Box<dyn Error>> {
        debug!("Processing event: {:?}", event);

        let event_type_bytes = event.event_type.clone().into_bytes();
        let creation_time = event.creation_time.into_bytes();
        let file_path = event.file_path.into_bytes();
        let event_key = event.event_key.into_bytes();

        match event.event_type.as_str() {
            "disassembled" => {
                let tx = pallet_template::tx()
                    .template_module()
                    .disassembled(event_type_bytes, creation_time, file_path, event_key);
                
                let tx_hash = self.api.tx()
                    .sign_and_submit_default(&tx, &self.signer)
                    .await
                    .map_err(|e| Box::new(AuditError::TransactionError(e.to_string())))?;
                
                println!("🔗 Transaction submitted to blockchain");
                println!("📝 Transaction hash: {}", tx_hash);
                println!("👤 Sender: {}", hex::encode(self.signer.public_key().0));

                // Subscribe only to blocks containing our transaction
                let mut blocks = self.api.blocks().subscribe_finalized().await
                    .map_err(|e| Box::new(AuditError::TransactionError(e.to_string())))?;

                let mut found = false;
                while let Some(block) = blocks.next().await {
                    if found {
                        break;
                    }
                    
                    let block = block.map_err(|e| Box::new(AuditError::TransactionError(e.to_string())))?;
                    println!("Checking block: {} (hash: {})", block.number(), block.hash());
                    
                    // Check if our transaction is in this block
                    if let Some(events) = block.events().await.ok() {
                        for event in events.iter() {
                            if let Ok(event) = event {
                                match event.phase() {
                                    subxt::events::Phase::ApplyExtrinsic(idx) => {
                                        println!("Found extrinsic {} in block {}", idx, block.hash());
                                        println!("Event name: {:?}", event.variant_name());
                                        println!("Event fields: {:?}", event.field_values());
                                        found = true;
                                        println!("✅ Transaction included in block: {}", block.hash());
                                        println!("✅ Block number: {}", block.number());
                                        break;
                                    },
                                    _ => continue,
                                }
                            }
                        }
                    }
                }
                
                debug!("Disassembled event processed.");
            }
            "reassembled" => {
                let tx = pallet_template::tx()
                    .template_module()
                    .reassembled(event_type_bytes, creation_time, file_path, event_key);
                
                let tx_hash = self.api.tx()
                    .sign_and_submit_default(&tx, &self.signer)
                    .await
                    .map_err(|e| Box::new(AuditError::TransactionError(e.to_string())))?;
                
                println!("🔗 Transaction submitted to blockchain");
                println!("📝 Transaction hash: {}", tx_hash);
                println!("👤 Sender: {}", hex::encode(self.signer.public_key().0));

                // Subscribe only to blocks containing our transaction
                let mut blocks = self.api.blocks().subscribe_finalized().await
                    .map_err(|e| Box::new(AuditError::TransactionError(e.to_string())))?;

                let mut found = false;
                while let Some(block) = blocks.next().await {
                    if found {
                        break;
                    }
                    
                    let block = block.map_err(|e| Box::new(AuditError::TransactionError(e.to_string())))?;
                    println!("Checking block: {} (hash: {})", block.number(), block.hash());
                    
                    // Check if our transaction is in this block
                    if let Some(events) = block.events().await.ok() {
                        for event in events.iter() {
                            if let Ok(event) = event {
                                match event.phase() {
                                    subxt::events::Phase::ApplyExtrinsic(idx) => {
                                        println!("Found extrinsic {} in block {}", idx, block.hash());
                                        println!("Event name: {:?}", event.variant_name());
                                        println!("Event fields: {:?}", event.field_values());
                                        found = true;
                                        println!("✅ Transaction included in block: {}", block.hash());
                                        println!("✅ Block number: {}", block.number());
                                        break;
                                    },
                                    _ => continue,
                                }
                            }
                        }
                    }
                }
                
                debug!("Reassembled event processed.");
            }
            _ => return Err(Box::new(AuditError::EventProcessingError(
                "Unknown event type".to_string()
            ))),
        }
        Ok(())
    }


    fn shutdown(&self) -> Result<(), Box<dyn Error>> {
        // Close the channel by dropping the sender
        let _ = self.tx_sender.send(AuditEvent {
            event_type: "shutdown".to_string(),
            creation_time: "".to_string(),
            file_path: "".to_string(),
            event_key: "".to_string(),
        });
        
        // Give a short grace period for pending events to complete
        tokio::time::sleep(tokio::time::Duration::from_secs(1));
        
        println!("Substrate/AZ audit system shutdown complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Add tests here
}