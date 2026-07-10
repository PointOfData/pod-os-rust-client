//! Transport-level dead-connection detection tests.
//!
//! These spin up a local `TcpListener`, drive a specific failure scenario, and
//! assert the connection client classifies it correctly (fatal connection-lost
//! vs benign idle timeout) and clears its `connected` flag on fatal errors.

use std::sync::Arc;
use std::time::Duration;

use pod_os_client::connection::client::{Client, ClientConfig};
use pod_os_client::connection::Retry;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

async fn connect_to(host: &str, port: &str) -> Arc<Client> {
    let cfg = ClientConfig {
        receive_timeout: Duration::from_millis(500),
        send_timeout: Duration::from_millis(500),
        ..Default::default()
    };
    let retry = Arc::new(Retry::new(0, Duration::from_millis(10), 2.0, false));
    Client::connect("tcp", host, port, "test-actor", retry, cfg)
        .await
        .expect("connect")
}

#[tokio::test]
async fn corrupt_prefix_is_fatal() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let _ = sock.write_all(b"!!garbage").await;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    let client = connect_to(&addr.ip().to_string(), &addr.port().to_string()).await;
    let err = client.receive().await.expect_err("expected error");
    assert!(err.is_connection_lost(), "expected connection lost, got {err:?}");
    assert!(!client.is_connected(), "connected flag should be cleared");
}

#[tokio::test]
async fn rst_mid_response_is_fatal() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            // Claim 100 bytes, send only the prefix + a few body bytes, then drop
            // the socket so the client hits EOF/RST mid-frame.
            let _ = sock.write_all(b"x00000064").await;
            let _ = sock.write_all(b"partial").await;
            // socket dropped here -> peer close mid-frame
        }
    });

    let client = connect_to(&addr.ip().to_string(), &addr.port().to_string()).await;
    let start = std::time::Instant::now();
    let err = client.receive().await.expect_err("expected error");
    assert!(err.is_connection_lost(), "expected connection lost, got {err:?}");
    assert!(!client.is_connected(), "connected flag should be cleared");
    assert!(start.elapsed() < Duration::from_millis(1500), "detection too slow");
}

#[tokio::test]
async fn idle_timeout_is_benign() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((sock, _)) = listener.accept().await {
            // Stay connected but silent.
            tokio::time::sleep(Duration::from_secs(2)).await;
            drop(sock);
        }
    });

    let client = connect_to(&addr.ip().to_string(), &addr.port().to_string()).await;
    let err = client.receive().await.expect_err("expected timeout");
    assert!(err.is_idle_timeout(), "expected idle timeout, got {err:?}");
    assert!(client.is_connected(), "connected flag must stay set on idle timeout");
}
