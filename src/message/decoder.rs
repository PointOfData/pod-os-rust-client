//! Wire-format decoder for Pod-OS messages.
//!
//! Mirrors the behaviour of the Go client's `message.DecodeMessage`.

use std::collections::HashMap;
use crate::message::{
    constants::max_message_size,
    errors::{DecodeError, MsgErrCode},
    intents,
    types::{
        BriefHitRecord, EventFields, LinkFields, Message, Envelope,
        PayloadData, PayloadFields, ResponseFields,
        StoreBatchEventRecord, StoreLinkBatchEventRecord, TagOutput,
    },
};

/// Minimum message size: 7 × 9-byte length fields.
const MIN_MSG_SIZE: usize = 63;

// ── Public entry point ───────────────────────────────────────────────────────

/// Decode a raw wire-format byte slice into a `Message`.
///
/// The slice must include the leading 9-byte `totalLength` field.
pub fn decode_message(raw: &[u8]) -> Result<Message, DecodeError> {
    let total = raw.len();

    // Size guards
    if total as i64 > max_message_size() {
        return Err(DecodeError::new(
            MsgErrCode::DecodePayloadTooLarge,
            format!("message {} bytes exceeds limit", total),
        ));
    }
    if total < MIN_MSG_SIZE {
        return Err(DecodeError::new(
            MsgErrCode::DecodeMessageTooShort,
            format!("message too short: {} bytes (minimum {})", total, MIN_MSG_SIZE),
        ));
    }

    // ── Parse 7 × 9-byte size fields ─────────────────────────────────────────
    let _total_len  = parse_len_field(&raw[0..9],  "totalLength")?;
    let to_len      = parse_len_field(&raw[9..18], "toLength")?;
    let from_len    = parse_len_field(&raw[18..27],"fromLength")?;
    let header_len  = parse_len_field(&raw[27..36],"headerLength")?;
    let msg_type    = parse_dec_field(&raw[36..45],"messageType")?;
    let _data_type  = parse_dec_field(&raw[45..54],"dataType")?;
    let payload_len = parse_len_field(&raw[54..63],"payloadLength")?;

    // Validate that the declared sizes fit within the actual buffer
    let expected = 63 + to_len + from_len + header_len + payload_len;
    if expected > total {
        return Err(DecodeError::new(
            MsgErrCode::DecodeInvalidSizeParam,
            format!("declared sizes total {} but buffer is {} bytes", expected, total),
        ));
    }

    // ── Slice content regions ─────────────────────────────────────────────────
    let mut pos = 63usize;
    let to_bytes     = &raw[pos..pos + to_len];      pos += to_len;
    let from_bytes   = &raw[pos..pos + from_len];    pos += from_len;
    let header_bytes = &raw[pos..pos + header_len];  pos += header_len;
    let payload_bytes = &raw[pos..pos + payload_len];

    let to   = String::from_utf8_lossy(to_bytes).into_owned();
    // Strip routing suffix from From (everything after first '|')
    let from_raw = String::from_utf8_lossy(from_bytes).into_owned();
    let from = match from_raw.find('|') {
        Some(idx) => from_raw[..idx].to_string(),
        None      => from_raw,
    };

    // ── Parse header ──────────────────────────────────────────────────────────
    let header_str = String::from_utf8_lossy(header_bytes);
    let header_map = parse_header(&header_str);

    // ── Resolve intent ────────────────────────────────────────────────────────
    // Priority: _type > _command > _db_cmd
    let command = header_map.get("_type")
        .or_else(|| header_map.get("_command"))
        .or_else(|| header_map.get("_db_cmd"))
        .map(|s| s.as_str())
        .unwrap_or("");

    let intent = intents::intent_from_message_type_and_command(msg_type, command)
        .cloned()
        .unwrap_or_default();

    // ── Build message ─────────────────────────────────────────────────────────
    let message_id = header_map.get("_msg_id").cloned().unwrap_or_default();

    let mut msg = Message {
        envelope: Envelope {
            to,
            from,
            intent: intent.clone(),
            message_id,
            ..Default::default()
        },
        ..Default::default()
    };

    // ── Transform header map into structs ──────────────────────────────────────
    transform_header(&header_map, &mut msg);

    // ── Decode payload based on intent ────────────────────────────────────────
    let payload_str = String::from_utf8_lossy(payload_bytes).into_owned();

    use intents::*;
    if intent == GET_EVENT || intent == GET_EVENT_RESPONSE {
        parse_get_event_response(&mut msg, &header_map, &payload_str);
    } else if intent == GET_EVENTS_FOR_TAGS || intent == GET_EVENTS_FOR_TAGS_RESPONSE {
        parse_get_events_for_tags_payload(&mut msg, &payload_str);
    } else if intent == STORE_BATCH_EVENTS || intent == STORE_BATCH_EVENTS_RESPONSE {
        parse_store_batch_events_payload(&mut msg, &payload_str);
    } else if intent == STORE_BATCH_LINKS || intent == STORE_BATCH_LINKS_RESPONSE {
        parse_link_event_batch_payload(&mut msg, &payload_str);
    } else if !payload_str.is_empty() {
        // Generic: store raw payload text
        msg.payload = Some(PayloadFields {
            data: PayloadData::Text(payload_str),
            ..Default::default()
        });
    }

    Ok(msg)
}

