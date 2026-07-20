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
use tokio::sync::{broadcast, oneshot, Notify, RwLock};
use uuid::Uuid;

use crate::{
    config::Config,
    connection::{
        client::{Client as ConnClient, ClientConfig},
        pool::{ChannelPool, ConnectionData, ConnectionFactory},
        retry::Retry,
        traits::{NoOpTracer, NoOpWireHook},
    },
    errors::{ErrCode, GatewayDError},
    log::{Level, Logger, NoOpLogger, TracingLogger},
    message::{
        decode_message, encode_message, intents,
        types::{Envelope, Message, SocketMessage},
    },
};

/// Liveness backstop fallback when config is unavailable.
const DEFAULT_CONNECTION_LIVENESS_TIMEOUT: Duration = Duration::from_secs(90);

// ── Connection state ──────────────────────────────────────────────────────────

/// Represents the current state of a [`Client`]'s connection.
/// Mirrors Go's `ConnectionState` type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connection is active (emitted after successful reconnect).
    Connected,
    /// Connection was lost (error is the cause).
    Disconnected,
    /// Reconnect attempt starting (error is the trigger that caused disconnect).
    Reconnecting,
    /// All reconnect attempts exhausted (error is the last failure).
    ReconnectFailed,
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected => write!(f, "connected"),
            Self::Disconnected => write!(f, "disconnected"),
            Self::Reconnecting => write!(f, "reconnecting"),
            Self::ReconnectFailed => write!(f, "reconnect_failed"),
        }
    }
}

// ── Error sentinel ────────────────────────────────────────────────────────────

/// Returned when the connection was lost while a request was in flight.
pub static ERR_CONNECTION_LOST: Lazy<GatewayDError> = Lazy::new(|| {
    GatewayDError::new(
        ErrCode::GatewayDisconnected,
        "connection to gateway was lost during request",
    )
});

// ── Global client registry ────────────────────────────────────────────────────

static CLIENT_REGISTRY: Lazy<RwLock<std::collections::HashMap<String, Arc<Client>>>> =
    Lazy::new(|| RwLock::new(std::collections::HashMap::new()));

static ACTOR_REGISTRY: Lazy<RwLock<std::collections::HashMap<String, Arc<Client>>>> =
    Lazy::new(|| RwLock::new(std::collections::HashMap::new()));

pub async fn get_client_by_gateway_actor_name(actor_name: &str) -> Option<Arc<Client>> {
    let reg = ACTOR_REGISTRY.read().await;
    reg.values()
        .find(|c| c.gateway_actor_name == actor_name)
        .cloned()
}

pub async fn get_client_count() -> usize {
    CLIENT_REGISTRY.read().await.len()
}

pub async fn remove_client_by_gateway_actor_name(actor_name: &str) {
    let mut actor_reg = ACTOR_REGISTRY.write().await;
    let keys_to_remove: Vec<String> = actor_reg
        .iter()
        .filter(|(_, c)| c.gateway_actor_name == actor_name)
        .map(|(k, _)| k.clone())
        .collect();
    for key in &keys_to_remove {
        actor_reg.remove(key);
    }
    drop(actor_reg);
    let mut client_reg = CLIENT_REGISTRY.write().await;
    for key in &keys_to_remove {
        client_reg.remove(key);
    }
}

async fn register_client(client: Arc<Client>) -> Result<(), GatewayDError> {
    let key = client.key.clone();
    CLIENT_REGISTRY.write().await.insert(key.clone(), client.clone());
    ACTOR_REGISTRY.write().await.insert(key, client);
    Ok(())
}

// ── Response channel types ────────────────────────────────────────────────────

type ResponseSender = oneshot::Sender<Result<Arc<Message>, GatewayDError>>;
type ResponseSenderRaw = oneshot::Sender<Result<(Arc<Message>, Vec<u8>), GatewayDError>>;

// ── Shutdown signal ───────────────────────────────────────────────────────────

/// Sent to the receiver task to request graceful shutdown.
type ShutdownTx = broadcast::Sender<()>;

/// Default capacity of the unsolicited-message broadcast channel.
const INCOMING_CHANNEL_CAPACITY: usize = 64;

// ── Client ────────────────────────────────────────────────────────────────────

pub struct Client {
    conn: Arc<ConnClient>,
    pool: Option<Arc<ChannelPool>>,
    pub cfg: Config,
    pub gateway_actor_name: String,
    pub client_name: String,
    key: String,

    // Concurrent mode: lock-free pending maps
    pending: DashMap<String, ResponseSender>,
    pending_raw: DashMap<String, ResponseSenderRaw>,

    /// Broadcast channel for unsolicited (push) messages from the gateway.
    /// Subscribe with `subscribe_incoming()` to receive actor requests.
    incoming_tx: broadcast::Sender<Arc<Message>>,

