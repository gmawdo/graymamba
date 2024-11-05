use std::sync::Arc;
use tracing::warn;


use graymamba::tcp::{NFSTcp, NFSTcpListener};
use graymamba::blockchain_audit::BlockchainAudit;
use graymamba::sharesbased_fs::SharesFS;
use graymamba::sharesbased_fs::{USER_ID, HASH_TAG};

extern crate secretsharing;
use config::{Config, File as ConfigFile};

const HOSTPORT: u32 = 2049;

async fn set_user_id_and_hashtag() {
    let mut user_id = USER_ID.write().unwrap();
    *user_id = "graymamba".to_string();

    let mut hash_tag = HASH_TAG.write().unwrap();
    hash_tag.clear(); // Clear the previous content
    hash_tag.push_str(&format!("{{{}}}:", user_id));
}

// Load settings from the configuration file
fn load_config() -> Config {
    let mut settings = Config::default();
    settings
        .merge(ConfigFile::with_name("config/settings.toml"))
        .expect("Failed to load configuration");

    // Retrieve log level from the configuration
    let log_level = settings
        .get::<String>("logging.level")
        .unwrap_or_else(|_| "warn".to_string());

    // Convert string to Level
    let level = match log_level.to_lowercase().as_str() {
        "error" => tracing::Level::ERROR,
        "warn" => tracing::Level::WARN,
        "info" => tracing::Level::INFO,
        "debug" => tracing::Level::DEBUG,
        "trace" => tracing::Level::TRACE,
        _ => tracing::Level::WARN, // Default to WARN if invalid
    };

    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .init();

    settings
}

#[tokio::main]
async fn main() {
    // Load settings from the configuration file
    let settings = load_config();

    set_user_id_and_hashtag().await;
    // Execute the two lines as a closure
    {
        let user_id = USER_ID.read().unwrap();
        let hash_tag = HASH_TAG.read().unwrap();
        println!("User ID: {}", *user_id);
        println!("Hash Tag: {}", *hash_tag);
    }
    
    use graymamba::redis_data_store::RedisDataStore;
    let data_store = Arc::new(RedisDataStore::new().expect("Failed to create a data store"));

    let blockchain_audit = if settings.get("enable_blockchain").unwrap_or(false) {
        match BlockchainAudit::new().await {
            Ok(module) => Some(Arc::new(module)),
            Err(e) => {
                eprintln!("Failed to create BlockchainAudit: {}", e);
                None
            }
        }
    } else {
        None
    };
    let fs = SharesFS::new(data_store, blockchain_audit);
    warn!("Created new SharesFS with data_store");
    let listener = NFSTcpListener::bind(&format!("0.0.0.0:{HOSTPORT}"), fs)
        .await
        .unwrap();
    listener.handle_forever().await.unwrap();
}

