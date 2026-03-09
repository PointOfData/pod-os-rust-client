//! Interfaces / traits exposed by the connection layer.

use std::sync::Arc;

// ── Tracer ───────────────────────────────────────────────────────────────────

pub trait Span: Send + Sync {
    fn end(&self);
    fn record_error(&self, err: &dyn std::error::Error);
    fn add_event(&self, name: &str);
}

pub trait Tracer: Send + Sync + 'static {
    fn start(&self, name: &str) -> Arc<dyn Span>;
}

pub struct NoOpSpan;
impl Span for NoOpSpan {
    fn end(&self) {}
    fn record_error(&self, _: &dyn std::error::Error) {}
    fn add_event(&self, _: &str) {}
}

pub struct NoOpTracer;
impl Tracer for NoOpTracer {
    fn start(&self, _: &str) -> Arc<dyn Span> { Arc::new(NoOpSpan) }
}

// ── WireHook ─────────────────────────────────────────────────────────────────

/// Observer for raw wire bytes — useful for logging / testing.
pub trait WireHook: Send + Sync + 'static {
    fn on_send(&self, raw: &[u8]);
    fn on_receive(&self, raw: &[u8]);
}

pub struct NoOpWireHook;
impl WireHook for NoOpWireHook {
    fn on_send(&self, _: &[u8]) {}
    fn on_receive(&self, _: &[u8]) {}
}

// ── IClient ──────────────────────────────────────────────────────────────────

/// Abstraction over `connection::Client`, allowing mocks in tests.
#[async_trait::async_trait]
pub trait IClient: Send + Sync {
    async fn send(&self, data: &[u8]) -> Result<(), crate::errors::GatewayDError>;
    async fn receive(&self) -> Result<Vec<u8>, crate::errors::GatewayDError>;
    async fn reconnect(&self) -> Result<(), crate::errors::GatewayDError>;
    async fn close(&self);
    fn is_connected(&self) -> bool;
    fn remote_addr(&self) -> String;
}
