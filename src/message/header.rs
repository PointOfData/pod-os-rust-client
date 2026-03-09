//! Per-intent header builders.
//!
//! The header is a tab-separated sequence of `key=value` pairs.  Most
//! `key=value` pairs are built by the hand-written functions below; the
//! `mapping.rs` (HeaderBuilder) reflection-based approach is kept as an
//! alternative for GetEvent / GetEventsForTags.

use crate::message::{
    intents::{self, Intent},
    types::Message,
};

/// Build the full header string for a message.
///
/// `connection_id_uuid` is the per-connection identifier sent by the gateway.
pub fn construct_header(msg: &Message, intent: &Intent, _connection_id_uuid: &str) -> String {
    use intents::*;

    if *intent == GATEWAY_ID         { return gateway_identify_connection_header(msg); }
    if *intent == GATEWAY_STREAM_ON  { return gateway_stream_on_header(msg); }
    if *intent == ACTOR_ECHO         { return actor_echo_header(msg); }
    if *intent == STORE_EVENT        { return store_event_message_header(msg); }
    if *intent == STORE_BATCH_EVENTS { return store_batch_events_message_header(msg); }
    if *intent == STORE_BATCH_TAGS   { return store_batch_tags_message_header(msg); }
    if *intent == GET_EVENT          { return get_event_message_header(msg); }
    if *intent == GET_EVENTS_FOR_TAGS { return get_events_for_tags_message_header(msg); }
    if *intent == LINK_EVENT         { return link_events_message_header(msg); }
    if *intent == UNLINK_EVENT       { return unlink_events_message_header(msg); }
    if *intent == STORE_BATCH_LINKS  { return batch_link_events_message_header(msg); }

    // Fallback: just include _msg_id if present
    if !msg.envelope.message_id.is_empty() {
        format!("_msg_id={}", msg.envelope.message_id)
    } else {
        String::new()
    }
}

// ── Per-intent header builders ───────────────────────────────────────────────

fn gateway_identify_connection_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add_if_nonempty("id:passcode", &msg.envelope.passcode);
    h.add_if_nonempty("id:user",     &msg.envelope.user_name);
    h.add_if_nonempty("id:name",     &msg.envelope.client_name);
    h.add_if_nonempty("_msg_id",     &msg.envelope.message_id);
    h.build()
}

fn gateway_stream_on_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

fn actor_echo_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

