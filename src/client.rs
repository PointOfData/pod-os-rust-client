//! Pod-OS async client, mirroring the Go `podos.Client`.
//!
//! # High-performance design
//!
//! - **Concurrent mode**: a single background task reads all incoming frames
//!   and dispatches responses to per-request `tokio::sync::oneshot` channels
//!   keyed on `MessageId`.  The send path holds only the `Mutex` for the
//!   duration of the write syscall.
//! - **DashMap** provides lock-free reads in the receiver task hot-path.
//! - **Synchronous mode**: sequential send → receive on the same connection.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};

use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio::sync::{oneshot, RwLock};
use uuid::Uuid;

use crate::{
    config::Config,
    connection::{
        client::{Client as ConnClient, ClientConfig},
        retry::Retry,
        traits::{NoOpTracer, NoOpWireHook},
    },
    errors::{ErrCode, GatewayDError},
    log::{Level, Logger, NoOpLogger, TracingLogger},
    message::{
        decode_message, encode_message,
        intents,
        types::{Envelope, Message, SocketMessage},
    },
};

// ── Error sentinel ────────────────────────────────────────────────────────────

/// Returned when the connection was lost while a request was in flight.
pub static ERR_CONNECTION_LOST: Lazy<GatewayDError> = Lazy::new(|| {
    GatewayDError::new(ErrCode::GatewayDisconnected, "connection to gateway was lost during request")
});

// ── Global client registry ────────────────────────────────────────────────────

static CLIENT_REGISTRY: Lazy<RwLock<std::collections::HashMap<String, Arc<Client>>>> =
    Lazy::new(|| RwLock::new(std::collections::HashMap::new()));

static ACTOR_REGISTRY: Lazy<RwLock<std::collections::HashMap<String, Arc<Client>>>> =
    Lazy::new(|| RwLock::new(std::collections::HashMap::new()));

pub async fn get_client_by_gateway_actor_name(actor_name: &str) -> Option<Arc<Client>> {
    ACTOR_REGISTRY.read().await.get(actor_name).cloned()
}

pub async fn get_client_count() -> usize {
    CLIENT_REGISTRY.read().await.len()
}

pub async fn remove_client_by_gateway_actor_name(actor_name: &str) {
    let mut actor_reg = ACTOR_REGISTRY.write().await;
    if let Some(c) = actor_reg.remove(actor_name) {
        CLIENT_REGISTRY.write().await.remove(&c.key);
    }
}

async fn register_client(client: Arc<Client>) -> Result<(), GatewayDError> {
    let key   = client.key.clone();
    let actor = client.cfg.gateway_actor_name.clone();
    CLIENT_REGISTRY.write().await.insert(key, client.clone());
    ACTOR_REGISTRY.write().await.insert(actor, client);
    Ok(())
}

// ── Response channel types ────────────────────────────────────────────────────

type ResponseSender    = oneshot::Sender<Result<Arc<Message>, GatewayDError>>;
type ResponseSenderRaw = oneshot::Sender<Result<(Arc<Message>, Vec<u8>), GatewayDError>>;

// ── Shutdown signal ───────────────────────────────────────────────────────────

/// Sent to the receiver task to request graceful shutdown.
type ShutdownTx = tokio::sync::broadcast::Sender<()>;

// ── Client ────────────────────────────────────────────────────────────────────

pub struct Client {
    conn: Arc<ConnClient>,
    pub cfg:               Config,
    pub gateway_actor_name: String,
    pub client_name:       String,
    key:                   String,

    // Concurrent mode: lock-free pending maps
    pending:     DashMap<String, ResponseSender>,
    pending_raw: DashMap<String, ResponseSenderRaw>,

    receiver_active:   AtomicBool,
    /// std::sync::Mutex so `start_receiver` can lock from sync context.
    receiver_shutdown: StdMutex<Option<ShutdownTx>>,

    // Reconnection state
    reconnecting:      AtomicBool,
    reconnect_attempt: AtomicUsize,

