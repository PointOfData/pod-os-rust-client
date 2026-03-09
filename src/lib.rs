//! # Pod-OS Rust Client
//!
//! High-performance async client for the [Pod-OS](https://github.com/PointOfData) Gateway.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use pod_os_client::{
//!     client::Client,
//!     config::Config,
//!     message::{intents, get_timestamp, types::{Envelope, EventFields, Message}},
//! };
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let cfg = Config {
//!         host:               "localhost".to_string(),
//!         port:               "7654".to_string(),
//!         client_name:        "my-agent".to_string(),
//!         gateway_actor_name: "neural-memory".to_string(),
//!         enable_concurrent_mode: true,
//!         ..Default::default()
//!     };
//!
//!     let client = Client::new(cfg).await?;
//!
//!     let mut msg = Message {
//!         envelope: Envelope {
//!             to:     "neural-memory@localhost:7654".to_string(),
//!             from:   "my-agent@localhost:7654".to_string(),
//!             intent: intents::STORE_EVENT.clone(),
//!             ..Default::default()
//!         },
//!         event: Some(EventFields {
//!             id:        "evt-001".to_string(),
//!             owner:     "owner-001".to_string(),
//!             timestamp: get_timestamp(),
//!             ..Default::default()
//!         }),
//!         ..Default::default()
//!     };
//!
//!     let resp = client.send_message(&mut msg).await?;
//!     println!("status: {}", resp.processing_status());
//!     Ok(())
//! }
//! ```
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │  Client  (src/client.rs)                     │
//! │  ┌─────────────────────────────────────────┐ │
//! │  │  ConnClient  (async TCP + retry)        │ │
//! │  └─────────────────────────────────────────┘ │
//! │  ┌──────────────┐  ┌────────────────────────┐│
//! │  │ DashMap      │  │ Background receiver    ││
//! │  │ pending{msg} │  │ tokio::spawn(recv_loop)││
//! │  └──────────────┘  └────────────────────────┘│
//! └──────────────────────────────────────────────┘
//! ```
//!
//! ## Performance
//!
//! - Concurrent mode uses `DashMap` + `oneshot` channels for lock-free
//!   response dispatch — capable of **100 K+ messages/second**.
//! - The write path acquires only a `tokio::sync::Mutex` for the duration of
//!   the write syscall.
//! - `bytes::BytesMut` avoids reallocations in the receive path.

pub mod client;
pub mod config;
pub mod connection;
pub mod errors;
pub mod knowledge;
pub mod log;
pub mod message;

// ── Top-level re-exports ─────────────────────────────────────────────────────

pub use client::{
    get_client_by_gateway_actor_name, get_client_count, is_connection_error, is_timeout_error,
    remove_client_by_gateway_actor_name, Client, ERR_CONNECTION_LOST,
};
pub use config::Config;
pub use errors::{ErrCode, GatewayDError};
pub use log::{Level, Logger, NoOpLogger, TracingLogger};
pub use message::{
    decode_message, encode_message, get_timestamp, get_timestamp_from_time, Envelope, Intent,
    Message, SocketMessage,
};
