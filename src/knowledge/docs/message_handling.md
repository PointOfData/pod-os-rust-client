## Pod-OS Message Handling

The client communicates with Gateway Actors via `Client::send_message()`. `send_message()` takes a `Message`, serializes the message into a byte message, and sends to a Gateway Actor's socket connection. The Gateway uses the To and From addressing to route the message. Messages are sent and received much like e-mail.

Message structure: Messages are composed of two address specifications, a header, a numeric message type, and an optional data payload which may be up to 2 gigabytes in size. The address specifications are ASCIIZ strings, as is the header. The message type is a standard 32-bit signed integer, and the data payload is an unformatted buffer. The payload size specification is a standard 32-bit signed integer.

Connection Event Sequence: When connecting to a Gateway Actor, a socket connection is first established at which point the Gateway Actor is aware that there is a client, but has no other information about the connection. The Gateway Actor assigns the connection a temporary internal name. Following the connection, an identifier message is sent by the connecting client which identifies the connection point so that message traffic can be routed appropriately.

The ID message is required before any other messages will be recognized. Until the ID message is received, all messages received from the new client will be ignored. However, shutdown or forced disconnect messages may be sent to the new service. Once an ID is established, messages can be addressed and delivered to the specified Actor@Gateway Actor.

Message uses one of two states: a. the Gateway is streaming responses for asynchronous message ("STREAM ON"), or b. synchronous message mode where the Client requests message one at a time from a mailbox queue ("STREAM OFF"). Default state is "STREAM OFF". The Pod-OS Dashboard client, for responsiveness uses STREAM ON by default as you can see from the connection sequence.

There are two uses cases to support:
1. Client uses `Client::send_message()` to send a message and process the response to manage Actors.
2. Optionally, customers may use SocketIO Events vended by Dashboard software acting as a proxy client to exchange JSON objects and stream binary payload attachments.

## Intents

Every message must have an `Intent`. The intent determines the wire `messageType` and header `_db_cmd`.

### NeuralMemory intents (messageType 1000 / 1001)

| Intent | Wire Command |
|--------|-------------|
| `STORE_EVENT` | `_db_cmd=store` |
| `STORE_BATCH_EVENTS` | `_db_cmd=store_batch` |
| `STORE_BATCH_TAGS` | `_db_cmd=tag_store_batch` |
| `GET_EVENT` | `_db_cmd=get` |
| `GET_EVENTS_FOR_TAGS` | `_db_cmd=events_for_tag` |
| `LINK_EVENT` | `_db_cmd=link` |
| `UNLINK_EVENT` | `_db_cmd=unlink` |
| `STORE_BATCH_LINKS` | `_db_cmd=link_batch` |

### System intents

| Intent | messageType | Description |
|--------|------------|-------------|
| `GATEWAY_ID` | 5 | Authentication / identity |
| `GATEWAY_STREAM_ON` | 10 | Enable streaming mode |
| `GATEWAY_STREAM_OFF` | 9 | Disable streaming mode |
| `ACTOR_REQUEST` | 4 | Actor-to-actor request |
| `ACTOR_RESPONSE` | 30 | Actor-to-actor response |
| `ACTOR_ECHO` | 2 | Echo / ping |
| `STATUS` | 3 | Status query |
| `ACTOR_REPORT` | 19 | Actor report |

## Timestamps

Always use the Pod-OS timestamp format: `+NNNNNNNNNN.NNNNNN` (POSIX epoch with 6 decimal microseconds).

```rust
use pod_os_client::message::get_timestamp;
let ts = get_timestamp(); // e.g. "+1741388400.123456"
```

## Tags

Tags are structured as `Tag { frequency, key, value }`. In the wire header:
- Format: `tag_0001=1:key=value` (1-indexed, 4-digit, `freq:key=value`)
- In batch payloads: `tag_0=1:key=value` (0-indexed)

## Validation

Enable validation by setting `PODOS_VALIDATE=1`.

```rust
let errs = msg.validate();
if !errs.is_empty() {
    eprintln!("{}", pod_os_client::message::ValidationReport(errs));
}
```

## Wire Frame Layout

Every message on the wire is prefixed with a 9-byte totalLength field (`x` + 8 hex digits). The wire frame layout is:

```text
[9]  totalLength     x + 8 hex digits  (includes all 7 nine-byte fields + content)
[9]  toLength        x + 8 hex digits
[9]  fromLength      x + 8 hex digits
[9]  headerLength    x + 8 hex digits
[9]  messageType     9 decimal digits   (NOT hex)
[9]  dataType        9 decimal digits   (NOT hex)
[9]  payloadLength   x + 8 hex digits
[toLength]           to address
[fromLength]         from address
[headerLength]       tab-separated key=value header
[payloadLength]      raw payload bytes
```

## Example: Sending a Message (Rust)

```rust
use pod_os_client::message::{
    intents, types::{Envelope, EventFields, Message},
};

let mut msg = Message {
    envelope: Envelope {
        to:     "neural-memory@gateway.local".to_string(),
        from:   "my-agent@gateway.local".to_string(),
        intent: intents::STORE_EVENT.clone(),
        ..Default::default()
    },
    event: Some(EventFields {
        owner:     "owner-001".to_string(),
        timestamp: pod_os_client::message::get_timestamp(),
        location:  "TERRA|47.619463|-122.518691".to_string(),
        location_separator: "|".to_string(),
        ..Default::default()
    }),
    ..Default::default()
};
let resp = client.send_message(&mut msg).await?;
```
