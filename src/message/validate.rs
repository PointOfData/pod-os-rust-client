//! Opt-in message validation, controlled by the `PODOS_VALIDATE` environment variable.
//!
//! Set `PODOS_VALIDATE=1` (or `true` / `yes`) to enable.  When disabled, all
//! validation functions return empty results immediately — zero cost on the hot
//! path.
//!
//! Produces dual-audience (engineer + LLM) validation errors for every intent,
//! covering envelope requirements, intent-specific struct fields, NeuralMemory
//! payload fields, and raw wire-format header correctness.

use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::HashMap;

use crate::message::{intents, types::Message};

// ── Global gate ──────────────────────────────────────────────────────────────

static VALIDATE_ENABLED: Lazy<bool> = Lazy::new(|| {
    std::env::var("PODOS_VALIDATE")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
});

pub fn validation_enabled() -> bool {
    *VALIDATE_ENABLED
}

// ── ValidationError ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub severity: String,
    pub intent: String,
    pub field: String,
    pub wire_field: String,
    pub rule: String,
    pub message: String,
    pub fix: String,
    pub example_code: String,
    pub references: Vec<String>,
}

pub type ValidationErrors = Vec<ValidationError>;

/// Format errors for terminal output using the dual-audience format.
///
/// ```text
/// [ERROR] LinkEvent / NeuralMemory.Link.Category (category): required
///   What: Category is required for LinkEvent and is missing.
///   Fix:  Set NeuralMemory.Link.Category to a non-empty string (e.g. "related").
///   Code: msg.neural_memory.link.category = "related".into()
/// ```
#[derive(Debug)]
pub struct ValidationReport(pub ValidationErrors);

impl std::fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for e in &self.0 {
            let tag = if e.severity == "error" {
                "[ERROR]"
            } else {
                "[WARN]"
            };
            let wire = if e.wire_field.is_empty() {
                String::new()
            } else {
                format!(" ({})", e.wire_field)
            };
            writeln!(
                f,
                "{} {} / {}{}: {}",
                tag, e.intent, e.field, wire, e.rule
            )?;
            writeln!(f, "  What: {}", e.message)?;
            if !e.fix.is_empty() {
                writeln!(f, "  Fix:  {}", e.fix)?;
            }
            if !e.example_code.is_empty() {
                writeln!(f, "  Code: {}", e.example_code)?;
            }
        }
        Ok(())
    }
}

/// Extension methods for `ValidationErrors` (`Vec<ValidationError>`).
pub trait ValidationErrorsExt {
    /// Format as JSON array for LLM prompt injection.
    fn llm_json(&self) -> String;
}