    receiver_active: AtomicBool,
    /// std::sync::Mutex so `start_receiver` can lock from sync context.
    receiver_shutdown: StdMutex<Option<ShutdownTx>>,
    keepalive_shutdown: StdMutex<Option<ShutdownTx>>,

    // Reconnection state
    reconnecting: AtomicBool,
    reconnect_attempt: AtomicUsize,

    // Shutdown guard — prevents reconnection after close()
    closed: AtomicBool,

    // Reconnect completion notification
    reconnect_notify: Notify,

    // Connection state observer
    state_handler: StdMutex<Option<Box<dyn Fn(ConnectionState, Option<&GatewayDError>) + Send + Sync>>>,

    logger: Arc<dyn Logger>,
}

impl Client {
    // ── Constructor ──────────────────────────────────────────────────────────

    pub async fn new(cfg: Config) -> Result<Arc<Self>, GatewayDError> {
        if cfg.client_name.is_empty() {
            return Err(GatewayDError::new(
                ErrCode::InvalidConfig,
                "ClientName must not be empty",
            ));
        }
        if cfg.gateway_actor_name.is_empty() {
            return Err(GatewayDError::new(
                ErrCode::InvalidConfig,
                "GatewayActorName must not be empty",
            ));
        }

        if cfg.host.is_empty() {
            return Err(GatewayDError::new(
                ErrCode::InvalidConfig,
                "Host must not be empty",
            ));
        }
        if cfg.port.is_empty() {
            return Err(GatewayDError::new(
                ErrCode::InvalidConfig,
                "Port must not be empty",
            ));
        }

        let key = format!("{}:{}", cfg.client_name, cfg.gateway_actor_name);
        if !cfg.skip_global_registry {
            // Return existing connected client, or remove stale disconnected entry
            if let Some(existing) = CLIENT_REGISTRY.read().await.get(&key).cloned() {
                if existing.is_connected() {
                    return Ok(existing);
                }
                CLIENT_REGISTRY.write().await.remove(&key);
                ACTOR_REGISTRY.write().await.remove(&key);
            }
        }

        let logger: Arc<dyn Logger> = cfg.logger.clone().unwrap_or_else(|| {
            if cfg.log_level > 0 {
                TracingLogger::build(Level::from(cfg.log_level))
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
            tracer: cfg.tracer.clone().unwrap_or_else(|| Arc::new(NoOpTracer)),
            logger: logger.clone(),
            wire_hook: cfg
                .wire_hook
                .clone()
                .unwrap_or_else(|| Arc::new(NoOpWireHook)),
            dial_timeout: cfg.dial_timeout,
            send_timeout: cfg.send_timeout,
            receive_timeout: cfg.receive_timeout,
            tcp_keep_alive_idle: cfg.tcp_keep_alive_idle,
            tcp_keep_alive_interval: cfg.tcp_keep_alive_interval,
            tcp_keep_alive_count: cfg.tcp_keep_alive_count,
            tcp_user_timeout: cfg.tcp_user_timeout,
        };

        let conn = ConnClient::connect(
            &cfg.network,
            &cfg.host,
            &cfg.port,
            &cfg.gateway_actor_name,
            retry,
            conn_cfg,
        )
        .await?;

        let enable_concurrent = cfg.enable_concurrent_mode;
        let streaming_enabled = cfg.streaming_enabled();

        let (incoming_tx, _) = broadcast::channel::<Arc<Message>>(INCOMING_CHANNEL_CAPACITY);

        let pool = if cfg.pool_config.max_capacity > 0 {
            let host = cfg.host.clone();
            let port = cfg.port.clone();
            let dial_timeout = cfg.dial_timeout;
            let factory: ConnectionFactory = Arc::new(move || {
                let host = host.clone();
                let port = port.clone();
                Box::pin(async move {
                    let addr = format!("{host}:{port}");
                    let stream = tokio::time::timeout(dial_timeout, tokio::net::TcpStream::connect(&addr))
                        .await
                        .map_err(|_| {
                            GatewayDError::new(
                                ErrCode::PoolInitializationFailed,
                                format!("pool dial timeout: {addr}"),
                            )
                        })?
                        .map_err(|e| {
                            GatewayDError::new(
                                ErrCode::PoolInitializationFailed,
                                format!("pool dial failed: {e}"),
                            )
                        })?;
                    Ok(ConnectionData::new(stream))
                })
            });
            let pool = ChannelPool::new(cfg.pool_config.max_capacity, factory);
            pool.initialize(cfg.pool_config.initial_capacity).await?;
            Some(pool)
        } else {
            None
        };

        let client = Arc::new(Self {
            conn,
            pool,
            gateway_actor_name: cfg.gateway_actor_name.clone(),
            client_name: cfg.client_name.clone(),
            key,
            cfg,
            pending: DashMap::new(),
            pending_raw: DashMap::new(),
            incoming_tx,
            receiver_active: AtomicBool::new(false),
            receiver_shutdown: StdMutex::new(None),
            keepalive_shutdown: StdMutex::new(None),
            reconnecting: AtomicBool::new(false),
            reconnect_attempt: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            reconnect_notify: Notify::new(),
            state_handler: StdMutex::new(None),
            logger,
        });

        client.authenticate().await?;

        if streaming_enabled {
            client.send_stream_on().await?;
        }

        if enable_concurrent {
            client.start_receiver();
        }

        client.start_keepalive_loop();

        if client.cfg.skip_global_registry {
            client.logger.info(
                "created unregistered client (warm-pool standby)",
                &[("key", &client.key)],
            );
            return Ok(client);
        }

        register_client(client.clone()).await?;
        Ok(client)
    }

    // ── Authentication ────────────────────────────────────────────────────────

    async fn authenticate(&self) -> Result<(), GatewayDError> {
        let msg = Message {
            envelope: Envelope {
                to: format!("$system@{}", self.gateway_actor_name),
                from: format!("{}@{}", self.client_name, self.gateway_actor_name),
                intent: intents::GATEWAY_ID.clone(),
                client_name: self.client_name.clone(),
                passcode: self.cfg.passcode.clone(),
                message_id: Uuid::new_v4().to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let encoded = encode_message(&msg, "").map_err(|e| {
            GatewayDError::new(
                ErrCode::AuthenticationFailed,
                format!("encode GatewayId: {e}"),
            )
        })?;
        self.conn.send(encoded.as_bytes()).await.map_err(|e| {
            GatewayDError::new(
                ErrCode::AuthenticationFailed,
                format!("send GatewayId: {}", e.message),
            )
        })?;

        let raw = self.conn.receive().await.map_err(|e| {
            GatewayDError::new(
                ErrCode::AuthenticationFailed,
                format!("receive GatewayId response: {}", e.message),
            )
        })?;
        let resp = decode_message(&raw).map_err(|e| {
            GatewayDError::new(
                ErrCode::AuthenticationFailed,
                format!("decode GatewayId response: {e}"),
            )
        })?;

        if resp.processing_status() == "ERROR" {
            return Err(GatewayDError::new(
                ErrCode::AuthenticationFailed,
                format!(
                    "gateway rejected authentication: {}",
                    resp.processing_message()
                ),
            ));
        }
        Ok(())
    }

    async fn send_stream_on(&self) -> Result<(), GatewayDError> {
        let msg = Message {
            envelope: Envelope {
                to: format!("$system@{}", self.gateway_actor_name),
                from: format!("{}@{}", self.client_name, self.gateway_actor_name),
                intent: intents::GATEWAY_STREAM_ON.clone(),
                message_id: Uuid::new_v4().to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let encoded = encode_message(&msg, "").map_err(|e| {
            GatewayDError::new(
                ErrCode::ClientSendFailed,
                format!("encode GatewayStreamOn: {e}"),
            )
        })?;
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
    pub async fn send_message_with_raw(
        &self,
        msg: &mut Message,
    ) -> Result<(Arc<Message>, Vec<u8>), GatewayDError> {
        self.autocorrect_envelope(msg);
        if self.receiver_active.load(Ordering::Acquire) {
            self.send_concurrent_raw(msg).await
        } else {
            let (m, raw) = self.send_sync_with_raw(msg).await?;
            Ok((Arc::new(m), raw))
        }
    }

    /// Send a message without waiting for a response.
    ///
    /// Use this for fire-and-forget messages such as `ACTOR_RESPONSE` where
    /// the gateway routes the message to the target actor but does not send
    /// a delivery receipt back to the sender.
    pub async fn send_without_response(&self, msg: &mut Message) -> Result<(), GatewayDError> {
        self.autocorrect_envelope(msg);
        let encoded = encode_message(msg, "")
            .map_err(|e| GatewayDError::new(ErrCode::ClientSendFailed, format!("encode: {e}")))?;
        self.conn.send(encoded.as_bytes()).await
    }

    /// Send a pre-encoded control message directly.
    pub async fn send_control_message(&self, msg: &SocketMessage) -> Result<(), GatewayDError> {
        self.conn.send(msg.as_bytes()).await
    }

    /// Send an app-level AIP Keepalive (message_type 18) on the primary connection.
    pub async fn send_keepalive(&self) -> Result<(), GatewayDError> {
        let mut msg = Message {
            envelope: Envelope {
                to: format!("$system@{}", self.gateway_actor_name),
                from: format!("{}@{}", self.client_name, self.gateway_actor_name),
                intent: intents::KEEPALIVE.clone(),
                client_name: self.client_name.clone(),
                message_id: Uuid::new_v4().to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        self.send_without_response(&mut msg).await
    }

    /// Send an app-level AIP GatewayDisconnect (message_type 6) on the primary connection.
    pub async fn send_disconnect(&self) -> Result<(), GatewayDError> {
        if !self.is_connected() {
            return Ok(());
        }
        let mut msg = Message {
            envelope: Envelope {
                to: format!("$system@{}", self.gateway_actor_name),
                from: format!("{}@{}", self.client_name, self.gateway_actor_name),
                intent: intents::GATEWAY_DISCONNECT.clone(),
                client_name: self.client_name.clone(),
                message_id: Uuid::new_v4().to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        self.send_without_response(&mut msg).await
    }

    pub fn is_connected(&self) -> bool {
        self.conn.is_connected()
    }
    pub fn client_name(&self) -> &str {
        &self.client_name
    }
    pub fn actor_name(&self) -> &str {
        &self.gateway_actor_name
    }
    pub fn is_reconnecting(&self) -> bool {
        self.reconnecting.load(Ordering::Acquire)
    }
    pub fn reconnect_attempt(&self) -> usize {
        self.reconnect_attempt.load(Ordering::Acquire)
    }
    pub fn is_receiver_active(&self) -> bool {
        self.receiver_active.load(Ordering::Acquire)
    }

    /// Registers a callback that fires on every connection state transition.
    ///
    /// The error parameter is:
    /// - [`ConnectionState::Disconnected`]: the error that caused the disconnect.
    /// - [`ConnectionState::Reconnecting`]: the trigger error (may be `None`).
    /// - [`ConnectionState::Connected`]: `None` (reconnect succeeded).
    /// - [`ConnectionState::ReconnectFailed`]: the last reconnect attempt error.
    ///
    /// Note: [`ConnectionState::Connected`] is not emitted for the initial connection
    /// because no handler can be registered before the constructor returns.
    ///
    /// The callback is invoked synchronously so it should be fast and non-blocking.
    pub fn on_connection_state_change(
        &self,
        f: impl Fn(ConnectionState, Option<&GatewayDError>) + Send + Sync + 'static,
    ) {
        let mut guard = self.state_handler.lock().expect("state_handler poisoned");
        *guard = Some(Box::new(f));
    }

    fn emit_state(&self, state: ConnectionState, err: Option<&GatewayDError>) {
        let guard = self.state_handler.lock().expect("state_handler poisoned");
        if let Some(ref f) = *guard {
            f(state, err);
        }
    }

    /// Subscribe to unsolicited (push) messages from the gateway.
    ///
    /// Returns a [`broadcast::Receiver`] that receives every inbound message
    /// that is *not* a response to a pending `send_message` call (i.e. actor
    /// requests initiated by a remote peer).  Multiple subscribers are
    /// supported; each gets its own independent copy.
    ///
    /// **Note on timed-out requests:** When `send_concurrent` times out,
    /// the pending entry is removed.  If the gateway replies *after* the
    /// timeout, the response will arrive here as an unsolicited message.
    /// Subscribers should be prepared for this.
    ///
    /// This requires the background receiver to be running
    /// (`enable_concurrent_mode: true` or manual `start_receiver()` call).
    pub fn subscribe_incoming(&self) -> broadcast::Receiver<Arc<Message>> {
        self.incoming_tx.subscribe()
    }

    /// Blocks until the client is connected or the reconnect attempt finishes.
    /// Returns `true` if the connection was restored, `false` otherwise.
    async fn wait_for_reconnect(&self) -> bool {
        if self.is_connected() {
            return true;
        }
        if self.closed.load(Ordering::Acquire) {
            return false;
        }
        if !self.reconnecting.load(Ordering::Acquire) {
            return false;
        }
        self.reconnect_notify.notified().await;
        self.is_connected() && !self.closed.load(Ordering::Acquire)
    }

    pub async fn close(&self) -> Result<(), GatewayDError> {
        self.closed.store(true, Ordering::Release);
        self.reconnect_notify.notify_waiters();
        self.stop_receiver();
        self.stop_keepalive_loop();
        remove_client_by_gateway_actor_name(&self.gateway_actor_name).await;
        if let Some(pool) = &self.pool {
            pool.close().await;
        }
        if let Err(e) = self.send_disconnect().await {
            self.logger.warn(
                "failed to send GatewayDisconnect before close",
                &[("error", &e.message), ("actor", &self.gateway_actor_name)],
            );
        }
        self.conn.close().await;
        Ok(())
    }

    fn start_keepalive_loop(self: &Arc<Self>) {
        let interval = self.cfg.keepalive_interval();
        if interval.is_zero() {
            return;
        }

        let (tx, mut rx) = broadcast::channel::<()>(1);
        {
            let mut guard = self
                .keepalive_shutdown
                .lock()
                .expect("keepalive_shutdown poisoned");
            *guard = Some(tx);
        }

        let this = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    result = rx.recv() => {
                        if result.is_err() {
                            break;
                        }
                    }
                    _ = ticker.tick() => {
                        if this.closed.load(Ordering::Acquire)
                            || !this.is_connected()
                            || this.is_reconnecting()
                        {
                            continue;
                        }
                        if let Err(e) = this.send_keepalive().await {
                            this.logger.debug(
                                "keepalive send failed",
                                &[("actor", &this.gateway_actor_name), ("error", &e.message)],
                            );
                        }
                        if let Some(pool) = &this.pool {
                            this.send_pool_keepalives(pool).await;
                        }
                    }
                }
            }
        });
    }

    fn stop_keepalive_loop(&self) {
        let tx = {
            let mut guard = self
                .keepalive_shutdown
                .lock()
                .expect("keepalive_shutdown poisoned");
            guard.take()
        };
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
    }

    async fn send_pool_keepalives(&self, pool: &ChannelPool) {
        let msg = Message {
            envelope: Envelope {
                to: format!("$system@{}", self.gateway_actor_name),
                from: format!("{}@{}", self.client_name, self.gateway_actor_name),
                intent: intents::KEEPALIVE.clone(),
                client_name: self.client_name.clone(),
                message_id: Uuid::new_v4().to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let wire = match encode_message(&msg, "") {
            Ok(encoded) => encoded.into_bytes(),
            Err(e) => {
                self.logger.debug(
                    "encode pool keepalive failed",
                    &[("error", &format!("{e}"))],
                );
                return;
            }
        };

        let _ = pool.ping_idle_connections(&wire).await;
    }

    // ── Receiver management ───────────────────────────────────────────────────

    /// Start the background receiver task.
    pub fn start_receiver(self: &Arc<Self>) {
        if self
            .receiver_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return; // already running
        }
        let (tx, _) = tokio::sync::broadcast::channel::<()>(1);
        let rx = tx.subscribe();
        {
            let mut guard = self
                .receiver_shutdown
                .lock()
                .expect("receiver_shutdown poisoned");
            *guard = Some(tx);
        }
        let this = self.clone();
        tokio::spawn(async move {
            this.receive_loop(rx).await;
        });
    }

    /// Stop the background receiver task.
    pub fn stop_receiver(&self) {
        let tx = {
            let mut guard = self
                .receiver_shutdown
                .lock()
                .expect("receiver_shutdown poisoned");
            guard.take()
        };
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
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
        match self.do_send_sync(msg).await {
            Err(ref e)
                if self.cfg.reconnect_config.is_enabled()
                    && e.is_connection_lost() =>
            {
                self.logger.info(
                    "sync send failed with connection error, attempting reconnection",
                    &[("error", &e.message)],
                );
                if self.attempt_reconnection_sync(Some(e)).await {
                    self.do_send_sync(msg).await
                } else {
                    Err(GatewayDError::new(
                        ErrCode::GatewayDisconnected,
                        "reconnection failed after sync send error",
                    ))
                }
            }
            other => other,
        }
    }

    async fn do_send_sync(&self, msg: &Message) -> Result<Message, GatewayDError> {
        let encoded = encode_message(msg, "")
            .map_err(|e| GatewayDError::new(ErrCode::ClientSendFailed, format!("encode: {e}")))?;
        self.conn.send(encoded.as_bytes()).await?;
        let raw = self.conn.receive().await?;
        decode_message(&raw)
            .map_err(|e| GatewayDError::new(ErrCode::InvalidResponse, format!("decode: {e}")))
    }

    async fn send_sync_with_raw(&self, msg: &Message) -> Result<(Message, Vec<u8>), GatewayDError> {
        match self.do_send_sync_with_raw(msg).await {
            Err(ref e)
                if self.cfg.reconnect_config.is_enabled()
                    && e.is_connection_lost() =>
            {
                self.logger.info(
                    "sync send (raw) failed with connection error, attempting reconnection",
                    &[("error", &e.message)],
                );
                if self.attempt_reconnection_sync(Some(e)).await {
                    self.do_send_sync_with_raw(msg).await
                } else {
                    Err(GatewayDError::new(
                        ErrCode::GatewayDisconnected,
                        "reconnection failed after sync send error",
                    ))
                }
            }
            other => other,
        }
    }

    async fn do_send_sync_with_raw(&self, msg: &Message) -> Result<(Message, Vec<u8>), GatewayDError> {
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

        if !self.conn.is_connected() && self.cfg.reconnect_config.is_enabled() {
            if !self.wait_for_reconnect().await {
                self.pending.remove(&id);
                return Err(GatewayDError::new(
                    ErrCode::GatewayDisconnected,
                    "connection to gateway was lost during request",
                ));
            }
        }

        let encoded = match encode_message(msg, "") {
            Ok(e) => e,
            Err(e) => {
                self.pending.remove(&id);
                return Err(GatewayDError::new(
                    ErrCode::ClientSendFailed,
                    format!("encode: {e}"),
                ));
            }
        };
        if let Err(e) = self.conn.send(encoded.as_bytes()).await {
            self.pending.remove(&id);
            return Err(e);
        }

        tokio::time::timeout(self.cfg.response_timeout, rx)
            .await
            .map_err(|_| {
                self.pending.remove(&id);
                GatewayDError::new(ErrCode::GatewayTimeout, "response timeout")
            })?
            .map_err(|_| {
                GatewayDError::new(
                    ErrCode::GatewayDisconnected,
                    "response channel closed: sender dropped (connection lost or receiver stopped)",
                )
            })?
    }

    async fn send_concurrent_raw(
        &self,
        msg: &Message,
    ) -> Result<(Arc<Message>, Vec<u8>), GatewayDError> {
        let id = msg.envelope.message_id.clone();
        let (tx, rx) = oneshot::channel::<Result<(Arc<Message>, Vec<u8>), GatewayDError>>();
        self.pending_raw.insert(id.clone(), tx);

        if !self.conn.is_connected() && self.cfg.reconnect_config.is_enabled() {
            if !self.wait_for_reconnect().await {
                self.pending_raw.remove(&id);
                return Err(GatewayDError::new(
                    ErrCode::GatewayDisconnected,
                    "connection to gateway was lost during request",
                ));
            }
        }

        let encoded = match encode_message(msg, "") {
            Ok(e) => e,
            Err(e) => {
                self.pending_raw.remove(&id);
                return Err(GatewayDError::new(
                    ErrCode::ClientSendFailed,
                    format!("encode: {e}"),
                ));
            }
        };
        if let Err(e) = self.conn.send(encoded.as_bytes()).await {
            self.pending_raw.remove(&id);
            return Err(e);
        }

        tokio::time::timeout(self.cfg.response_timeout, rx)
            .await
            .map_err(|_| {
                self.pending_raw.remove(&id);
                GatewayDError::new(ErrCode::GatewayTimeout, "response timeout")
            })?
            .map_err(|_| {
                GatewayDError::new(
                    ErrCode::GatewayDisconnected,
                    "response channel closed: sender dropped (connection lost or receiver stopped)",
                )
            })?
    }

    // ── Receiver loop ─────────────────────────────────────────────────────────

    /// Emit the disconnected state, fail every in-flight caller immediately with a
    /// retryable ConnectionLost error, then spawn a reconnect.
    fn handle_connection_lost(self: &Arc<Self>, err: &GatewayDError) {
        self.emit_state(ConnectionState::Disconnected, Some(err));
        self.fail_all_pending();
        if self.cfg.reconnect_config.is_enabled() {
            let arc = self.clone();
            let trigger = GatewayDError::new(err.code, err.message.clone());
            tokio::spawn(async move {
                arc.attempt_reconnection(Some(&trigger)).await;
            });
        }
    }

    /// Resolve all pending requests with a retryable ConnectionLost error so
    /// callers fail fast instead of blocking until their own response timeout.
    fn fail_all_pending(&self) {
        let lost = || {
            GatewayDError::new(
                ErrCode::ConnectionLost,
                "connection to gateway was lost during request",
            )
        };
        let keys: Vec<String> = self.pending.iter().map(|e| e.key().clone()).collect();
        for k in keys {
            if let Some((_, tx)) = self.pending.remove(&k) {
                let _ = tx.send(Err(lost()));
            }
        }
        let raw_keys: Vec<String> = self.pending_raw.iter().map(|e| e.key().clone()).collect();
        for k in raw_keys {
            if let Some((_, tx)) = self.pending_raw.remove(&k) {
                let _ = tx.send(Err(lost()));
            }
        }
    }

    async fn receive_loop(self: &Arc<Self>, mut shutdown: tokio::sync::broadcast::Receiver<()>) {
        // Liveness backstop: if requests are pending but no frame arrives for
        // this long, the connection is dead even without a hard TCP error.
        let mut last_activity = std::time::Instant::now();
        let loop_timeout = self.cfg.receive_loop_timeout();
        let liveness_timeout = self.cfg.connection_liveness_timeout();
        loop {
            let raw = tokio::select! {
                _ = shutdown.recv() => break,
                result = self.conn.receive_with_timeout(loop_timeout) => result,
            };

            match raw {
                // Benign idle timeout: still alive unless we have pending requests
                // and have heard nothing for too long (liveness backstop).
                Err(ref e) if e.is_idle_timeout() => {
                    let pending = self.pending.len() + self.pending_raw.len();
                    if pending == 0 || last_activity.elapsed() <= liveness_timeout {
                        continue;
                    }
                    self.logger.error(
                        "liveness timeout: pending requests with no frames received; treating connection as dead",
                        &[("error", &e.message)],
                    );
                    self.handle_connection_lost(e);
                    break;
                }

                // Everything else from the hardened transport is fatal: fail all
                // in-flight callers fast, then reconnect.
                Err(ref e) => {
                    self.logger
                        .warn("connection lost in receiver", &[("error", &e.message)]);
                    self.handle_connection_lost(e);
                    break;
                }

                Ok(raw) => {
                    last_activity = std::time::Instant::now();
                    match decode_message(&raw) {
                        Err(e) => {
                            self.logger
                                .error("decode error", &[("error", &e.to_string())]);
                        }
                        Ok(msg) => {
                            let msg_id = msg.envelope.message_id.clone();
                            let arc_msg = Arc::new(msg);

                            if let Some((_, tx)) = self.pending.remove(&msg_id) {
                                let _ = tx.send(Ok(arc_msg));
                            } else if let Some((_, tx)) = self.pending_raw.remove(&msg_id) {
                                let _ = tx.send(Ok((arc_msg, raw)));
                            } else {
                                // Unsolicited push message — forward to subscribers.
                                // Errors are ignored: no subscribers is fine.
                                let _ = self.incoming_tx.send(arc_msg);
                            }
                        }
                    }
                }
            }
        }
        self.receiver_active.store(false, Ordering::Release);
    }

    // ── Reconnection ──────────────────────────────────────────────────────────

    /// Attempt to reconnect with exponential backoff.
    ///
    /// `trigger_err` is the error that caused the reconnection attempt (forwarded
    /// to the [`ConnectionState::Reconnecting`] callback). Pass `None` when unknown.
    ///
    /// Returns `true` if reconnection succeeded. On completion (success or failure),
    /// notifies all `wait_for_reconnect` waiters.
    async fn attempt_reconnection(self: &Arc<Self>, trigger_err: Option<&GatewayDError>) -> bool {
        if !self.cfg.reconnect_config.is_enabled() {
            return false;
        }
        if self.closed.load(Ordering::Acquire) {
            return false;
        }

        if self
            .reconnecting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return self.wait_for_reconnect().await;
        }

        self.emit_state(ConnectionState::Reconnecting, trigger_err);

        let rc = &self.cfg.reconnect_config;
        let max = rc.max_retries;
        let mut delay_secs = rc.initial_backoff().as_secs_f64();
        let max_secs = rc.max_backoff().as_secs_f64();
        let mult = rc.backoff_multiplier();

        let mut last_err: Option<GatewayDError> = None;
        let mut success = false;

        for attempt in 0.. {
            if self.closed.load(Ordering::Acquire) {
                break;
            }
            if max > 0 && attempt >= max {
                self.logger
                    .error("reconnection: max retries exhausted", &[("max", &max)]);
                break;
            }
            self.reconnect_attempt.store(attempt + 1, Ordering::Release);

            self.logger.info(
                "reconnect attempt",
                &[("attempt", &(attempt + 1)), ("max", &max)],
            );

            tokio::time::sleep(Duration::from_secs_f64(delay_secs)).await;

            if self.closed.load(Ordering::Acquire) {
                break;
            }

            match self.conn.reconnect().await {
                Ok(()) => match self.re_authenticate().await {
                    Ok(()) => {
                        success = true;
                        break;
                    }
                    Err(e) => {
                        self.logger
                            .error("re-authentication failed", &[("error", &e.message)]);
                        last_err = Some(e);
                    }
                },
                Err(e) => {
                    self.logger
                        .warn("reconnect attempt failed", &[("error", &e.message)]);
                    last_err = Some(e);
                }
            }

            delay_secs = (delay_secs * mult).min(max_secs);
        }

        self.reconnecting.store(false, Ordering::Release);
        self.reconnect_attempt.store(0, Ordering::Release);

        if success {
            self.emit_state(ConnectionState::Connected, None);
            self.start_receiver();
        } else {
            self.emit_state(ConnectionState::ReconnectFailed, last_err.as_ref());
        }

        self.reconnect_notify.notify_waiters();
        success
    }

    /// Sync-mode reconnection: same backoff logic but doesn't restart the receiver.
    /// Used when the background receiver is not active.
    async fn attempt_reconnection_sync(&self, trigger_err: Option<&GatewayDError>) -> bool {
        if !self.cfg.reconnect_config.is_enabled() || self.closed.load(Ordering::Acquire) {
            return false;
        }

        if self
            .reconnecting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return self.wait_for_reconnect().await;
        }

        self.emit_state(ConnectionState::Reconnecting, trigger_err);

        let rc = &self.cfg.reconnect_config;
        let max = rc.max_retries;
        let mut delay_secs = rc.initial_backoff().as_secs_f64();
        let max_secs = rc.max_backoff().as_secs_f64();
        let mult = rc.backoff_multiplier();

        let mut last_err: Option<GatewayDError> = None;
        let mut success = false;

        for attempt in 0.. {
            if self.closed.load(Ordering::Acquire) {
                break;
            }
            if max > 0 && attempt >= max {
                self.logger
                    .error("reconnection: max retries exhausted", &[("max", &max)]);
                break;
            }
            self.reconnect_attempt.store(attempt + 1, Ordering::Release);

            tokio::time::sleep(Duration::from_secs_f64(delay_secs)).await;

            if self.closed.load(Ordering::Acquire) {
                break;
            }

            match self.conn.reconnect().await {
                Ok(()) => match self.re_authenticate().await {
                    Ok(()) => {
                        success = true;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                    }
                },
                Err(e) => {
                    last_err = Some(e);
                }
            }

            delay_secs = (delay_secs * mult).min(max_secs);
        }

        self.reconnecting.store(false, Ordering::Release);
        self.reconnect_attempt.store(0, Ordering::Release);

        if success {
            self.emit_state(ConnectionState::Connected, None);
        } else {
            self.emit_state(ConnectionState::ReconnectFailed, last_err.as_ref());
        }

        self.reconnect_notify.notify_waiters();
        success
    }

    async fn re_authenticate(&self) -> Result<(), GatewayDError> {
        self.authenticate().await?;
        if self.cfg.streaming_enabled() {
            self.send_stream_on().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod disconnect_tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    fn test_client(conn: Arc<ConnClient>, cfg: Config) -> Arc<Client> {
        let logger: Arc<dyn Logger> = cfg
            .logger
            .clone()
            .unwrap_or_else(|| Arc::new(NoOpLogger));
        let gateway_actor_name = cfg.gateway_actor_name.clone();
        let client_name = cfg.client_name.clone();
        let (incoming_tx, _) = broadcast::channel(INCOMING_CHANNEL_CAPACITY);
        Arc::new(Client {
            conn,
            pool: None,
            cfg,
            gateway_actor_name,
            client_name,
            key: "test-key".to_string(),
            pending: DashMap::new(),
            pending_raw: DashMap::new(),
            incoming_tx,
            receiver_active: AtomicBool::new(false),
            receiver_shutdown: StdMutex::new(None),
            keepalive_shutdown: StdMutex::new(None),
            reconnecting: AtomicBool::new(false),
            reconnect_attempt: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            reconnect_notify: Notify::new(),
            state_handler: StdMutex::new(None),
            logger,
        })
    }

    #[tokio::test]
    async fn close_sends_disconnect_before_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let (read_done_tx, read_done_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let _ = accepted_tx.send(());
                let mut buf = vec![0u8; 4096];
                if let Ok(n) = sock.read(&mut buf).await {
                    let _ = read_done_tx.send(buf[..n].to_vec());
                }
            }
        });

        let retry = Arc::new(Retry::new(0, Duration::from_millis(10), 2.0, false));
        let conn_cfg = ClientConfig {
            logger: Arc::new(NoOpLogger),
            ..Default::default()
        };
        let conn = ConnClient::connect(
            "tcp",
            &addr.ip().to_string(),
            &addr.port().to_string(),
            "test-actor",
            retry,
            conn_cfg,
        )
        .await
        .expect("connect");

        accepted_rx.await.expect("server accept");

        let cfg = Config {
            host: addr.ip().to_string(),
            port: addr.port().to_string(),
            client_name: "close-test-client".to_string(),
            gateway_actor_name: "zeroth.pod-os.com".to_string(),
            logger: Some(Arc::new(NoOpLogger)),
            ..Default::default()
        };

        let client = test_client(conn, cfg);
        client.close().await.expect("close");

        let got = read_done_rx.await.expect("server read");
        let wire = String::from_utf8_lossy(&got);
        assert!(
            wire.contains("000000006"),
            "server did not receive GatewayDisconnect frame; got={wire}"
        );
    }
}

// Connection-loss is classified by typed error (GatewayDError::is_connection_lost
// / is_idle_timeout) rather than substring matching on error messages.
