
use r2d2_redis_cluster::{r2d2, RedisClusterConnectionManager};
use r2d2_redis_cluster::r2d2::Pool;
use config::{Config, File as ConfigFile, ConfigError}; 


pub struct RedisClusterPool {
    pub pool: Pool<RedisClusterConnectionManager>,
}

impl RedisClusterPool {
    pub fn new(redis_urls: Vec<&str>) -> RedisClusterPool {
        let manager = RedisClusterConnectionManager::new(redis_urls).unwrap();
        let pool = r2d2::Pool::builder()
            .max_size(25) // Set the maximum number of connections in the pool
            .build(manager)
            .unwrap();
        
        RedisClusterPool { pool }
    }

    pub fn get_connection1(&self) -> Result<RedisClusterConnection, r2d2::Error> {
        let pooled_connection = self.pool.get()?;
        let connection = pooled_connection.deref_mut();  // Assuming deref_mut() gets us to the underlying connection type
        // Need to safely check and cast to RedisClusterConnection, this part is pseudocode:
        if let Some(redis_conn) = connection.as_any().downcast_ref::<RedisClusterConnection>() {
            Ok(redis_conn.clone())
        } else {
            Err(r2d2::Error::Custom("Failed to downcast to RedisClusterConnection".into()))
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
        let redis_nodes: Vec<String> = settings
            .get::<Vec<String>>("cluster_nodes")?;
        let redis_nodes: Vec<&str> = redis_nodes.iter().map(|s| s.as_str()).collect();
        //let redis_nodes: Vec<&str> = redis_nodes.iter().map(AsRef::as_ref).collect();

        
        Ok(RedisClusterPool::new(redis_nodes))
    }
    

    
}