impl ValidationErrorsExt for ValidationErrors {
    fn llm_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

// ── Struct-level validation ──────────────────────────────────────────────────

impl Message {
    /// Validate the message struct.  Returns an empty `Vec` when
    /// `PODOS_VALIDATE` is not set, making it free to call on the hot path.
    pub fn validate(&self) -> ValidationErrors {
        if !validation_enabled() {
            return Vec::new();
        }
        let mut errs = ValidationErrors::new();
        validate_envelope(self, &mut errs);
        if !self.envelope.intent.is_zero() {
            dispatch_intent_validator(self, &mut errs);
        }
        errs
    }
}

fn validate_envelope(msg: &Message, errs: &mut ValidationErrors) {
    let intent_name = msg.envelope.intent.name;

    if msg.envelope.to.is_empty() {
        push_err(
            errs,
            "error",
            intent_name,
            "Envelope.To",
            "to",
            "required",
            "To address is required and is missing",
            "Set Envelope.To to \"actor@gateway\" format",
            "msg.envelope.to = \"actor@gateway.local\".into()",
        );
    } else if !msg.envelope.to.contains('@') {
        push_err(
            errs,
            "error",
            intent_name,
            "Envelope.To",
            "to",
            "format",
            "To address must match name@gateway format",
            "Include '@' separator between actor name and gateway",
            "msg.envelope.to = \"actor@gateway.local\".into()",
        );
    }

    if msg.envelope.from.is_empty() {
        push_err(
            errs,
            "error",
            intent_name,
            "Envelope.From",
            "from",
            "required",
            "From address is required and is missing",
            "Set Envelope.From to \"client@gateway\" format",
            "msg.envelope.from = \"client@gateway.local\".into()",
        );
    } else if !msg.envelope.from.contains('@') {
        push_err(
            errs,
            "error",
            intent_name,
            "Envelope.From",
            "from",
            "format",
            "From address must match name@gateway format",
            "Include '@' separator between client name and gateway",
            "msg.envelope.from = \"client@gateway.local\".into()",
        );
    }

    if msg.envelope.intent.is_zero() {
        push_err(
            errs,
            "error",
            intent_name,
            "Envelope.Intent",
            "intent",
            "required",
            "Intent must be non-zero (set to a valid intent)",
            "Assign an intent constant (e.g. intents::STORE_EVENT)",
            "msg.envelope.intent = intents::STORE_EVENT.clone()",
        );
    }

    if msg.envelope.intent == intents::GATEWAY_ID && msg.envelope.client_name.is_empty() {
        push_err(
            errs,
            "error",
            intent_name,
            "Envelope.ClientName",
            "id:name",
            "required",
            "ClientName is required for GatewayId",
            "Set Envelope.ClientName to your client identifier",
            "msg.envelope.client_name = \"my-client\".into()",
        );
    }
}

fn dispatch_intent_validator(msg: &Message, errs: &mut ValidationErrors) {
    use intents::*;
    let intent = &msg.envelope.intent;

    if *intent == STORE_EVENT {
        validate_store_event(msg, errs);
    } else if *intent == STORE_BATCH_EVENTS {
        validate_store_batch_events(msg, errs);
    } else if *intent == STORE_BATCH_TAGS {
        validate_store_batch_tags(msg, errs);
    } else if *intent == GET_EVENT {
        validate_get_event(msg, errs);
    } else if *intent == GET_EVENTS_FOR_TAGS {
        validate_get_events_for_tags(msg, errs);
    } else if *intent == LINK_EVENT {
        validate_link_event(msg, errs);
    } else if *intent == UNLINK_EVENT {
        validate_unlink_event(msg, errs);
    } else if *intent == STORE_BATCH_LINKS {
        validate_store_batch_links(msg, errs);
    } else if *intent == GATEWAY_ID {
        validate_gateway_id(msg, errs);
    } else if *intent == GATEWAY_STREAM_ON || *intent == GATEWAY_STREAM_OFF {
        // No required fields beyond envelope
    } else if *intent == ACTOR_REQUEST {
        // Header writes _type=status automatically; no struct validation needed
    } else if *intent == ACTOR_RESPONSE || *intent == STATUS {
        // All fields optional per spec
    } else if *intent == ACTOR_REPORT {
        validate_actor_report(msg, errs);
    } else if intent.message_type == 1001 {
        validate_response_intent(msg, errs);
    }
}

// ── Per-intent struct validators ─────────────────────────────────────────────

fn validate_store_event(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "StoreEvent";
    let event = match &msg.event {
        None => {
            push_err(
                errs,
                "error",
                intent,
                "Message.Event",
                "",
                "nil_struct",
                "Event fields are required for StoreEvent",
                "Initialize msg.event = Some(EventFields { owner: ..., location: ..., .. })",
                "",
            );
            return;
        }
        Some(e) => e,
    };

    if event.timestamp.is_empty() {
        push_err(
            errs,
            "warn",
            intent,
            "Event.Timestamp",
            "timestamp",
            "required",
            "Timestamp is required; encoder auto-fills if empty but explicit is preferred",
            "Set Event.Timestamp to a UTC timestamp string",
            "event.timestamp = get_timestamp()",
        );
    }

    if event.owner.is_empty() && event.owner_unique_id.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "Event.Owner/OwnerUniqueID",
            "owner/owner_unique_id",
            "one_of_required",
            "Either Owner or OwnerUniqueID is required for StoreEvent",
            "Set event.owner or event.owner_unique_id",
            "event.owner = \"owner-id\".into()",
        );
    }

    required_field(
        errs,
        intent,
        "Event.Location",
        "loc",
        &event.location,
        "Location is required for StoreEvent",
    );
    required_field(
        errs,
        intent,
        "Event.LocationSeparator",
        "loc_delim",
        &event.location_separator,
        "LocationSeparator is required for StoreEvent",
    );
}

fn validate_store_batch_events(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "StoreBatchEvents";
    let specs = match &msg.neural_memory {
        Some(nm) => &nm.batch_events,
        None => {
            push_err(
                errs,
                "error",
                intent,
                "Message.NeuralMemory",
                "",
                "nil_struct",
                "NeuralMemory is required for StoreBatchEvents",
                "Initialize msg.neural_memory with batch_events",
                "",
            );
            return;
        }
    };

    if specs.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "NeuralMemory.BatchEvents",
            "",
            "required",
            "At least one BatchEventSpec is required",
            "Push events into neural_memory.batch_events",
            "",
        );
        return;
    }

    for (i, spec) in specs.iter().enumerate() {
        let e = &spec.event;
        let prefix = format!("NeuralMemory.BatchEvents[{}]", i);

        if e.timestamp.is_empty() {
            push_err(
                errs,
                "warn",
                intent,
                &format!("{}.Event.Timestamp", prefix),
                "timestamp",
                "required",
                &format!("Timestamp is required for batch event {}", i),
                "Set event.timestamp",
                "",
            );
        }
        if e.owner.is_empty() && e.owner_unique_id.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Event.Owner/OwnerUniqueID", prefix),
                "owner/owner_unique_id",
                "one_of_required",
                &format!(
                    "Either Owner or OwnerUniqueID is required for batch event {}",
                    i
                ),
                "Set event.owner or event.owner_unique_id",
                "",
            );
        }
        if e.location.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Event.Location", prefix),
                "loc",
                "required",
                &format!("Location is required for batch event {}", i),
                "Set event.location",
                "",
            );
        }
        if e.location_separator.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Event.LocationSeparator", prefix),
                "loc_delim",
                "required",
                &format!("LocationSeparator is required for batch event {}", i),
                "Set event.location_separator",
                "",
            );
        }
    }
}

