//! Network address resolution, mirroring the Go client's `connection.Resolve`.

use crate::errors::{ErrCode, GatewayDError};

/// Supported network families.
const TCP_NETWORKS: &[&str] = &["tcp", "tcp4", "tcp6"];
const UDP_NETWORKS: &[&str] = &["udp", "udp4", "udp6"];
const UNIX_NETWORKS: &[&str] = &["unix", "unixgram", "unixpacket"];

/// Resolve and validate a `(network, address)` pair.
///
/// Returns the canonical `host:port` string (TCP/UDP) or socket path (Unix).
/// Panics on unsupported networks — matching Go behaviour.
pub fn resolve(network: &str, address: &str) -> Result<String, GatewayDError> {
    if TCP_NETWORKS.contains(&network) || UDP_NETWORKS.contains(&network) {
        resolve_tcp_udp(address)
    } else if UNIX_NETWORKS.contains(&network) {
        Ok(address.to_string())
    } else {
        panic!(
            "pod-os-client: unsupported network '{}' — expected tcp/udp/unix",
            network
        );
    }
}

fn resolve_tcp_udp(address: &str) -> Result<String, GatewayDError> {
    use std::net::ToSocketAddrs;
    let mut addrs = address.to_socket_addrs().map_err(|e| {
        GatewayDError::wrap(
            ErrCode::ResolveFailed,
            format!("cannot resolve '{}'", address),
            e,
        )
    })?;
    addrs.next().map(|a| a.to_string()).ok_or_else(|| {
        GatewayDError::new(
            ErrCode::ResolveFailed,
            format!("no addresses for '{}'", address),
        )
    })
}

/// Build a `host:port` address string from separate components.
pub fn make_addr(host: &str, port: &str) -> String {
    format!("{}:{}", host, port)
}
