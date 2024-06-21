// src/app_state.rs

use rdkafka::producer::FutureProducer;
use rdkafka::config::ClientConfig;
use std::sync::Arc;

use once_cell::sync::Lazy;
use std::sync::Mutex;
use config::{Config, File};

pub static APP_STATE: Lazy<Mutex<AppState>> = Lazy::new(|| {
    let kafka_brokers = load_kafka_brokers_from_config().expect("Failed to load Kafka brokers from config");
    Mutex::new(AppState::new(&kafka_brokers))
});
pub struct AppState {
    pub producer: Arc<FutureProducer>,
}

impl AppState {
    pub fn new(kafka_brokers: &str) -> AppState {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", kafka_brokers)
            .create()
            .expect("Producer creation error");

        AppState {
            producer: Arc::new(producer),
        }
    }
}

// src/app_state.rs

// Other parts of the file remain unchanged

fn load_kafka_brokers_from_config() -> Result<String, config::ConfigError> {
    let mut settings = Config::default();
    
    let file_result = settings.merge(File::with_name("config/settings.toml"));
    
    match file_result {
        Ok(_) => {
            settings.merge(config::Environment::with_prefix("APP"))?;
        },
        Err(e) => {
            if let config::ConfigError::NotFound(_) = e {
                println!("Warning: config/settings.toml file not found. Using default configuration.");
                settings.set("kafka_brokers", "localhost:9092")?;
            } else {
                return Err(e);
            }
        }
    }
    
    settings.get::<String>("kafka_brokers")
}