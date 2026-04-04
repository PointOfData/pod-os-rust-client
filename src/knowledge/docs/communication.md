## Pod-OS Communication

All communication with Pod-OS occurs through messages delivered to a Gateway via some sort of connection, usually a socket. The Gateway manages message routing to the Actors hosted by the Gateway. Messages are used to control an Actor, store and retrieve events from Evolutionary Neural Memory (an Actor), and communicate with custom Services hosted by the Actor.

The client software operates as an Actor. Each client connection to a Gateway acts as a separate Actor connection. Client Actors are identified by their ClientName and ActorName which must be the same. Clients may choose to connect to multiple Gateways or manage multiple connections to the same Gateway. In this case, each connection is a separate Actor and connection. Clients are encouraged to implement connection pooling to improve performance. A client connects to a Gateway using a unique ClientName name and all message routing is based on this name. In this way, a client can connect to multiple Actors and send messages to each Actor independently as well as track multiple conversations. Client Actors may have multiple connections to the same Actor, and each connection is a separately-named Actor and connection.

The ID message contains the following information:
- The name of the client or service connecting to the Actor.
- The type of connection (socket, REST, etc.).
- The version of the Pod-OS software being used.
- The timestamp of the connection.
- The unique identifier for the connection.
- The signature of the connection.
- The private key for the connection.

Messages use one of two states: 1. the Actor is streaming responses for asynchronous message ("STREAM ON"), or 2. synchronous message mode where the Client requests message one at a time from a mailbox queue ("STREAM OFF"). Default state is "STREAM OFF". The Pod-OS Dashboard client(this software) for responsiveness uses STREAM ON by default as you can see from the connection sequence.

Gateway Network. Each Gateway is a self-contained unit that integrates a messaging backbone. Gateways manage Actors which are individual, autonomous computing units Gateways plus Actors therefore form an amorphous computing platform, consisting of a number of cooperative Actors which may be running on one or more physical devices. Each Actor is completely autonomous and may communicate with any other Actor, so long as the originating Actor has the information and rights necessary to do so (an Actor requesting communication with another
Actor may be rebuffed). An Actor reacts to other Actors by examining the content of messages as well as communicating via gateways to the outside world.

The Pod-OS model is concurrent, distributed processing. Each Gateway manages multiple local Actors, each of which runs independently of the Gateway process. Gateways mediate message traffic between multiple native processes. Messages are transferred between tasks using a queued message system, and messages are transferred between Gateways, Actors, and applications using the same message structures. Gateway provisions allow for non-continuously connected Actors to receive messages; messages may be stored in a mailbox which is then transmitted to a process when it makes a connection with the Actor hosting the mailbox. Messages not only carry information, but in keeping with the event-oriented concept design of Pod-OS, each message retains an audit trail recording all Gateways and Actors through which it has passed, and a reply will contain a wormhole route for return path processing. While useful for audit trails or historical analysis, trails can also be used to prevent unwanted closed processing loops in large concurrent systems.

### Connecting (Rust)

```rust
use pod_os_client::{client::Client, config::Config};

let cfg = Config {
    host:               "localhost".to_string(),
    port:               "7654".to_string(),
    client_name:        "my-agent".to_string(),
    gateway_actor_name: "neural-memory".to_string(),
    enable_concurrent_mode: true,
    ..Default::default()
};

let client = Client::new(cfg).await?;
```

### Addressing

All messages use the format `name@gateway` for both `To` and `From` addresses.

- **To**: `"actor@gateway.domain"` — the target actor on the gateway
- **From**: `"clientName@gateway.domain"` — your client identity

### Message IDs

Every request should have a unique `MessageId` (UUID v4). The client auto-generates one if absent.

### Error Handling

Gateway responses with `processing_status() == "ERROR"` indicate application-level errors.
Use `processing_message()` to get the human-readable error description.

### Concurrent Mode

When `enable_concurrent_mode` is `true`, a background task routes responses by `MessageId` via channels. Unsolicited (push) messages are forwarded to subscribers via `client.subscribe_incoming()`.

Note: When a `send_message` request times out, the pending entry is removed. If the gateway replies *after* the timeout, the response will arrive as an unsolicited message on the incoming channel.