fn validate_store_batch_tags(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "StoreBatchTags";

    let has_event_ref = msg
        .event
        .as_ref()
        .map(|e| !e.id.is_empty() || !e.unique_id.is_empty())
        .unwrap_or(false);
    if !has_event_ref {
        push_err(
            errs,
            "error",
            intent,
            "Event.Id/UniqueId",
            "event_id/unique_id",
            "one_of_required",
            "Either Event.Id or Event.UniqueId is required to identify the target event",
            "Set event.id or event.unique_id",
            "",
        );
    }

    let has_owner = msg
        .event
        .as_ref()
        .map(|e| !e.owner.is_empty() || !e.owner_unique_id.is_empty())
        .unwrap_or(false);
    if !has_owner {
        push_err(
            errs,
            "error",
            intent,
            "Event.Owner/OwnerUniqueID",
            "owner/owner_unique_id",
            "one_of_required",
            "Either Owner or OwnerUniqueID is required for StoreBatchTags",
            "Set event.owner or event.owner_unique_id",
            "",
        );
    }

    let tags = match &msg.neural_memory {
        Some(nm) => &nm.tags,
        None => {
            push_err(
                errs,
                "error",
                intent,
                "Message.NeuralMemory",
                "",
                "nil_struct",
                "NeuralMemory is required for StoreBatchTags",
                "Initialize msg.neural_memory with tags",
                "",
            );
            return;
        }
    };

    if tags.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "NeuralMemory.Tags",
            "",
            "required",
            "Tags list must not be empty",
            "Push tags into neural_memory.tags",
            "",
        );
        return;
    }

    for (i, tag) in tags.iter().enumerate() {
        if tag.key.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("NeuralMemory.Tags[{}].Key", i),
                "key",
                "payload_format",
                &format!("Tag key must not be empty at index {}", i),
                "Set tag.key to a non-empty string",
                "",
            );
        }
    }
}

fn validate_get_event(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "GetEvent";
    let event = match &msg.event {
        None => {
            push_err(
                errs,
                "error",
                intent,
                "Message.Event",
                "",
                "nil_struct",
                "Event fields are required for GetEvent (to identify the event)",
                "Initialize msg.event = Some(EventFields { id: ... OR unique_id: ... })",
                "",
            );
            return;
        }
        Some(e) => e,
    };

    if event.id.is_empty() && event.unique_id.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "Event.Id/UniqueId",
            "event_id/unique_id",
            "one_of_required",
            "Either Event.Id or Event.UniqueId is required to identify the event",
            "Set event.id or event.unique_id",
            "",
        );
    }
}

fn validate_get_events_for_tags(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "GetEventsForTags";
    let nm = match &msg.neural_memory {
        None => {
            push_err(
                errs,
                "error",
                intent,
                "Message.NeuralMemory",
                "",
                "nil_struct",
                "NeuralMemory is required for GetEventsForTags",
                "Initialize msg.neural_memory = Some(NeuralMemoryFields { get_events_for_tags: Some(...), .. })",
                "",
            );
            return;
        }
        Some(nm) => nm,
    };

    if nm.get_events_for_tags.is_none() {
        push_err(
            errs,
            "error",
            intent,
            "NeuralMemory.GetEventsForTags",
            "",
            "nil_struct",
            "GetEventsForTags options struct is required",
            "Initialize neural_memory.get_events_for_tags = Some(GetEventsForTagsOptions { ... })",
            "",
        );
    }
    // All individual fields within GetEventsForTagsOptions are OPTIONAL.
    // msg.Event is NOT required and NOT dereferenced by the header builder.
}

fn validate_link_event(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "LinkEvent";
    let link = match msg.link() {
        None => {
            push_err(
                errs,
                "error",
                intent,
                "NeuralMemory.Link",
                "",
                "nil_struct",
                "Link fields are required for LinkEvent",
                "Initialize neural_memory.link = Some(LinkFields { ... })",
                "",
            );
            return;
        }
        Some(l) => l,
    };

    // (EventA AND EventB) OR (UniqueIdA AND UniqueIdB)
    let has_event_pair = !link.event_a.is_empty() && !link.event_b.is_empty();
    let has_unique_pair = !link.unique_id_a.is_empty() && !link.unique_id_b.is_empty();
    if !has_event_pair && !has_unique_pair {
        push_err(
            errs,
            "error",
            intent,
            "Link.EventA+EventB / UniqueIdA+UniqueIdB",
            "event_id_a+event_id_b / unique_id_a+unique_id_b",
            "one_of_required",
            "Either (EventA and EventB) or (UniqueIdA and UniqueIdB) are required",
            "Set link.event_a + link.event_b, or link.unique_id_a + link.unique_id_b",
            "",
        );
    }

    required_field(
        errs,
        intent,
        "Link.Category",
        "category",
        &link.category,
        "Category is required for LinkEvent",
    );
    required_field(
        errs,
        intent,
        "Link.Timestamp",
        "timestamp",
        &link.timestamp,
        "Timestamp is required for LinkEvent (belongs to Link, not Event)",
    );
    required_field(
        errs,
        intent,
        "Link.Location",
        "loc",
        &link.location,
        "Location is required for LinkEvent",
    );
    required_field(
        errs,
        intent,
        "Link.LocationSeparator",
        "loc_delim",
        &link.location_separator,
        "LocationSeparator is required for LinkEvent",
    );

    // OwnerID OR OwnerUniqueID
    if link.owner_id.is_empty() && link.owner_unique_id.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "Link.OwnerID/OwnerUniqueID",
            "owner_event_id/owner_unique_id",
            "one_of_required",
            "Either OwnerID or OwnerUniqueID is required for LinkEvent",
            "Set link.owner_id or link.owner_unique_id",
            "",
        );
    }
}

