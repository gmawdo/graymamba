use std::sync::Arc;
use graymamba::kernel::protocol::tcp::{NFSTcp, NFSTcpListener};
use graymamba::sharesfs::SharesFS;

use graymamba::audit_adapters::irrefutable_audit::IrrefutableAudit;
#[cfg(feature = "merkle_audit")]
use graymamba::audit_adapters::merkle_audit::MerkleBasedAuditSystem;

#[cfg(feature = "az_audit")]
use graymamba::audit_adapters::substrate_audit::SubstrateAuditSystem;

use config::{Config, File as ConfigFile};

use tokio::signal;
use std::io::Write;
use tracing_subscriber::EnvFilter;
use hyper::{Body, Response, Server, Request, Method, StatusCode};
use hyper::service::{make_service_fn, service_fn};
use prometheus::{Encoder, TextEncoder};
use std::convert::Infallible;
use std::net::SocketAddr;

use graymamba::kernel::metrics;
use tracing::{info, error};

const HOSTPORT: u32 = 2049;

async fn metrics_handler(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    info!("Received metrics request: {} {}", req.method(), req.uri().path());
    
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/metrics") => {
            let encoder = TextEncoder::new();
            let mut buffer = vec![];
            let metric_families = metrics::REGISTRY.gather();
            info!("Number of metric families: {}", metric_families.len());
            encoder.encode(&metric_families, &mut buffer).unwrap();
            
            Ok(Response::builder()
                .header("Content-Type", encoder.format_type())
                .body(Body::from(buffer))
                .unwrap())
        }
        (&Method::GET, "/health") => {
            info!("Health check requested");
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Body::from("OK"))
                .unwrap())
        }
        _ => {
            info!("Not found: {} {}", req.method(), req.uri().path());
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not Found"))
                .unwrap())
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging first
    // Load settings but skip logging config since we've already set it up
    let mut settings = Config::default();
    settings
        .merge(ConfigFile::with_name("config/settings.toml"))
        .expect("Failed to load configuration");

    // Retrieve log settings from configuration
    let base_level = settings
        .get::<String>("logging.level")
        .unwrap_or_else(|_| "warn".to_string());

    // Build the filter with both base level and all module directives
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            let mut filter = EnvFilter::new(&base_level);
            if let Ok(filters) = settings.get::<Vec<String>>("logging.module_filter") {
                for module_filter in filters {
                    filter = filter.add_directive(module_filter.parse().unwrap());
                }
            }
            filter
        });
    println!("filter: {:?}", filter);

    // Single initialization with combined settings
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)  // Don't show target module
        .with_thread_ids(false)  // Don't show thread IDs
        .with_thread_names(false)  // Don't show thread names
        .with_file(true)  // Don't show file names
        .with_line_number(true)  // Don't show line numbers
        .with_level(true)  // Do show log levels
        .compact()  // Use compact formatting
        .init();

    let version = env!("CARGO_PKG_VERSION");
    println!("Application version: {}", version);

    // Print enabled features
    println!("Enabled features:");

    SharesFS::set_namespace_id_and_community(settings.get_str("storage.namespace_id").unwrap().as_str(), settings.get_str("storage.community").unwrap().as_str()).await;

    let data_store = {
        #[cfg(feature = "redis_store")]
        {
            use graymamba::backingstore::redis_data_store::RedisDataStore;
            Arc::new(RedisDataStore::new()
                .expect("Failed to create Redis data store"))
        }

        #[cfg(feature = "rocksdb_store")]
        {
            use graymamba::backingstore::rocksdb_data_store::RocksDBDataStore;
            Arc::new(RocksDBDataStore::new(
                settings.get_str("storage.rocksdb_path")
                    .expect("Failed to get rocksdb_path from settings")
                    .as_str()
            ).expect("Failed to create RocksDB data store"))
        }

        #[cfg(not(any(feature = "redis_store", feature = "rocksdb_store")))]
        compile_error!("Either 'redis_store' or 'rocksdb_store' feature must be enabled");
    };
    

    let audit_system: Arc<dyn IrrefutableAudit> = {
        #[cfg(feature = "merkle_audit")]
        {
            match MerkleBasedAuditSystem::new().await {
                Ok(audit) => {
                    println!("✅ Merkle-based audit initialization successful");
                    Arc::new(audit)
                },
                Err(e) => {
                    eprintln!("❌ Fatal Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        
        #[cfg(feature = "az_audit")]
        {
            match SubstrateAuditSystem::new().await {
                Ok(audit) => {
                    println!("✅ Aleph Zero audit initialization successful");
                    Arc::new(audit)
                },
                Err(e) => {
                    eprintln!("❌ Fatal Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        
        #[cfg(not(any(feature = "merkle_audit", feature = "az_audit")))]
        compile_error!("Either 'merkle_audit' or 'az_audit' feature must be enabled");
    };

    let shares_fs = SharesFS::new(data_store, audit_system.clone());
    let shares_fs_clone = shares_fs.clone();
    tokio::spawn(async move {
        shares_fs_clone.start_monitoring().await;
    });

    println!("🚀 graymamba launched");
    
    // Initialize metrics before starting the server
    metrics::init();
    
    // Start metrics server
    let metrics_addr = SocketAddr::from(([0, 0, 0, 0], 9091));
    println!("Starting metrics server on {}", metrics_addr);
    
    let metrics_service = make_service_fn(|_conn| async {
        Ok::<_, Infallible>(service_fn(metrics_handler))
    });

    let metrics_server = Server::bind(&metrics_addr).serve(metrics_service);
    let metrics_handle = tokio::spawn(async move {
        info!("Metrics server is now listening on http://{}", metrics_addr);
        if let Err(e) = metrics_server.await {
            error!("Metrics server error: {}", e);
        }
        info!("Metrics server stopped");
    });

    // Start NFS server
    let listener = NFSTcpListener::bind(&format!("0.0.0.0:{HOSTPORT}"), shares_fs)
        .await
        .unwrap();
    
    let nfs_handle = tokio::spawn(async move {
        listener.handle_forever().await
    });

    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            println!("Received shutdown signal");
            
            // Cleanup
            audit_system.shutdown().unwrap();
            
            // Abort both server tasks
            metrics_handle.abort();
            nfs_handle.abort();
            
            std::io::stdout().flush().unwrap();
        }
        Err(err) => {
            eprintln!("Error handling ctrl-c: {}", err);
            std::io::stderr().flush().unwrap();
        }
    }

    Ok(())
}

