//! Wire-format encoder for Pod-OS messages.
//!
//! Wire frame layout:
//! ```text
//! [9]  totalLength     x + 8 hex digits  (includes all 7 nine-byte fields + content)
//! [9]  toLength        x + 8 hex digits
//! [9]  fromLength      x + 8 hex digits
//! [9]  headerLength    x + 8 hex digits
//! [9]  messageType     9 decimal digits   (NOT hex)
//! [9]  dataType        9 decimal digits   (NOT hex)
//! [9]  payloadLength   x + 8 hex digits
//! [toLength]           to address
//! [fromLength]         from address
//! [headerLength]       tab-separated key=value header
//! [payloadLength]      raw payload bytes
//! ```

use crate::message::{
    constants::max_message_size,
    errors::{EncodeError, MsgErrCode},
    header::construct_header,
    intents,
    types::{
        BatchEventSpec, BatchLinkEventSpec, Message, PayloadData, SocketMessage, TagList,
        TagValue,
    },
};

/// Encode a `Message` into its wire representation.
///
/// `conversation_uuid` is set in the header for connection tracking; pass an
/// empty string to omit it.
pub fn encode_message(
    msg:               &Message,
    conversation_uuid: &str,
) -> Result<SocketMessage, EncodeError> {
    // ── Address validation ───────────────────────────────────────────────────
    validate_address(&msg.envelope.to, "To", MsgErrCode::EncodeInvalidToAddress)?;
    validate_address(&msg.envelope.from, "From", MsgErrCode::EncodeInvalidFromAddress)?;

    // ── Header ───────────────────────────────────────────────────────────────
    let header  = construct_header(msg, &msg.envelope.intent.clone(), conversation_uuid);
    let to      = msg.envelope.to.as_bytes();
    let from    = msg.envelope.from.as_bytes();
    let header_bytes = header.as_bytes();

    // ── Payload ──────────────────────────────────────────────────────────────
    let intent = &msg.envelope.intent;
    let payload = build_payload(msg, intent)?;
    let payload_bytes = payload.as_bytes();

    // ── Size guard ───────────────────────────────────────────────────────────
    let max = max_message_size();
    if payload_bytes.len() as i64 > max {
        return Err(EncodeError::new(
            MsgErrCode::EncodePayloadTooLarge,
            format!("payload {} bytes exceeds limit {} bytes", payload_bytes.len(), max),
        ));
    }

    // total = 6 remaining 9-byte fields + to + from + header + payload
    let content_len = 54
        + to.len()
        + from.len()
        + header_bytes.len()
        + payload_bytes.len();

    if content_len as i64 > max {
        return Err(EncodeError::new(
            MsgErrCode::EncodePayloadTooLarge,
            format!("total message {} bytes exceeds limit {} bytes", content_len, max),
        ));
    }

    // ── Assemble wire frame ─────────────────────────────────────────────────
    let msg_type  = msg.envelope.intent.message_type;
    let data_type = msg.payload.as_ref().map(|p| p.data_type.as_wire_int()).unwrap_or(0);

    let mut buf = Vec::with_capacity(9 + content_len);
    write_hex_len(&mut buf, content_len);          // totalLength
    write_hex_len(&mut buf, to.len());             // toLength
    write_hex_len(&mut buf, from.len());           // fromLength
    write_hex_len(&mut buf, header_bytes.len());   // headerLength
    write_dec_9  (&mut buf, msg_type as u64);      // messageType  (decimal)
    write_dec_9  (&mut buf, data_type as u64);     // dataType     (decimal)
    write_hex_len(&mut buf, payload_bytes.len());  // payloadLength

    buf.extend_from_slice(to);
    buf.extend_from_slice(from);
    buf.extend_from_slice(header_bytes);
    buf.extend_from_slice(payload_bytes);

    Ok(SocketMessage::new(buf))
}

// ── Length field helpers ─────────────────────────────────────────────────────

/// `x` + 8 lowercase hex digits.
#[inline]
fn write_hex_len(buf: &mut Vec<u8>, n: usize) {
    let s = format!("x{:08x}", n);
    buf.extend_from_slice(s.as_bytes());
}

/// 9 zero-padded decimal digits.
#[inline]
fn write_dec_9(buf: &mut Vec<u8>, n: u64) {
    let s = format!("{:09}", n);
    buf.extend_from_slice(s.as_bytes());
}

// ── Address validation ───────────────────────────────────────────────────────

fn validate_address(addr: &str, field: &str, code: MsgErrCode) -> Result<(), EncodeError> {
    if let Some(at) = addr.find('@') {
        let name    = &addr[..at];
        let gateway = &addr[at + 1..];
        if name.is_empty() {
            return Err(EncodeError::new(code, format!("{field}: actor name is empty")).with_field(field));
        }
        if gateway.is_empty() {
            return Err(EncodeError::new(code, format!("{field}: gateway name is empty")).with_field(field));
        }
    } else {
        return Err(EncodeError::new(code, format!("{field}: address missing '@': {addr}")).with_field(field));
    }
    Ok(())
}

// ── Payload building ─────────────────────────────────────────────────────────