fn validate_unlink_event(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "UnlinkEvent";
    // UnlinkEvent uses NeuralMemory.Link (same struct as LinkEvent),
    // consistent with the header builder which reads msg.link().
    let link = match msg.link() {
        None => {
            push_err(
                errs,
                "error",
                intent,
                "NeuralMemory.Link",
                "",
                "nil_struct",
                "Link fields are required for UnlinkEvent (set neural_memory.link, not unlink)",
                "Initialize neural_memory.link = Some(LinkFields { id: ... OR unique_id: ... })",
                "",
            );
            return;
        }
        Some(l) => l,
    };

    if link.id.is_empty() && link.unique_id.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "Link.Id/UniqueId",
            "event_id/unique_id",
            "one_of_required",
            "Either Link.Id or Link.UniqueId is required to identify the link event",
            "Set link.id or link.unique_id",
            "",
        );
    }

    // LocationSeparator is required if Location is present
    if !link.location.is_empty() && link.location_separator.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "Link.LocationSeparator",
            "loc_delim",
            "required",
            "LocationSeparator is required when Location is provided",
            "Set link.location_separator (e.g. \"|\")",
            "",
        );
    }
}

fn validate_store_batch_links(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "StoreBatchLinks";
    let specs = match &msg.neural_memory {
        Some(nm) => &nm.batch_links,
        None => {
            push_err(
                errs,
                "error",
                intent,
                "Message.NeuralMemory",
                "",
                "nil_struct",
                "NeuralMemory is required for StoreBatchLinks",
                "Initialize msg.neural_memory with batch_links",
                "",
            );
            return;
        }
    };

    if specs.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "NeuralMemory.BatchLinks",
            "",
            "required",
            "At least one BatchLinkEventSpec is required",
            "Push specs into neural_memory.batch_links",
            "",
        );
        return;
    }

    for (i, spec) in specs.iter().enumerate() {
        let prefix = format!("NeuralMemory.BatchLinks[{}]", i);
        let e = &spec.event;
        let l = &spec.link;

        // Event fields
        if e.timestamp.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Event.Timestamp", prefix),
                "timestamp",
                "required",
                &format!("Event timestamp is required for batch link {}", i),
                "Set event.timestamp",
                "",
            );
        }
        if e.owner.is_empty() && e.owner_unique_id.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Event.Owner/OwnerUniqueID", prefix),
                "owner/owner_unique_id",
                "one_of_required",
                &format!(
                    "Either event Owner or OwnerUniqueID is required for batch link {}",
                    i
                ),
                "Set event.owner or event.owner_unique_id",
                "",
            );
        }
        if e.location.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Event.Location", prefix),
                "loc",
                "required",
                &format!("Event location is required for batch link {}", i),
                "Set event.location",
                "",
            );
        }
        if e.location_separator.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Event.LocationSeparator", prefix),
                "loc_delim",
                "required",
                &format!("Event location separator is required for batch link {}", i),
                "Set event.location_separator",
                "",
            );
        }

        // Link fields
        if l.timestamp.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Link.Timestamp", prefix),
                "timestamp",
                "required",
                &format!(
                    "Link timestamp is required for batch link {} (not auto-generated)",
                    i
                ),
                "Set link.timestamp explicitly",
                "",
            );
        }
        let has_event_pair = !l.event_a.is_empty() && !l.event_b.is_empty();
        let has_unique_pair = !l.unique_id_a.is_empty() && !l.unique_id_b.is_empty();
        if !has_event_pair && !has_unique_pair {
            push_err(
                errs,
                "error",
                intent,
                &format!(
                    "{}.Link.EventA+EventB / UniqueIdA+UniqueIdB",
                    prefix
                ),
                "event_id_a+event_id_b / unique_id_a+unique_id_b",
                "one_of_required",
                &format!(
                    "Either (EventA+EventB) or (UniqueIdA+UniqueIdB) required for batch link {}",
                    i
                ),
                "Set link.event_a+link.event_b or link.unique_id_a+link.unique_id_b",
                "",
            );
        }
        if l.category.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Link.Category", prefix),
                "category",
                "required",
                &format!("Link category is required for batch link {}", i),
                "Set link.category",
                "",
            );
        }
        if l.owner_id.is_empty() && l.owner_unique_id.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Link.OwnerID/OwnerUniqueID", prefix),
                "owner_event_id/owner_unique_id",
                "one_of_required",
                &format!(
                    "Either link OwnerID or OwnerUniqueID is required for batch link {}",
                    i
                ),
                "Set link.owner_id or link.owner_unique_id",
                "",
            );
        }
        if l.location.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Link.Location", prefix),
                "loc",
                "required",
                &format!("Link location is required for batch link {}", i),
                "Set link.location",
                "",
            );
        }
        if l.location_separator.is_empty() {
            push_err(
                errs,
                "error",
                intent,
                &format!("{}.Link.LocationSeparator", prefix),
                "loc_delim",
                "required",
                &format!(
                    "Link location separator is required for batch link {}",
                    i
                ),
                "Set link.location_separator",
                "",
            );
        }
    }
}

