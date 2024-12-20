use std::net::SocketAddr;
use tokio::net::TcpStream;
use std::error::Error;
use tokio::time::sleep;
use std::time::Duration;
use crate::rpc::access::ACCESS_READ;
use crate::rpc::mount::MountReply;
use crate::rpc::{send_rpc_message, receive_rpc_reply};

mod rpc;

#[derive(Debug)]
struct NfsSession {
    file_handle: [u8; 16],
    dir_file_handles: Vec<([u8; 16], String, u64)>, // (handle, name, size)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr: SocketAddr = "127.0.0.1:2049".parse()?;
    let mut stream = TcpStream::connect(addr).await?;
    
    println!("Connected to NFS server");
    
    // First do NULL call
    let null_call = rpc::null::build_null_call(1);
    println!("Sending NULL call");
    send_rpc_message(&mut stream, &null_call).await?;
    
    match receive_rpc_reply(&mut stream).await {
        Ok(reply) => {
            println!("Received NULL reply: {:02x?}", reply);
        },
        Err(e) => {
            println!("Error receiving reply: {}", e);
            return Err(e);
        }
    }
    
    // Then do MOUNT call
    let mount_call = rpc::mount::build_mount_call(2, "joseph");
    println!("Sending MOUNT call");
    send_rpc_message(&mut stream, &mount_call).await?;
    
    let session = match receive_rpc_reply(&mut stream).await {
        Ok(reply) => {
            println!("Received MOUNT reply: {:02x?}", reply);
            if let Ok(mount_reply) = MountReply::from_bytes(&reply) {
                if mount_reply.status == 0 {
                    Some(NfsSession {
                        file_handle: mount_reply.file_handle,
                        dir_file_handles: Vec::new(),
                    })
                } else {
                    println!("Mount failed with status: {}", mount_reply.status);
                    None
                }
            } else {
                None
            }
        },
        Err(e) => {
            println!("Error receiving reply: {}", e);
            None
        }
    };