fn build_payload(msg: &Message, intent: &crate::message::intents::Intent) -> Result<String, EncodeError> {
    use intents::*;
    if *intent == GATEWAY_ID || *intent == GATEWAY_STREAM_ON {
        return Ok(String::new());
    }
    if *intent == STORE_BATCH_EVENTS {
        let specs = msg.neural_memory.as_ref()
            .map(|n| n.batch_events.as_slice())
            .unwrap_or(&[]);
        return Ok(format_batch_events_payload(specs));
    }
    if *intent == STORE_BATCH_LINKS {
        let specs = msg.neural_memory.as_ref()
            .map(|n| n.batch_links.as_slice())
            .unwrap_or(&[]);
        return Ok(format_batch_link_events_payload(specs));
    }
    if *intent == STORE_BATCH_TAGS {
        // Prefer NeuralMemory.Tags, fall back to Payload data
        if let Some(nm) = &msg.neural_memory {
            if !nm.tags.is_empty() {
                return Ok(format_batch_tags_payload(&nm.tags));
            }
        }
    }

    // Default: stringify Payload.Data
    if let Some(payload) = &msg.payload {
        match &payload.data {
            PayloadData::Text(s)   => return Ok(s.clone()),
            PayloadData::Binary(b) => return Ok(String::from_utf8_lossy(b).into_owned()),
            PayloadData::Lines(lines) => return Ok(lines.join("\n")),
            PayloadData::Empty     => {}
        }
    }
    Ok(String::new())
}

// ── Payload formatters ───────────────────────────────────────────────────────

/// Format a batch of events as newline-separated, tab-delimited records.
///
/// Each event line: `field=value\tfield=value\t...`
/// Tags are appended as: `tab tag_0=freq:key=value tab tag_1=...`
pub fn format_batch_events_payload(events: &[BatchEventSpec]) -> String {
    let mut out = String::new();
    for (i, spec) in events.iter().enumerate() {
        if i > 0 { out.push('\n'); }
        let e = &spec.event;
        append_event_fields(&mut out, e);
        // Tags are 0-indexed in batch payloads (unlike header which is 1-indexed)
        for (ti, tag) in spec.tags.iter().enumerate() {
            out.push('\t');
            out.push_str(&format!("tag_{}={}:{}={}", ti, tag.frequency, tag.key, serialize_tag_value(&tag.value)));
        }
    }
    out
}

/// Format a batch of link+event specs as newline-separated records.
pub fn format_batch_link_events_payload(events: &[BatchLinkEventSpec]) -> String {
    let mut out = String::new();
    for (i, spec) in events.iter().enumerate() {
        if i > 0 { out.push('\n'); }
        append_event_fields(&mut out, &spec.event);
        let l = &spec.link;
        if !l.event_a.is_empty()    { push_field(&mut out, "event_id_a",  &l.event_a); }
        if !l.unique_id_a.is_empty() { push_field(&mut out, "unique_id_a", &l.unique_id_a); }
        if !l.event_b.is_empty()    { push_field(&mut out, "event_id_b",  &l.event_b); }
        if !l.unique_id_b.is_empty() { push_field(&mut out, "unique_id_b", &l.unique_id_b); }
        if l.strength_a != 0.0 { push_field(&mut out, "strength_a", &l.strength_a.to_string()); }
        if l.strength_b != 0.0 { push_field(&mut out, "strength_b", &l.strength_b.to_string()); }
        if !l.category.is_empty()   { push_field(&mut out, "category",    &l.category); }
        if !l.r#type.is_empty()     { push_field(&mut out, "type",        &l.r#type); }
    }
    out
}

fn append_event_fields(out: &mut String, e: &crate::message::types::EventFields) {
    let fields: &[(&str, &str)] = &[
        ("unique_id", &e.unique_id),
        ("event_id",  &e.id),
        ("local_id",  &e.local_id),
        ("owner",     &e.owner),
        ("timestamp", &e.timestamp),
        ("type",      &e.r#type),
        ("loc",       &e.location),
        ("loc_delim", &e.location_separator),
        ("mime",      &e.payload_data.mime_type),
    ];
    let mut first = true;
    for (key, val) in fields {
        if !val.is_empty() {
            if !first { out.push('\t'); }
            first = false;
            out.push_str(key);
            out.push('=');
            out.push_str(val);
        }
    }
}

fn push_field(out: &mut String, key: &str, val: &str) {
    out.push('\t');
    out.push_str(key);
    out.push('=');
    out.push_str(val);
}

/// Format tags as newline-separated `freq=key=value` lines.
pub fn format_batch_tags_payload(tags: &TagList) -> String {
    tags.iter()
        .map(|t| format!("{}={}={}", t.frequency, t.key, serialize_tag_value(&t.value)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Serialize a tag value to a wire-safe string.
pub fn serialize_tag_value(value: &TagValue) -> String {
    match value {
        TagValue::Text(s)  => s.clone(),
        TagValue::Int(n)   => n.to_string(),
        TagValue::Float(f) => f.to_string(),
        TagValue::Bool(b)  => b.to_string(),
        TagValue::Json(j)  => j.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{
        constants::set_max_message_size,
        intents,
        types::{Envelope, Message, PayloadData, PayloadFields},
    };

    fn minimal_msg(intent: &'static intents::Intent) -> Message {
        Message {
            envelope: Envelope {
                to:     "actor@gateway.local".to_string(),
                from:   "client@gateway.local".to_string(),
                intent: intent.clone(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn encode_gateway_id_has_correct_prefix() {
        let msg = minimal_msg(&intents::GATEWAY_ID);
        let sm  = encode_message(&msg, "test-uuid").unwrap();
        let s   = std::str::from_utf8(sm.as_bytes()).unwrap();
        assert!(s.starts_with('x'), "total length must start with 'x'");
    }

    #[test]
    fn encode_rejects_oversized_payload() {
        set_max_message_size(10);
        let mut msg = minimal_msg(&intents::STORE_EVENT);
        msg.payload = Some(PayloadFields {
            data: PayloadData::Text("x".repeat(100)),
            ..Default::default()
        });
        assert!(encode_message(&msg, "").is_err());
        set_max_message_size(2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn encode_rejects_missing_at_in_to() {
        let mut msg = minimal_msg(&intents::ACTOR_ECHO);
        msg.envelope.to = "no-at-sign".to_string();
        assert!(encode_message(&msg, "").is_err());
    }
}
