//! Channel-based connection pool, mirroring the Go client's `ChannelPool`.
//!
//! # Design
//!
//! - A fixed-capacity `tokio::sync::mpsc` channel acts as the pool.
//! - A `tokio::sync::Semaphore` enforces the maximum concurrent checkouts.
//! - `PoolConn` returns the connection to the pool on `drop` instead of
//!   closing it — matching Go's `PoolConn.Close()` behaviour.

use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};
use uuid::Uuid;

use crate::errors::{ErrCode, GatewayDError};

// ── Connection data ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ConnectionData {
    pub conn: tokio::net::TcpStream,
    pub uuid: String,
}

impl ConnectionData {
    pub fn new(conn: tokio::net::TcpStream) -> Self {
        Self {
            conn,
            uuid: Uuid::new_v4().to_string(),
        }
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

pub type ConnectionFactory = Arc<
    dyn Fn() -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ConnectionData, GatewayDError>> + Send>,
        > + Send
        + Sync,
>;

// ── ChannelPool ───────────────────────────────────────────────────────────────

pub struct ChannelPool {
    tx: mpsc::Sender<ConnectionData>,
    rx: tokio::sync::Mutex<mpsc::Receiver<ConnectionData>>,
    semaphore: Arc<Semaphore>,
    factory: ConnectionFactory,
    max_cap: usize,
}

impl ChannelPool {
    /// Create an empty pool with the given capacity.
    pub fn new(max_cap: usize, factory: ConnectionFactory) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(max_cap);
        Arc::new(Self {
            tx,
            rx: tokio::sync::Mutex::new(rx),
            semaphore: Arc::new(Semaphore::new(max_cap)),
            factory,
            max_cap,
        })
    }

    /// Pre-fill the pool with `initial_cap` connections.
    pub async fn initialize(self: &Arc<Self>, initial_cap: usize) -> Result<(), GatewayDError> {
        let n = initial_cap.min(self.max_cap);
        for _ in 0..n {
            let conn = (self.factory)().await?;
            self.tx.send(conn).await.map_err(|_| {
                GatewayDError::new(
                    ErrCode::PoolInitializationFailed,
                    "pool channel closed during init",
                )
            })?;
        }
        Ok(())
    }

    /// Obtain a connection from the pool (or create a new one), respecting max capacity.
    pub async fn get(&self) -> Result<PoolConn, GatewayDError> {
        // Acquire semaphore permit to enforce max-cap
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| GatewayDError::new(ErrCode::PoolExhausted, "pool semaphore closed"))?;

        // Try to get an idle connection, or create a fresh one
        let data = {
            let mut rx = self.rx.lock().await;
            rx.try_recv().ok()
        };

        let data = match data {
            Some(d) => d,
            None => (self.factory)().await?,
        };

        Ok(PoolConn {
            data: Some(data),
            tx: self.tx.clone(),
            _permit: permit,
        })
    }

    /// Number of idle connections in the pool.
    pub fn idle_count(&self) -> usize {
        self.max_cap - self.semaphore.available_permits()
    }

    /// Close all idle connections by draining the channel.
    pub async fn close(&self) {
        let mut rx = self.rx.lock().await;
        while rx.try_recv().is_ok() {}
    }
}

// ── PoolConn ──────────────────────────────────────────────────────────────────

/// RAII guard: returns the underlying connection to the pool when dropped.
pub struct PoolConn {
    data: Option<ConnectionData>,
    tx: mpsc::Sender<ConnectionData>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl PoolConn {
    pub fn uuid(&self) -> &str {
        &self.data.as_ref().unwrap().uuid
    }
}

impl Drop for PoolConn {
    fn drop(&mut self) {
        if let Some(data) = self.data.take() {
            // Non-blocking: if the pool channel is full, discard the connection
            let _ = self.tx.try_send(data);
        }
    }
}