    logger: Arc<dyn Logger>,
}

impl Client {
    // ── Constructor ──────────────────────────────────────────────────────────

    pub async fn new(cfg: Config) -> Result<Arc<Self>, GatewayDError> {
        if cfg.client_name.is_empty() {
            return Err(GatewayDError::new(ErrCode::InvalidConfig, "ClientName must not be empty"));
        }
        if cfg.gateway_actor_name.is_empty() {
            return Err(GatewayDError::new(ErrCode::InvalidConfig, "GatewayActorName must not be empty"));
        }

        let key = format!("{}:{}", cfg.client_name, cfg.gateway_actor_name);
        // Return existing connected client from registry
        if let Some(existing) = CLIENT_REGISTRY.read().await.get(&key).cloned() {
            if existing.is_connected() { return Ok(existing); }
        }

        let logger: Arc<dyn Logger> = cfg.logger.clone().unwrap_or_else(|| {
            if cfg.log_level > 0 {
                TracingLogger::new(Level::from(cfg.log_level))
            } else {
                Arc::new(NoOpLogger)
            }
        });

        let retry = Arc::new(Retry::new(
            cfg.retry_config.retries,
            cfg.retry_config.backoff,
            cfg.retry_config.backoff_multiplier,
            cfg.retry_config.disable_backoff_caps,
        ));

        let conn_cfg = ClientConfig {
            tracer:          cfg.tracer.clone().unwrap_or_else(|| Arc::new(NoOpTracer)),
            logger:          logger.clone(),
            wire_hook:       cfg.wire_hook.clone().unwrap_or_else(|| Arc::new(NoOpWireHook)),
            dial_timeout:    cfg.dial_timeout,
            send_timeout:    cfg.send_timeout,
            receive_timeout: cfg.receive_timeout,
        };

        let conn = ConnClient::connect(
            &cfg.network,
            &cfg.host,
            &cfg.port,
            &cfg.gateway_actor_name,
            retry,
            conn_cfg,
        ).await?;

        let enable_concurrent = cfg.enable_concurrent_mode;
        let streaming_enabled = cfg.streaming_enabled();

        let client = Arc::new(Self {
            conn:                  conn,
            gateway_actor_name:    cfg.gateway_actor_name.clone(),
            client_name:           cfg.client_name.clone(),
            key:                   key,
            cfg,
            pending:               DashMap::new(),
            pending_raw:           DashMap::new(),
            receiver_active:       AtomicBool::new(false),
            receiver_shutdown:     StdMutex::new(None),
            reconnecting:          AtomicBool::new(false),
            reconnect_attempt:     AtomicUsize::new(0),
            logger,
        });

        client.authenticate().await?;

        if streaming_enabled {
            client.send_stream_on().await?;
        }

        if enable_concurrent {
            client.start_receiver();
        }

        register_client(client.clone()).await?;
        Ok(client)
    }

    // ── Authentication ────────────────────────────────────────────────────────

