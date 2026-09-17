//! StoreEvent MEM_REPLY correlation tests.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use pod_os_client::{
    client::Client,
    config::{Config, ReconnectConfig},
    message::{
        decode_message, encode_message, intents,
        types::{Envelope, EventFields, Message, ResponseFields},
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

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

fn test_config(host: &str, port: &str, client_name: &str) -> Config {
    Config {
        host: host.to_string(),
        port: port.to_string(),
        client_name: client_name.to_string(),
        gateway_actor_name: "test.local".to_string(),
        enable_concurrent_mode: true,
        enable_streaming: Some(false),
        response_timeout: std::time::Duration::from_secs(5),
        receive_loop_timeout: std::time::Duration::from_millis(100),
        connection_liveness_timeout: Some(std::time::Duration::ZERO),
        reconnect_config: ReconnectConfig {
            initial_backoff: std::time::Duration::from_millis(50),
            max_retries: 1,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn ok_gateway_response(inbound: &Message) -> Message {
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

fn store_event_reply(inbound: &Message, echo_msg_id: bool, unique_id: &str) -> Message {
    Message {
        envelope: Envelope {
            to: inbound.envelope.from.clone(),
            from: "memory@test.local".to_string(),
            intent: intents::STORE_EVENT_RESPONSE.clone(),
            message_id: if echo_msg_id {
                inbound.envelope.message_id.clone()
            } else {
                String::new()
            },
            ..Default::default()
        },
        event: Some(EventFields {
            unique_id: unique_id.to_string(),
            id: "+1787151707.000000".to_string(),
            r#type: "iris:task_result".to_string(),
            ..Default::default()
        }),
        response: Some(ResponseFields {
            status: "OK".to_string(),
            message: "stored".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

struct MemGateway {
    store_replies: Arc<AtomicUsize>,
    echo_msg_id: bool,
    reply_unique_id: String,
}

impl MemGateway {
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
                write_frame(&mut stream, &ok_gateway_response(&inbound)).await;
            } else if intent == intents::GATEWAY_STREAM_ON.message_type {
                write_frame(&mut stream, &ok_gateway_response(&inbound)).await;
            } else if intent == intents::STORE_EVENT.message_type {
                self.store_replies.fetch_add(1, Ordering::SeqCst);
                let uid = inbound
                    .event
                    .as_ref()
                    .map(|e| e.unique_id.clone())
                    .unwrap_or_else(|| self.reply_unique_id.clone());
                write_frame(
                    &mut stream,
                    &store_event_reply(&inbound, self.echo_msg_id, &uid),
                )
                .await;
            } else {
                write_frame(&mut stream, &ok_gateway_response(&inbound)).await;
            }
        }
    }
}

async fn spawn_mem_gateway(echo_msg_id: bool) -> (String, String, Arc<MemGateway>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let gateway = Arc::new(MemGateway {
        store_replies: Arc::new(AtomicUsize::new(0)),
        echo_msg_id,
        reply_unique_id: "task_result-task-abc".to_string(),
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
async fn store_event_reply_with_event_type_in_type_field_correlates() {
    let (host, port, gateway) = spawn_mem_gateway(true).await;
    let client = Client::new(test_config(&host, &port, "store-client"))
        .await
        .expect("connect");

    let mut msg = Message {
        envelope: Envelope {
            to: "memory@test.local".to_string(),
            from: "store-client@test.local".to_string(),
            intent: intents::STORE_EVENT.clone(),
            message_id: "corr-msg-1".to_string(),
            ..Default::default()
        },
        event: Some(EventFields {
            unique_id: "task_result-task-abc".to_string(),
            owner: "task-abc".to_string(),
            location: "TERRA|0|0".to_string(),
            location_separator: "|".to_string(),
            r#type: "iris:task_result".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let resp = client.send_message(&mut msg).await.expect("store_event reply");
    assert_eq!(resp.processing_status(), "OK");
    assert_eq!(gateway.store_replies.load(Ordering::SeqCst), 1);

    client.close().await.ok();
}

#[tokio::test]
async fn store_event_reply_without_msg_id_matches_by_unique_id() {
    let (host, port, gateway) = spawn_mem_gateway(false).await;
    let client = Client::new(test_config(&host, &port, "store-client-2"))
        .await
        .expect("connect");

    let mut msg = Message {
        envelope: Envelope {
            to: "memory@test.local".to_string(),
            from: "store-client-2@test.local".to_string(),
            intent: intents::STORE_EVENT.clone(),
            message_id: "corr-msg-2".to_string(),
            ..Default::default()
        },
        event: Some(EventFields {
            unique_id: "task_result-task-abc".to_string(),
            owner: "task-abc".to_string(),
            location: "TERRA|0|0".to_string(),
            location_separator: "|".to_string(),
            r#type: "iris:task_result".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let resp = client.send_message(&mut msg).await.expect("unique_id fallback");
    assert_eq!(resp.processing_status(), "OK");
    assert_eq!(gateway.store_replies.load(Ordering::SeqCst), 1);

    client.close().await.ok();
}

#[tokio::test]
async fn liveness_probe_cancel_does_not_leave_stale_pending() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let probe_count = Arc::new(AtomicUsize::new(0));

    let probes = Arc::clone(&probe_count);
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            loop {
                let Some(raw) = read_frame(&mut stream).await else {
                    break;
                };
                let Ok(inbound) = decode_message(&raw) else {
                    continue;
                };
                if inbound.envelope.intent.message_type == intents::GATEWAY_ID.message_type {
                    write_frame(&mut stream, &ok_gateway_response(&inbound)).await;
                } else if inbound.envelope.intent.message_type
                    == intents::GATEWAY_STREAM_ON.message_type
                {
                    write_frame(&mut stream, &ok_gateway_response(&inbound)).await;
                } else if inbound.envelope.intent.message_type
                    == intents::STATUS_REQUEST.message_type
                {
                    probes.fetch_add(1, Ordering::SeqCst);
                    // Hold probe response so outer liveness timeout cancels send_message.
                    continue;
                }
            }
        }
    });

    let mut cfg = test_config(&addr.ip().to_string(), &addr.port().to_string(), "probe-pending");
    cfg.liveness_probe_interval = Some(std::time::Duration::from_millis(100));
    cfg.liveness_probe_timeout = Some(std::time::Duration::from_millis(50));
    cfg.liveness_probe_max_failures = Some(10);
    cfg.connection_liveness_timeout = None;

    let client = Client::new(cfg).await.expect("connect");

    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(
        probe_count.load(Ordering::SeqCst) >= 1,
        "expected at least one liveness probe"
    );
    assert_eq!(
        client.in_flight_request_count(),
        0,
        "cancelled liveness probes must not leave stale pending entries"
    );

    client.close().await.ok();
}

#[test]
fn decode_mem_reply_with_event_type_in_type_field() {
    let request = Message {
        envelope: Envelope {
            to: "memory@test.local".to_string(),
            from: "iris@test.local".to_string(),
            intent: intents::STORE_EVENT.clone(),
            message_id: "req-1".to_string(),
            ..Default::default()
        },
        event: Some(EventFields {
            unique_id: "task_result-x".to_string(),
            owner: "task-x".to_string(),
            location: "TERRA|0|0".to_string(),
            location_separator: "|".to_string(),
            r#type: "iris:task_result".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let wire = encode_message(&request, "").expect("encode request");
    let inbound = decode_message(wire.as_bytes()).expect("decode request");

    let reply = store_event_reply(&inbound, true, "task_result-x");
    let reply_wire = encode_message(&reply, "").expect("encode reply");
    let decoded = decode_message(reply_wire.as_bytes()).expect("decode reply");
    assert_eq!(decoded.envelope.intent, intents::STORE_EVENT_RESPONSE);
    assert_eq!(decoded.envelope.message_id, "req-1");
}
