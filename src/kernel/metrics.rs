use lazy_static::lazy_static;
use prometheus::{IntCounter, IntGauge, Registry, register_int_counter_with_registry, register_int_gauge_with_registry};
use tracing::info;

lazy_static! {
    pub static ref REGISTRY: Registry = {
        info!("Initializing Prometheus registry");
        Registry::new()
    };

    pub static ref ACTIVE_CONNECTIONS: IntGauge = {
        info!("Registering active_connections metric");
        register_int_gauge_with_registry!(
            "nfs_active_connections",
            "Number of active NFS connections",
            REGISTRY
        ).unwrap()
    };

    pub static ref TOTAL_CONNECTIONS: IntCounter = register_int_counter_with_registry!(
        "nfs_total_connections",
        "Total number of NFS connections handled",
        REGISTRY
    ).unwrap();

    pub static ref RPC_REQUESTS_TOTAL: IntCounter = register_int_counter_with_registry!(
        "nfs_rpc_requests_total",
        "Total number of RPC requests received",
        REGISTRY
    ).unwrap();

    pub static ref RPC_ERRORS_TOTAL: IntCounter = register_int_counter_with_registry!(
        "nfs_rpc_errors_total",
        "Total number of RPC errors encountered",
        REGISTRY
    ).unwrap();

    pub static ref FRAGMENTS_PROCESSED: IntCounter = register_int_counter_with_registry!(
        "nfs_fragments_processed",
        "Total number of RPC fragments processed",
        REGISTRY
    ).unwrap();

    pub static ref BYTES_RECEIVED: IntCounter = register_int_counter_with_registry!(
        "nfs_bytes_received_total",
        "Total number of bytes received",
        REGISTRY
    ).unwrap();

    pub static ref BYTES_SENT: IntCounter = register_int_counter_with_registry!(
        "nfs_bytes_sent_total",
        "Total number of bytes sent",
        REGISTRY
    ).unwrap();
}

pub fn init() {
    info!("Ensuring metrics are initialized");
    lazy_static::initialize(&REGISTRY);
    lazy_static::initialize(&ACTIVE_CONNECTIONS);
    lazy_static::initialize(&TOTAL_CONNECTIONS);
    lazy_static::initialize(&RPC_REQUESTS_TOTAL);
    lazy_static::initialize(&RPC_ERRORS_TOTAL);
    lazy_static::initialize(&FRAGMENTS_PROCESSED);
    lazy_static::initialize(&BYTES_RECEIVED);
    lazy_static::initialize(&BYTES_SENT);
    info!("Metrics initialization complete");
}