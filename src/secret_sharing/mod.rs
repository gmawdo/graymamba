use shamir_secret_sharing::ShamirSecretSharing;
use config::{Config, File};
use serde::Deserialize;
use num_bigint::{BigInt, Sign};
use std::str::FromStr;
use std::collections::HashMap;
use rayon::prelude::*;
use rayon::ThreadPool;
//use base64::{engine::general_purpose, Engine as _}; 
//use flate2::write::{ZlibEncoder, ZlibDecoder};
//use flate2::Compression;
//use std::io::Write;
use anyhow::{Result, Error};
//use tokio; // For async runtime support

// Custom deserialization function for BigInt
fn deserialize_bigint<'de, D>(deserializer: D) -> Result<BigInt, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    BigInt::from_str(&s).map_err(serde::de::Error::custom)
}

#[derive(Deserialize)]
pub struct Settings {
    pub chunk_size: usize,
    pub threshold: usize,
    pub share_amount: usize,
    pub thread_number: usize,
    #[serde(deserialize_with = "deserialize_bigint")]
    pub prime: BigInt,
}

pub struct SecretSharingService {
    settings: Settings,
    sss: ShamirSecretSharing,
    pool: ThreadPool,
}

impl SecretSharingService {
    // Constructor function
    pub fn new() -> Result<Self, Error> {
        let mut config = Config::default();
        config.merge(File::with_name("config/settings.toml"))?;
        let settings: Settings = config.try_into()?;
        
        let sss = ShamirSecretSharing {
            threshold: settings.threshold,
            share_amount: settings.share_amount,
            prime: settings.prime.clone(),
        };

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(settings.thread_number)
            .build()?;

        Ok(Self { settings, sss, pool })
    }

    // Synchronous wrapper for dis_assembly
    /*
    fn dis_assembly_sync(&self, secret_data_value: &str) -> Result<String, anyhow::Error> {
        tokio::runtime::Runtime::new().unwrap().block_on(self.dis_assembly(secret_data_value))
    }

    // Synchronous wrapper for re_assembly
    fn re_assembly_sync(&self, shares_json: &str) -> Result<String, anyhow::Error>{
        tokio::runtime::Runtime::new().unwrap().block_on(self.re_assembly(shares_json))
    }*/

    pub async fn dis_assembly(&self, secret_data_value: &str) -> Result<String, anyhow::Error> {
                
        let secret = secret_data_value.as_bytes();
        let chunks = secret.chunks(self.settings.chunk_size).map(|chunk| chunk.to_vec()).collect::<Vec<_>>();
      
        
        let all_chunk_shares: Vec<HashMap<String, Vec<String>>> = self.pool.install(|| {
            chunks.par_iter().map(|chunk| {
                let secret_bigint = BigInt::from_bytes_be(Sign::Plus, chunk);
                let shares = self.sss.split(secret_bigint);
    
                let mut chunk_map = HashMap::new();
                chunk_map.insert(
                    "shares".to_string(),
                    shares
                        .into_iter()
                        .take(self.settings.share_amount)
                        .map(|(_index, share)| share.to_str_radix(10))
                        .collect(),
                );
                chunk_map
            }).collect()
        });

        // Convert the collection to a JSON array
        let json_value = serde_json::to_vec(&all_chunk_shares)?;

        /* this commented becuase i wanted to see the shares as an array in redis as the shares are supposed to be in different locations
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&json_value)?;
        let compressed_data = encoder.finish()?;                
        Ok(general_purpose::STANDARD.encode(&compressed_data))*/

        Ok(String::from_utf8(json_value)?)
    }

    pub async fn re_assembly(&self, stored_value: &str) -> Result<String, anyhow::Error> {

        /*let compressed_data = general_purpose::STANDARD.decode(stored_value)?;
        let mut decoder = ZlibDecoder::new(Vec::new());
        decoder.write_all(&compressed_data)?;
        let json_data = decoder.finish()?;*/

        //let json_data = general_purpose::STANDARD.decode(stored_value)?;
        //let shares: Vec<HashMap<String, Vec<String>>> = serde_json::from_slice(&json_data)?;

        let shares: Vec<HashMap<String, Vec<String>>> = serde_json::from_str(stored_value)?;
    
        let recovered_chunks: Vec<Vec<u8>> = self.pool.install(|| {
            shares.par_iter().map(|chunk_map| {
                if let Some(chunk_shares) = chunk_map.get("shares") {
                    if chunk_shares.len() < self.settings.threshold {
                        return Vec::new(); // We expect at least threshold number of shares
                    }
                    let indices_and_shares: Vec<(usize, BigInt)> = chunk_shares
                        .iter()
                        .enumerate()
                        .map(|(index, share)| {
                            let share_bigint = BigInt::parse_bytes(share.as_bytes(), 10).expect("Invalid share format");
                            (index + 1, share_bigint) // Indices are assumed to start from 1
                        })
                        .collect();
    
                    //let recovered_secret = self.sss.recover(&indices_and_shares[0..self.sss.threshold as usize]);
                    let recovered_secret = self.sss.recover(&indices_and_shares[0..self.settings.threshold]);
                    recovered_secret.to_bytes_be().1
                } else {
                    Vec::new()
                }
            }).collect()
        });
    
        
        let recovered_chunks_flattened: Vec<u8> = recovered_chunks.into_iter().flatten().collect();
        Ok(String::from_utf8_lossy(&recovered_chunks_flattened).into_owned())
    }

    pub async fn disassemble(&self, secret: &str) -> Result<String, String> {
        match self.dis_assembly(secret).await {
            Ok(result) => Ok(result),
            Err(e) => Err(e.to_string())
        }
    }

    pub async fn reassemble(&self, shares: &str) -> Result<String, String> {
        match self.re_assembly(shares).await {
            Ok(result) => Ok(result),
            Err(e) => Err(e.to_string())
        }
    }
}