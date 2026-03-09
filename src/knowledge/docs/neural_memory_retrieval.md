# Pod-OS NeuralMemory Retrieval Prompts

## Get Event by ID

```rust
use pod_os_client::message::{
    intents, types::{Envelope, EventFields, GetEventOptions, Message, NeuralMemoryFields},
};

let mut msg = Message {
    envelope: Envelope {
        intent: intents::GET_EVENT.clone(),
        ..Default::default()
    },
    event: Some(EventFields {
        id: "event-001".to_string(),
        ..Default::default()
    }),
    neural_memory: Some(NeuralMemoryFields {
        get_event: Some(GetEventOptions {
            send_data: true,
            get_tags:  true,
            get_links: true,
            ..Default::default()
        }),
        ..Default::default()
    }),
    ..Default::default()
};
let resp = client.send_message(&mut msg).await?;
let event = &resp.response.as_ref().unwrap().event_records;
```

## Get Events for Tags (Search)

```rust
use pod_os_client::message::types::{GetEventsForTagsOptions, NeuralMemoryFields};

let opts = GetEventsForTagsOptions {
    event_pattern: "my-tag-key=my-tag-value".to_string(),
    get_all_data:  true,
    end_result:    100,
    ..Default::default()
};
```

## Response Fields

| Field | Description |
|-------|-------------|
| `response.status` | `"OK"` or `"ERROR"` |
| `response.message` | Human-readable message |
| `response.event_records` | Returned events (GetEventsForTags) |
| `response.total_events` | Total matching events |
| `response.returned_events` | Returned in this response |
| `response.start_result` / `end_result` | Pagination |
