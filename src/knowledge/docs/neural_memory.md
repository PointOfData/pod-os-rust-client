# Pod-OS NeuralMemory Event Prompts

## Storing Events

```rust
use pod_os_client::message::{
    self, intents, types::{Envelope, EventFields, Message},
};

let mut msg = Message {
    envelope: Envelope {
        to:     "neural-memory@gateway.local".to_string(),
        from:   "my-agent@gateway.local".to_string(),
        intent: intents::STORE_EVENT.clone(),
        ..Default::default()
    },
    event: Some(EventFields {
        id:        "event-001".to_string(),
        owner:     "owner-001".to_string(),
        timestamp: message::get_timestamp(),
        r#type:    "observation".to_string(),
        ..Default::default()
    }),
    ..Default::default()
};
let resp = client.send_message(&mut msg).await?;
```

## Batch Store

Use `STORE_BATCH_EVENTS` with `neural_memory.batch_events` for high-throughput ingestion (100K+ msg/s).

## Linking Events

```rust
use pod_os_client::message::{intents, types::{LinkFields, NeuralMemoryFields, Message, Envelope}};

let mut msg = Message {
    envelope: Envelope {
        intent: intents::LINK_EVENT.clone(),
        ..Default::default()
    },
    neural_memory: Some(NeuralMemoryFields {
        link: Some(LinkFields {
            owner:     "owner-001".to_string(),
            event_a:   "event-001".to_string(),
            event_b:   "event-002".to_string(),
            strength_a: 1.0,
            strength_b: 1.0,
            ..Default::default()
        }),
        ..Default::default()
    }),
    ..Default::default()
};
```
