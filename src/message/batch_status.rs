//! Helpers for StoreBatchLinks / StoreBatchEvents batch record outcomes.

use crate::message::types::Message;

/// Returns an error message when StoreBatchLinks batch record reports failures.
/// Envelope status can be OK while `links_with_errors > 0` or batch record status is ERROR.
pub fn batch_links_failed(msg: &Message) -> Option<String> {
    let resp = msg.response.as_ref()?;
    let rec = &resp.store_link_batch_event_record;
    if rec.status.eq_ignore_ascii_case("ERROR") || rec.links_with_errors > 0 {
        let text = rec.message.trim();
        return Some(if text.is_empty() {
            "link store failed".to_string()
        } else {
            text.to_string()
        });
    }
    if resp.storage_error_count > 0 {
        return Some("link store failed".to_string());
    }
    None
}

/// Returns an error message when StoreBatchEvents batch record reports failures.
pub fn batch_events_failed(msg: &Message) -> Option<String> {
    let resp = msg.response.as_ref()?;
    let rec = &resp.store_batch_event_record;
    if rec.status.eq_ignore_ascii_case("ERROR") {
        let text = rec.message.trim();
        return Some(if text.is_empty() {
            "batch event store failed".to_string()
        } else {
            text.to_string()
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::types::{ResponseFields, StoreLinkBatchEventRecord};

    #[test]
    fn batch_links_failed_detects_error_status() {
        let msg = Message {
            response: Some(ResponseFields {
                store_link_batch_event_record: StoreLinkBatchEventRecord {
                    status: "ERROR".to_string(),
                    message: "OWNER EVENT NOT FOUND".to_string(),
                    links_with_errors: 2,
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            batch_links_failed(&msg).as_deref(),
            Some("OWNER EVENT NOT FOUND")
        );
    }
}
