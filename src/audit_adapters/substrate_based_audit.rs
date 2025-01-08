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
//use subxt::ext::codec::Decode;

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
        println!("🔑 Using account Alice: {}", hex::encode(signer.public_key().0));
        
        let (tx_sender, rx) = tokio_mpsc::channel(100);
        
        let audit = SubstrateBasedAudit {
            api,
            signer,
            tx_sender,
        };
        
        // Verify metadata
        audit.verify_metadata().await?;
        
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
        let event_type_bytes = event.event_type.as_bytes().to_vec();
        let creation_time = event.creation_time.to_string().as_bytes().to_vec();
        let file_path = event.file_path.as_bytes().to_vec();
        let event_key = event.event_key.as_bytes().to_vec();

        // Validate input lengths before sending
        self.validate_input_lengths(
            &event_type_bytes,
            &creation_time,
            &file_path,
            &event_key,
        )?;

        debug!("📤 Sending event to blockchain:");
        debug!("   Type: {} ({} bytes)", event.event_type, event_type_bytes.len());
        debug!("   Time: {} ({} bytes)", event.creation_time, creation_time.len());
        debug!("   Path: {} ({} bytes)", event.file_path, file_path.len());
        debug!("   Key: {} ({} bytes)", event.event_key, event_key.len());

        match event.event_type.as_str() {
            "disassembled" => {
                let tx = pallet_template::tx()
                    .template_module()
                    .disassembled(event_type_bytes, creation_time, file_path, event_key);
                
                let tx_hash = self.api.tx()
                    .sign_and_submit_default(&tx, &self.signer)
                    .await
                    .map_err(|e| Box::new(AuditError::TransactionError(e.to_string())))?;
                
                debug!("🔗 Transaction submitted to blockchain");
                debug!("📝 Transaction hash: {}", tx_hash);
                debug!("👤 Sender: {}", hex::encode(self.signer.public_key().0));

                // Subscribe to events using the events subscription
                let mut sub = self.api.blocks().subscribe_finalized().await?;
                
                while let Some(block) = sub.next().await {
                    let block = block?;
                    debug!("🔍 Block #{}", block.header().number);
                    
                    // Get events for this block
                    if let Ok(events) = block.events().await {
                        for event in events.iter() {
                            if let Ok(event) = event {
                                if event.pallet_name() == "TemplateModule" {
                                    debug!("   ✨ Found pallet event!");
                                    debug!("   • Name: {}", event.variant_name());
                                    debug!("   • Phase: {:?}", event.phase());
                                    
                                    return Ok(());  // Exit after finding our event
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
                
                debug!("🔗 Transaction submitted to blockchain");
                debug!("📝 Transaction hash: {}", tx_hash);
                debug!("👤 Sender: {}", hex::encode(self.signer.public_key().0));

                // Subscribe to events using the events subscription
                let mut sub = self.api.blocks().subscribe_finalized().await?;
                
                while let Some(block) = sub.next().await {
                    let block = block?;
                    debug!("🔍 Block #{}", block.header().number);
                    
                    // Get events for this block
                    if let Ok(events) = block.events().await {
                        for event in events.iter() {
                            if let Ok(event) = event {
                                if event.pallet_name() == "TemplateModule" {
                                    debug!("   ✨ Found pallet event!");
                                    debug!("   • Name: {}", event.variant_name());
                                    debug!("   • Phase: {:?}", event.phase());
                                    
                                    if let Ok(fields) = event.field_values() {
                                        let fields_str = format!("{:?}", fields);

                                        // The structure is Named([...])
                                        if let Some(content) = fields_str.strip_prefix("Named([") {
                                            if let Some(inner) = content.strip_suffix("])") {
                                                // Split on "), (" to get each field
                                                let fields: Vec<&str> = inner.split("), (").collect();
                                                
                                                for field in fields {
                                                    // Extract field name
                                                    if let Some(_name_end) = field.find("\", ") {
                                                        let name = field
                                                            .trim_start_matches('(')
                                                            .trim_start_matches('"')
                                                            .split('"')
                                                            .next()
                                                            .unwrap_or("");
                                                        debug!("   • Field: {}", name);
                                                        
                                                        // If this is the event field, parse its inner structure
                                                        if name == "event" {
                                                            if let Some(event_start) = field.find("Named([") {
                                                                let _event_content = &field[event_start..];
                                                                //debug!("   • Event content: {}", event_content);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    return Ok(());  // Exit after finding our event
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
        let _ = tokio::time::sleep(tokio::time::Duration::from_secs(1));
        
        println!("Substrate/AZ audit system shutdown complete");
        Ok(())
    }
}

impl SubstrateBasedAudit {
    fn validate_input_lengths(
        &self,
        event_type: &[u8],
        creation_time: &[u8],
        file_path: &[u8],
        event_key: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        if event_type.len() > 64 {
            return Err("Event type exceeds 64 bytes".into());
        }
        if creation_time.len() > 64 {
            return Err("Creation time exceeds 64 bytes".into());
        }
        if file_path.len() > 256 {
            return Err("File path exceeds 256 bytes".into());
        }
        if event_key.len() > 128 {
            return Err("Event key exceeds 128 bytes".into());
        }
        Ok(())
    }

    async fn verify_metadata(&self) -> Result<(), Box<dyn Error>> {
        debug!("\n🔍 Verifying Metadata:");
        
        // Get metadata from the API
        let metadata = self.api.metadata();
        
        // Look for our pallet
        if let Some(pallet) = metadata.pallet_by_name("TemplateModule") {
            debug!("✅ Found pallet: {}", pallet.name());
            
            // Check calls
            debug!("\nCalls:");
            for call in pallet.call_variants().unwrap_or_default() {
                debug!("   • {}", call.name);
            }
            
            // Check events
            debug!("\nEvents:");
            for event in pallet.event_variants().unwrap_or_default() {
                debug!("   • {}", event.name);
            }
            
            Ok(())
        } else {
            Err("Pallet 'TemplateModule' not found in metadata!".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Add tests here
}