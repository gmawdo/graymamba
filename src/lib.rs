
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

pub use redis_pool::RedisClusterPool;
pub mod data_store;
pub mod redis_data_store;

pub mod redis_pool {
    use r2d2_redis_cluster::{r2d2, RedisClusterConnectionManager};
    use r2d2_redis_cluster::r2d2::Pool;
    use config::{Config, File as ConfigFile, ConfigError}; 
    
   
    
    pub struct RedisClusterPool {
        pub pool: Pool<RedisClusterConnectionManager>,
       
    }
    
    impl RedisClusterPool {
        pub fn new(redis_urls: Vec<&str>) -> RedisClusterPool {
            let manager = RedisClusterConnectionManager::new(redis_urls.clone()).unwrap();
            let pool = r2d2::Pool::builder()
                .max_size(100) // Set the maximum number of connections in the pool
                .build(manager)
                .unwrap();
            
            RedisClusterPool { 
                pool,
                 }
        }

        pub fn get_connection(&self) -> r2d2::PooledConnection<RedisClusterConnectionManager> {
            self.pool.get().unwrap()
        }

        
    
        pub fn from_config_file() -> Result<RedisClusterPool, ConfigError> {
            // Load settings from the configuration file
            let mut settings = Config::default();
            settings
                .merge(ConfigFile::with_name("config/settings.toml"))?;
            
            // Retrieve Redis cluster nodes from the configuration
            let redis_nodes: Vec<String> = settings.get::<Vec<String>>("cluster_nodes")?;
            let redis_nodes: Vec<&str> = redis_nodes.iter().map(|s| s.as_str()).collect();
            //let redis_nodes: Vec<&str> = redis_nodes.iter().map(AsRef::as_ref).collect();
    
            
            Ok(RedisClusterPool::new(redis_nodes))
        }

        
        
    
        
    }


}

pub mod nfs_module {
    use config::{Config, ConfigError, File};
    use subxt::blocks;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use subxt::{
        PolkadotConfig,
        utils::{AccountId32, MultiAddress},
        OnlineClient, blocks::{Block, BlocksClient}, 
    };
    use tokio::sync::Mutex;
    use subxt_signer::sr25519::{dev, Keypair};
    use tokio::runtime::Runtime;
    use subxt::backend::{legacy::LegacyRpcMethods, rpc::RpcClient};
    use subxt::config::DefaultExtrinsicParamsBuilder as Params;

    #[subxt::subxt(runtime_metadata_path = "metadata.scale")]
    pub mod pallet_template {}

    type MyConfig = PolkadotConfig;

    // struct DisReAssemblyStorage;

    // impl Address for DisReAssemblyStorage {
    //     type Target = FSEvent;
    //     type Keys = Vec<u8>;
    //     type IsFetchable = subxt::utils::Yes;
    //     type IsDefaultable = subxt::utils::Yes;
    //     type IsIterable = subxt::utils::Yes;

    //     fn pallet_name(&self) -> &str {
    //         "PalletName"
    //     }

    //     fn entry_name(&self) -> &str {
    //         "DisReAssembly"
    //     }

    //     fn append_entry_bytes(&self, _metadata: &Metadata, _bytes: &mut Vec<u8>) -> Result<(), subxt::ext::subxt_core::Error> {
    //         // Add any additional bytes needed to dig into maps, if necessary
    //         Ok(())
    //     }
    // }

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
            rpc: &LegacyRpcMethods<PolkadotConfig>,
            signer: &Keypair,
            account_id: &AccountId32,
            event: &Event,
        ) -> Result<(), Box<dyn std::error::Error>> {
            println!("Preparing to send event...");
        
            // Data to be used in calls (convert event data to Vec<u8>)
            let event_type: Vec<u8> = event.event_type.clone().into_bytes();
            let creation_time: Vec<u8> = event.creation_time.clone().into_bytes();
            let file_path: Vec<u8> = event.file_path.clone().into_bytes();
            let event_key: Vec<u8> = event.event_key.clone().into_bytes();
        
            // // Log the data for debugging
            // println!("Event Type: {:?}", event_type);
            // println!("Creation Time: {:?}", creation_time);
            // println!("File Path: {:?}", file_path);
            // println!("Event Key: {:?}", event_key);

          

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

#[cfg(not(target_os = "windows"))]
pub mod fs_util;

pub mod tcp;
pub mod vfs;