fn validate_gateway_id(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "GatewayId";

    // UserName and Passcode are co-required
    if !msg.envelope.passcode.is_empty() && msg.envelope.user_name.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "Envelope.UserName",
            "id:user",
            "required",
            "UserName is required when Passcode is provided",
            "Set Envelope.UserName",
            "msg.envelope.user_name = \"username\".into()",
        );
    }
    if !msg.envelope.user_name.is_empty() && msg.envelope.passcode.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            "Envelope.Passcode",
            "id:passcode",
            "required",
            "Passcode is required when UserName is provided",
            "Set Envelope.Passcode",
            "msg.envelope.passcode = \"passcode\".into()",
        );
    }
}

fn validate_actor_report(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "ActorReport";
    match &msg.response {
        None => push_err(
            errs,
            "error",
            intent,
            "Message.Response",
            "",
            "nil_struct",
            "Response fields are required for ActorReport",
            "Initialize msg.response = Some(ResponseFields { status: ..., message: ... })",
            "",
        ),
        Some(r) => {
            required_field(
                errs,
                intent,
                "Response.Status",
                "_status",
                &r.status,
                "Response status is required for ActorReport",
            );
            required_field(
                errs,
                intent,
                "Response.Message",
                "_msg",
                &r.message,
                "Response message is required for ActorReport",
            );
        }
    }
}

fn validate_response_intent(msg: &Message, errs: &mut ValidationErrors) {
    let intent = msg.envelope.intent.name;
    match &msg.response {
        None => push_err(
            errs,
            "error",
            intent,
            "Message.Response",
            "",
            "nil_struct",
            "Response fields are required for response intents",
            "Initialize msg.response = Some(ResponseFields { status: ... })",
            "",
        ),
        Some(r) => {
            if r.status.is_empty() {
                push_err(
                    errs,
                    "warn",
                    intent,
                    "Response.Status",
                    "_status",
                    "required",
                    "Response.Status should be set for response intents",
                    "Set response.status (e.g. \"OK\" or \"ERROR\")",
                    "",
                );
            }
        }
    }
}

// ── Wire-level validation ────────────────────────────────────────────────────

/// Validate raw wire bytes.
///
/// Stage 1 checks framing, size, and To/From address format.
/// Stage 2 decodes the message and checks per-intent header fields.
///
/// Returns empty `Vec` when validation is disabled.
pub fn validate_raw_message(raw: &[u8]) -> ValidationErrors {
    if !validation_enabled() {
        return Vec::new();
    }
    let mut errs = ValidationErrors::new();

    if raw.is_empty() {
        push_err(
            &mut errs,
            "error",
            "",
            "raw",
            "wire",
            "nil_struct",
            "Raw message is nil/empty",
            "Provide a non-empty byte slice",
            "",
        );
        return errs;
    }

    // Stage 1: framing validation
    if raw.len() < 63 {
        push_err(
            &mut errs,
            "error",
            "",
            "raw",
            "wire",
            "format",
            &format!(
                "Message too short: {} bytes (minimum 63 for 7 x 9-byte length fields)",
                raw.len()
            ),
            "Ensure message has complete length prefix block",
            "",
        );
        return errs;
    }

    let max = crate::message::constants::max_message_size();
    if raw.len() as i64 > max {
        push_err(
            &mut errs,
            "error",
            "",
            "raw",
            "wire",
            "format",
            &format!(
                "Message {} bytes exceeds limit {} bytes",
                raw.len(),
                max
            ),
            "Reduce message size or increase MaxMessageSizeBytes",
            "",
        );
        return errs;
    }

    // Parse length prefixes for To/From address validation
    let to_len = parse_wire_len(&raw[9..18]);
    let from_len = parse_wire_len(&raw[18..27]);

    if let (Some(tl), Some(fl)) = (to_len, from_len) {
        if 63 + tl + fl <= raw.len() {
            let to = String::from_utf8_lossy(&raw[63..63 + tl]);
            let from_raw = String::from_utf8_lossy(&raw[63 + tl..63 + tl + fl]);
            let from = from_raw.split('|').next().unwrap_or("");

            if to.is_empty() || !to.contains('@') {
                push_err(
                    &mut errs,
                    "error",
                    "",
                    "To",
                    "to",
                    "format",
                    "To address must be non-empty and match name@gateway format",
                    "Encode a valid To address",
                    "",
                );
            }
            if from.is_empty() || !from.contains('@') {
                push_err(
                    &mut errs,
                    "error",
                    "",
                    "From",
                    "from",
                    "format",
                    "From address must be non-empty and match name@gateway format",
                    "Encode a valid From address",
                    "",
                );
            }
        }
    } else {
        push_err(
            &mut errs,
            "error",
            "",
            "toLength/fromLength",
            "wire",
            "format",
            "Failed to parse toLength or fromLength prefix fields",
            "Encode length fields as 'x' + 8 hex digits",
            "",
        );
    }

    // Stage 2: decode and validate per-intent header fields
    match crate::message::decoder::decode_message(raw) {
        Err(e) => {
            push_err(
                &mut errs,
                "error",
                "",
                "raw",
                "wire",
                "format",
                &e.to_string(),
                "Ensure message is correctly encoded per the wire frame spec",
                "",
            );
        }
        Ok(msg) => {
            validate_wire_header_fields(&msg, raw, &mut errs);
        }
    }

    errs
}

