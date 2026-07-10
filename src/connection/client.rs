//! Async TCP connection client, mirroring the Go `connection.Client`.
//!
//! # Wire protocol
//!
//! Every message on the wire is prefixed with a 9-byte **totalLength** field
//! (`x` + 8 hex digits).  The receiver reads those 9 bytes first, then reads
//! the remaining `totalLength - 9` bytes to obtain the full frame, which is
//! passed to `DecodeMessage`.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use bytes::BytesMut;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Mutex,
    time::timeout,
};

use crate::{
    connection::{
        resolver,
        retry::Retry,
        traits::{Tracer, WireHook},
    },
    errors::{ErrCode, GatewayDError},
    log::{Logger, NoOpLogger},
};

/// Initial read chunk size; grows as messages arrive.
const INITIAL_CHUNK_SIZE: usize = 512;
/// Maximum consecutive zero-byte writes before aborting.
const MAX_ZERO_WRITES: usize = 3;
/// 9-byte length prefix size.
const LEN_PREFIX_BYTES: usize = 9;

/// Aggressive keepalive so a dead/half-open peer surfaces as a hard read error
/// within ~30s, plus a matching TCP_USER_TIMEOUT (Linux) so unacknowledged
/// writes fail fast instead of blocking for the OS default.
const KEEPALIVE_IDLE: Duration = Duration::from_secs(15);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const KEEPALIVE_RETRIES: u32 = 3;
const TCP_USER_TIMEOUT: Duration = Duration::from_secs(30);

/// Apply TCP_NODELAY, aggressive keepalive, and TCP_USER_TIMEOUT to a freshly
/// dialed stream. Errors are ignored (best-effort tuning).
fn apply_tcp_options(
    stream: &TcpStream,
    idle: Duration,
    interval: Duration,
    retries: u32,
    user_timeout: Duration,
) {
    stream.set_nodelay(true).ok();
    let sock_ref = socket2::SockRef::from(stream);

    let ka_idle = if idle.is_zero() { KEEPALIVE_IDLE } else { idle };
    let ka_interval = if interval.is_zero() {
        KEEPALIVE_INTERVAL
    } else {
        interval
    };
    let ka_retries = if retries == 0 {
        KEEPALIVE_RETRIES
    } else {
        retries
    };
    let ka_user_timeout = if user_timeout.is_zero() {
        TCP_USER_TIMEOUT
    } else {
        user_timeout
    };

    let ka = socket2::TcpKeepalive::new()
        .with_time(ka_idle)
        .with_interval(ka_interval);
    // Probe count and TCP_USER_TIMEOUT are Linux-specific knobs.
    #[cfg(target_os = "linux")]
    let ka = ka.with_retries(ka_retries);
    sock_ref.set_tcp_keepalive(&ka).ok();

    #[cfg(target_os = "linux")]
    {
        sock_ref.set_tcp_user_timeout(Some(ka_user_timeout)).ok();
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (ka_retries, ka_user_timeout);
    }
}

// ── ClientConfig ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ClientConfig {
    pub tracer: Arc<dyn Tracer>,
    pub logger: Arc<dyn Logger>,
    pub wire_hook: Arc<dyn WireHook>,
    pub dial_timeout: Duration,
    pub send_timeout: Duration,
    pub receive_timeout: Duration,
    pub tcp_keep_alive_idle: Duration,
    pub tcp_keep_alive_interval: Duration,
    pub tcp_keep_alive_count: u32,
    pub tcp_user_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        use crate::connection::traits::{NoOpTracer, NoOpWireHook};
        Self {
            tracer: Arc::new(NoOpTracer),
            logger: Arc::new(NoOpLogger),
            wire_hook: Arc::new(NoOpWireHook),
            dial_timeout: Duration::from_secs(10),
            send_timeout: Duration::from_secs(30),
            receive_timeout: Duration::from_secs(30),
            tcp_keep_alive_idle: Duration::ZERO,
            tcp_keep_alive_interval: Duration::ZERO,
            tcp_keep_alive_count: 0,
            tcp_user_timeout: Duration::ZERO,
        }
    }
}

// ── Client ───────────────────────────────────────────────────────────────────

/// Async TCP connection to a Pod-OS gateway.
pub struct Client {
    write_half: Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>,
    read_half: Mutex<Option<tokio::net::tcp::OwnedReadHalf>>,
    connected: AtomicBool,

