/*
Note this is a terminal based TEST app.
It is used to help perfect the raw NFS protocol messages for use in apps like data_room.
*/
use std::net::SocketAddr;
use tokio::net::TcpStream;
use std::error::Error;
use tokio::time::sleep;
use std::time::Duration;

use graymamba::nfsclient::{
    self,
    access::ACCESS_READ,
    mount::MountReply,
    send_rpc_message,
    receive_rpc_reply,
};

#[derive(Debug)]
struct NfsSession { //when we establish a mount we get THE handle, we preserve it here - misnomer file_handle may be better called fs_handle
    file_handle: [u8; 16],
    dir_file_handles: Vec<([u8; 16], String, u64)>, // (handle, name, size) - we need to keep track of the handles for the files in the main directory at the moment
}

fn print_file_attributes(attrs: &nfsclient::Fattr3, prefix: &str) {
    println!("{}:", prefix);
    println!("  Type: {}", match attrs.file_type {
        1 => "Regular File",
        2 => "Directory",
        3 => "Block Device",
        4 => "Character Device",
        5 => "Symbolic Link",
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    // Coonect to Nfs server
    let addr: SocketAddr = "127.0.0.1:2049".parse()?;
    let mut stream = TcpStream::connect(addr).await?;
    println!("Connected to NFS server");
    
    // First do NULL call to check comms
    let null_call = nfsclient::null::build_null_call(1);
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
    let mount_call = nfsclient::mount::build_mount_call(2, "joseph"); // mary, jesus, joseph are the test drives
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

    // if we have a valid filesystem handle then we can test basic operations
    if let Some(mut session) = session {
        println!("Got a filesystem handle: {:02x?}", session.file_handle);
        
        // Now do GETATTR call
        let getattr_call = nfsclient::getattr::build_getattr_call(3, &session.file_handle);
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
        let lookup_call = nfsclient::lookup::build_lookup_call(4, &session.file_handle, "."); // "." means current directory
        println!("Sending LOOKUP call");
        send_rpc_message(&mut stream, &lookup_call).await?;
        sleep(Duration::from_millis(100)).await;
        
        match receive_rpc_reply(&mut stream).await {
            Ok(reply) => {
                println!("Received LOOKUP reply: {:02x?}", reply);
                match nfsclient::lookup::LookupReply::from_bytes(&reply) {
                    Ok(lookup_reply) => {
                        println!("LOOKUP Status: {}", lookup_reply.status);
                        if let Some(fh) = lookup_reply.file_handle {
                            println!("New file handle: {:02x?}", fh);
                        }
                        if let Some(attrs) = lookup_reply.attributes {
                            print_file_attributes(&attrs, "File attributes");
                        }
                        if let Some(dir_attrs) = lookup_reply.dir_attributes {
                            print_file_attributes(&dir_attrs, "Directory attributes");
                        }
                    },
                    Err(e) => {
                        println!("Error parsing LOOKUP reply: {}", e);
                    }
                }
            },
            Err(e) => {
                println!("Error receiving reply: {}", e);
            }
        }

        // Now do READDIRPLUS call using the mount file handle
        println!("\nSending READDIRPLUS call");
        let readdirplus_call = nfsclient::readdirplus::build_readdirplus_call(
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
                match nfsclient::readdirplus::ReaddirplusReply::from_bytes(&reply) {
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
    
        // Now do an ACCESS call for just the first file handle from the directory listing
        for (handle, name, _size) in session.dir_file_handles.into_iter().take(1) {
            println!("\nDEBUG: File handle for '{}' is: {:02x?}", name, handle);
            println!("DEBUG: Handle length: {}", handle.len());
            
            let access_call = nfsclient::access::build_access_call(
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
                    match nfsclient::access::AccessReply::from_bytes(&reply) {
                        Ok(access_reply) => {
                            println!("Access reply status: {}", access_reply.status);
                            println!("Access rights granted: {:08x}", access_reply.access);
                        },
                        Err(e) => println!("Error parsing ACCESS reply: {}", e)
                    }
                },
                Err(e) => println!("Error receiving ACCESS reply: {}", e)
            }

            // Now do a READ call for the same file
            println!("\nSending READ call for file: {}", name);
            let read_call = nfsclient::read::build_read_call(
                7,          // xid
                &handle,    // file handle
                0,          // offset (start of file)
                1024       // count (read up to 1024 bytes)
            );
            send_rpc_message(&mut stream, &read_call).await?;
            
            sleep(Duration::from_millis(100)).await;
            
            match receive_rpc_reply(&mut stream).await {
                Ok(reply) => {
                    match nfsclient::read::ReadReply::from_bytes(&reply) {
                        Ok(read_data) => {
                            println!("Read reply status: {}", read_data.status);
                            if read_data.status == 0 {
                                println!("Read {} bytes", read_data.count);
                                println!("EOF: {}", read_data.eof);
                                println!("Content ({}): {:?}", read_data.data.len(), String::from_utf8_lossy(&read_data.data));
                            }
                        },
                        Err(e) => println!("Error parsing READ reply: {}", e)
                    }
                },
                Err(e) => println!("Error receiving READ reply: {}", e)
            }
        }
    }

    Ok(())
}