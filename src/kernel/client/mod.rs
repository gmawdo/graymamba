use crate::kernel::protocol::rpc::{_msg_type, call_body, reply_body};
use crate::kernel::api::nfs::{self, fattr3, nfs_fh3};
use crate::kernel::handlers::nfs::router::NFSProgram;
use crate::kernel::protocol::xdr::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use std::net::SocketAddr;
use anyhow::Result;
use std::io::{Read, Write};
use num_traits::FromPrimitive;

use tracing::debug;

use crate::kernel::protocol::rpc::opaque_auth;
use crate::kernel::api::mount::{self, dirpath, mountres3_ok};

#[derive(Debug)]
pub struct NFSClient {
    pub stream: TcpStream,
    pub xid: u32,
    pub _root_fh: Option<nfs_fh3>,
}

#[derive(Debug)]
struct RPCMessage {
    xid: u32,
    msg_type: _msg_type,
    body: RPCBody,
}

#[derive(Debug)]
enum RPCBody {
    Call(call_body),
    Reply(reply_body),
}

impl XDR for RPCMessage {
    fn serialize<W: Write>(&self, dst: &mut W) -> std::io::Result<()> {
        self.xid.serialize(dst)?;
        (self.msg_type as u32).serialize(dst)?;
        match &self.body {
            RPCBody::Call(call) => call.serialize(dst),
            RPCBody::Reply(reply) => reply.serialize(dst),
        }
    }

    fn deserialize<R: Read>(&mut self, src: &mut R) -> std::io::Result<()> {
        self.xid.deserialize(src)?;
        let mut msg_type: u32 = 0;
        msg_type.deserialize(src)?;
        
        self.msg_type = _msg_type::from_u32(msg_type).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid message type")
        })?;
        
        match self.msg_type {
            _msg_type::CALL => {
                let mut call = call_body::default();
                call.deserialize(src)?;
                self.body = RPCBody::Call(call);
            }
            _msg_type::REPLY => {
                let mut reply = reply_body::default();
                reply.deserialize(src)?;
                self.body = RPCBody::Reply(reply);
            }
        }
        Ok(())
    }
}

impl Default for RPCMessage {
    fn default() -> Self {
        Self {
            xid: 0,
            msg_type: _msg_type::REPLY,
            body: RPCBody::Reply(reply_body::default()),
        }
    }
}

impl NFSClient {
    pub async fn connect(address: &str) -> Result<Self> {
        let addr: SocketAddr = format!("{}:2049", address).parse()?;
        debug!("=== Connecting to NFS server at {} ===", addr);
        let stream = TcpStream::connect(addr).await?;
        debug!("=== Connected from local port {} ===", stream.local_addr()?);
        Ok(Self {
            stream,
            xid: 0,
            _root_fh: None,
        })
    }

    pub async fn mount(&mut self, username: &str) -> Result<()> {
        debug!("=== Mounting user's directory {}===", username);
        let mount_path = format!("/{}'s drive", username);
        
        // 1. MOUNT program NULL call
        let null_xid = self.next_xid().await;
        let null_request = RPCMessage {
            xid: null_xid,
            msg_type: _msg_type::CALL,
            body: RPCBody::Call(call_body {
                rpcvers: 2,
                prog: mount::PROGRAM,
                vers: mount::VERSION,
                proc: 0, // MOUNTPROC3_NULL
                cred: opaque_auth::default(),
                verf: opaque_auth::default(),
            }),
        };
        
        self.send_and_receive_message(&null_request).await?;

        // 2. MOUNT program MNT call
        let mnt_xid = self.next_xid().await;
        let mnt_request = RPCMessage {
            xid: mnt_xid,
            msg_type: _msg_type::CALL,
            body: RPCBody::Call(call_body {
                rpcvers: 2,
                prog: mount::PROGRAM,
                vers: mount::VERSION,
                proc: 1, // MOUNTPROC3_MNT
                cred: opaque_auth::default(),
                verf: opaque_auth::default(),
            }),
        };

        let mut buf = Vec::new();
        mnt_request.serialize(&mut buf)?;
        let mount_path_vec: dirpath = mount_path.as_bytes().to_vec();
        mount_path_vec.serialize(&mut buf)?;
        
        self.send_message(&buf).await?;
        let response_buf = self.receive_message().await?;
        
        // Parse mount response to get filehandle
        let mut cursor = std::io::Cursor::new(response_buf);
        let mut rpc_response = RPCMessage::default();
        rpc_response.deserialize(&mut cursor)?;
        
        let mut mountres = mountres3_ok {
            fhandle: Vec::new(),
            auth_flavors: Vec::new(),
        };
        mountres.deserialize(&mut cursor)?;
        
        // Store root filehandle
        self._root_fh = Some(nfs_fh3 { 
            data: mountres.fhandle 
        });
        
        Ok(())
    }

