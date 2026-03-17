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

## Storing Data

`STORE_DATA` stores raw payload data associated with a unique identifier, timestamp, and location.
Unlike `STORE_EVENT`, it does not include tags.

Required fields: `event.unique_id` OR `event.id`, `event.timestamp`, `event.location`,
`event.location_separator`, `payload.data`, `payload.mime_type`.

```rust
use pod_os_client::message::{
    self, intents,
    types::{Envelope, EventFields, Message, PayloadData, PayloadFields},
};

let mut msg = Message {
    envelope: Envelope {
        to:     "neural-memory@gateway.local".to_string(),
        from:   "my-agent@gateway.local".to_string(),
        intent: intents::STORE_DATA.clone(),
        ..Default::default()
    },
    event: Some(EventFields {
        unique_id: "my-data-uuid".to_string(),
        timestamp: message::get_timestamp(),
        location:  "TERRA|47.619463|-122.518691".to_string(),
        location_separator: "|".to_string(),
        ..Default::default()
    }),
    payload: Some(PayloadFields {
        data:      PayloadData::Text("binary or text content".to_string()),
        mime_type: "application/octet-stream".to_string(),
        ..Default::default()
    }),
    ..Default::default()
};
let resp = client.send_message(&mut msg).await?;
```

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