// ── Length field parsers ─────────────────────────────────────────────────────

/// Parse a 9-byte length field.  Accepts `x` + 8 hex digits OR 9 decimal digits.
fn parse_len_field(field: &[u8], name: &str) -> Result<usize, DecodeError> {
    let s = std::str::from_utf8(field).map_err(|_| {
        DecodeError::new(MsgErrCode::DecodeInvalidSizeParam, format!("{name}: non-UTF8"))
    })?;
    if s.starts_with('x') {
        usize::from_str_radix(&s[1..], 16).map_err(|_| {
            DecodeError::new(MsgErrCode::DecodeInvalidSizeParam, format!("{name}: invalid hex: {s}"))
        })
    } else {
        s.trim_start_matches('0').parse::<usize>()
            .or_else(|_| if s == "000000000" { Ok(0) } else { Err(()) })
            .map_err(|_| DecodeError::new(MsgErrCode::DecodeInvalidSizeParam, format!("{name}: invalid decimal: {s}")))
    }
}

/// Parse a 9-byte decimal integer field (messageType / dataType).
fn parse_dec_field(field: &[u8], name: &str) -> Result<i32, DecodeError> {
    let s = std::str::from_utf8(field).map_err(|_| {
        DecodeError::new(MsgErrCode::DecodeInvalidMessageType, format!("{name}: non-UTF8"))
    })?;
    s.trim_start_matches('0').parse::<i32>()
        .or_else(|_| if s == "000000000" { Ok(0) } else { Err(()) })
        .map_err(|_| DecodeError::new(MsgErrCode::DecodeInvalidMessageType, format!("{name}: invalid int: {s}")))
}

// ── Header parser ────────────────────────────────────────────────────────────

fn parse_header(s: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for part in s.split('\t') {
        if part.is_empty() { continue; }
        // Split only on the FIRST '=' (SplitN equivalent)
        if let Some(eq) = part.find('=') {
            let key = part[..eq].to_string();
            let val = part[eq + 1..].to_string();
            map.insert(key, val);
        }
    }
    map
}

// ── Header → struct transformation ──────────────────────────────────────────

fn transform_header(hm: &HashMap<String, String>, msg: &mut Message) {
    let mut response = ResponseFields::default();
    let mut event    = EventFields::default();

    for (k, v) in hm {
        match k.as_str() {
            "_status"                 => response.status = v.clone(),
            "_msg"                    => response.message = v.clone(),
            "_total_event_hits" | "_count" | "total_link_requests_found"
                                      => response.total_events = v.parse().unwrap_or(0),
            "_links_ok"               => response.storage_success_count = v.parse().unwrap_or(0),
            "_links_with_errors"      => response.storage_error_count = v.parse().unwrap_or(0),
            "_returned_event_hits"    => response.returned_events = v.parse().unwrap_or(0),
            "_start_result"           => response.start_result = v.parse().unwrap_or(0),
            "_end_result"             => response.end_result = v.parse().unwrap_or(0),
            "_set_link_count" | "_link_count" => response.link_count = v.parse().unwrap_or(0),
            "_tag_count"              => response.tag_count = v.parse().unwrap_or(0),
            "link_event"              => response.link_id = v.clone(),
            "_is_buffered"            => response.is_buffered = v == "Y" || v == "true",
            // Event fields decoded inline
            _ => decode_event_field(k, v, &mut event),
        }
    }

    if !response.status.is_empty() || response.total_events != 0 {
        msg.response = Some(response);
    }
    if !event.id.is_empty() || !event.unique_id.is_empty() || !event.owner.is_empty() {
        msg.event = Some(event);
    }
}