    async fn next_xid(&mut self) -> u32 {
        self.xid = self.xid.wrapping_add(1);
        self.xid
    }

    pub async fn get_root_attributes(&mut self) -> Result<fattr3> {
        let xid = self.next_xid().await;
        
        let request = RPCMessage {
            xid,
            msg_type: _msg_type::CALL,
            body: RPCBody::Call(call_body {
                rpcvers: 2,
                prog: nfs::PROGRAM,
                vers: nfs::VERSION,
                proc: NFSProgram::NFSPROC3_GETATTR as u32,
                cred: opaque_auth::default(),
                verf: opaque_auth::default(),
            }),
        };

        let mut buf = Vec::new();
        request.serialize(&mut buf)?;
        self.stream.write_all(&buf).await?;
        self.stream.flush().await?;
        
        let mut response_buf = Vec::new();
        self.stream.read_to_end(&mut response_buf).await?;
        let mut cursor = std::io::Cursor::new(response_buf);
        
        let mut response = RPCMessage {
            xid: 0,
            msg_type: _msg_type::CALL,
            body: RPCBody::Call(call_body::default()),
        };
        response.deserialize(&mut cursor)?;
        
        let mut attrs = fattr3::default();
        attrs.deserialize(&mut cursor)?;
        
        Ok(attrs)
    }

    async fn send_message(&mut self, buf: &[u8]) -> Result<()> {
        let msg_len = (buf.len() as u32).to_be_bytes();
        self.stream.write_all(&msg_len).await?;
        self.stream.write_all(buf).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<Vec<u8>> {
        let mut size_buf = [0u8; 4];
        self.stream.read_exact(&mut size_buf).await?;
        let msg_size = u32::from_be_bytes(size_buf);
        
        let mut response_buf = vec![0; msg_size as usize];
        self.stream.read_exact(&mut response_buf).await?;
        Ok(response_buf)
    }

    async fn send_and_receive_message(&mut self, request: &RPCMessage) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        request.serialize(&mut buf)?;
        self.send_message(&buf).await?;
        self.receive_message().await
    }

    async fn send_rpc_message(&mut self, prog: u32, vers: u32, proc: u32) -> Result<Vec<u8>> {
        let xid = self.next_xid().await;
        let request = RPCMessage {
            xid,
            msg_type: _msg_type::CALL,
            body: RPCBody::Call(call_body {
                rpcvers: 2,
                prog,
                vers,
                proc,
                cred: opaque_auth::default(),
                verf: opaque_auth::default(),
            }),
        };
        
        let mut buf = Vec::new();
        request.serialize(&mut buf)?;
        self.send_message(&buf).await?;
        self.receive_message().await
    }
}

impl Drop for NFSClient {
    fn drop(&mut self) {
        // Best effort to flush any pending writes
        if let Err(e) = self.stream.try_write(&mut Vec::new()) {
            eprintln!("Error flushing NFS connection on drop: {}", e);
        }
    }
}