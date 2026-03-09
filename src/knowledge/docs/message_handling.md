# Pod-OS Message Handling Prompts

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

## Timestamps

Always use the Pod-OS timestamp format: `+NNNNNNNNNN.NNNNNN` (POSIX epoch with 6 decimal microseconds).

```rust
use pod_os_client::message::get_timestamp;
let ts = get_timestamp(); // e.g. "+1741388400.123456"
```

## Tags

Tags are structured as `Tag { frequency, key, value }`.  In the wire header:
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