/// Stage 2: per-intent header field validation on decoded wire messages.
fn validate_wire_header_fields(msg: &Message, raw: &[u8], errs: &mut ValidationErrors) {
    let header_len = parse_wire_len(&raw[27..36]).unwrap_or(0);
    let to_len = parse_wire_len(&raw[9..18]).unwrap_or(0);
    let from_len = parse_wire_len(&raw[18..27]).unwrap_or(0);
    let payload_len = parse_wire_len(&raw[54..63]).unwrap_or(0);
    let header_start = 63 + to_len + from_len;

    if header_start + header_len > raw.len() {
        return;
    }

    let header_bytes = &raw[header_start..header_start + header_len];
    let header_str = String::from_utf8_lossy(header_bytes);
    let hm: HashMap<String, String> = header_str
        .split('\t')
        .filter_map(|p| {
            let eq = p.find('=')?;
            Some((p[..eq].to_string(), p[eq + 1..].to_string()))
        })
        .collect();

    let msg_type = msg.envelope.intent.message_type;
    let db_cmd = hm.get("_db_cmd").map(|s| s.as_str()).unwrap_or("");

    // NeuralMemory requests (message_type 1000)
    if msg_type == 1000 {
        if db_cmd.is_empty() {
            push_err(
                errs,
                "error",
                msg.envelope.intent.name,
                "_db_cmd",
                "_db_cmd",
                "header_missing",
                "_db_cmd header field is required for NeuralMemory requests",
                "Ensure the encoder writes _db_cmd",
                "",
            );
            return;
        }

        match db_cmd {
            "store" => {
                if !hm.contains_key("timestamp") {
                    push_err(
                        errs,
                        "warn",
                        "StoreEvent",
                        "timestamp",
                        "timestamp",
                        "header_missing",
                        "timestamp header field missing (encoder should auto-fill)",
                        "Set event.timestamp before encoding",
                        "",
                    );
                }
            }
            "store_batch" => {
                if payload_len == 0 {
                    push_err(
                        errs,
                        "error",
                        "StoreBatchEvents",
                        "payload",
                        "payload",
                        "required",
                        "Payload is required for store_batch (batch events are payload-only)",
                        "Provide batch events in neural_memory.batch_events",
                        "",
                    );
                }
            }
            "tag_store_batch" => {
                let has_id =
                    hm.contains_key("event_id") || hm.contains_key("unique_id");
                if !has_id {
                    push_err(
                        errs,
                        "error",
                        "StoreBatchTags",
                        "event_id/unique_id",
                        "event_id/unique_id",
                        "header_missing",
                        "event_id or unique_id required in header for tag_store_batch",
                        "Set event.id or event.unique_id",
                        "",
                    );
                }
                let has_owner =
                    hm.contains_key("owner") || hm.contains_key("owner_unique_id");
                if !has_owner {
                    push_err(
                        errs,
                        "error",
                        "StoreBatchTags",
                        "owner/owner_unique_id",
                        "owner/owner_unique_id",
                        "header_missing",
                        "owner or owner_unique_id required in header for tag_store_batch",
                        "Set event.owner or event.owner_unique_id",
                        "",
                    );
                }
            }
            "get" => {
                let has_id =
                    hm.contains_key("event_id") || hm.contains_key("unique_id");
                if !has_id {
                    push_err(
                        errs,
                        "error",
                        "GetEvent",
                        "event_id/unique_id",
                        "event_id/unique_id",
                        "header_missing",
                        "event_id or unique_id required in header for get",
                        "Set event.id or event.unique_id",
                        "",
                    );
                }
            }
            "events_for_tag" => {
                if !hm.contains_key("buffer_results") {
                    push_err(
                        errs,
                        "warn",
                        "GetEventsForTags",
                        "buffer_results",
                        "buffer_results",
                        "header_missing",
                        "buffer_results should be present (always written by encoder)",
                        "Ensure get_events_for_tags options are initialized",
                        "",
                    );
                }
            }
            "link" => {
                let has_pair = (hm.contains_key("event_id_a")
                    && hm.contains_key("event_id_b"))
                    || (hm.contains_key("unique_id_a")
                        && hm.contains_key("unique_id_b"));
                if !has_pair {
                    push_err(
                        errs,
                        "error",
                        "LinkEvent",
                        "event_id_a+event_id_b / unique_id_a+unique_id_b",
                        "event_id_a/unique_id_a",
                        "header_missing",
                        "Link source and target identifiers required in header",
                        "Set link endpoints (event_a/b or unique_id_a/b)",
                        "",
                    );
                }
                for field in &["category", "strength_a", "strength_b"] {
                    if !hm.contains_key(*field) {
                        push_err(
                            errs,
                            "error",
                            "LinkEvent",
                            field,
                            field,
                            "header_missing",
                            &format!("{} required in link header", field),
                            &format!("Set link.{}", field),
                            "",
                        );
                    }
                }
                let has_owner = hm.contains_key("owner_event_id")
                    || hm.contains_key("owner_unique_id");
                if !has_owner {
                    push_err(
                        errs,
                        "error",
                        "LinkEvent",
                        "owner_event_id/owner_unique_id",
                        "owner_event_id/owner_unique_id",
                        "header_missing",
                        "owner_event_id or owner_unique_id required in link header",
                        "Set link.owner_id or link.owner_unique_id",
                        "",
                    );
                }
                if !hm.contains_key("timestamp") {
                    push_err(
                        errs,
                        "error",
                        "LinkEvent",
                        "timestamp",
                        "timestamp",
                        "header_missing",
                        "timestamp required in link header",
                        "Set link.timestamp",
                        "",
                    );
                }
            }
            "unlink" => {
                let has_id =
                    hm.contains_key("event_id") || hm.contains_key("unique_id");
                if !has_id {
                    push_err(
                        errs,
                        "error",
                        "UnlinkEvent",
                        "event_id/unique_id",
                        "event_id/unique_id",
                        "header_missing",
                        "event_id or unique_id required in unlink header",
                        "Set link.id or link.unique_id",
                        "",
                    );
                }
            }
            "link_batch" => {
                if payload_len == 0 {
                    push_err(
                        errs,
                        "error",
                        "StoreBatchLinks",
                        "payload",
                        "payload",
                        "required",
                        "Payload is required for link_batch",
                        "Provide batch links in neural_memory.batch_links",
                        "",
                    );
                }
            }
            _ => {}
        }
    }
    // NeuralMemory responses (message_type 1001)
    else if msg_type == 1001 {
        if !hm.contains_key("_status") {
            push_err(
                errs,
                "warn",
                msg.envelope.intent.name,
                "_status",
                "_status",
                "header_missing",
                "Response _status field typically present (brief-hit responses may omit)",
                "Verify response includes _status",
                "",
            );
        }

        match db_cmd {
            "get" => {
                let has_id =
                    hm.contains_key("_event_id") || hm.contains_key("event_id");
                if !has_id {
                    push_err(
                        errs,
                        "error",
                        "GetEventResponse",
                        "_event_id",
                        "_event_id",
                        "header_missing",
                        "_event_id or event_id required in get response",
                        "Response must include the event identifier",
                        "",
                    );
                }
            }
            "link" => {
                if !hm.contains_key("link_event") {
                    push_err(
                        errs,
                        "error",
                        "LinkEventResponse",
                        "link_event",
                        "link_event",
                        "header_missing",
                        "link_event (assigned link ID) required in link response",
                        "Response must include link_event",
                        "",
                    );
                }
            }
            "store" | "store_batch" | "tag_store_batch" => {
                if !hm.contains_key("_count") {
                    push_err(
                        errs,
                        "warn",
                        msg.envelope.intent.name,
                        "_count",
                        "_count",
                        "header_missing",
                        "_count typically present in store/batch response",
                        "Verify response includes _count",
                        "",
                    );
                }
            }
            "link_batch" => {
                if !hm.contains_key("_links_ok") {
                    push_err(
                        errs,
                        "warn",
                        "StoreBatchLinksResponse",
                        "_links_ok",
                        "_links_ok",
                        "header_missing",
                        "_links_ok typically present in link_batch response",
                        "Verify response includes _links_ok",
                        "",
                    );
                }
            }
            _ => {}
        }
    }
    // Non-NeuralMemory intents
    else {
        match msg_type {
            5 => {
                // GatewayId
                if !hm.contains_key("id:name")
                    || hm.get("id:name").map_or(true, |v| v.is_empty())
                {
                    push_err(
                        errs,
                        "error",
                        "GatewayId",
                        "id:name",
                        "id:name",
                        "header_missing",
                        "id:name must be present and non-empty for GatewayId",
                        "Set envelope.client_name",
                        "",
                    );
                }
            }
            2 => {
                // ActorEcho
                if !hm.contains_key("_msg_id")
                    || hm.get("_msg_id").map_or(true, |v| v.is_empty())
                {
                    push_err(
                        errs,
                        "error",
                        "ActorEcho",
                        "_msg_id",
                        "_msg_id",
                        "header_missing",
                        "_msg_id must be present and non-empty for ActorEcho",
                        "Set envelope.message_id",
                        "",
                    );
                }
            }
            4 => {
                // ActorRequest
                let type_val = hm.get("_type").map(|s| s.as_str()).unwrap_or("");
                if type_val != "status" {
                    push_err(
                        errs,
                        "error",
                        "ActorRequest",
                        "_type",
                        "_type",
                        "header_value",
                        "_type must be 'status' for ActorRequest",
                        "Encoder should write _type=status",
                        "",
                    );
                }
            }
            10 | 9 => {} // GatewayStreamOn/Off: no required fields
            3 | 30 | 19 => {} // Status/ActorResponse/ActorReport: no required header fields
            _ => {}
        }
    }
}

