
#![cfg_attr(feature = "strict", deny(warnings))]

mod context;
mod rpc;
mod rpcwire;
mod write_counter;
mod xdr;

mod mount;
mod mount_handlers;

mod portmap;
mod portmap_handlers;

pub mod nfs;
mod nfs_handlers;

pub mod data_store;
pub mod redis_data_store;

pub mod nfs_module {
    use config::{Config, ConfigError, File};
    
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use subxt::{PolkadotConfig,utils::AccountId32,OnlineClient};
    
    use subxt_signer::sr25519::Keypair;
    use subxt_signer::sr25519::dev;
    use tokio::runtime::Runtime;
    use subxt::backend::legacy::LegacyRpcMethods;
    use subxt::backend::rpc::RpcClient;
    

    #[subxt::subxt(runtime_metadata_path = "metadata.scale")]
    pub mod pallet_template {}

    #[allow(dead_code)]
    pub struct NFSModule {
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

    impl NFSModule {
        pub async fn new() -> Result<NFSModule, ConfigError> {
            let mut settings = Config::default();
            settings.merge(File::with_name("config/settings.toml"))?;

            let enable_blockchain: bool = settings.get("enable_blockchain")?;

            let ws_url: String = settings.get("substrate.ws_url")?;

            
            // Create the RPC client
            let rpc_client = RpcClient::from_url(&ws_url).await.expect("Failed to create RPC client");

            // Use this to construct our RPC methods
            let rpc = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client.clone());

            // Create the API client
            let api = OnlineClient::<PolkadotConfig>::from_rpc_client(rpc_client).await.expect("Failed to create BlockChain Connection");
                
            // let api = OnlineClient::<MyConfig>::from_url(ws_url).await.expect("Failed to create BlockChain Connection");

            println!("Connection with BlockChain Node established.");


            let account_id: AccountId32 = dev::alice().public_key().into();
            let signer = dev::alice();

            let rpc_clone = rpc.clone();

            let api_clone = api.clone();
            let signer_clone = signer.clone();
            let account_id_clone = account_id.clone();

            let (tx_sender, tx_receiver) = mpsc::channel();

            // Spawn the event sending thread
            thread::spawn(move || {
                let rt = Runtime::new().unwrap();
                rt.block_on(async move {
                    //NFSModule::event_handler(tx_receiver, api_clone, signer_clone, account_id_clone).await;
                    NFSModule::event_handler(tx_receiver, api_clone, rpc_clone ,signer_clone, account_id_clone).await;
                });
            });

            Ok(NFSModule {
                api,
                account_id,
                signer,
                tx_sender,
                rpc,
                enable_blockchain,
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
                match NFSModule::send_event(&api, &rpc, &signer, &account_id, &event).await {
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
                    .map(|e| {
                        println!("Disassembled call submitted, waiting for transaction to be finalized...");
                        e
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
                .map(|e| {
                    println!("Reassembled call submitted, waiting for transaction to be finalized...");
                    e
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
}

pub mod channel_buffer {

    use tokio::sync::Mutex;
    use tokio::time::{Duration, Instant};
    use bytes::{BytesMut, Bytes};
    use std::sync::Arc;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    
    
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
    
    pub struct ActiveWrite {
        pub channel: Arc<ChannelBuffer>,
        pub last_activity: Instant,
    }
    
    }
    

#[cfg(not(target_os = "windows"))]
pub mod fs_util;

pub mod tcp;
pub mod vfs;