fn decode_event_field(key: &str, val: &str, event: &mut EventFields) {
    match key {
        "_event_id" | "event_id"     => event.id        = val.to_string(),
        "local_id" | "_event_local_id" => event.local_id = val.to_string(),
        "unique_id" | "_unique_id" | "tag:1:_unique_id"
                                     => event.unique_id  = val.to_string(),
        "event_type" | "_type" | "type" => event.r#type = val.to_string(),
        "_owner_id" | "owner" | "_event_owner" => event.owner = val.to_string(),
        "timestamp" | "_timestamp"   => event.timestamp = val.to_string(),
        "_hits"                      => event.hits = val.parse().unwrap_or(0),
        "_year"  => event.date_time.year  = val.parse().unwrap_or(0),
        "_month" => event.date_time.month = val.parse().unwrap_or(0),
        "_day"   => event.date_time.day   = val.parse().unwrap_or(0),
        "_hour"  => event.date_time.hour  = val.parse().unwrap_or(0),
        "_min"   => event.date_time.min   = val.parse().unwrap_or(0),
        "_sec"   => event.date_time.sec   = val.parse().unwrap_or(0),
        "_usec"  => event.date_time.usec  = val.parse().unwrap_or(0),
        _ => {}
    }
}

// ── GetEventsForTags payload parser ─────────────────────────────────────────

/// O(N) single-pass parser for GetEventsForTags response payloads.
///
/// Record prefixes:
/// - `_event_id=...`   → event record
/// - `_link=...`       → link record
/// - `_linktag=...`    → tags for a link
/// - `_targettag=...`  → tags for a target event
/// - `_brief_hit=...`  → brief hit record (mutually exclusive with events)
fn parse_get_events_for_tags_payload(msg: &mut Message, payload: &str) {
    let mut events:   Vec<EventFields>              = Vec::new();
    let mut brief_hits: Vec<BriefHitRecord>         = Vec::new();
    let mut event_tags: HashMap<String, Vec<TagOutput>>  = HashMap::new();
    let mut event_links: HashMap<String, Vec<LinkFields>> = HashMap::new();
    let mut link_tags:  HashMap<String, Vec<TagOutput>>  = HashMap::new();
    let mut target_tags: HashMap<String, Vec<TagOutput>> = HashMap::new();

    for line in payload.lines() {
        if line.is_empty() { continue; }
        if let Some(rest) = line.strip_prefix("_event_id=") {
            let fields = parse_tab_line(rest);
            let mut e = EventFields::default();
            e.id = fields.get("_event_id").cloned()
                .or_else(|| Some(rest.split('\t').next().unwrap_or("").to_string()))
                .unwrap_or_default();
            // Actually rest is the remainder after "_event_id="; first field is the event_id value
            // The full line is: _event_id=ID\tfield=val\t...
            // We need to re-parse the full line
            let full_fields = parse_tab_line(line);
            apply_event_fields(&full_fields, &mut e);
            events.push(e);
        } else if let Some(rest) = line.strip_prefix("_brief_hit=") {
            let parts: Vec<&str> = rest.splitn(2, '\t').collect();
            if !parts.is_empty() {
                brief_hits.push(BriefHitRecord {
                    event_id:   parts[0].to_string(),
                    total_hits: parts.get(1).and_then(|s| s.strip_prefix("_total_hits=")).and_then(|n| n.parse().ok()).unwrap_or(0),
                });
            }
        } else if let Some(rest) = line.strip_prefix("_link=") {
            let lf = parse_link_line(rest);
            let event_id = lf.event_a.clone();
            event_links.entry(event_id).or_default().push(lf);
        } else if let Some(rest) = line.strip_prefix("_linktag=") {
            // _linktag=linkId\tfreq\tcategory\tvalue
            if let Some(tag) = parse_tag_output_line(rest) {
                let link_id = rest.split('\t').next().unwrap_or("").to_string();
                link_tags.entry(link_id).or_default().push(tag);
            }
        } else if let Some(rest) = line.strip_prefix("_targettag=") {
            if let Some(tag) = parse_tag_output_line(rest) {
                let event_id = rest.split('\t').next().unwrap_or("").to_string();
                target_tags.entry(event_id).or_default().push(tag);
            }
        }
    }

    // Assembly: attach tags and links to events
    for event in &mut events {
        if let Some(tags) = event_tags.remove(&event.id) {
            event.tags = tags;
        }
        if let Some(links) = event_links.remove(&event.id) {
            for mut link in links {
                if let Some(lt) = link_tags.remove(&link.id) { link.tags = lt; }
                if let Some(tt) = target_tags.remove(&link.event_b) { link.target_tags = tt; }
                event.links.push(link);
            }
        }
    }

    let resp = msg.response.get_or_insert_with(ResponseFields::default);
    resp.event_records = events;
    resp.brief_hits    = brief_hits;
}

