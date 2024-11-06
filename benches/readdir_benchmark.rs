use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use graymamba::sharesbased_fs::SharesFS;
use graymamba::vfs::NFSFileSystem;
use graymamba::test_store::TestDataStore;
use graymamba::nfs::*;
use std::sync::Arc;
use tokio::runtime::Runtime;
use std::collections::HashMap;

use lazy_static::lazy_static;

lazy_static! {
    static ref RUNTIME: Runtime = Runtime::new().unwrap();
}


async fn setup_test_data(fs: &SharesFS, size: usize) -> Result<(), nfsstat3> {
    let root_id = 1u64;
    let (_namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
    
    // Create root directory with metadata
    fs.create_test_entry(0, "/", root_id).await?;
    fs.data_store.hset_multiple(&format!("{}/", hash_tag), &[
        ("type", "2"), ("mode", "0755"), ("nlink", "2"), ("uid", "0"), ("gid", "0"),
        ("size", "4096"), ("fileid", &root_id.to_string()), ("used", "4096"), ("rdev", "0"),
        ("access_time_secs", "0"), ("access_time_nsecs", "0"), ("modification_time_secs", "0"),
        ("modification_time_nsecs", "0"), ("change_time_secs", "0"), ("change_time_nsecs", "0")
    ]).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;

    // Create test directory with metadata
    let test_dir_id = 2u64;
    fs.create_test_entry(root_id, "/test_dir", test_dir_id).await?;
    fs.data_store.hset_multiple(&format!("{}/test_dir", hash_tag), &[
        ("type", "2"), ("mode", "0755"), ("nlink", "2"), ("uid", "0"), ("gid", "0"),
        ("size", "4096"), ("fileid", &test_dir_id.to_string()), ("used", "4096"), ("rdev", "0"),
        ("access_time_secs", "0"), ("access_time_nsecs", "0"), ("modification_time_secs", "0"),
        ("modification_time_nsecs", "0"), ("change_time_secs", "0"), ("change_time_nsecs", "0")
    ]).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;

    // Create test files with metadata
    for i in 0..size {
        let file_id = (i + 3) as u64;
        let path = format!("/test_dir/file_{}", i);
        fs.create_test_entry(test_dir_id, &path, file_id).await?;
        fs.data_store.hset_multiple(&format!("{}{}", hash_tag, path), &[
            ("type", "1"), ("mode", "0644"), ("nlink", "1"), ("uid", "0"), ("gid", "0"),
            ("size", "0"), ("fileid", &file_id.to_string()), ("used", "0"), ("rdev", "0"),
            ("access_time_secs", "0"), ("access_time_nsecs", "0"), ("modification_time_secs", "0"),
            ("modification_time_nsecs", "0"), ("change_time_secs", "0"), ("change_time_nsecs", "0")
        ]).await.map_err(|_| nfsstat3::NFS3ERR_IO)?;
    }
    Ok(())
}

#[allow(dead_code)]
async fn print_fs_structure(fs: &SharesFS) -> Result<(), nfsstat3> {
    let (namespace_id, hash_tag) = SharesFS::get_namespace_id_and_hash_tag().await;
    
    println!("\nFilesystem Structure:");
    println!("--------------------");
    
    // Get all nodes from the data store
    let nodes_key = format!("{}/{}_nodes", hash_tag, namespace_id);
    let _nodes = fs.data_store.zscan_match(&nodes_key, "").await
        .map_err(|_| nfsstat3::NFS3ERR_IO)?;
    
    // Get all path mappings
    let path_key = format!("{}/{}_id_to_path", hash_tag, namespace_id);
    let mappings = fs.data_store.hgetall(&path_key).await
        .map_err(|_| nfsstat3::NFS3ERR_IO)?;
    
    // Create id -> path mapping
    let mut id_to_path: HashMap<u64, String> = HashMap::new();
    for (id_str, path) in mappings {
        if let Ok(id) = id_str.parse::<u64>() {
            id_to_path.insert(id, path);
        }
    }
    
    // Print structure
    for (id, path) in id_to_path.iter() {
        println!("ID: {:>3} -> Path: {}", id, path);
    }
    
    println!("\nTotal entries: {}", id_to_path.len());
    println!("--------------------\n");
    
    Ok(())
}

fn benchmark_readdir(c: &mut Criterion) {
    let mut group = c.benchmark_group("readdir_operations");
    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(30));

    // Test different directory sizes
    for size in [100, 1000, 10000].iter() {
        let data_store = Arc::new(TestDataStore::new());
        let fs = SharesFS::new(data_store, None);
        
        // Setup test data using the runtime
        if let Err(e) = RUNTIME.block_on(setup_test_data(&fs, *size)) {
            eprintln!("Failed to setup test data: {:?}", e);
            continue;
        }

        // Print filesystem structure to verify setup
        /*
        if let Err(e) = RUNTIME.block_on(print_fs_structure(&fs)) {
            eprintln!("Failed to print filesystem structure: {:?}", e);
            continue;
        }*/

        let fs = Arc::new(fs);

        // Benchmark sequential readdir
        {
            let fs_clone = Arc::clone(&fs);
            group.bench_with_input(
                BenchmarkId::new("sequential_readdir", size),
                size,
                |b, &size| {
                    b.iter(|| {
                        let fs = Arc::clone(&fs_clone);
                        RUNTIME.block_on(async move {
                            fs.readdir(2, 0, size).await.unwrap()
                        })
                    });
                },
            );
        }

        // Benchmark parallel readdir
        {
            let fs_clone = Arc::clone(&fs);
            group.bench_with_input(
                BenchmarkId::new("parallel_readdir", size),
                size,
                |b, &size| {
                    b.iter(|| {
                        let fs = Arc::clone(&fs_clone);
                        RUNTIME.block_on(async move {
                            fs.readdir_parallel(2, 0, size).await.unwrap()
                        })
                    });
                },
            );
        }
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .with_plots()
        .sample_size(10);
    targets = benchmark_readdir
}
criterion_main!(benches);