/// Parse a 9-byte wire length field ('x' + 8 hex digits, or 9 decimal digits).
fn parse_wire_len(field: &[u8]) -> Option<usize> {
    let s = std::str::from_utf8(field).ok()?;
    if let Some(hex) = s.strip_prefix('x') {
        usize::from_str_radix(hex, 16).ok()
    } else {
        let trimmed = s.trim_start_matches('0');
        if trimmed.is_empty() {
            Some(0)
        } else {
            trimmed.parse().ok()
        }
    }
}

// ── AI-assisted remediation ──────────────────────────────────────────────────

/// Submit `ValidationErrors` to a vLLM-compatible `/v1/chat/completions`
/// endpoint and return the AI-generated corrected code.
///
/// Only available when compiled with `--features knowledge-ai`.
/// Returns `Ok("")` when validation is disabled or errors are empty.
///
/// Uses a per-error prompt template matching the Go plan's specification
/// for AI-assisted remediation via vLLM.
#[cfg(feature = "knowledge-ai")]
pub async fn explain_validation_errors(
    errs: &ValidationErrors,
    endpoint: &str,
    model: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if !validation_enabled() || errs.is_empty() {
        return Ok(String::new());
    }

    let mut prompts = Vec::new();
    for e in errs {
        prompts.push(format!(
            "You are a Pod-OS Rust client expert. A message validation error occurred.\n\n\
             Intent: {}\n\
             Struct Path: {}\n\
             Wire Field: {}\n\
             Rule Violated: {}\n\
             Description: {}\n\
             Suggested Fix: {}\n\
             Example Code: {}\n\
             Source References: {}\n\n\
             Task: Provide corrected Rust code for this message construction. \
             Show all required fields for the {} intent. \
             If multiple valid approaches exist (e.g. event_a/event_b vs \
             unique_id_a/unique_id_b), show both. Use only types from the \
             message module.",
            e.intent,
            e.field,
            e.wire_field,
            e.rule,
            e.message,
            e.fix,
            e.example_code,
            e.references.join(", "),
            e.intent
        ));
    }

    let combined_prompt = prompts.join("\n\n---\n\n");

    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": combined_prompt}]
    });
    let http_resp = client
        .post(format!("{}/v1/chat/completions", endpoint))
        .json(&payload)
        .send()
        .await?;
    let status = http_resp.status();
    let resp: serde_json::Value = http_resp.json().await?;
    if !status.is_success() {
        let detail = resp["error"]["message"]
            .as_str()
            .or_else(|| resp["error"].as_str())
            .unwrap_or("unknown error");
        return Err(format!("vLLM API returned {}: {}", status, detail).into());
    }
    if let Some(err_obj) = resp.get("error") {
        let detail = err_obj["message"]
            .as_str()
            .or_else(|| err_obj.as_str())
            .unwrap_or("unknown error");
        return Err(format!("vLLM API error: {}", detail).into());
    }
    Ok(resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn required_field(
    errs: &mut ValidationErrors,
    intent: &str,
    field: &str,
    wire: &str,
    value: &str,
    msg: &str,
) {
    if value.is_empty() {
        push_err(
            errs,
            "error",
            intent,
            field,
            wire,
            "required",
            msg,
            &format!("Set the {} field before encoding", field),
            "",
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_err(
    errs: &mut ValidationErrors,
    severity: &str,
    intent: &str,
    field: &str,
    wire_field: &str,
    rule: &str,
    message: &str,
    fix: &str,
    example_code: &str,
) {
    errs.push(ValidationError {
        severity: severity.to_string(),
        intent: intent.to_string(),
        field: field.to_string(),
        wire_field: wire_field.to_string(),
        rule: rule.to_string(),
        message: message.to_string(),
        fix: fix.to_string(),
        example_code: example_code.to_string(),
        references: Vec::new(),
    });
}
