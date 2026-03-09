//! Opt-in message validation, controlled by the `PODOS_VALIDATE` environment variable.
//!
//! Set `PODOS_VALIDATE=1` (or `true` / `yes`) to enable.  When disabled, all
//! validation functions return empty results immediately — zero cost on the hot
//! path.

use once_cell::sync::Lazy;
use serde::Serialize;

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
    pub severity: String, // "error" | "warn"
    pub intent: String,
    pub field: String,      // Go struct dot-path
    pub wire_field: String, // wire key
    pub rule: String,       // "required", "one_of_required", "format", ...
    pub message: String,
    pub fix: String,
    pub example_code: String,
    pub references: Vec<String>,
}

pub type ValidationErrors = Vec<ValidationError>;

/// Format errors for terminal output and implement `std::error::Error`.
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
            writeln!(
                f,
                "{tag} intent={} field={} rule={}: {}",
                e.intent, e.field, e.rule, e.message
            )?;
            if !e.fix.is_empty() {
                writeln!(f, "  Fix: {}", e.fix)?;
            }
            if !e.example_code.is_empty() {
                writeln!(f, "  Example:\n    {}", e.example_code)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ValidationReport {}

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
        if errs.is_empty() {
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
            "To address is required",
            "Set To to \"actor@gateway.domain\"",
            "",
        );
    } else if !msg.envelope.to.contains('@') {
        push_err(
            errs,
            "error",
            intent_name,
            "Envelope.To",
            "to",
            "format",
            "To must be 'actor@gateway'",
            "Include '@' in the To address",
            "",
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
            "From address is required",
            "Set From to \"client@gateway.domain\"",
            "",
        );
    } else if !msg.envelope.from.contains('@') {
        push_err(
            errs,
            "error",
            intent_name,
            "Envelope.From",
            "from",
            "format",
            "From must be 'client@gateway'",
            "Include '@' in the From address",
            "",
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
            "Intent must be set",
            "Assign an intent (e.g. intents::STORE_EVENT)",
            "",
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
        validate_gateway_stream(msg, errs);
    } else if *intent == ACTOR_REQUEST {
        validate_actor_request(msg, errs);
    } else if *intent == ACTOR_RESPONSE {
        validate_actor_response(msg, errs);
    } else if intent.message_type == 1001 {
        validate_response_intent(msg, errs);
    }
}

fn validate_store_event(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "StoreEvent";
    match &msg.event {
        None => push_err(
            errs,
            "error",
            intent,
            "Message.Event",
            "",
            "nil_struct",
            "Event fields are required for StoreEvent",
            "Set msg.event = Some(EventFields { id: ..., owner: ..., timestamp: ... })",
            "",
        ),
        Some(e) => {
            required_field(
                errs,
                intent,
                "Event.Id",
                "_event_id",
                &e.id,
                "event_id is required for StoreEvent",
            );
            required_field(
                errs,
                intent,
                "Event.Owner",
                "owner",
                &e.owner,
                "owner is required for StoreEvent",
            );
            required_field(
                errs,
                intent,
                "Event.Timestamp",
                "timestamp",
                &e.timestamp,
                "timestamp is required for StoreEvent",
            );
        }
    }
}

fn validate_store_batch_events(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "StoreBatchEvents";
    let count = msg
        .neural_memory
        .as_ref()
        .map(|n| n.batch_events.len())
        .unwrap_or(0);
    if count == 0 {
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
    }
}

fn validate_store_batch_tags(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "StoreBatchTags";
    let has_tags = msg
        .neural_memory
        .as_ref()
        .map(|n| !n.tags.is_empty())
        .unwrap_or(false);
    if !has_tags {
        push_err(
            errs,
            "error",
            intent,
            "NeuralMemory.Tags",
            "",
            "required",
            "Tags list must not be empty",
            "Push Tags into neural_memory.tags",
            "",
        );
    }
    // Require event reference
    let has_ref = msg
        .event
        .as_ref()
        .map(|e| !e.id.is_empty() || !e.unique_id.is_empty())
        .unwrap_or(false);
    if !has_ref {
        push_err(
            errs,
            "error",
            intent,
            "Event.Id/UniqueId",
            "event_id/unique_id",
            "one_of_required",
            "Either Event.Id or Event.UniqueId is required",
            "Set event.id or event.unique_id",
            "",
        );
    }
}

fn validate_get_event(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "GetEvent";
    let has_ref = msg
        .event
        .as_ref()
        .map(|e| !e.id.is_empty() || !e.unique_id.is_empty())
        .unwrap_or(false);
    if !has_ref {
        push_err(
            errs,
            "error",
            intent,
            "Event.Id/UniqueId",
            "event_id/unique_id",
            "one_of_required",
            "Either Event.Id or Event.UniqueId is required",
            "Set event.id or event.unique_id",
            "",
        );
    }
}

