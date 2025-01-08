use anyhow::anyhow;
use std::io::Cursor;
use std::io::{Read, Write};
use tracing::{error, trace, warn};

use crate::kernel::protocol::context::RPCContext;
use crate::kernel::protocol::rpc::*;
use crate::kernel::protocol::xdr::*;

use crate::kernel::api::mount;
use crate::kernel::api::nfs;
use crate::kernel::api::portmap;

use crate::kernel::handlers::nfs::router::handle_nfs;

use crate::kernel::handlers::mount_handlers;

use crate::kernel::handlers::portmap_handlers;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::DuplexStream;
use tokio::sync::mpsc;
use tracing::debug;
#[cfg(feature = "metrics")]
use graymamba::kernel::metrics::*;
// Information from RFC 5531
// https://datatracker.ietf.org/doc/html/rfc5531

async fn handle_rpc(
    input: &mut impl Read,
    output: &mut impl Write,
    mut context: RPCContext,
) -> Result<(), anyhow::Error> {
    debug!("Starting RPC message deserialization");
    let mut recv = rpc_msg::default();
    match recv.deserialize(input) {
        Ok(_) => debug!("Successfully deserialized RPC message"),
        Err(e) => {
            debug!("Failed to deserialize RPC message: {:?}", e);
            return Err(anyhow!("Failed to deserialize RPC message: {}", e));
        }
    }
    let xid = recv.xid;
    if let rpc_body::CALL(call) = recv.body {
        if let auth_flavor::AUTH_UNIX = call.cred.flavor {
            let mut auth = auth_unix::default();
            auth.deserialize(&mut Cursor::new(&call.cred.body))?;
            context.auth = auth;
        }
        if call.rpcvers != 2 {
            warn!("Invalid RPC version {} != 2", call.rpcvers);
            rpc_vers_mismatch(xid).serialize(output)?;
            return Ok(());
        }
        if call.prog == nfs::PROGRAM {
            handle_nfs(xid, call, input, output, &context).await
        } else if call.prog == portmap::PROGRAM {
            portmap_handlers::handle_portmap(xid, call, input, output, &context)
        } else if call.prog == mount::PROGRAM {
            mount_handlers::handle_mount(xid, call, input, output, &context).await
        } else {
            warn!(
                "Unknown RPC Program number {} != {}",
                call.prog,
                nfs::PROGRAM
            );
            prog_unavail_reply_message(xid).serialize(output)?;
            Ok(())
        }
    } else {
        error!("Unexpectedly received a Reply instead of a Call");
        Err(anyhow!("Bad RPC Call format"))
    }
}

/// RFC 1057 Section 10
/// When RPC messages are passed on top of a byte stream transport
/// protocol (like TCP), it is necessary to delimit one message from
/// another in order to detect and possibly recover from protocol errors.
/// This is called record marking (RM).  Sun uses this RM/TCP/IP
/// transport for passing RPC messages on TCP streams.  One RPC message
/// fits into one RM record.
///
/// A record is composed of one or more record fragments.  A record
/// fragment is a four-byte header followed by 0 to (2**31) - 1 bytes of
/// fragment data.  The bytes encode an unsigned binary number; as with
/// XDR integers, the byte order is from highest to lowest.  The number
/// encodes two values -- a boolean which indicates whether the fragment
/// is the last fragment of the record (bit value 1 implies the fragment
/// is the last fragment) and a 31-bit unsigned binary value which is the
/// length in bytes of the fragment's data.  The boolean value is the
/// highest-order bit of the header; the length is the 31 low-order bits.
/// (Note that this record specification is NOT in XDR standard form!)
async fn read_fragment(
    socket: &mut DuplexStream,
    append_to: &mut Vec<u8>,
) -> Result<bool, anyhow::Error> {
    let mut header_buf = [0_u8; 4];
    debug!("Attempting to read fragment header...");
    socket.read_exact(&mut header_buf).await?;
    debug!("Fragment header raw: {:?}", header_buf);
    let fragment_header = u32::from_be_bytes(header_buf);
    let is_last = (fragment_header & (1 << 31)) > 0;
    let length = (fragment_header & ((1 << 31) - 1)) as usize;
    debug!("Reading fragment length:{}, last:{}", length, is_last);
    
    let start_offset = append_to.len();
    debug!("Current buffer size: {}, extending to: {}", start_offset, start_offset + length);
    append_to.resize(append_to.len() + length, 0);
    
    // Read in chunks to handle partial reads
    let mut bytes_read = 0;
    while bytes_read < length {
        let remaining = length - bytes_read;
        debug!("Reading chunk: {} bytes read of {}, remaining: {}", bytes_read, length, remaining);
        
        let read_result = socket
            .read(&mut append_to[start_offset + bytes_read..start_offset + length])
            .await?;
        
        if read_result == 0 {
            debug!("Connection closed after reading {} of {} bytes", bytes_read, length);
            return Err(anyhow!("Connection closed before complete fragment was read"));
        }
        
        bytes_read += read_result;
        debug!("Chunk read complete: {} bytes in this chunk", read_result);
    }

    debug!(
        "Fragment read complete - length:{}, last:{}, total_bytes_read:{}",
        length,
        is_last,
        bytes_read
    );
    
    Ok(is_last)
}

