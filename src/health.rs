//! Actor health-check helpers for non-Neural Memory socket Actors.
//!
//! Responds to inbound [`intents::STATUS_REQUEST`] probes (message_type 110)
//! with [`intents::STATUS`] replies (message_type 3) echoing the probe
//! `message_id`.

use std::sync::Arc;

use uuid::Uuid;

use crate::client::Client;
use crate::message::{
    encode_message,
    intents,
    types::{Envelope, Message, ResponseFields},
};

/// Constructs a [`intents::STATUS`] response for an inbound [`intents::STATUS_REQUEST`] probe.
pub fn build_status_health_reply(client: &Client, inbound: &Message) -> Message {
    build_status_health_reply_for(
        client.client_name(),
        client.actor_name(),
        inbound,
    )
}

/// Spawns a background task that replies to inbound `StatusRequest` probes.
///
/// Requires `enable_concurrent_mode: true` so unsolicited frames are forwarded
/// via [`Client::subscribe_incoming`].
pub fn respond_to_health_checks(client: Arc<Client>) {
    let mut rx = client.subscribe_incoming();
    tokio::spawn(async move {
        while let Ok(inbound) = rx.recv().await {
            if inbound.intent().name != intents::STATUS_REQUEST.name {
                continue;
            }
            let reply = build_status_health_reply(&client, &inbound);
            if let Ok(wire) = encode_message(&reply, &Uuid::new_v4().to_string()) {
                let _ = client.send_control_message(&wire).await;
            }
        }
    });
}

fn build_status_health_reply_for(
    client_name: &str,
    gateway_actor_name: &str,
    inbound: &Message,
) -> Message {
    Message {
        envelope: Envelope {
            to: inbound.from().to_string(),
            from: format!("{client_name}@{gateway_actor_name}"),
            intent: intents::STATUS.clone(),
            client_name: client_name.to_string(),
            message_id: inbound.message_id().to_string(),
            ..Default::default()
        },
        response: Some(ResponseFields {
            status: "OK".to_string(),
            message: "actor is healthy".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::decode_message;

    fn status_request_inbound() -> Message {
        Message {
            envelope: Envelope {
                from: "probe-client@zeroth.pod-os.com".to_string(),
                intent: intents::STATUS_REQUEST.clone(),
                message_id: "probe-msg-1".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn build_status_health_reply_fields() {
        let inbound = status_request_inbound();
        let reply = build_status_health_reply_for("socket-actor", "zeroth.pod-os.com", &inbound);

        assert_eq!(reply.intent().name, intents::STATUS.name);
        assert_eq!(reply.message_id(), "probe-msg-1");
        assert_eq!(reply.to(), "probe-client@zeroth.pod-os.com");
        assert_eq!(reply.from(), "socket-actor@zeroth.pod-os.com");
        assert_eq!(reply.processing_status(), "OK");
        assert_eq!(
            reply.response.as_ref().map(|r| r.message.as_str()),
            Some("actor is healthy")
        );
    }

    #[test]
    fn build_status_health_reply_round_trip() {
        let inbound = status_request_inbound();
        let reply = build_status_health_reply_for("socket-actor", "zeroth.pod-os.com", &inbound);
        let wire = encode_message(&reply, "conv-uuid").expect("encode");
        let wire_str = std::str::from_utf8(wire.as_bytes()).expect("utf8");
        assert!(wire_str.contains("000000003"));

        let decoded = decode_message(wire.as_bytes()).expect("decode");
        assert_eq!(decoded.intent().message_type, intents::STATUS.message_type);
        assert_eq!(decoded.message_id(), "probe-msg-1");
        assert_eq!(decoded.processing_status(), "OK");
    }

    #[test]
    fn status_request_probe_round_trip() {
        let probe = Message {
            envelope: Envelope {
                to: "socket-actor@gateway.pod-os.com".to_string(),
                from: "dashboard@zeroth.pod-os.com".to_string(),
                intent: intents::STATUS_REQUEST.clone(),
                client_name: "dashboard".to_string(),
                message_id: "health-1".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let wire = encode_message(&probe, "conv-uuid").expect("encode");
        let wire_str = std::str::from_utf8(wire.as_bytes()).expect("utf8");
        assert!(wire_str.contains("000000110"));

        let decoded = decode_message(wire.as_bytes()).expect("decode");
        assert_eq!(decoded.intent().name, intents::STATUS_REQUEST.name);
        assert_eq!(decoded.message_id(), "health-1");
    }
}
