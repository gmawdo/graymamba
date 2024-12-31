fn main() {
    // Check if exactly one audit feature is enabled
    let merkle_enabled = std::env::var("CARGO_FEATURE_MERKLE_AUDIT").is_ok();
    let az_enabled = std::env::var("CARGO_FEATURE_AZ_AUDIT").is_ok();

    match (merkle_enabled, az_enabled) {
        (false, false) => panic!("Either 'merkle_audit' or 'az_audit' feature must be enabled"),
        (true, true) => panic!("Only one audit feature can be enabled at a time"),
        _ => {} // Exactly one feature is enabled, which is what we want
    }
}