    // Now do GETATTR call if we have a valid session
    if let Some(mut session) = session {
        println!("Got file handle: {:02x?}", session.file_handle);
        
        let getattr_call = rpc::getattr::build_getattr_call(3, &session.file_handle);
        println!("Sending GETATTR call");
        send_rpc_message(&mut stream, &getattr_call).await?;
        
        // Add small delay
        sleep(Duration::from_millis(100)).await;
        
        match receive_rpc_reply(&mut stream).await {
            Ok(reply) => {
                println!("Received GETATTR reply: {:02x?}", reply);
            },
            Err(e) => {
                println!("Error receiving reply: {}", e);
            }
        }

        // Now do LOOKUP call
        let lookup_call = rpc::lookup::build_lookup_call(4, &session.file_handle, "."); // "." means current directory
        println!("Sending LOOKUP call");
        send_rpc_message(&mut stream, &lookup_call).await?;
        
        sleep(Duration::from_millis(100)).await;
        
        match receive_rpc_reply(&mut stream).await {
            Ok(reply) => {
                println!("Received LOOKUP reply: {:02x?}", reply);
                match rpc::lookup::LookupReply::from_bytes(&reply) {
                    Ok(lookup_reply) => {
                        println!("LOOKUP Status: {}", lookup_reply.status);
                        if let Some(fh) = lookup_reply.file_handle {
                            println!("New file handle: {:02x?}", fh);
                        }
                        if let Some(attrs) = lookup_reply.attributes {
                            println!("File attributes:");
                            println!("  Type: {}", match attrs.file_type {
                                2 => "Directory",
                                1 => "Regular file",
                                3 => "Block device",
                                4 => "Character device",
                                5 => "Symbolic link",
                                6 => "Socket",
                                7 => "FIFO",
                                _ => "Unknown",
                            });
                            println!("  Mode: {:o}", attrs.mode);
                            println!("  Links: {}", attrs.nlink);
                            println!("  UID: {}", attrs.uid);
                            println!("  GID: {}", attrs.gid);
                            println!("  Size: {} bytes", attrs.size);
                            println!("  Used: {} bytes", attrs.used);
                        }
                        if let Some(dir_attrs) = lookup_reply.dir_attributes {
                            println!("Directory attributes:");
                            println!("  Type: {}", match dir_attrs.file_type {
                                2 => "Directory",
                                1 => "Regular file",
                                _ => "Unknown",
                            });
                            println!("  Mode: {:o}", dir_attrs.mode);
                            println!("  Size: {} bytes", dir_attrs.size);
                        }
                    },
                    Err(e) => {
                        println!("Error parsing LOOKUP reply: {}", e);
                    }
                }

                // Now do READDIRPLUS call using the mount file handle
                println!("\nSending READDIRPLUS call");
                let readdirplus_call = rpc::readdirplus::build_readdirplus_call(
                    5,              // xid
                    &session.file_handle,
                    0,              // cookie
                    0,              // cookieverf
                    8192,          // dircount (from Wireshark)
                    32768          // maxcount (from Wireshark)
                );
                send_rpc_message(&mut stream, &readdirplus_call).await?;
                
                sleep(Duration::from_millis(100)).await;
                
                match receive_rpc_reply(&mut stream).await {
                    Ok(reply) => {
                        match rpc::readdirplus::ReaddirplusReply::from_bytes(&reply) {
                            Ok(readdir_reply) => {
                                println!("\nREADDIRPLUS Status: {}", readdir_reply.status);
                                if readdir_reply.status == 0 {
                                    println!("\nDirectory contents:");
                                    for entry in readdir_reply.entries {
                                        print!("  {} (id: {})", entry.name, entry.fileid);
                                        if let (Some(attrs), Some(handle)) = (&entry.name_attributes, &entry.name_handle) {
                                            println!(" - {} ({} bytes)", 
                                                match attrs.file_type {
                                                    1 => {
                                                        session.dir_file_handles.push((*handle, entry.name.clone(), attrs.size));
                                                        "Regular file"
                                                    },
                                                    2 => "Directory",
                                                    _ => { 
                                                        session.dir_file_handles.push((*handle, entry.name.clone(), attrs.size));
                                                        "Unknown"
                                                    },
                                                },
                                                attrs.size
                                            );
                                        }
                                    }
                                    println!("\nFound {} files to process", session.dir_file_handles.len());
                                }
                            },
                            Err(e) => println!("Error parsing READDIRPLUS reply: {}", e)
                        }
                    },
                    Err(e) => println!("Error receiving READDIRPLUS reply: {}", e)
                }
            },
            Err(e) => {
                println!("Error receiving reply: {}", e);
            }
        }
    
        // Now do an ACCESS call for just the first file handle
        for (handle, name, _size) in session.dir_file_handles.into_iter().take(1) {
            println!("\nDEBUG: File handle for '{}' is: {:02x?}", name, handle);
            println!("DEBUG: Handle length: {}", handle.len());
            
            let access_call = rpc::access::build_access_call(
                6, 
                &handle,
                ACCESS_READ  // Use the constant from access.rs
            );
            println!("Sending ACCESS call for file: {}", name);
            send_rpc_message(&mut stream, &access_call).await?;
            
            // Add small delay
            sleep(Duration::from_millis(100)).await;
            
            match receive_rpc_reply(&mut stream).await {
                Ok(reply) => {
                    match rpc::access::AccessReply::from_bytes(&reply) {
                        Ok(access_reply) => {
                            println!("Access reply status: {}", access_reply.status);
                            println!("Access rights granted: {:08x}", access_reply.access);
                        },
                        Err(e) => println!("Error parsing ACCESS reply: {}", e)
                    }
                },
                Err(e) => println!("Error receiving ACCESS reply: {}", e)
            }
        }
    }

    Ok(())
}