fn apply_event_fields(fields: &HashMap<String, String>, e: &mut EventFields) {
    for (k, v) in fields {
        match k.as_str() {
            "_event_id" | "event_id" => e.id = v.clone(),
            "unique_id" | "_unique_id" => e.unique_id = v.clone(),
            "owner"                  => e.owner = v.clone(),
            "timestamp" | "_timestamp" => e.timestamp = v.clone(),
            "_type" | "type"         => e.r#type = v.clone(),
            "_hits"                  => e.hits = v.parse().unwrap_or(0),
            _ if k.starts_with("tag:") => {
                if let Some(t) = parse_inline_tag(k, v) { e.tags.push(t); }
            }
            _ => {}
        }
    }
}

fn parse_inline_tag(key: &str, value: &str) -> Option<TagOutput> {
    // Format: tag:freq:key=value
    let parts: Vec<&str> = key.splitn(3, ':').collect();
    if parts.len() < 3 { return None; }
    let freq = parts[1].parse().unwrap_or(0);
    Some(TagOutput {
        frequency: freq,
        key:       parts[2].to_string(),
        value:     value.to_string(),
        ..Default::default()
    })
}

fn parse_link_line(s: &str) -> LinkFields {
    let fields = parse_tab_line(s);
    LinkFields {
        id:         fields.get("link_id").cloned().unwrap_or_default(),
        event_a:    fields.get("event_id_a").cloned().unwrap_or_default(),
        event_b:    fields.get("event_id_b").cloned().unwrap_or_default(),
        unique_id_a: fields.get("unique_id_a").cloned().unwrap_or_default(),
        unique_id_b: fields.get("unique_id_b").cloned().unwrap_or_default(),
        strength_a: fields.get("strength_a").and_then(|v| v.parse().ok()).unwrap_or(0.0),
        strength_b: fields.get("strength_b").and_then(|v| v.parse().ok()).unwrap_or(0.0),
        category:   fields.get("category").cloned().unwrap_or_default(),
        r#type:     fields.get("type").cloned().unwrap_or_default(),
        owner:      fields.get("owner").cloned().unwrap_or_default(),
        timestamp:  fields.get("timestamp").cloned().unwrap_or_default(),
        ..Default::default()
    }
}

fn parse_tag_output_line(s: &str) -> Option<TagOutput> {
    let parts: Vec<&str> = s.split('\t').collect();
    if parts.len() < 3 { return None; }
    Some(TagOutput {
        frequency: parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0),
        category:  parts.get(2).cloned().unwrap_or_default().to_string(),
        value:     parts.get(3).cloned().unwrap_or_default().to_string(),
        ..Default::default()
    })
}

