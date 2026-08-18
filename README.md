# pod-os-rust-client

High-performance async Rust client for the [Pod-OS](https://github.com/PointOfData) — optimized for software engineers, machine-learning scientists, and generative-AI coding/debugging agents.

## Features

| Feature | Detail |
|---------|--------|
| **Exact wire compatibility** | Identical 9-byte length-prefix framing and header format as the Go client |
| **100 K+ messages/second** | `DashMap` + `oneshot` channels for lock-free concurrent response dispatch |
| **Async-first** | Built on `tokio`; zero blocking operations in the hot path |
| **Auto-reconnect** | Exponential-backoff reconnection with configurable max retries |
| **Connection pool** | Optional channel-based pool (mirrors `ChannelPool` in the Go client) |
| **Opt-in validation** | Zero-cost unless `PODOS_VALIDATE=1` is set |
| **Embedded AI prompts** | Knowledge docs compiled into the binary for GenAI agent use |
| **OpenTelemetry-ready** | `Tracer` / `WireHook` interfaces for observability |

## Quick Start

```toml
# Cargo.toml
[dependencies]
pod-os-client = { git = "https://github.com/PointOfData/pod-os-rust-client" }
tokio = { version = "1", features = ["full"] }
```

```rust
use pod_os_client::{
    client::Client,
    config::Config,
    message::{intents, get_timestamp, types::{Envelope, EventFields, Message}},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config {
        host:               "localhost".to_string(),
        port:               "7654".to_string(),
        client_name:        "my-agent".to_string(),
        gateway_actor_name: "neural-memory".to_string(),
        enable_concurrent_mode: true,  // 100K+ msg/s mode
        ..Default::default()
    };

    let client = Client::new(cfg).await?;

    // Store an event
    let mut msg = Message {
        envelope: Envelope {
            to:     "neural-memory@localhost:7654".to_string(),
            from:   "my-agent@localhost:7654".to_string(),
            intent: intents::STORE_EVENT.clone(),
            ..Default::default()
        },
        event: Some(EventFields {
            id:        "evt-001".to_string(),
            owner:     "owner-001".to_string(),
            timestamp: get_timestamp(),
            r#type:    "observation".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let resp = client.send_message(&mut msg).await?;
    println!("status: {}", resp.processing_status());
    Ok(())
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Client  (src/client.rs)                                    │
│                                                             │
│  send_message()  ──►  DashMap<MessageId, oneshot::Sender>  │
│       ▲                          │                          │
│       │                          ▼                          │
│  ConnClient::send()    Background receive_loop()            │
│  (tokio::Mutex on      → decode_message()                   │
│   write half)          → dispatch to oneshot channel        │
└─────────────────────────────────────────────────────────────┘
```

## Wire Protocol

The Pod-OS wire format uses **9-byte length-prefix fields**:

```
[9]  totalLength     "x" + 8 lowercase hex digits  (e.g. x000001a2)
[9]  toLength        "x" + 8 hex digits
[9]  fromLength      "x" + 8 hex digits
[9]  headerLength    "x" + 8 hex digits
[9]  messageType     9 zero-padded decimal digits   (NOT hex)
[9]  dataType        9 zero-padded decimal digits   (NOT hex)
[9]  payloadLength   "x" + 8 hex digits
[toLength]           to address          (e.g. "actor@gateway.local")
[fromLength]         from address        (e.g. "client@gateway.local")
[headerLength]       tab-separated key=value header
[payloadLength]      raw payload bytes
```

## Intents

Every message carries an **Intent** that determines the wire `messageType` and
the NeuralMemory `_db_cmd` header field.

```rust
use pod_os_client::message::intents;

// NeuralMemory operations (messageType 1000 → request, 1001 → response)
intents::STORE_EVENT           // _db_cmd=store
intents::STORE_DATA            // _db_cmd=store_data  (raw payload; no tags)
intents::STORE_BATCH_EVENTS    // _db_cmd=store_batch
intents::STORE_BATCH_TAGS      // _db_cmd=tag_store_batch
intents::GET_EVENT             // _db_cmd=get
intents::GET_EVENTS_FOR_TAGS   // _db_cmd=events_for_tag
intents::LINK_EVENT            // _db_cmd=link
intents::UNLINK_EVENT          // _db_cmd=unlink
intents::STORE_BATCH_LINKS     // _db_cmd=link_batch

// Gateway control
intents::GATEWAY_ID            // messageType 5  — authentication
intents::GATEWAY_STREAM_ON     // messageType 10 — enable streaming
intents::ACTOR_ECHO            // messageType 2  — ping
intents::STATUS_REQUEST        // messageType 110 — actor health probe
intents::STATUS                // messageType 3  — actor health reply
```

## Timestamps

```rust
use pod_os_client::message::get_timestamp;

// "+1741388400.123456"  — POSIX epoch with 6-decimal microseconds
let ts = get_timestamp();
```

## Tags

```rust
use pod_os_client::message::types::{Tag, TagValue};

let tag = Tag {
    frequency: 1,
    key:       "classification".to_string(),
    value:     TagValue::Text("urgent".to_string()),
    ..Default::default()
};
```

In the wire header: `tag_0001=1:classification=urgent` (1-indexed, 4-digit, `freq:key=value`).

## Validation

Enable at startup by setting the environment variable `PODOS_VALIDATE=1`.

```rust
let errs = msg.validate();
if !errs.is_empty() {
    eprintln!("{}", pod_os_client::message::ValidationReport(errs));
}
```

When `PODOS_VALIDATE` is unset, `validate()` returns immediately with zero allocations.

## AI Agent Knowledge Documents

Embedded documentation is compiled into the binary for GenAI agent prompting:

```rust
use pod_os_client::knowledge;

let doc = knowledge::get_document("communication").unwrap();
let doc = knowledge::get_document("message-handling").unwrap();
let doc = knowledge::get_document("neural-memory").unwrap();
let doc = knowledge::get_document("neural-memory-retrieval").unwrap();

let all = knowledge::list_documents();
```

## Configuration Reference

```rust
use pod_os_client::config::{Config, RetryConfig, ReconnectConfig};
use std::time::Duration;

let cfg = Config {
    // Connection
    network:            "tcp".to_string(),
    host:               "gateway.example.com".to_string(),
    port:               "7654".to_string(),
    gateway_actor_name: "neural-memory".to_string(),

    // Identity
    client_name:        "my-service".to_string(),
    passcode:           "secret".to_string(),

    // Retry on connect
    retry_config: RetryConfig {
        retries:            5,
        backoff:            Duration::from_millis(500),
        backoff_multiplier: 2.0,
        ..Default::default()
    },

    // Timeouts
    dial_timeout:    Duration::from_secs(10),
    send_timeout:    Duration::from_secs(30),
    receive_timeout: Duration::from_secs(30),

    // High-throughput concurrent mode
    enable_concurrent_mode: true,
    response_timeout:       Duration::from_secs(30),

    // Auto-reconnect
    reconnect_config: ReconnectConfig {
        max_retries:        10,
        initial_backoff:    Duration::from_secs(1),
        backoff_multiplier: 2.0,
        max_backoff:        Duration::from_secs(60),
        ..Default::default()
    },

    // Streaming (true by default)
    enable_streaming: Some(true),

    // App-level AIP Keepalive (default 30s; Some(ZERO) disables)
    keepalive_interval: None,

    // Logging  (0=off, 1=error, 2=warn, 3=info, 4=debug)
    log_level: 3,

    ..Default::default()
};
```

## App-Level Keepalive

`Client` sends periodic AIP `Keepalive` frames (message_type 18) on the primary connection and idle pooled connections. Configure `keepalive_interval`: `None` uses 30s, `Some(Duration::ZERO)` disables. The tokio interval task starts after authentication, pauses while disconnected/reconnecting, and stops on `close()`.

On explicit close (`client.close().await`), the client sends a fire-and-forget AIP `GatewayDisconnect` frame (message_type 6) on the ID-authenticated primary connection, then closes the TCP socket. Reconnect also sends Disconnect when the prior socket is still writable. Abrupt connection loss may not be able to send Disconnect. Unauthenticated pool sockets are closed without Disconnect because they never completed a GatewayId handshake.

## Connection Liveness

In concurrent mode the receive loop treats idle read timeouts as benign. Long-running
requests no longer trigger reconnect solely because no inbound frame arrived.

Instead, a background task sends periodic **`StatusRequest`** probes (message_type 110)
to `$system@<gateway>`. Configure:

| Field | Default | Meaning |
|---|---|---|
| `liveness_probe_interval` | 30s | Time between probes when no recent inbound traffic |
| `liveness_probe_timeout` | 5s | Per-probe response wait |
| `liveness_probe_max_failures` | 3 | Consecutive failures before reconnect |
| `connection_liveness_timeout` | enabled | Set `Some(Duration::ZERO)` to disable probes |

Keepalive frames (type 18) remain a cheap outbound heartbeat but are not used as the
death detector because they do not wait for a reply.

On reconnect, the client sends **`GatewayDisconnect`** on the old socket when still
writable before opening a new TCP connection and re-sending `GatewayId`.

## Actor Health Checks (Non-Neural Memory Actors)

Neural Memory Actors are typically probed with store/get intents. **Socket Actors** use the lightweight AIP `StatusRequest` / `Status` pair instead:

| Intent | message_type | Role |
|---|---|---|
| `STATUS_REQUEST` | 110 | Inbound health probe (envelope + optional `_msg_id`) |
| `STATUS` | 3 | Health reply (`_status`, `_msg`, echoed `_msg_id`) |

### Responding to probes (Actor side)

Enable concurrent mode and call `respond_to_health_checks` after `Client::new`:

```rust
use std::sync::Arc;
use pod_os_client::{Client, Config, respond_to_health_checks};

let cfg = Config {
    enable_concurrent_mode: true,
    ..Default::default()
};
let client = Client::new(cfg).await?;
respond_to_health_checks(client.clone());
```

This subscribes to unsolicited inbound frames via `subscribe_incoming()` and replies to `STATUS_REQUEST` probes with fire-and-forget `send_control_message`.

### Sending probes (monitor side)

```rust
use pod_os_client::message::{intents, types::{Envelope, Message}};
use uuid::Uuid;

let probe_id = Uuid::new_v4().to_string();
let mut probe = Message {
    envelope: Envelope {
        to: "my-socket-actor@zeroth.pod-os.com".into(),
        from: format!("{}@{}", client.client_name(), client.actor_name()),
        intent: intents::STATUS_REQUEST.clone(),
        client_name: client.client_name().to_string(),
        message_id: probe_id.clone(),
        ..Default::default()
    },
    ..Default::default()
};
let resp = client.send_message(&mut probe).await?;
assert_eq!(resp.processing_status(), "OK");
assert_eq!(resp.message_id(), probe_id);
```

## Performance

Designed to sustain **100 K+ messages per second** in concurrent mode:

- `DashMap` provides lock-free reads in the receiver task's message-dispatch hot-path
- `tokio::sync::oneshot` channels give zero-overhead per-request response delivery
- `bytes::BytesMut` with adaptive chunk sizes avoids reallocations in the receive path
- TCP `nodelay` is set automatically to minimise latency
- The send `Mutex` is held only during the write syscall

## Mapping to Go Client

| Go | Rust |
|----|------|
| `podos.Client` | `client::Client` |
| `config.Config` | `config::Config` |
| `message.Message` | `message::types::Message` |
| `message.EncodeMessage()` | `message::encode_message()` |
| `message.DecodeMessage()` | `message::decode_message()` |
| `message.IntentType.StoreEvent` | `message::intents::STORE_EVENT` |
| `message.IntentType.StoreData` | `message::intents::STORE_DATA` |
| `message.GetTimestamp()` | `message::get_timestamp()` |
| `message.Validate()` | `msg.validate()` |
| `errors.GatewayDError` | `errors::GatewayDError` |
| `connection.ChannelPool` | `connection::pool::ChannelPool` |
| `connection.Retry` | `connection::retry::Retry` |
| `log.Logger` (interface) | `log::Logger` (trait) |
| `knowledge.GetDocument()` | `knowledge::get_document()` |
| `podos.RespondToHealthChecks()` | `respond_to_health_checks()` |
| `podos.BuildStatusHealthReply()` | `build_status_health_reply()` |
| `Client.SetUnmatchedMessageHandler()` | `Client::subscribe_incoming()` |

## License

MIT
