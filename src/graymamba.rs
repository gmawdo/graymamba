use std::sync::Arc;


use graymamba::tcp::{NFSTcp, NFSTcpListener};
use graymamba::sharesbased_fs::SharesFS;
use graymamba::sharesbased_fs::{NAMESPACE_ID, HASH_TAG};

#[cfg(feature = "irrefutable_audit")]
use graymamba::audit_adapters::audit_system::AuditSystem;
#[cfg(feature = "irrefutable_audit")]
use graymamba::irrefutable_audit::IrrefutableAudit; 

extern crate secretsharing;
use config::{Config, File as ConfigFile};

use tokio::signal;
use std::io::Write;

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
    if cfg!(feature = "irrefutable_audit") {
        println!(" - irrefutable_audit");
    }

    let _settings = load_config();

    set_namespace_id_and_hashtag().await;
    
    use graymamba::redis_data_store::RedisDataStore;
    let data_store = Arc::new(RedisDataStore::new().expect("Failed to create a data store"));

    use graymamba::rocksdb_data_store::RocksDBDataStore;
    let _data_store2 = Arc::new(RocksDBDataStore::new("theROCKSDB").expect("Failed to create a data store"));

    #[cfg(feature = "irrefutable_audit")]
    let audit_system =    match AuditSystem::new().await {
        Ok(audit) => {
            println!("✅ Irrefutable audit initialisation successful");
            Some(Arc::new(audit) as Arc<dyn IrrefutableAudit>)
        },

        Err(e) => {
            eprintln!("❌ Fatal Error: {}", e);
            std::process::exit(1);
        }
    };

    #[cfg(not(feature = "irrefutable_audit"))]
    let audit_system = None;

    let shares_fs = SharesFS::new(data_store, audit_system.clone());
    let shares_fs_clone = shares_fs.clone();
    tokio::spawn(async move {
        shares_fs_clone.start_monitoring().await;
    });

    println!("🚀 graymamba launched");
    let listener = NFSTcpListener::bind(&format!("0.0.0.0:{HOSTPORT}"), shares_fs)
        .await
        .unwrap();
    // Start the listener in a separate task
    let _listener_handle = tokio::spawn(async move {
        listener.handle_forever().await
    });

    // Wait for ctrl+c
    match signal::ctrl_c().await {
        Ok(()) => {
            println!("Received shutdown signal");
            std::io::stdout().flush().unwrap();  // Ensure output is displayed
        }
        Err(err) => {
            eprintln!("Error handling ctrl-c: {}", err);
            std::io::stderr().flush().unwrap();
        }
    }

    // Perform cleanup
    #[cfg(feature = "irrefutable_audit")]
    if let Some(audit) = audit_system {
        std::io::stdout().flush().unwrap();
        audit.shutdown().unwrap();
    }
}

