use crate::irrefutable_audit::{IrrefutableAudit, AuditEvent, AuditError};
use async_trait::async_trait;
use std::error::Error;

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use config::{Config, File};
use subxt::backend::legacy::LegacyRpcMethods;
use subxt::backend::rpc::RpcClient;
//use subxt::utils::AccountId32;
use subxt::OnlineClient;
use subxt::PolkadotConfig;
use subxt_signer::sr25519::dev;
use subxt_signer::sr25519::Keypair;
use tokio::runtime::Runtime;

#[subxt::subxt(runtime_metadata_path = "metadata.scale")]
pub mod pallet_template {}

pub struct BlockchainAudit {
    api: OnlineClient<PolkadotConfig>,
    //account_id: AccountId32,
    signer: Keypair,
    tx_sender: Sender<AuditEvent>,
    //rpc: LegacyRpcMethods<PolkadotConfig>,
}


#[derive(Debug)]
pub enum BlockchainError {
    ConnectionFailed(String),
    ConfigError(String),
}

impl std::fmt::Display for BlockchainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockchainError::ConnectionFailed(msg) => write!(f, "Blockchain Connection Error: {}", msg),
            BlockchainError::ConfigError(msg) => write!(f, "Blockchain Config Error: {}", msg),
        }
    }
}

impl std::error::Error for BlockchainError {}

#[async_trait]
impl IrrefutableAudit for BlockchainAudit {
    async fn new() -> Result<Self, Box<dyn Error>> {
        let mut settings = Config::default();
        settings.merge(File::with_name("config/settings.toml"))
            .map_err(|e| BlockchainError::ConfigError(e.to_string()))?;

        let ws_url: String = settings.get("substrate.ws_url")
            .map_err(|e| BlockchainError::ConfigError(e.to_string()))?;
        
        // Attempt to create RPC client
        let rpc_client = RpcClient::from_url(&ws_url).await
            .map_err(|e| BlockchainError::ConnectionFailed(
                format!("Failed to connect to blockchain at {}: {}. Please ensure the node is running.", ws_url, e)
            ))?;

        // Create the API client
        let api = OnlineClient::<PolkadotConfig>::from_rpc_client(rpc_client.clone()).await
            .map_err(|e| BlockchainError::ConnectionFailed(
                format!("Failed to establish blockchain connection: {}", e)
            ))?;

        let _rpc = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client);
        println!("✅ Connection with BlockChain Node established.");

        //let account_id: AccountId32 = dev::alice().public_key().into();
        let signer = dev::alice();
        
        let (tx_sender, rx) = mpsc::channel();
        
        let audit = BlockchainAudit {
            api,
            signer,
            tx_sender,
        };
        
        // Spawn the event handler
        Self::spawn_event_handler(rx)?;
        
        Ok(audit)
    }

    fn get_sender(&self) -> &Sender<AuditEvent> {
        &self.tx_sender
    }

    fn spawn_event_handler(receiver: Receiver<AuditEvent>) -> Result<(), Box<dyn Error>> {
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async move {
                while let Ok(event) = receiver.recv() {
                    // Just process the event - no new connection needed
                    println!("Processing event: {:?}", event);
                    // TODO: Add actual event processing logic here
                }
            });
        });
        Ok(())
    }

    async fn process_event(&self, event: AuditEvent) -> Result<(), Box<dyn Error>> {
        println!("Processing event: {:?}", event);

        let event_type_bytes = event.event_type.clone().into_bytes();
        let creation_time = event.creation_time.into_bytes();
        let file_path = event.file_path.into_bytes();
        let event_key = event.event_key.into_bytes();

        match event.event_type.as_str() {
            "disassembled" => {
                let tx = pallet_template::tx()
                    .template_module()
                    .disassembled(event_type_bytes, creation_time, file_path, event_key);
                
                self.api.tx()
                    .sign_and_submit_default(&tx, &self.signer)
                    .await
                    .map_err(|e| Box::new(AuditError::TransactionError(e.to_string())))?;
                
                println!("Disassembled event processed.");
            }
            "reassembled" => {
                let tx = pallet_template::tx()
                    .template_module()
                    .reassembled(event_type_bytes, creation_time, file_path, event_key);
                
                self.api.tx()
                    .sign_and_submit_default(&tx, &self.signer)
                    .await
                    .map_err(|e| Box::new(AuditError::TransactionError(e.to_string())))?;
                
                println!("Reassembled event processed.");
            }
            _ => return Err(Box::new(AuditError::EventProcessingError(
                "Unknown event type".to_string()
            ))),
        }
        Ok(())
    }

    fn shutdown(&self) -> Result<(), Box<dyn Error>> {
        // Drop sender to signal handler thread to stop
        // Additional cleanup if needed
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Add tests here
}