pub async fn write_fragment(
    socket: &mut tokio::net::TcpStream,
    buf: &[u8],
) -> Result<(), anyhow::Error> {
    // TODO: split into many fragments
    assert!(buf.len() < (1 << 31));
    // set the last flag
    let fragment_header = buf.len() as u32 + (1 << 31);
    let header_buf = u32::to_be_bytes(fragment_header);
    socket.write_all(&header_buf).await?;
    trace!("Writing fragment length:{}", buf.len());
    socket.write_all(buf).await?;
    Ok(())
}

pub type SocketMessageType = Result<Vec<u8>, anyhow::Error>;

/// The Socket Message Handler reads from a TcpStream and spawns off
/// subtasks to handle each message. replies are queued into the
/// reply_send_channel.
#[derive(Debug)]
pub struct SocketMessageHandler {
    cur_fragment: Vec<u8>,
    socket_receive_channel: DuplexStream,
    reply_send_channel: mpsc::UnboundedSender<SocketMessageType>,
    context: RPCContext,
}

impl SocketMessageHandler {
    /// Creates a new SocketMessageHandler with the receiver for queued message replies
    pub fn new(
        context: &RPCContext,
    ) -> (
        Self,
        DuplexStream,
        mpsc::UnboundedReceiver<SocketMessageType>,
    ) {
        let (socksend, sockrecv) = tokio::io::duplex(256000);
        let (msgsend, msgrecv) = mpsc::unbounded_channel();
        (
            Self {
                cur_fragment: Vec::new(),
                socket_receive_channel: sockrecv,
                reply_send_channel: msgsend,
                context: context.clone(),
            },
            socksend,
            msgrecv,
        )
    }

    /// Reads a fragment from the socket. This should be looped.
    pub async fn read(&mut self) -> Result<(), anyhow::Error> {
        debug!("Starting to read new fragment");
        let is_last = match read_fragment(&mut self.socket_receive_channel, &mut self.cur_fragment).await {
            Ok(last) => {
                debug!("Successfully read fragment, is_last: {}", last);
                last
            },
            Err(e) => {
                debug!("Error reading fragment: {:?}", e);
                return Err(e);
            }
        };
        #[cfg(feature = "metrics")]
        FRAGMENTS_PROCESSED.inc();
        
        if is_last {
            #[cfg(feature = "metrics")]
            let fragment_size = self.cur_fragment.len();
            #[cfg(feature = "metrics")]
            BYTES_RECEIVED.inc_by(fragment_size as u64);
            
            debug!("Processing last fragment, current buffer size: {}", self.cur_fragment.len());
            let fragment = std::mem::take(&mut self.cur_fragment);
            let context = self.context.clone();
            let send = self.reply_send_channel.clone();
            
            tokio::spawn(async move {
                let mut write_buf: Vec<u8> = Vec::new();
                let mut write_cursor = Cursor::new(&mut write_buf);
                debug!("Starting RPC handler with fragment size: {}", fragment.len());
                let maybe_reply =
                    handle_rpc(&mut Cursor::new(fragment), &mut write_cursor, context).await;
                match maybe_reply {
                    Err(e) => {
                        error!("RPC Error: {:?}", e);
                        let _ = send.send(Err(e));
                    }
                    Ok(_) => {
                        let _ = std::io::Write::flush(&mut write_cursor);
                        drop(write_cursor);
                        debug!("RPC handler completed successfully, response size: {}", write_buf.len());
                        let _ = send.send(Ok(write_buf));
                    }
                }
            });
        }
        Ok(())
    }
}