fn validate_get_events_for_tags(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "GetEventsForTags";
    let has_pattern = msg
        .neural_memory
        .as_ref()
        .and_then(|n| n.get_events_for_tags.as_ref())
        .map(|o| !o.event_pattern.is_empty())
        .unwrap_or(false);
    if !has_pattern {
        push_err(
            errs,
            "error",
            intent,
            "NeuralMemory.GetEventsForTags.EventPattern",
            "event_pattern",
            "required",
            "EventPattern must be set",
            "Set neural_memory.get_events_for_tags.event_pattern",
            "",
        );
    }
}

fn validate_link_event(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "LinkEvent";
    let link = msg.neural_memory.as_ref().and_then(|n| n.link.as_ref());
    match link {
        None => push_err(
            errs,
            "error",
            intent,
            "NeuralMemory.Link",
            "",
            "nil_struct",
            "Link fields are required",
            "Set neural_memory.link",
            "",
        ),
        Some(l) => {
            required_field(
                errs,
                intent,
                "Link.Owner",
                "owner",
                &l.owner,
                "Link owner is required",
            );
            let has_a = !l.event_a.is_empty() || !l.unique_id_a.is_empty();
            let has_b = !l.event_b.is_empty() || !l.unique_id_b.is_empty();
            if !has_a {
                push_err(
                    errs,
                    "error",
                    intent,
                    "Link.EventA/UniqueIdA",
                    "event_id_a/unique_id_a",
                    "one_of_required",
                    "Link source (event_a or unique_id_a) required",
                    "Set link.event_a or link.unique_id_a",
                    "",
                );
            }
            if !has_b {
                push_err(
                    errs,
                    "error",
                    intent,
                    "Link.EventB/UniqueIdB",
                    "event_id_b/unique_id_b",
                    "one_of_required",
                    "Link target (event_b or unique_id_b) required",
                    "Set link.event_b or link.unique_id_b",
                    "",
                );
            }
        }
    }
}

fn validate_unlink_event(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "UnlinkEvent";
    let link = msg.neural_memory.as_ref().and_then(|n| n.unlink.as_ref());
    match link {
        None => push_err(
            errs,
            "error",
            intent,
            "NeuralMemory.Unlink",
            "",
            "nil_struct",
            "Unlink fields required",
            "Set neural_memory.unlink",
            "",
        ),
        Some(l) => {
            required_field(
                errs,
                intent,
                "Link.Owner",
                "owner",
                &l.owner,
                "Unlink owner required",
            );
        }
    }
}

fn validate_store_batch_links(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "StoreBatchLinks";
    let count = msg
        .neural_memory
        .as_ref()
        .map(|n| n.batch_links.len())
        .unwrap_or(0);
    if count == 0 {
        push_err(
            errs,
            "error",
            intent,
            "NeuralMemory.BatchLinks",
            "",
            "required",
            "At least one BatchLinkEventSpec required",
            "Push into neural_memory.batch_links",
            "",
        );
    }
}

fn validate_gateway_id(msg: &Message, errs: &mut ValidationErrors) {
    let intent = "GatewayId";
    required_field(
        errs,
        intent,
        "Envelope.ClientName",
        "id:name",
        &msg.envelope.client_name,
        "ClientName is required for GatewayId",
    );
}

fn validate_gateway_stream(_msg: &Message, _errs: &mut ValidationErrors) {}
fn validate_actor_request(_msg: &Message, _errs: &mut ValidationErrors) {}
fn validate_actor_response(_msg: &Message, _errs: &mut ValidationErrors) {}
fn validate_response_intent(_msg: &Message, _errs: &mut ValidationErrors) {}

// ── Wire-level validation ────────────────────────────────────────────────────

/// Validate raw wire bytes without full decoding.
///
/// Returns empty `Vec` when validation is disabled.
pub fn validate_raw_message(raw: &[u8]) -> ValidationErrors {
    if !validation_enabled() {
        return Vec::new();
    }
    let mut errs = ValidationErrors::new();
    validate_wire_stage1(raw, &mut errs);
    errs
}

fn validate_wire_stage1(raw: &[u8], errs: &mut ValidationErrors) {
    use crate::message::decoder::decode_message;
    // Delegate to decoder for structural checks; collect any decode errors
    if let Err(e) = decode_message(raw) {
        push_err(
            errs,
            "error",
            "",
            "raw",
            "wire",
            "header_missing",
            &e.to_string(),
            "Ensure message is correctly encoded",
            "",
        );
    }
}

// ── AI-assisted remediation ──────────────────────────────────────────────────

/// Submit `ValidationErrors` to a vLLM-compatible `/v1/chat/completions`
/// endpoint and return the AI-generated corrected code.
///
/// Only available when compiled with `--features knowledge-ai`.
/// Returns `Ok("")` when validation is disabled or errors are empty.
#[cfg(feature = "knowledge-ai")]
pub async fn explain_validation_errors(
    errs: &ValidationErrors,
    endpoint: &str,
    model: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if !validation_enabled() || errs.is_empty() {
        return Ok(String::new());
    }
    let prompt = format!(
        "Fix these Pod-OS message validation errors and return corrected Rust code:\n{}",
        errs.llm_json()
    );
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}]
    });
    let resp: serde_json::Value = client
        .post(format!("{}/v1/chat/completions", endpoint))
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;
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
            &format!("Set the {field} field before encoding"),
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