    pub network: String,
    pub host: String,
    pub port: String,
    pub actor_name: String,
    pub group_name: String,

    pub receive_timeout: Duration,
    pub send_timeout: Duration,
    pub dial_timeout: Duration,

    chunk_size: std::sync::atomic::AtomicUsize,

    retry: Arc<Retry>,
    logger: Arc<dyn Logger>,
    wire_hook: Arc<dyn WireHook>,
    _tracer: Arc<dyn Tracer>,
    tcp_keep_alive_idle: Duration,
    tcp_keep_alive_interval: Duration,
    tcp_keep_alive_count: u32,
    tcp_user_timeout: Duration,
}

impl Client {
    /// Dial and connect, retrying according to the supplied `Retry` policy.
    pub async fn connect(
        network: &str,
        host: &str,
        port: &str,
        actor_name: &str,
        retry: Arc<Retry>,
        cfg: ClientConfig,
    ) -> Result<Arc<Self>, GatewayDError> {
        const SUPPORTED_TCP: &[&str] = &["tcp", "tcp4", "tcp6"];
        if !SUPPORTED_TCP.contains(&network) {
            return Err(GatewayDError::new(
                ErrCode::InvalidNetwork,
                format!(
                    "unsupported network '{}': only TCP (tcp/tcp4/tcp6) is implemented",
                    network
                ),
            ));
        }
        let addr = resolver::make_addr(host, port);
        let dial_timeout = cfg.dial_timeout;
        let logger = cfg.logger.clone();

        let stream = retry
            .run(|attempt| {
                let addr = addr.clone();
                let logger = logger.clone();
                async move {
                    logger.debug("dialing gateway", &[("addr", &addr), ("attempt", &attempt)]);
                    timeout(dial_timeout, TcpStream::connect(&addr))
                        .await
                        .map_err(|_| {
                            GatewayDError::new(
                                ErrCode::ClientDialFailed,
                                format!("dial timeout: {}", addr),
                            )
                        })?
                        .map_err(|e| {
                            GatewayDError::wrap(
                                ErrCode::ClientDialFailed,
                                format!("dial failed: {}", addr),
                                e,
                            )
                        })
                }
            })
            .await?;

        // TCP_NODELAY + aggressive keepalive + TCP_USER_TIMEOUT for fast
        // dead-connection detection.
        apply_tcp_options(
            &stream,
            cfg.tcp_keep_alive_idle,
            cfg.tcp_keep_alive_interval,
            cfg.tcp_keep_alive_count,
            cfg.tcp_user_timeout,
        );

        let (read_half, write_half) = stream.into_split();

        Ok(Arc::new(Self {
            write_half: Mutex::new(Some(write_half)),
            read_half: Mutex::new(Some(read_half)),
            connected: AtomicBool::new(true),
            network: network.to_string(),
            host: host.to_string(),
            port: port.to_string(),
            actor_name: actor_name.to_string(),
            group_name: String::new(),
            receive_timeout: cfg.receive_timeout,
            send_timeout: cfg.send_timeout,
            dial_timeout: cfg.dial_timeout,
            chunk_size: std::sync::atomic::AtomicUsize::new(INITIAL_CHUNK_SIZE),
            retry,
            logger: cfg.logger,
            wire_hook: cfg.wire_hook,
            _tracer: cfg.tracer,
            tcp_keep_alive_idle: cfg.tcp_keep_alive_idle,
            tcp_keep_alive_interval: cfg.tcp_keep_alive_interval,
            tcp_keep_alive_count: cfg.tcp_keep_alive_count,
            tcp_user_timeout: cfg.tcp_user_timeout,
        }))
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    /// Mark the transport dead and re-tag the error as a fatal ConnectionLost so
    /// the receive loop reconnects and fails in-flight callers.
    fn mark_lost(&self, err: GatewayDError) -> GatewayDError {
        self.connected.store(false, Ordering::Release);
        if err.code == ErrCode::ConnectionLost {
            err
        } else {
            GatewayDError::new(ErrCode::ConnectionLost, err.message)
        }
    }

    pub fn remote_addr(&self) -> String {
        resolver::make_addr(&self.host, &self.port)
    }

    // ── Send ─────────────────────────────────────────────────────────────────

    /// Write `data` to the TCP stream, respecting `send_timeout`.
    pub async fn send(&self, data: &[u8]) -> Result<(), GatewayDError> {
        if !self.is_connected() {
            return Err(GatewayDError::new(
                ErrCode::ClientNotConnected,
                "client is not connected",
            ));
        }
        let mut guard = self.write_half.lock().await;
        let writer = guard
            .as_mut()
            .ok_or_else(|| GatewayDError::new(ErrCode::ClientNotConnected, "write half is gone"))?;

        let mut zero_writes = 0usize;
        let mut pos = 0usize;

        while pos < data.len() {
            let write_fut = writer.write(&data[pos..]);
            // A write timeout or error means the socket is dead (RST, broken
            // pipe, or TCP_USER_TIMEOUT). Mark disconnected and surface a fatal
            // connection-lost error so senders stop and the receiver reconnects.
            let n = match timeout(self.send_timeout, write_fut).await {
                Err(_) => {
                    self.connected.store(false, Ordering::Release);
                    return Err(GatewayDError::new(ErrCode::ConnectionLost, "send timeout"));
                }
                Ok(Err(e)) => {
                    self.connected.store(false, Ordering::Release);
                    return Err(GatewayDError::wrap(ErrCode::ConnectionLost, "write error", e));
                }
                Ok(Ok(n)) => n,
            };

            if n == 0 {
                zero_writes += 1;
                if zero_writes >= MAX_ZERO_WRITES {
                    self.connected.store(false, Ordering::Release);
                    return Err(GatewayDError::new(
                        ErrCode::ConnectionLost,
                        "repeated zero-byte writes",
                    ));
                }
            } else {
                zero_writes = 0;
                pos += n;
            }
        }

        self.wire_hook.on_send(data);
        Ok(())
    }

    // ── Receive ──────────────────────────────────────────────────────────────

    /// Read one complete framed message from the TCP stream.
    ///
    /// Blocks until the full `totalLength` bytes have been read.
    /// Returns the raw bytes (including the 9-byte length prefix) for
    /// `decode_message`.
    pub async fn receive(&self) -> Result<Vec<u8>, GatewayDError> {
        self.receive_with_timeout(self.receive_timeout).await
    }

    /// Read one complete framed message with an explicit timeout override.
    pub async fn receive_with_timeout(
        &self,
        receive_timeout: Duration,
    ) -> Result<Vec<u8>, GatewayDError> {
        if !self.is_connected() {
            return Err(GatewayDError::new(
                ErrCode::ClientNotConnected,
                "client is not connected",
            ));
        }
        let mut guard = self.read_half.lock().await;
        let reader = guard
            .as_mut()
            .ok_or_else(|| GatewayDError::new(ErrCode::ClientNotConnected, "read half is gone"))?;

        // ── Read the 9-byte length prefix ─────────────────────────────────
        // A timeout here is a benign idle read (no frame was in progress). Any
        // other error means the socket is dead -> fatal.
        let mut prefix = [0u8; LEN_PREFIX_BYTES];
        let read_prefix = reader.read_exact(&mut prefix);
        match timeout(receive_timeout, read_prefix).await {
            Err(_) => {
                return Err(GatewayDError::new(
                    ErrCode::ReceiveIdleTimeout,
                    "receive idle timeout (prefix)",
                ));
            }
            Ok(Err(e)) => {
                self.connected.store(false, Ordering::Release);
                return Err(GatewayDError::wrap(
                    ErrCode::ConnectionLost,
                    "read prefix error",
                    e,
                ));
            }
            Ok(Ok(_)) => {}
        }

        let total_len = parse_length_prefix(&prefix).map_err(|e| self.mark_lost(e))?;
        if total_len < LEN_PREFIX_BYTES {
            return Err(self.mark_lost(GatewayDError::new(
                ErrCode::ConnectionLost,
                "declared totalLength < 9",
            )));
        }
        let max_size = crate::message::constants::max_message_size() as usize;
        if total_len > max_size {
            return Err(self.mark_lost(GatewayDError::new(
                ErrCode::ConnectionLost,
                format!(
                    "declared totalLength {} exceeds max message size {}",
                    total_len, max_size
                ),
            )));
        }
        let body_len = total_len - LEN_PREFIX_BYTES;

        // ── Read the body in adaptive chunks ─────────────────────────────
        let mut buf = BytesMut::with_capacity(total_len);
        buf.extend_from_slice(&prefix);
        buf.resize(total_len, 0);

        let chunk_size = self
            .chunk_size
            .load(Ordering::Relaxed)
            .max(INITIAL_CHUNK_SIZE);
        let mut pos = LEN_PREFIX_BYTES;

        while pos < total_len {
            let end = (pos + chunk_size).min(total_len);
            let slice = &mut buf[pos..end];
            let read_fut = reader.read_exact(slice);
            // We are mid-frame: a timeout or any error here means the stream is
            // desynced / the peer is dead -> fatal.
            match timeout(receive_timeout, read_fut).await {
                Err(_) => {
                    return Err(self.mark_lost(GatewayDError::new(
                        ErrCode::ConnectionLost,
                        "receive timeout mid-frame (body)",
                    )));
                }
                Ok(Err(e)) => {
                    return Err(self.mark_lost(GatewayDError::wrap(
                        ErrCode::ConnectionLost,
                        "read body error",
                        e,
                    )));
                }
                Ok(Ok(_)) => {}
            }
            pos = end;
        }

        // Grow chunk size toward 4 KiB for large messages
        if body_len > chunk_size {
            let new_size = (chunk_size * 2).min(4096);
            self.chunk_size.store(new_size, Ordering::Relaxed);
        }

        let data = buf.freeze().to_vec();
        self.wire_hook.on_receive(&data);
        Ok(data)
    }

    // ── Reconnect ────────────────────────────────────────────────────────────

    /// Close the current connection and re-dial.
    pub async fn reconnect(&self) -> Result<(), GatewayDError> {
        self.close().await;

        let addr = resolver::make_addr(&self.host, &self.port);
        let logger = self.logger.clone();
        let dial_timeout = self.dial_timeout;

        let stream = self
            .retry
            .run(|attempt| {
                let addr = addr.clone();
                let logger = logger.clone();
                async move {
                    logger.debug("reconnecting", &[("addr", &addr), ("attempt", &attempt)]);
                    timeout(dial_timeout, TcpStream::connect(&addr))
                        .await
                        .map_err(|_| {
                            GatewayDError::new(ErrCode::ClientReconnectFailed, "reconnect timeout")
                        })?
                        .map_err(|e| {
                            GatewayDError::wrap(
                                ErrCode::ClientReconnectFailed,
                                "reconnect failed",
                                e,
                            )
                        })
                }
            })
            .await?;

        // Re-apply TCP tuning after reconnection.
        apply_tcp_options(
            &stream,
            self.tcp_keep_alive_idle,
            self.tcp_keep_alive_interval,
            self.tcp_keep_alive_count,
            self.tcp_user_timeout,
        );

        let (r, w) = stream.into_split();
        *self.write_half.lock().await = Some(w);
        *self.read_half.lock().await = Some(r);
        self.connected.store(true, Ordering::Release);
        Ok(())
    }

    /// Close the connection, unblocking any pending reads/writes.
    pub async fn close(&self) {
        self.connected.store(false, Ordering::Release);
        // Dropping write half triggers a graceful TCP close
        *self.write_half.lock().await = None;
        *self.read_half.lock().await = None;
    }
}

// ── Length prefix parser ─────────────────────────────────────────────────────

/// Parse a 9-byte length prefix.  Accepts `x` + 8 hex digits OR 9 decimal digits.
fn parse_length_prefix(prefix: &[u8; 9]) -> Result<usize, GatewayDError> {
    let s = std::str::from_utf8(prefix).map_err(|_| {
        GatewayDError::new(ErrCode::ClientReceiveFailed, "length prefix is not UTF-8")
    })?;
    if let Some(hex) = s.strip_prefix('x') {
        usize::from_str_radix(hex, 16).map_err(|_| {
            GatewayDError::new(
                ErrCode::ClientReceiveFailed,
                format!("invalid hex prefix: {s}"),
            )
        })
    } else {
        s.trim_start_matches('0')
            .parse::<usize>()
            .or_else(|_| if s == "000000000" { Ok(0) } else { Err(()) })
            .map_err(|_| {
                GatewayDError::new(
                    ErrCode::ClientReceiveFailed,
                    format!("invalid decimal prefix: {s}"),
                )
            })
    }
}