fn store_event_message_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add("_db_cmd", "store");

    if let Some(event) = &msg.event {
        h.add_if_nonempty("unique_id",  &event.unique_id);
        h.add_if_nonempty("event_id",   &event.id);
        h.add_if_nonempty("owner",      &event.owner);
        h.add_if_nonempty("timestamp",  &event.timestamp);
        let loc_sep = if event.location_separator.is_empty() { "|" } else { &event.location_separator };
        h.add_if_nonempty("loc_delim",  loc_sep);
        h.add_if_nonempty("loc",        &event.location);
        h.add_if_nonempty("type",       &event.r#type);
        if let Some(payload) = &msg.payload {
            h.add_if_nonempty("mime", &payload.mime_type);
        }
        // Tags: 1-indexed, 4-digit zero-padded, format: freq:key=value
        for (i, tag) in event.tags.iter().enumerate() {
            let key = format!("tag_{:04}", i + 1);
            let val = format!("{}:{}={}", tag.frequency, tag.key, tag.value);
            h.add(&key, &val);
        }
    }
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

fn store_batch_events_message_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add("_db_cmd", "store_batch");
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

fn store_batch_tags_message_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add("_db_cmd", "tag_store_batch");
    if let Some(event) = &msg.event {
        h.add_if_nonempty("event_id",  &event.id);
        h.add_if_nonempty("unique_id", &event.unique_id);
        h.add_if_nonempty("owner",     &event.owner);
        h.add_if_nonempty("owner_unique_id", &event.owner_unique_id);
    }
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

fn get_event_message_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add("_db_cmd", "get");

    if let Some(event) = &msg.event {
        h.add_if_nonempty("event_id",  &event.id);
        h.add_if_nonempty("unique_id", &event.unique_id);
    }

    if let Some(opts) = msg.get_event_opts() {
        if opts.send_data     { h.add("send_data", "Y"); }
        if opts.local_id_only { h.add("local_id_only", "Y"); }
        if opts.get_tags      { h.add("get_tags", "Y"); }
        if opts.get_links     { h.add("get_links", "Y"); }
        if opts.get_link_tags { h.add("get_link_tags", "Y"); }
        if opts.get_target_tags { h.add("get_target_tags", "Y"); }
        if opts.first_link != 0 { h.add("first_link",  &opts.first_link.to_string()); }
        if opts.link_count  != 0 { h.add("link_count", &opts.link_count.to_string()); }
        if let Some(tf) = opts.tag_format {
            h.add("tag_format", &tf.to_string());
        }
        h.add("request_format", &opts.request_format.to_string());
        h.add_if_nonempty("event_facet_filter",  &opts.event_facet_filter);
        h.add_if_nonempty("link_facet_filter",   &opts.link_facet_filter);
        h.add_if_nonempty("target_facet_filter", &opts.target_facet_filter);
        h.add_if_nonempty("category_filter",     &opts.category_filter);
        h.add_if_nonempty("tag_filter",          &opts.tag_filter);
    }
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

fn get_events_for_tags_message_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add("_db_cmd", "events_for_tag");

    if let Some(opts) = msg.get_events_for_tags_opts() {
        h.add_if_nonempty("event_pattern",      &opts.event_pattern);
        h.add_if_nonempty("event_pattern_high", &opts.event_pattern_high);
        h.add("buffer_results", if opts.buffer_results { "Y" } else { "N" });
        if opts.include_brief_hits   { h.add("include_brief_hits", "Y"); }
        if opts.get_all_data         { h.add("get_all_data", "Y"); }
        if opts.count_only           { h.add("count_only", "Y"); }
        if opts.get_match_links      { h.add("get_match_links", "Y"); }
        if opts.count_match_links    { h.add("count_match_links", "Y"); }
        if opts.get_link_tags        { h.add("get_link_tags", "Y"); }
        if opts.get_target_tags      { h.add("get_target_tags", "Y"); }
        if opts.get_event_object_count { h.add("get_event_object_count", "Y"); }
        if opts.include_tag_stats    { h.add("include_tag_stats", "Y"); }
        if opts.invert_hit_tag_filter { h.add("invert_hit_tag_filter", "Y"); }
        if opts.first_link != 0      { h.add("first_link",  &opts.first_link.to_string()); }
        if opts.link_count != 0      { h.add("link_count",  &opts.link_count.to_string()); }
        if opts.events_per_message != 0 { h.add("events_per_message", &opts.events_per_message.to_string()); }
        if opts.start_result != 0    { h.add("start_result", &opts.start_result.to_string()); }
        if opts.end_result != 0      { h.add("end_result",   &opts.end_result.to_string()); }
        if opts.min_event_hits != 0  { h.add("min_event_hits", &opts.min_event_hits.to_string()); }
        h.add_if_nonempty("link_tag_filter",       &opts.link_tag_filter);
        h.add_if_nonempty("linked_events_filter",  &opts.linked_events_filter);
        h.add_if_nonempty("link_category",         &opts.link_category);
        h.add_if_nonempty("owner",                 &opts.owner);
        h.add_if_nonempty("owner_unique_id",       &opts.owner_unique_id);
        h.add_if_nonempty("hit_tag_filter",        &opts.hit_tag_filter);
        h.add("buffer_format", if opts.buffer_format.is_empty() { "0" } else { &opts.buffer_format });
    }
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

fn link_events_message_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add("_db_cmd", "link");

    if let Some(link) = msg.link() {
        h.add_if_nonempty("event_id",  &link.id);
        h.add_if_nonempty("owner",     &link.owner);
        // Use event_id_a/b or unique_id_a/b
        if !link.event_a.is_empty() {
            h.add("event_id_a", &link.event_a);
        } else {
            h.add_if_nonempty("unique_id_a", &link.unique_id_a);
        }
        if !link.event_b.is_empty() {
            h.add("event_id_b", &link.event_b);
        } else {
            h.add_if_nonempty("unique_id_b", &link.unique_id_b);
        }
        if link.strength_a != 0.0 { h.add("strength_a", &link.strength_a.to_string()); }
        if link.strength_b != 0.0 { h.add("strength_b", &link.strength_b.to_string()); }
        h.add_if_nonempty("category",   &link.category);
        let loc_sep = if link.location_separator.is_empty() { "|" } else { &link.location_separator };
        h.add_if_nonempty("loc_delim",  loc_sep);
        h.add_if_nonempty("loc",        &link.location);
        h.add_if_nonempty("type",       &link.r#type);
        h.add_if_nonempty("timestamp",  &link.timestamp);
        // Owner reference
        if !link.owner_id.is_empty() {
            h.add("owner_event_id", &link.owner_id);
        } else {
            h.add_if_nonempty("owner_unique_id", &link.owner_unique_id);
        }
    }
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

fn unlink_events_message_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add("_db_cmd", "unlink");

    if let Some(link) = msg.link() {
        h.add_if_nonempty("owner", &link.owner);
        if !link.id.is_empty() {
            h.add("event_id", &link.id);
        } else {
            h.add_if_nonempty("unique_id", &link.unique_id);
        }
        let loc_sep = if link.location_separator.is_empty() { "|" } else { &link.location_separator };
        h.add_if_nonempty("loc_delim", loc_sep);
        h.add_if_nonempty("loc",       &link.location);
        h.add_if_nonempty("timestamp", &link.timestamp);
    }
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

fn batch_link_events_message_header(msg: &Message) -> String {
    let mut h = Header::new();
    h.add("_db_cmd", "link_batch");
    h.add_if_nonempty("_msg_id", &msg.envelope.message_id);
    h.build()
}

// ── Header builder helper ────────────────────────────────────────────────────

struct Header {
    parts: Vec<String>,
}

impl Header {
    fn new() -> Self { Self { parts: Vec::new() } }

    fn add(&mut self, key: &str, value: &str) {
        self.parts.push(format!("{}={}", key, value));
    }

    fn add_if_nonempty(&mut self, key: &str, value: &str) {
        if !value.is_empty() {
            self.add(key, value);
        }
    }

    fn build(self) -> String {
        self.parts.join("\t")
    }
}