    async fn authenticate(&self) -> Result<(), GatewayDError> {
        let msg = Message {
            envelope: Envelope {
                to:          format!("$system@{}", self.gateway_actor_name),
                from:        format!("{}@{}", self.client_name, self.gateway_actor_name),
                intent:      intents::GATEWAY_ID.clone(),
                client_name: self.client_name.clone(),
                passcode:    self.cfg.passcode.clone(),
                message_id:  Uuid::new_v4().to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let encoded = encode_message(&msg, "")
            .map_err(|e| GatewayDError::new(ErrCode::AuthenticationFailed, format!("encode GatewayId: {e}")))?;
        self.conn.send(encoded.as_bytes()).await
            .map_err(|e| GatewayDError::new(ErrCode::AuthenticationFailed, format!("send GatewayId: {}", e.message)))?;

        let raw = self.conn.receive().await
            .map_err(|e| GatewayDError::new(ErrCode::AuthenticationFailed, format!("receive GatewayId response: {}", e.message)))?;
        let resp = decode_message(&raw)
            .map_err(|e| GatewayDError::new(ErrCode::AuthenticationFailed, format!("decode GatewayId response: {e}")))?;

        if resp.processing_status() == "ERROR" {
            return Err(GatewayDError::new(
                ErrCode::AuthenticationFailed,
                format!("gateway rejected authentication: {}", resp.processing_message()),
            ));
        }
        Ok(())
    }

    async fn send_stream_on(&self) -> Result<(), GatewayDError> {
        let msg = Message {
            envelope: Envelope {
                to:         format!("$system@{}", self.gateway_actor_name),
                from:       format!("{}@{}", self.client_name, self.gateway_actor_name),
                intent:     intents::GATEWAY_STREAM_ON.clone(),
                message_id: Uuid::new_v4().to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let encoded = encode_message(&msg, "")
            .map_err(|e| GatewayDError::new(ErrCode::ClientSendFailed, format!("encode GatewayStreamOn: {e}")))?;
        self.conn.send(encoded.as_bytes()).await
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Send a message and await its response.
    pub async fn send_message(&self, msg: &mut Message) -> Result<Arc<Message>, GatewayDError> {
        self.autocorrect_envelope(msg);
        if self.receiver_active.load(Ordering::Acquire) {
            self.send_concurrent(msg).await
        } else {
            self.send_sync(msg).await.map(Arc::new)
        }
    }

    /// Same as `send_message` but also returns the raw wire bytes.
    pub async fn send_message_with_raw(&self, msg: &mut Message) -> Result<(Arc<Message>, Vec<u8>), GatewayDError> {
        self.autocorrect_envelope(msg);
        if self.receiver_active.load(Ordering::Acquire) {
            self.send_concurrent_raw(msg).await
        } else {
            let (m, raw) = self.send_sync_with_raw(msg).await?;
            Ok((Arc::new(m), raw))
        }
    }

    /// Send a pre-encoded control message directly.
    pub async fn send_control_message(&self, msg: &SocketMessage) -> Result<(), GatewayDError> {
        self.conn.send(msg.as_bytes()).await
    }

    pub fn is_connected(&self)     -> bool  { self.conn.is_connected() }
    pub fn client_name(&self)      -> &str  { &self.client_name }
    pub fn actor_name(&self)       -> &str  { &self.gateway_actor_name }
    pub fn is_reconnecting(&self)  -> bool  { self.reconnecting.load(Ordering::Acquire) }
    pub fn reconnect_attempt(&self) -> usize { self.reconnect_attempt.load(Ordering::Acquire) }
    pub fn is_receiver_active(&self) -> bool { self.receiver_active.load(Ordering::Acquire) }

    pub async fn close(&self) -> Result<(), GatewayDError> {
        self.stop_receiver();
        remove_client_by_gateway_actor_name(&self.gateway_actor_name).await;
        self.conn.close().await;
        Ok(())
    }

    // ── Receiver management ───────────────────────────────────────────────────

    /// Start the background receiver task.
    pub fn start_receiver(self: &Arc<Self>) {
        if self.receiver_active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return; // already running
        }
        let (tx, _) = tokio::sync::broadcast::channel::<()>(1);
        let rx = tx.subscribe();
        {
            let mut guard = self.receiver_shutdown.lock().expect("receiver_shutdown poisoned");
            *guard = Some(tx);
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            if let Some(client) = weak.upgrade() {
                client.receive_loop(rx).await;
            }
        });
    }

    /// Stop the background receiver task.
    pub fn stop_receiver(&self) {
        let tx = {
            let mut guard = self.receiver_shutdown.lock().expect("receiver_shutdown poisoned");
            guard.take()
        };
        if let Some(tx) = tx { let _ = tx.send(()); }
        self.pending.clear();
        self.pending_raw.clear();
        self.receiver_active.store(false, Ordering::Release);
    }

    // ── Internal send paths ───────────────────────────────────────────────────

    fn autocorrect_envelope(&self, msg: &mut Message) {
        if msg.envelope.client_name.is_empty() {
            msg.envelope.client_name = self.client_name.clone();
        }
        if msg.envelope.from.is_empty() {
            msg.envelope.from = format!("{}@{}", self.client_name, self.gateway_actor_name);
        }
        if msg.envelope.message_id.is_empty() {
            msg.envelope.message_id = Uuid::new_v4().to_string();
        }
    }

    async fn send_sync(&self, msg: &Message) -> Result<Message, GatewayDError> {
        let encoded = encode_message(msg, "")
            .map_err(|e| GatewayDError::new(ErrCode::ClientSendFailed, format!("encode: {e}")))?;
        self.conn.send(encoded.as_bytes()).await?;
        let raw = self.conn.receive().await?;
        decode_message(&raw)
            .map_err(|e| GatewayDError::new(ErrCode::InvalidResponse, format!("decode: {e}")))
    }

    async fn send_sync_with_raw(&self, msg: &Message) -> Result<(Message, Vec<u8>), GatewayDError> {
        let encoded = encode_message(msg, "")
            .map_err(|e| GatewayDError::new(ErrCode::ClientSendFailed, format!("encode: {e}")))?;
        self.conn.send(encoded.as_bytes()).await?;
        let raw = self.conn.receive().await?;
        let decoded = decode_message(&raw)
            .map_err(|e| GatewayDError::new(ErrCode::InvalidResponse, format!("decode: {e}")))?;
        Ok((decoded, raw))
    }

    async fn send_concurrent(&self, msg: &Message) -> Result<Arc<Message>, GatewayDError> {
        let id = msg.envelope.message_id.clone();
        let (tx, rx) = oneshot::channel::<Result<Arc<Message>, GatewayDError>>();
        self.pending.insert(id.clone(), tx);

        let encoded = match encode_message(msg, "") {
            Ok(e)  => e,
            Err(e) => {
                self.pending.remove(&id);
                return Err(GatewayDError::new(ErrCode::ClientSendFailed, format!("encode: {e}")));
            }
        };
        if let Err(e) = self.conn.send(encoded.as_bytes()).await {
            self.pending.remove(&id);
            return Err(e);
        }

        tokio::time::timeout(self.cfg.response_timeout, rx)
            .await
            .map_err(|_| { self.pending.remove(&id); GatewayDError::new(ErrCode::GatewayTimeout, "response timeout") })?
            .map_err(|_| GatewayDError::new(ErrCode::GatewayDisconnected, "response channel dropped"))?
    }

    async fn send_concurrent_raw(&self, msg: &Message) -> Result<(Arc<Message>, Vec<u8>), GatewayDError> {
        let id = msg.envelope.message_id.clone();
        let (tx, rx) = oneshot::channel::<Result<(Arc<Message>, Vec<u8>), GatewayDError>>();
        self.pending_raw.insert(id.clone(), tx);

        let encoded = match encode_message(msg, "") {
            Ok(e)  => e,
            Err(e) => {
                self.pending_raw.remove(&id);
                return Err(GatewayDError::new(ErrCode::ClientSendFailed, format!("encode: {e}")));
            }
        };
        if let Err(e) = self.conn.send(encoded.as_bytes()).await {
            self.pending_raw.remove(&id);
            return Err(e);
        }

        tokio::time::timeout(self.cfg.response_timeout, rx)
            .await
            .map_err(|_| { self.pending_raw.remove(&id); GatewayDError::new(ErrCode::GatewayTimeout, "response timeout") })?
            .map_err(|_| GatewayDError::new(ErrCode::GatewayDisconnected, "response channel dropped"))?
    }

    // ── Receiver loop ─────────────────────────────────────────────────────────

    async fn receive_loop(
        self: &Arc<Self>,
        mut shutdown: tokio::sync::broadcast::Receiver<()>,
    ) {
        loop {
            let raw = tokio::select! {
                _ = shutdown.recv() => break,
                result = self.conn.receive() => result,
            };

            match raw {
                Err(ref e) if is_timeout_error(&e.message) => continue,

                Err(ref e) if is_connection_error(&e.message) => {
                    self.logger.warn("connection lost in receiver", &[("error", &e.message)]);
                    self.pending.clear();
                    self.pending_raw.clear();
                    if self.cfg.reconnect_config.is_enabled() {
                        let arc = self.clone();
                        tokio::spawn(async move { arc.attempt_reconnection().await; });
                    }
                    break;
                }

                Err(ref e) => {
                    self.logger.error("receive error (non-fatal)", &[("error", &e.message)]);
                    continue;
                }

                Ok(raw) => {
                    match decode_message(&raw) {
                        Err(e) => {
                            self.logger.error("decode error", &[("error", &e.to_string())]);
                        }
                        Ok(msg) => {
                            let msg_id  = msg.envelope.message_id.clone();
                            let arc_msg = Arc::new(msg);

                            if let Some((_, tx)) = self.pending.remove(&msg_id) {
                                let _ = tx.send(Ok(arc_msg));
                            } else if let Some((_, tx)) = self.pending_raw.remove(&msg_id) {
                                let _ = tx.send(Ok((arc_msg, raw)));
                            }
                            // Unmatched push messages are silently dropped
                        }
                    }
                }
            }
        }
        self.receiver_active.store(false, Ordering::Release);
    }

    // ── Reconnection ──────────────────────────────────────────────────────────

    async fn attempt_reconnection(self: &Arc<Self>) {
        if !self.cfg.reconnect_config.is_enabled() { return; }
        self.reconnecting.store(true, Ordering::Release);

        let rc  = &self.cfg.reconnect_config;
        let max = rc.max_retries;
        let mut delay_secs = rc.initial_backoff().as_secs_f64();
        let max_secs = rc.max_backoff().as_secs_f64();
        let mult = rc.backoff_multiplier();

        for attempt in 0.. {
            if max > 0 && attempt >= max {
                self.logger.error("reconnection: max retries exhausted", &[("max", &max)]);
                break;
            }
            self.reconnect_attempt.store(attempt + 1, Ordering::Release);

            tokio::time::sleep(Duration::from_secs_f64(delay_secs)).await;

            match self.conn.reconnect().await {
                Ok(()) => {
                    match self.re_authenticate().await {
                        Err(e) => {
                            self.logger.error("re-authentication failed", &[("error", &e.message)]);
                        }
                        Ok(()) => {
                            self.reconnecting.store(false, Ordering::Release);
                            self.reconnect_attempt.store(0, Ordering::Release);
                            self.start_receiver();
                            return;
                        }
                    }
                }
                Err(e) => {
                    self.logger.warn("reconnect attempt failed", &[("error", &e.message)]);
                }
            }

            delay_secs = (delay_secs * mult).min(max_secs);
        }
        self.reconnecting.store(false, Ordering::Release);
    }

    async fn re_authenticate(&self) -> Result<(), GatewayDError> {
        self.authenticate().await?;
        if self.cfg.streaming_enabled() {
            self.send_stream_on().await?;
        }
        Ok(())
    }
}

// ── Error classification ──────────────────────────────────────────────────────

pub fn is_timeout_error(s: &str) -> bool {
    let lo = s.to_lowercase();
    lo.contains("timeout") || lo.contains("deadline exceeded") || lo.contains("i/o timeout")
}

pub fn is_connection_error(s: &str) -> bool {
    let lo = s.to_lowercase();
    lo.contains("eof")
        || lo.contains("connection reset")
        || lo.contains("broken pipe")
        || lo.contains("connection refused")
        || lo.contains("connection closed")
        || lo.contains("use of closed network")
        || lo.contains("forcibly closed")
}
