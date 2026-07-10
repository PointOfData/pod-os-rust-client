//! AIP health probe message construction and success evaluation.

use uuid::Uuid;

use crate::errors::GatewayDError;
use crate::message::{
    intents,
    types::{Envelope, GetEventsForTagsOptions, Message, NeuralMemoryFields, PayloadData, PayloadFields},
};

/// Reports whether an actor type runs pod_aip_db and can answer Neural-Memory intents
/// such as GetEventsForTags.
pub fn is_neural_memory_backed_for_health_probe(actor_type: &str) -> bool {
    match actor_type.trim().to_ascii_lowercase().as_str() {
        "pod_db" | "evolutionary-neural-memory" | "neural_memory" | "neural-memory" => true,
        _ => false,
    }
}

/// Constructs the AIP health probe for one actor based on type.
/// NM-backed actors use GetEventsForTags (CountOnly); socket/shell and other types
/// use StatusRequest.
pub fn build_actor_health_probe_message(
    actor_address: &str,
    from_address: &str,
    client_name: &str,
    actor_type: &str,
) -> Message {
    let message_id = Uuid::new_v4().to_string();

    if is_neural_memory_backed_for_health_probe(actor_type) {
        let health_check_tag = format!("_podos_health_check_{}", Uuid::new_v4());
        let search_clause = format!("health_check={health_check_tag}");
        return Message {
            envelope: Envelope {
                to: actor_address.to_string(),
                from: from_address.to_string(),
                intent: intents::GET_EVENTS_FOR_TAGS.clone(),
                client_name: client_name.to_string(),
                message_id,
                ..Default::default()
            },
            payload: Some(PayloadFields {
                data: PayloadData::Text(search_clause),
                ..Default::default()
            }),
            neural_memory: Some(NeuralMemoryFields {
                get_events_for_tags: Some(GetEventsForTagsOptions {
                    count_only: true,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
    }

    Message {
        envelope: Envelope {
            to: actor_address.to_string(),
            from: from_address.to_string(),
            intent: intents::STATUS_REQUEST.clone(),
            client_name: client_name.to_string(),
            message_id,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Reports whether a health probe transport and AIP status indicate success.
pub fn actor_health_probe_succeeded(
    err: Option<&GatewayDError>,
    resp: Option<&Message>,
) -> bool {
    if err.is_some() {
        return false;
    }
    resp.map(|m| m.processing_status() != "ERROR").unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::types::ResponseFields;

    #[test]
    fn neural_memory_types() {
        for typ in [
            "neural_memory",
            "pod_db",
            "evolutionary-neural-memory",
            "Neural_Memory",
            "neural-memory",
        ] {
            assert!(is_neural_memory_backed_for_health_probe(typ), "{typ}");
        }
        for typ in ["socket", "router", "shell", "", "gateway"] {
            assert!(!is_neural_memory_backed_for_health_probe(typ), "{typ}");
        }
    }

    #[test]
    fn probe_intent_by_type() {
        let socket_msg = build_actor_health_probe_message(
            "mysocket@gateway.pod-os.com",
            "client@zeroth.pod-os.com",
            "client",
            "socket",
        );
        assert_eq!(socket_msg.envelope.intent.name, intents::STATUS_REQUEST.name);
        assert!(!socket_msg.envelope.message_id.is_empty());

        let nm_msg = build_actor_health_probe_message(
            "account@zeroth.pod-os.com",
            "client@zeroth.pod-os.com",
            "client",
            "neural_memory",
        );
        assert_eq!(
            nm_msg.envelope.intent.name,
            intents::GET_EVENTS_FOR_TAGS.name
        );
        let opts = nm_msg
            .neural_memory
            .as_ref()
            .and_then(|nm| nm.get_events_for_tags.as_ref())
            .expect("CountOnly options");
        assert!(opts.count_only);
    }

    #[test]
    fn probe_succeeded() {
        assert!(actor_health_probe_succeeded(None, None));
        assert!(actor_health_probe_succeeded(None, Some(&Message::default())));
        let err_resp = Message {
            response: Some(ResponseFields {
                status: "ERROR".into(),
                message: "fail".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(!actor_health_probe_succeeded(None, Some(&err_resp)));
    }
}
