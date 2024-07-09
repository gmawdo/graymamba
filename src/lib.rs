
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

pub mod kafka_producer {
    use std::sync::Arc;
    use rdkafka::producer::{FutureProducer, FutureRecord};
    use rdkafka::ClientConfig;
    use config::{Config, ConfigError, File};
    use std::time::Duration;

    pub struct KafkaProducer {
        producer: Arc<FutureProducer>,
        topic: String,
    }

    impl KafkaProducer {
        pub fn new() -> Result<KafkaProducer, ConfigError> {
            // Load settings from the configuration file
            let mut settings = Config::default();
            settings.merge(File::with_name("config/settings.toml"))?;

            // Retrieve the bootstrap.servers value from the configuration
            let brokers: String = settings.get("bootstrap.servers")?;
            let topic: String = settings.get("kafka.topic")?;

            // Configure Kafka producer
            let producer: FutureProducer = ClientConfig::new()
                .set("bootstrap.servers", &brokers)
                .create()
                .expect("Failed to create Kafka producer");

            Ok(KafkaProducer {
                producer: Arc::new(producer),
                topic,
            })
        }

            pub async fn send_event(&self, creation_time: &str, event_type: &str, file_path: &str, event_key: &str) {
                
                // Format the event data string
                let event_data = format!("{}, {}, {}, {}", creation_time, event_type, file_path, event_key);

                // Send event to Kafka
                let record = FutureRecord::to(&self.topic)
                    .payload(&event_data)
                    .key(event_key);
        
                // Asynchronously send the record to Kafka
                if let Err(e) = self.producer.send(record, Duration::from_secs(5)).await {
                    eprintln!("Error sending event to Kafka: {:?}", e);
                }
            }
        }

}


pub mod nfs_module {
    use config::{Config, ConfigError, File};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use subxt::{
        PolkadotConfig,
        utils::{AccountId32, MultiAddress},
        OnlineClient,
    };
    use tokio::sync::Mutex;
    use subxt_signer::sr25519::{dev, Keypair};
    use tokio::runtime::Runtime;

    #[subxt::subxt(runtime_metadata_path = "metadata.scale")]
    pub mod pallet_template {}

    type MyConfig = PolkadotConfig;

    pub struct NFSModule {
        api: OnlineClient<PolkadotConfig>,
        account_id: AccountId32,
        signer: Keypair,
        tx_sender: Sender<Event>,
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

            let ws_url: String = settings.get("substrate.ws_url")?;

            let api = OnlineClient::<MyConfig>::from_url(ws_url).await.expect("Failed to create BlockChain Connection");

            println!("Connection with BlockChain Node established.");


            let account_id: AccountId32 = dev::alice().public_key().into();
            let signer = dev::alice();

            let api_clone = api.clone();
            let signer_clone = signer.clone();
            let account_id_clone = account_id.clone();

            let (tx_sender, tx_receiver) = mpsc::channel();

            // Spawn the event sending thread
            thread::spawn(move || {
                let rt = Runtime::new().unwrap();
                rt.block_on(async move {
                    //NFSModule::event_handler(tx_receiver, api_clone, signer_clone, account_id_clone).await;
                    NFSModule::event_handler(tx_receiver, api_clone, signer_clone, account_id_clone).await;
                });
            });

            Ok(NFSModule {
                api,
                account_id,
                signer,
                tx_sender,
            })
        }

        async fn event_handler(
            rx: Receiver<Event>,
            api: OnlineClient<PolkadotConfig>,
            signer: Keypair,
            account_id: AccountId32,
        ) {
            while let Ok(event) = rx.recv() {
                // match NFSModule::send_event(&api, &signer, &account_id, &event).await {
                match NFSModule::send_event(&api, &signer, &account_id, &event).await {
                    Ok(_) => println!("............................"),
                    Err(e) => println!("Failed to send event: {:?}", e),
                }
            }
        }

        async fn send_event(
            api: &OnlineClient<PolkadotConfig>,
            signer: &Keypair,
            account_id: &AccountId32,
            event: &Event,
        ) -> Result<(), Box<dyn std::error::Error>> {
            println!("Preparing to send event...");

            // Data to be used in calls (convert event data to Vec<u8>)
            let creation_time: Vec<u8> = event.creation_time.clone().into_bytes();
            let file_path: Vec<u8> = event.file_path.clone().into_bytes();
            let event_key: Vec<u8> = event.event_key.clone().into_bytes();

            // Log the data for debugging
            println!("Creation Time: {:?}", creation_time);
            println!("File Path: {:?}", file_path);
            println!("Event Key: {:?}", event_key);

          

            if event.event_type == "disassembled" {

                // Call the disassembled function
                let disassembled_call = pallet_template::tx()
                    .template_module()
                    .disassembled(creation_time.clone(), file_path.clone(), event_key.clone());
            
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
                .reassembled(creation_time.clone(), file_path.clone(), event_key.clone());
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








#[cfg(not(target_os = "windows"))]
pub mod fs_util;

pub mod tcp;
pub mod vfs;

pub mod app_state;



