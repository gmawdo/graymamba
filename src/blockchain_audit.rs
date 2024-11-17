use subxt::backend::legacy::LegacyRpcMethods;
use subxt::OnlineClient;
use config::{Config, File};

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use subxt::{PolkadotConfig,utils::AccountId32};

use subxt_signer::sr25519::Keypair;
use subxt_signer::sr25519::dev;
use tokio::runtime::Runtime;
use subxt::backend::rpc::RpcClient;

#[subxt::subxt(runtime_metadata_path = "metadata.scale")]
pub mod pallet_template {}

#[allow(dead_code)]
pub struct BlockchainAudit {
    api: OnlineClient<PolkadotConfig>,
    account_id: AccountId32,
    signer: Keypair,
    tx_sender: Sender<Event>,

    rpc: LegacyRpcMethods<PolkadotConfig>,
    enable_blockchain: bool
}

pub struct Event {
    creation_time: String,
    event_type: String,
    file_path: String,
    event_key: String,
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

impl BlockchainAudit {
    pub async fn new() -> Result<BlockchainAudit, BlockchainError> {
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

        let rpc = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client);
        println!("✅ Connection with BlockChain Node established.");

        let account_id: AccountId32 = dev::alice().public_key().into();
        let signer = dev::alice();
        let (tx_sender, tx_receiver) = mpsc::channel();

        let rpc_clone = rpc.clone();
        let api_clone = api.clone();
        let signer_clone = signer.clone();
        let account_id_clone = account_id.clone();

        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async move {
                BlockchainAudit::event_handler(
                    tx_receiver, 
                    api_clone,
                    rpc_clone,
                    signer_clone, 
                    account_id_clone
                ).await;
            });
        });

        Ok(BlockchainAudit {
            api,
            account_id,
            signer,
            tx_sender,
            rpc,
            enable_blockchain: true,
        })
    }

    async fn event_handler(
        rx: Receiver<Event>,
        api: OnlineClient<PolkadotConfig>,
        rpc: LegacyRpcMethods<PolkadotConfig>,
        signer: Keypair,
        account_id: AccountId32,
    ) {
        while let Ok(event) = rx.recv() {
            // match NFSModule::send_event(&api, &signer, &account_id, &event).await {
            match BlockchainAudit::send_event(&api, &rpc, &signer, &account_id, &event).await {
                Ok(_) => println!("............................"),
                Err(e) => println!("Failed to send event: {:?}", e),
            }
        }
    }

    async fn send_event(
        api: &OnlineClient<PolkadotConfig>,
        _rpc: &LegacyRpcMethods<PolkadotConfig>,
        signer: &Keypair,
        _account_id: &AccountId32,
        event: &Event,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("Preparing to send event...");
    
        // Data to be used in calls (convert event data to Vec<u8>)
        let event_type: Vec<u8> = event.event_type.clone().into_bytes();
        let creation_time: Vec<u8> = event.creation_time.clone().into_bytes();
        let file_path: Vec<u8> = event.file_path.clone().into_bytes();
        let event_key: Vec<u8> = event.event_key.clone().into_bytes();

        if event.event_type == "disassembled" {
            // Call the disassembled function
            let disassembled_call = pallet_template::tx()
                .template_module()
                .disassembled(event_type.clone() ,creation_time.clone(), file_path.clone(), event_key.clone());
        
                let _disassembled_events = api.clone()
                .tx()
                .sign_and_submit_then_watch_default(&disassembled_call, &signer.clone())
                .await
                .inspect(|_e| {
                    println!("Disassembled call submitted, waiting for transaction to be finalized...");
                })?
                .wait_for_finalized_success()
                .await?;
            println!("Disassembled event processed."); 
        } else if  event.event_type == "reassembled" {
            // Call the reassembled function
            let reassembled_call = pallet_template::tx()
                .template_module()
                .reassembled(event_type.clone(), creation_time.clone(), file_path.clone(), event_key.clone());
                let _reassembled_events = api.clone()
                .tx()
                .sign_and_submit_then_watch_default(&reassembled_call, &signer.clone())
                .await
                .inspect(|_e| {
                    println!("Reassembled call submitted, waiting for transaction to be finalized...");
                })?
                .wait_for_finalized_success()
                .await?;
                println!("Reassembled event processed.");   
        } else {
                eprintln!("Unknown event type");       
        }

        Ok(())
    }

    pub fn trigger_event(&self, creation_time: &str, event_type: &str, file_path: &str, event_key: &str) {
        if self.enable_blockchain {    
            let event = Event {
                creation_time: creation_time.to_string(),
                event_type: event_type.to_string(),
                file_path: file_path.to_string(),
                event_key: event_key.to_string(),
            };
            self.tx_sender.send(event).unwrap();
        }
    }
}