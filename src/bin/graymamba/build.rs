fn main() {
    // Only check features when building the graymamba binary
    if std::env::var("CARGO_BIN_NAME").unwrap() == "graymamba" {
        // Check audit features
        let merkle_enabled = std::env::var("CARGO_FEATURE_MERKLE_AUDIT").is_ok();
        let az_enabled = std::env::var("CARGO_FEATURE_AZ_AUDIT").is_ok();

        match (merkle_enabled, az_enabled) {
            (false, false) => panic!("Either 'merkle_audit' or 'az_audit' feature must be enabled for graymamba"),
            (true, true) => panic!("Only one audit feature can be enabled at a time for graymamba"),
            _ => {}
        }

        // Check store features
        let redis_enabled = std::env::var("CARGO_FEATURE_REDIS_STORE").is_ok();
        let rocksdb_enabled = std::env::var("CARGO_FEATURE_ROCKSDB_STORE").is_ok();

        match (redis_enabled, rocksdb_enabled) {
            (false, false) => panic!("Either 'redis_store' or 'rocksdb_store' feature must be enabled for graymamba"),
            (true, true) => panic!("Only one store feature can be enabled at a time for graymamba"),
            _ => {}
        }
    }
}