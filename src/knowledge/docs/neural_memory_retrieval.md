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

## Bulk GetEvent by ID

To retrieve multiple events by ID in a single request, use `GET_EVENTS_FOR_TAGS` with a series of OR clauses — one per event. This avoids issuing one `GET_EVENT` request per event. Each search clause must use `filter_type:eq` for an exact match, and the same tag value pattern is supplied to both `low` and `filter_low`.

Which ID type to use is an either/or decision:

- **By internal EventId**: tag value pattern is `event_id=<event_id>`
- **By developer UniqueId**: tag value pattern is `\x01u=<unique_id>`. The `\x01` byte prefix (ASCII SOH, `0x01`) is required to avoid collisions with user-defined tag keys.

```rust
use pod_os_client::message::{
    intents,
    types::{Envelope, GetEventsForTagsOptions, Message, NeuralMemoryFields, SearchOptions},
};

// Option A — by internal EventId
let event_ids = vec![
    "2024.01.15.14.30.45.123456@actor1|location1|segment1",
    "2024.01.16.09.00.00.000001@actor1|location1|segment1",
];
let clauses: String = event_ids
    .iter()
    .map(|eid| {
        let val = format!("event_id={}", eid);
        format!("clause_type:S\tboolean:or\tlow:{}\tfilter_type:eq\tfilter_low:{}\n", val, val)
    })
    .collect();

// Option B — by developer UniqueId (\x01 prefix avoids tag key collisions)
let unique_ids = vec!["uid-001", "uid-002", "uid-003"];
let clauses: String = unique_ids
    .iter()
    .map(|uid| {
        let val = format!("\x01u={}", uid);
        format!("clause_type:S\tboolean:or\tlow:{}\tfilter_type:eq\tfilter_low:{}\n", val, val)
    })
    .collect();

let mut msg = Message {
    envelope: Envelope {
        intent: intents::GET_EVENTS_FOR_TAGS.clone(),
        ..Default::default()
    },
    neural_memory: Some(NeuralMemoryFields {
        search: Some(SearchOptions {
            clause:         clauses,
            buffer_results: true,
            buffer_format:  "0".to_string(),
            ..Default::default()
        }),
        get_events_for_tags: Some(GetEventsForTagsOptions {
            buffer_results: true,
            ..Default::default()
        }),
        ..Default::default()
    }),
    ..Default::default()
};

let resp = client.send_message(&mut msg).await?;
let events = &resp.response.as_ref().unwrap().event_records;
```

Each clause uses `boolean:or` so the result set accumulates one event per ID. The `filter_type:eq` enforces an exact match against the tag value. The response `event_records` list will contain one entry per matched event.

## Response Fields

| Field | Description |
|-------|-------------|
| `response.status` | `"OK"` or `"ERROR"` |
| `response.message` | Human-readable message |
| `response.event_records` | Returned events (GetEventsForTags) |
| `response.total_events` | Total matching events |
| `response.returned_events` | Returned in this response |
| `response.start_result` / `end_result` | Pagination |
