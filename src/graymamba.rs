use std::sync::Arc;
use tracing::warn;


use graymamba::tcp::{NFSTcp, NFSTcpListener};
use graymamba::sharesbased_fs::SharesFS;
use graymamba::sharesbased_fs::{NAMESPACE_ID, HASH_TAG};

#[cfg(feature = "blockchain_audit")]
use graymamba::blockchain_audit::BlockchainAudit;

extern crate secretsharing;
use config::{Config, File as ConfigFile};

const HOSTPORT: u32 = 2049;

async fn set_namespace_id_and_hashtag() {
    let mut namespace_id = NAMESPACE_ID.write().unwrap();
    *namespace_id = "graymamba".to_string();

    let mut hash_tag = HASH_TAG.write().unwrap();
    hash_tag.clear(); // Clear the previous content
    hash_tag.push_str(&format!("{{{}}}:", namespace_id));
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
    let version = env!("CARGO_PKG_VERSION");
    println!("Application version: {}", version);

    // Print enabled features
    println!("Enabled features:");
    if cfg!(feature = "blockchain_audit") {
        println!(" - blockchain_audit");
    }

    let _settings = load_config();

    set_namespace_id_and_hashtag().await;
    
    use graymamba::redis_data_store::RedisDataStore;
    let data_store = Arc::new(RedisDataStore::new().expect("Failed to create a data store"));

    #[cfg(feature = "blockchain_audit")]
    let blockchain_audit = match BlockchainAudit::new().await {
        Ok(module) => Some(Arc::new(module)),
        Err(e) => {
            eprintln!("❌ Failed to create BlockchainAudit: {}", e);
            None
        }
    };

    #[cfg(not(feature = "blockchain_audit"))]
    let blockchain_audit = None;

    let shares_fs = SharesFS::new(data_store, blockchain_audit);
    let shares_fs_clone = shares_fs.clone();
    tokio::spawn(async move {
        shares_fs_clone.start_monitoring().await;
    });

    warn!("Created new SharesFS with data_store");
    println!("🚀 FS has launched");
    let listener = NFSTcpListener::bind(&format!("0.0.0.0:{HOSTPORT}"), shares_fs)
        .await
        .unwrap();
    listener.handle_forever().await.unwrap();
}

