//! Reconnect serialization and application-level liveness probe tests.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use pod_os_client::{
    client::Client,
    config::{Config, ReconnectConfig},
    message::{
        decode_message, encode_message, intents,
        types::{Envelope, Message, ResponseFields},
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn ok_response(inbound: &Message) -> Message {
    Message {
        envelope: Envelope {
            to: inbound.envelope.from.clone(),
            from: inbound.envelope.to.clone(),
            intent: intents::STATUS.clone(),
            message_id: inbound.envelope.message_id.clone(),
            ..Default::default()
        },
        response: Some(ResponseFields {
            status: "OK".to_string(),
            message: "OK".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn read_frame(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut prefix = [0u8; 9];
    if stream.read_exact(&mut prefix).await.is_err() {
        return None;
    }
    let s = std::str::from_utf8(&prefix).ok()?;
    let total_len = if let Some(hex) = s.strip_prefix('x') {
        usize::from_str_radix(hex, 16).ok()?
    } else {
        s.trim_start_matches('0').parse().ok()?
    };
    let mut buf = prefix.to_vec();
    let rest = total_len.saturating_sub(9);
    if rest > 0 {
        let mut body = vec![0u8; rest];
        if stream.read_exact(&mut body).await.is_err() {
            return None;
        }
        buf.extend_from_slice(&body);
    }
    Some(buf)
}

async fn write_frame(stream: &mut TcpStream, msg: &Message) {
    let wire = encode_message(msg, "").expect("encode");
    stream.write_all(wire.as_bytes()).await.expect("write");
}

struct MockGateway {
    gateway_id_count: Arc<AtomicUsize>,
    disconnect_count: Arc<AtomicUsize>,
    hold_rpc: Arc<AtomicUsize>,
}

impl MockGateway {
    async fn run(self: Arc<Self>, mut stream: TcpStream) {
        loop {
            let Some(raw) = read_frame(&mut stream).await else {
                break;
            };
            let Ok(inbound) = decode_message(&raw) else {
                continue;
            };
            let intent = inbound.envelope.intent.message_type;

            if intent == intents::GATEWAY_ID.message_type {
                self.gateway_id_count.fetch_add(1, Ordering::SeqCst);
                write_frame(&mut stream, &ok_response(&inbound)).await;
            } else if intent == intents::GATEWAY_DISCONNECT.message_type {
                self.disconnect_count.fetch_add(1, Ordering::SeqCst);
            } else if intent == intents::STATUS_REQUEST.message_type {
                write_frame(&mut stream, &ok_response(&inbound)).await;
            } else if intent == intents::GATEWAY_STREAM_ON.message_type {
                write_frame(&mut stream, &ok_response(&inbound)).await;
            } else if self.hold_rpc.load(Ordering::SeqCst) > 0 {
                // Deliberately hold long-running RPC responses.
                continue;
            } else {
                write_frame(&mut stream, &ok_response(&inbound)).await;
            }
        }
    }
}

fn test_config(host: &str, port: &str, client_name: &str) -> Config {
    Config {
        host: host.to_string(),
        port: port.to_string(),
        client_name: client_name.to_string(),
        gateway_actor_name: "test.local".to_string(),
        enable_concurrent_mode: true,
        enable_streaming: Some(false),
        response_timeout: std::time::Duration::from_secs(30),
        receive_loop_timeout: std::time::Duration::from_millis(100),
        liveness_probe_interval: Some(std::time::Duration::from_millis(200)),
        liveness_probe_timeout: Some(std::time::Duration::from_secs(1)),
        liveness_probe_max_failures: Some(2),
        reconnect_config: ReconnectConfig {
            initial_backoff: std::time::Duration::from_millis(50),
            max_retries: 3,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn race_test_config(host: &str, port: &str, client_name: &str) -> Config {
    let mut cfg = test_config(host, port, client_name);
    cfg.liveness_probe_max_failures = Some(1);
    cfg.liveness_probe_interval = Some(std::time::Duration::from_millis(100));
    cfg
}

async fn spawn_mock() -> (String, String, Arc<MockGateway>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let gateway = Arc::new(MockGateway {
        gateway_id_count: Arc::new(AtomicUsize::new(0)),
        disconnect_count: Arc::new(AtomicUsize::new(0)),
        hold_rpc: Arc::new(AtomicUsize::new(0)),
    });

    let gw = Arc::clone(&gateway);
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            gw.run(stream).await;
        }
    });

    (
        addr.ip().to_string(),
        addr.port().to_string(),
        gateway,
    )
}

#[tokio::test]
async fn long_pending_rpc_does_not_reconnect_when_probes_succeed() {
    let (host, port, gateway) = spawn_mock().await;
    gateway.hold_rpc.store(1, Ordering::SeqCst);

    let cfg = test_config(&host, &port, "probe-client");
    let client = Client::new(cfg).await.expect("connect");
    assert_eq!(gateway.gateway_id_count.load(Ordering::SeqCst), 1);

    let mut rpc = Message {
        envelope: Envelope {
            to: "memory@test.local".to_string(),
            from: "probe-client@test.local".to_string(),
            intent: intents::STORE_EVENT.clone(),
            message_id: "long-rpc-1".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };

    let client2 = Arc::clone(&client);
    tokio::spawn(async move {
        let _ = client2.send_message(&mut rpc).await;
    });

    // Longer than the old 90s backstop would need; with probes we should stay connected.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    assert!(client.is_connected(), "expected socket to remain up during long RPC");
    assert_eq!(
        gateway.gateway_id_count.load(Ordering::SeqCst),
        1,
        "expected no re-registration while probes succeed"
    );

    client.close().await.ok();
}

#[tokio::test]
async fn client_new_during_reconnect_waits_for_existing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let gateway_id_count = Arc::new(AtomicUsize::new(0));
    let accept_once = Arc::new(AtomicUsize::new(0));

    let ids = Arc::clone(&gateway_id_count);
    let accepts = Arc::clone(&accept_once);
    tokio::spawn(async move {
        while accepts.load(Ordering::SeqCst) < 2 {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            accepts.fetch_add(1, Ordering::SeqCst);
            if let Some(raw) = read_frame(&mut stream).await {
                if let Ok(inbound) = decode_message(&raw) {
                    if inbound.envelope.intent.message_type == intents::GATEWAY_ID.message_type {
                        ids.fetch_add(1, Ordering::SeqCst);
                    }
                    write_frame(&mut stream, &ok_response(&inbound)).await;
                }
            }
            // Hold connection open without further reads to simulate a dead socket.
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    let client = Client::new(race_test_config(
        &addr.ip().to_string(),
        &addr.port().to_string(),
        "race-client",
    ))
    .await
    .expect("connect");
    assert_eq!(gateway_id_count.load(Ordering::SeqCst), 1);

    // Server stops reading after auth; probe failures trigger reconnect.
    for _ in 0..50 {
        if client.is_reconnecting() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert!(client.is_reconnecting(), "expected reconnect to be in progress");

    let second = Client::new(race_test_config(
        &addr.ip().to_string(),
        &addr.port().to_string(),
        "race-client",
    ))
    .await
    .expect("second new");
    assert!(
        Arc::ptr_eq(&client, &second),
        "Client::new should return existing client while reconnecting"
    );

    client.close().await.ok();
}

#[tokio::test]
async fn reconnect_sends_disconnect_before_second_gateway_id() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let gateway_id_count = Arc::new(AtomicUsize::new(0));
    let disconnect_count = Arc::new(AtomicUsize::new(0));
    let ignore_probes = Arc::new(AtomicUsize::new(0));

    let ids = Arc::clone(&gateway_id_count);
    let disc = Arc::clone(&disconnect_count);
    let ignore = Arc::clone(&ignore_probes);

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let ids = Arc::clone(&ids);
            let disc = Arc::clone(&disc);
            let ignore = Arc::clone(&ignore);
            tokio::spawn(async move {
                let mut saw_id = false;
                loop {
                    let Some(raw) = read_frame(&mut stream).await else {
                        break;
                    };
                    let Ok(inbound) = decode_message(&raw) else {
                        continue;
                    };
                    match inbound.envelope.intent.message_type {
                        t if t == intents::GATEWAY_ID.message_type => {
                            ids.fetch_add(1, Ordering::SeqCst);
                            saw_id = true;
                            ignore.store(1, Ordering::SeqCst);
                            write_frame(&mut stream, &ok_response(&inbound)).await;
                        }
                        t if t == intents::GATEWAY_DISCONNECT.message_type => {
                            disc.fetch_add(1, Ordering::SeqCst);
                        }
                        t if t == intents::STATUS_REQUEST.message_type => {
                            if ignore.load(Ordering::SeqCst) == 0 || !saw_id {
                                write_frame(&mut stream, &ok_response(&inbound)).await;
                            }
                            // After auth, stop answering probes to force reconnect.
                        }
                        _ => {}
                    }
                }
            });
        }
    });

    let mut cfg = test_config(&addr.ip().to_string(), &addr.port().to_string(), "disc-client");
    cfg.liveness_probe_max_failures = Some(2);
    cfg.liveness_probe_interval = Some(std::time::Duration::from_millis(100));
    cfg.reconnect_config.initial_backoff = std::time::Duration::from_millis(50);

    let client = Client::new(cfg).await.expect("connect");
    assert_eq!(gateway_id_count.load(Ordering::SeqCst), 1);

    for _ in 0..100 {
        if gateway_id_count.load(Ordering::SeqCst) >= 2
            && disconnect_count.load(Ordering::SeqCst) >= 1
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert!(
        disconnect_count.load(Ordering::SeqCst) >= 1,
        "expected GatewayDisconnect before re-registration"
    );
    assert!(
        gateway_id_count.load(Ordering::SeqCst) >= 2,
        "expected reconnect to complete GATEWAY_ID"
    );

    client.close().await.ok();
}