// ── GetEvent response parser ─────────────────────────────────────────────────

fn parse_get_event_response(msg: &mut Message, hm: &HashMap<String, String>, payload: &str) {
    // Header-embedded tags: event_tag:<num>:<freq>=key=value
    let mut tags: Vec<TagOutput> = Vec::new();
    for (k, v) in hm {
        if k.starts_with("event_tag:") {
            if let Some(t) = parse_event_tag_field(k, v) {
                tags.push(t);
            }
        }
    }

    // Payload-embedded links
    let mut links: Vec<LinkFields> = Vec::new();
    let mut link_tags: HashMap<String, Vec<TagOutput>> = HashMap::new();

    for line in payload.lines() {
        if line.is_empty() { continue; }
        if let Some(rest) = line.strip_prefix("_link=") {
            links.push(parse_link_line(rest));
        } else if let Some(rest) = line.strip_prefix("_linktag\t") {
            if let Some(t) = parse_tag_output_line(rest) {
                let link_id = rest.split('\t').next().unwrap_or("").to_string();
                link_tags.entry(link_id).or_default().push(t);
            }
        }
    }

    // Attach link tags
    for link in &mut links {
        if let Some(lt) = link_tags.remove(&link.id) {
            link.tags = lt;
        }
    }

    // Apply to event
    let event = msg.event.get_or_insert_with(EventFields::default);
    event.tags  = tags;
    event.links = links;
}

/// Parse `event_tag:<num>:<freq>=key=value` → `TagOutput`.
pub fn parse_event_tag_field(key: &str, val: &str) -> Option<TagOutput> {
    // key format: event_tag:<num>:<freq>
    let parts: Vec<&str> = key.splitn(3, ':').collect();
    if parts.len() < 3 { return None; }
    let freq: i32 = parts[2].parse().unwrap_or(0);
    // val format: key=value
    if let Some(eq) = val.find('=') {
        Some(TagOutput {
            frequency: freq,
            key:       val[..eq].to_string(),
            value:     val[eq + 1..].to_string(),
            ..Default::default()
        })
    } else {
        Some(TagOutput { frequency: freq, key: val.to_string(), ..Default::default() })
    }
}

// ── StoreBatchEvents response parser ─────────────────────────────────────────

fn parse_store_batch_events_payload(msg: &mut Message, payload: &str) {
    let mut record = StoreBatchEventRecord::default();
    let mut events: Vec<EventFields> = Vec::new();

    for line in payload.lines() {
        if line.is_empty() { continue; }
        let fields = parse_tab_line(line);
        let mut e = EventFields::default();
        for (k, v) in &fields {
            match k.as_str() {
                "_status"  => { record.status = v.clone(); continue; }
                "_msg"     => { record.message = v.clone(); continue; }
                "_count"   => { record.event_count = v.parse().unwrap_or(0); continue; }
                _ => {}
            }
            decode_event_field(k, v, &mut e);
        }
        if !e.id.is_empty() || !e.unique_id.is_empty() {
            events.push(e);
        }
    }
    record.event_results = events;
    let resp = msg.response.get_or_insert_with(ResponseFields::default);
    resp.store_batch_event_record = record;
}

// ── StoreBatchLinks response parser ──────────────────────────────────────────

fn parse_link_event_batch_payload(msg: &mut Message, payload: &str) {
    let mut record = StoreLinkBatchEventRecord::default();
    let mut link_results: Vec<LinkFields> = Vec::new();

    for line in payload.lines() {
        if line.is_empty() { continue; }
        let fields = parse_tab_line(line);
        let mut is_summary = false;
        let mut lf = LinkFields::default();

        for (k, v) in &fields {
            match k.as_str() {
                "_status"                    => { record.status = v.clone(); is_summary = true; }
                "_msg"                       => record.message = v.clone(),
                "total_link_requests_found"  => record.total_link_requests_found = v.parse().unwrap_or(0),
                "_links_ok"                  => { record.links_ok = v.parse().unwrap_or(0); is_summary = true; }
                "_links_with_errors"         => record.links_with_errors = v.parse().unwrap_or(0),
                "link_id"                    => lf.id = v.clone(),
                "event_id_a"                 => lf.event_a = v.clone(),
                "event_id_b"                 => lf.event_b = v.clone(),
                "unique_id_a"                => lf.unique_id_a = v.clone(),
                "unique_id_b"                => lf.unique_id_b = v.clone(),
                "link_status"                => lf.status = v.clone(),
                _ => {}
            }
        }
        if !is_summary && (!lf.id.is_empty() || !lf.event_a.is_empty()) {
            link_results.push(lf);
        }
    }
    record.link_results = link_results;
    let resp = msg.response.get_or_insert_with(ResponseFields::default);
    resp.store_link_batch_event_record = record;
}

