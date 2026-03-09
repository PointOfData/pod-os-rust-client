# Pod-OS Communication Prompts

## Overview

This document provides guidance for AI agents and software engineers on how to communicate with a Pod-OS Gateway using the Rust client.

## Connecting

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

## Addressing

All messages use the format `name@gateway` for both `To` and `From` addresses.

- **To**: `"actor@gateway.domain"` — the target actor on the gateway
- **From**: `"clientName@gateway.domain"` — your client identity

## Message IDs

Every request should have a unique `MessageId` (UUID v4). The client auto-generates one if absent.

## Error Handling

Gateway responses with `ProcessingStatus() == "ERROR"` indicate application-level errors.
Use `processing_message()` to get the human-readable error description.
