//! Client configuration types, mirroring the Go client's `config` package.

use crate::{
    connection::traits::{Tracer, WireHook},
    log::Logger,
};
use std::{sync::Arc, time::Duration};

// ── RetryConfig ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub retries: usize,
    pub backoff: Duration,
    pub backoff_multiplier: f64,
    pub disable_backoff_caps: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            retries: 3,
            backoff: Duration::from_millis(500),
            backoff_multiplier: 2.0,
            disable_backoff_caps: false,
        }
    }
}

// ── PoolConfig ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct PoolConfig {
    pub initial_capacity: usize,
    pub max_capacity: usize,
}

// ── ReconnectConfig ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// `None` means enabled (same as Go's `nil` default).
    pub enabled: Option<bool>,
    /// 0 = unlimited.
    pub max_retries: usize,
    pub initial_backoff: Duration,
    pub backoff_multiplier: f64,
    pub max_backoff: Duration,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            max_retries: 10,
            initial_backoff: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            max_backoff: Duration::from_secs(60),
        }
    }
}

impl ReconnectConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
    pub fn initial_backoff(&self) -> Duration {
        self.initial_backoff
    }
    pub fn backoff_multiplier(&self) -> f64 {
        self.backoff_multiplier
    }
    pub fn max_backoff(&self) -> Duration {
        self.max_backoff
    }
}

// ── Config ────────────────────────────────────────────────────────────────────

/// Top-level client configuration.
pub struct Config {
    // ── Connection ──────────────────────────────────────────────────────────
    /// Network family: `"tcp"`, `"tcp4"`, `"tcp6"`, `"udp"`, `"unix"`, etc.
    pub network: String,
    pub host: String,
    pub port: String,
    pub gateway_actor_name: String,

    // ── Identity ────────────────────────────────────────────────────────────
    /// Required: identifies this client to the gateway.
    pub client_name: String,
    pub passcode: String,

    // ── Retry ───────────────────────────────────────────────────────────────
    pub retry_config: RetryConfig,

    // ── Timeouts ────────────────────────────────────────────────────────────
    pub dial_timeout: Duration,
    pub receive_timeout: Duration,
    pub send_timeout: Duration,

    // ── Connection pool ──────────────────────────────────────────────────────
    pub pool_config: PoolConfig,

    // ── Streaming ───────────────────────────────────────────────────────────
    /// `None` or `Some(true)` → send `GatewayStreamOn`; `Some(false)` → skip.
    pub enable_streaming: Option<bool>,

    // ── Concurrent mode ──────────────────────────────────────────────────────
    /// When true, a background task routes responses by `MessageId` via channels.
    pub enable_concurrent_mode: bool,
    /// Per-request response wait timeout.
    pub response_timeout: Duration,

    // ── Reconnection ─────────────────────────────────────────────────────────
    pub reconnect_config: ReconnectConfig,

    // ── Logging ─────────────────────────────────────────────────────────────
    pub log_level: u8,
    pub logger: Option<Arc<dyn Logger>>,

    // ── OpenTelemetry ────────────────────────────────────────────────────────
    pub enable_tracing: bool,
    pub tracer_name: String,
    pub tracer: Option<Arc<dyn Tracer>>,

    // ── Wire observer ────────────────────────────────────────────────────────
    pub wire_hook: Option<Arc<dyn WireHook>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: "tcp".to_string(),
            host: String::new(),
            port: String::new(),
            gateway_actor_name: String::new(),
            client_name: String::new(),
            passcode: String::new(),
            retry_config: RetryConfig::default(),
            dial_timeout: Duration::from_secs(10),
            receive_timeout: Duration::from_secs(30),
            send_timeout: Duration::from_secs(30),
            pool_config: PoolConfig::default(),
            enable_streaming: None,
            enable_concurrent_mode: false,
            response_timeout: Duration::from_secs(30),
            reconnect_config: ReconnectConfig::default(),
            log_level: 0,
            logger: None,
            enable_tracing: false,
            tracer_name: String::new(),
            tracer: None,
            wire_hook: None,
        }
    }
}

impl Config {
    /// Whether `GatewayStreamOn` should be sent after authentication.
    pub fn streaming_enabled(&self) -> bool {
        self.enable_streaming.unwrap_or(true)
    }
}