// ── Tab-delimited line parser ─────────────────────────────────────────────────

fn parse_tab_line(line: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for part in line.split('\t') {
        if part.is_empty() { continue; }
        if let Some(eq) = part.find('=') {
            map.insert(part[..eq].to_string(), part[eq + 1..].to_string());
        }
    }
    map
}

/// Parse tags from a payload string (tab-separated: freq\tcategory\tvalue per line).
pub fn parse_tags_from_payload(payload: &str) -> Vec<TagOutput> {
    payload.lines().filter_map(|line| {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            Some(TagOutput {
                frequency: parts[0].parse().unwrap_or(0),
                category:  parts[1].to_string(),
                value:     parts[2].to_string(),
                ..Default::default()
            })
        } else { None }
    }).collect()
}

/// Replace the `From` field in a raw wire message without re-encoding.
///
/// Surgically updates `fromLength`, `headerLength`, and `totalLength`
/// in the 9-byte fields (index 18–26, 27–35, 0–8 respectively).
pub fn replace_from_in_raw_message(raw: &[u8], new_from: &str) -> Result<Vec<u8>, DecodeError> {
    if raw.len() < MIN_MSG_SIZE {
        return Err(DecodeError::new(MsgErrCode::DecodeMessageTooShort, "raw message too short"));
    }
    let old_from_len = parse_len_field(&raw[18..27], "fromLength")?;
    let header_len   = parse_len_field(&raw[27..36], "headerLength")?;
    let payload_len  = parse_len_field(&raw[54..63], "payloadLength")?;
    let old_to_len   = parse_len_field(&raw[9..18],  "toLength")?;

    let new_from_bytes = new_from.as_bytes();
    let new_from_len   = new_from_bytes.len();
    let new_total      = 54 + old_to_len + new_from_len + header_len + payload_len;

    let mut out = Vec::with_capacity(9 + new_total);

    // totalLength
    out.extend_from_slice(format!("x{:08x}", new_total).as_bytes());
    // toLength (unchanged)
    out.extend_from_slice(&raw[9..18]);
    // fromLength (updated)
    out.extend_from_slice(format!("x{:08x}", new_from_len).as_bytes());
    // headerLength (unchanged)
    out.extend_from_slice(&raw[27..36]);
    // messageType, dataType, payloadLength (unchanged)
    out.extend_from_slice(&raw[36..63]);
    // to (unchanged)
    let to_start = 63;
    out.extend_from_slice(&raw[to_start..to_start + old_to_len]);
    // from (new)
    out.extend_from_slice(new_from_bytes);
    // header + payload (unchanged)
    let rest_start = to_start + old_to_len + old_from_len;
    out.extend_from_slice(&raw[rest_start..]);

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{
        encoder::encode_message,
        intents,
        types::{Envelope, Message},
    };

    #[test]
    fn decode_rejects_too_short() {
        assert!(decode_message(&[0u8; 10]).is_err());
    }

    #[test]
    fn roundtrip_gateway_id() {
        let msg = Message {
            envelope: Envelope {
                to:     "$system@gateway.local".to_string(),
                from:   "client@gateway.local".to_string(),
                intent: intents::GATEWAY_ID.clone(),
                client_name: "test".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let encoded  = encode_message(&msg, "").unwrap();
        let decoded  = decode_message(encoded.as_bytes()).unwrap();
        assert_eq!(decoded.envelope.to,   "$system@gateway.local");
        assert_eq!(decoded.envelope.from, "client@gateway.local");
    }
}
