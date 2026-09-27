//! Extended tag formats: GetEvent `tag_format=1` header tags and GetEventsForTags
//! `buffer_format=1` tag lines (storage timestamps and tag owners).

use crate::message::types::{Message, TagOutput, TagOwnerOutput};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Event key Pod-OS reports as the owner of tags that have no owning event (e.g. tags
/// created under the `$sys` owner).
const NULL_OWNER_KEY_PREFIX: &str = "+0000000000.000000";

/// Parse one GetEvent response header field carrying a tag.
///
/// ```text
/// tag_format=0: event_tag:nnnnnnnnn:fffffffff=key=value
/// tag_format=1: event_tag:nnnnnnnnn:fffffffff:ssssssssss.uuuuuu[:owner_id]=key=value
/// ```
///
/// The name is split into at most 4 pieces so owner IDs containing ':' stay intact.
pub(crate) fn parse_event_tag_header(name: &str, value: &str) -> Option<TagOutput> {
    let rest = name.strip_prefix("event_tag:")?;
    let parts: Vec<&str> = rest.splitn(4, ':').collect();
    if parts.len() < 2 {
        return None;
    }

    let (key, value) = split_tag_key_value(value);
    Some(TagOutput {
        tag_number: parts[0].parse().unwrap_or(0),
        frequency: parts[1].parse().unwrap_or(1),
        timestamp: parts.get(2).map_or_else(String::new, |s| s.to_string()),
        owner: parts.get(3).map_or_else(String::new, |s| normalize_tag_owner(s)),
        key,
        value,
        ..Default::default()
    })
}

/// Order GetEvent tags by their database tag counter.
pub(crate) fn sort_tags_by_number(tags: &mut [TagOutput]) {
    tags.sort_by_key(|t| t.tag_number);
}

/// Split a `"key=value"` tag string. A string without a key keeps the whole text as the value.
pub(crate) fn split_tag_key_value(s: &str) -> (String, String) {
    match s.find('=') {
        Some(eq) if eq > 0 => (s[..eq].to_string(), s[eq + 1..].to_string()),
        _ => (String::new(), s.to_string()),
    }
}

/// Map Pod-OS "no owner" markers to an empty string.
pub(crate) fn normalize_tag_owner(owner: &str) -> String {
    if owner == "NULL" || owner.starts_with(NULL_OWNER_KEY_PREFIX) {
        String::new()
    } else {
        owner.to_string()
    }
}

/// Repair `buffer_format=1` payloads requested with `get_tag_owner` or
/// `get_tag_owner_unique_id`. Pod-OS writes each tag's owner after the tag line's newline,
/// so it runs into the next record:
///
/// ```text
/// _event_tag=K\t...\ttag_timestamp=T\n\towner=O_event_tag=K\t...
/// ```
///
/// Each owner is rejoined to the tag line it belongs to and the record break is restored.
/// Payloads already in the documented form (owner before the newline) are unchanged.
pub(crate) fn normalize_tag_owner_lines(payload: &str) -> std::borrow::Cow<'_, str> {
    if !payload.contains("\n\towner=") {
        return std::borrow::Cow::Borrowed(payload);
    }
    let joined = payload.replace("\n\towner=", "\towner=");

    const MARKER: &str = "\towner=";
    let mut out = String::with_capacity(joined.len() + 64);
    let mut rest = joined.as_str();
    while let Some(i) = rest.find(MARKER) {
        out.push_str(&rest[..i + MARKER.len()]);
        rest = &rest[i + MARKER.len()..];

        let owner_end = rest.find(['\t', '\n']).unwrap_or(rest.len());
        if let Some(j) = rest[..owner_end].find("_event_tag=") {
            out.push_str(&rest[..j]);
            out.push('\n');
            rest = &rest[j..];
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

/// Build a tag from `buffer_format=1` tag fields: `tag_freq`, `tag_value` (key=value),
/// `tag_timestamp`, and `owner` (event key or unique ID, present with `get_tag_owner` /
/// `get_tag_owner_unique_id`).
pub(crate) fn parse_event_tag_payload_fields(fields: &HashMap<String, String>) -> TagOutput {
    let mut tag = TagOutput::default();
    if let Some(freq) = fields.get("tag_freq").and_then(|f| f.parse().ok()) {
        tag.frequency = freq;
    }
    if let Some(tag_value) = fields.get("tag_value") {
        (tag.key, tag.value) = split_tag_key_value(tag_value);
    }
    tag.timestamp = fields.get("tag_timestamp").cloned().unwrap_or_default();
    tag.owner = fields
        .get("owner")
        .map_or_else(String::new, |o| normalize_tag_owner(o));
    tag
}

/// Value of the `_unique_id` (or `unique_id`) tag, if present.
pub(crate) fn unique_id_from_tags(tags: &[TagOutput]) -> Option<&str> {
    tags.iter()
        .find(|t| t.key == "_unique_id" || t.key == "unique_id")
        .map(|t| t.value.as_str())
}

/// Parse a Pod-OS `"[+|-]ssssssssss.uuuuuu"` timestamp (UTC).
pub(crate) fn parse_posix_timestamp(s: &str) -> Option<SystemTime> {
    if s.is_empty() {
        return None;
    }
    let (neg, s) = match s.as_bytes()[0] {
        b'+' => (false, &s[1..]),
        b'-' => (true, &s[1..]),
        _ => (false, s),
    };
    let (sec_str, frac_str) = s.split_once('.').unwrap_or((s, ""));
    if sec_str.is_empty() || !sec_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let sec: u64 = sec_str.parse().ok()?;
    let mut usec: u64 = 0;
    if !frac_str.is_empty() {
        if !frac_str.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let frac = &frac_str[..frac_str.len().min(6)];
        usec = format!("{frac:0<6}").parse().ok()?;
    }
    let offset = Duration::from_secs(sec) + Duration::from_micros(usec);
    if neg {
        UNIX_EPOCH.checked_sub(offset)
    } else {
        UNIX_EPOCH.checked_add(offset)
    }
}

/// Move decoded tag owners from `TagOutput::owner` to `TagOutput::owner_unique_id` when
/// `request` asked for owners by unique ID (`TagOwnerOutput::UniqueId`). Pod-OS responses do
/// not say which owner form they carry, so `decode_message` always fills `owner`.
/// `Client::send_message` and `Client::send_message_with_raw` apply this automatically; call it yourself when decoding
/// raw responses with `decode_message`.
pub fn apply_tag_owner_output(request: &Message, response: &mut Message) {
    if request.tag_owner_output() != TagOwnerOutput::UniqueId {
        return;
    }
    fn move_owners(tags: &mut [TagOutput]) {
        for tag in tags.iter_mut().filter(|t| !t.owner.is_empty()) {
            tag.owner_unique_id = std::mem::take(&mut tag.owner);
        }
    }
    if let Some(event) = response.event.as_mut() {
        move_owners(&mut event.tags);
    }
    if let Some(resp) = response.response.as_mut() {
        for record in &mut resp.event_records {
            move_owners(&mut record.tags);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER_KEY: &str = "+1790266973.206042\x01TERRA\x0247.6\x02-122.5";

    fn tag(n: i64, freq: i32, ts: &str, owner: &str, key: &str, value: &str) -> TagOutput {
        TagOutput {
            tag_number: n,
            frequency: freq,
            timestamp: ts.into(),
            owner: owner.into(),
            key: key.into(),
            value: value.into(),
            ..Default::default()
        }
    }

    #[test]
    fn parse_event_tag_header_formats() {
        let cases = [
            ("format 0", "event_tag:000000017:4", "k=v", tag(17, 4, "", "", "k", "v")),
            (
                "format 1 without owner",
                "event_tag:000000002:3:1790266974.325930",
                "k=a=b",
                tag(2, 3, "1790266974.325930", "", "k", "a=b"),
            ),
            (
                "format 1 with event key owner",
                &format!("event_tag:000000003:1:1790266974.000001:{OWNER_KEY}"),
                "k=v",
                tag(3, 1, "1790266974.000001", OWNER_KEY, "k", "v"),
            ),
            (
                "format 1 owner containing colons",
                "event_tag:000000004:1:1790266974.000001:urn:a:b",
                "k=v",
                tag(4, 1, "1790266974.000001", "urn:a:b", "k", "v"),
            ),
            (
                "format 1 NULL owner",
                "event_tag:000000005:1:1790266974.000001:NULL",
                "k=v",
                tag(5, 1, "1790266974.000001", "", "k", "v"),
            ),
            (
                "format 1 null event key owner",
                "event_tag:000000006:1:1790266974.000001:+0000000000.000000\x01000000.000000",
                "k=v",
                tag(6, 1, "1790266974.000001", "", "k", "v"),
            ),
            (
                "unparseable frequency defaults to 1, keyless value",
                "event_tag:000000007:x",
                "=v",
                tag(7, 1, "", "", "", "=v"),
            ),
        ];
        for (name, header, value, want) in cases {
            assert_eq!(parse_event_tag_header(header, value), Some(want), "{name}");
        }
        assert_eq!(parse_event_tag_header("unique_id", "x"), None);
        assert_eq!(parse_event_tag_header("event_tag:1", "k=v"), None);
    }

    #[test]
    fn sort_tags_by_number_is_stable() {
        let mut tags = vec![
            tag(3, 1, "", "", "c", ""),
            tag(1, 1, "", "", "a", "first"),
            tag(1, 1, "", "", "a", "second"),
        ];
        sort_tags_by_number(&mut tags);
        let order: Vec<&str> = tags.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(order, ["first", "second", ""]);
    }

    #[test]
    fn normalize_tag_owner_lines_repairs_server_form() {
        let spec = "_event_tag=E\ttag_freq=1\ttag_value=a=b\ttag_timestamp=1.000001\towner=O1\n\
                    _event_tag=E\ttag_freq=2\ttag_value=c=d\ttag_timestamp=1.000002\towner=O2\n";
        assert_eq!(normalize_tag_owner_lines(spec), spec);

        let server = "_event_id=E\n_event_tag=E\ttag_freq=1\ttag_value=a=b\ttag_timestamp=1.000001\n\
                      \towner=O1_event_tag=E\ttag_freq=2\ttag_value=c=d\ttag_timestamp=1.000002\n\towner=O2\n\n";
        let want = "_event_id=E\n_event_tag=E\ttag_freq=1\ttag_value=a=b\ttag_timestamp=1.000001\towner=O1\n\
                    _event_tag=E\ttag_freq=2\ttag_value=c=d\ttag_timestamp=1.000002\towner=O2\n\n";
        assert_eq!(normalize_tag_owner_lines(server), want);
    }

    #[test]
    fn parse_posix_timestamp_values() {
        let at = |s, us| Some(UNIX_EPOCH + Duration::from_secs(s) + Duration::from_micros(us));
        assert_eq!(parse_posix_timestamp("1790266974.325930"), at(1_790_266_974, 325_930));
        assert_eq!(parse_posix_timestamp("+1790266974.5"), at(1_790_266_974, 500_000));
        assert_eq!(parse_posix_timestamp("1790266974"), at(1_790_266_974, 0));
        assert_eq!(parse_posix_timestamp("1790266974.1234567"), at(1_790_266_974, 123_456));
        assert_eq!(
            parse_posix_timestamp("-1.5"),
            UNIX_EPOCH.checked_sub(Duration::from_micros(1_500_000))
        );
        assert_eq!(parse_posix_timestamp(""), None);
        assert_eq!(parse_posix_timestamp("abc"), None);
        assert_eq!(parse_posix_timestamp("12.x"), None);
    }
}
