//! Extended tag formats: GetEvent `tag_format=1` and GetEventsForTags `buffer_format=1`.
//!
//! Fixtures in `tests/fixtures/tag_format` are raw Pod-OS response frames captured live from a
//! kind cluster (see `src/knowledge/docs/intent_field_validation.plan.md`, "Tag formats").

use std::time::{Duration, UNIX_EPOCH};

use pod_os_client::message::{
    apply_tag_owner_output, decode_message, encode_message, intents, validate_raw_message,
    Envelope, EventFields, GetEventOptions, GetEventsForTagsOptions, Message, NeuralMemoryFields,
    PayloadData, PayloadFields, ResponseFields, TagOutput, TagOwnerOutput, ValidationErrors,
};

const OWNER_KEY: &str = "+1790266973.206042\x01TERRA\x0247.6\x02-122.5";
const TARGET_KEY: &str = "+1790266973.420335\x01TERRA\x0247.6\x02-122.5";
const OWNED_KEY: &str = "+1790266973.723095\x01TERRA\x0247.6\x02-122.5";
const OWNER_UID: &str = "tagprobe-owner-dlnoodm3eox6";
const TARGET_UID: &str = "tagprobe-target-dlnoodm3eox6";
const OWNED_UID: &str = "tagprobe-owned-dlnoodm3eox6";
const PROBE_VALUE: &str = "dlnoodm3eox6";

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tag_format/",
            $name,
            ".bin"
        ))
    };
}

fn decode(raw: &[u8]) -> Message {
    decode_message(raw).expect("decode fixture")
}

fn find_tags<'a>(tags: &'a [TagOutput], key: &str, value: &str) -> Vec<&'a TagOutput> {
    tags.iter()
        .filter(|t| t.key == key && (value.is_empty() || t.value == value))
        .collect()
}

fn event_by_key<'a>(msg: &'a Message, key: &str) -> &'a EventFields {
    msg.search_event_records()
        .iter()
        .find(|e| e.id == key)
        .unwrap_or_else(|| panic!("event {key:?} not found"))
}

#[test]
fn decode_get_event_tag_format_0() {
    let msg = decode(fixture!("get_event_tag_format_0"));
    let tags = &msg.event.as_ref().unwrap().tags;
    assert_eq!(tags.len(), 36);
    for (i, tag) in tags.iter().enumerate() {
        assert_eq!(tag.tag_number, i as i64 + 1, "tags must be ordered by tag number");
        assert!(tag.timestamp.is_empty() && tag.owner.is_empty(), "tag {i}: {tag:?}");
    }
    let color = find_tags(tags, "color", "red");
    assert_eq!(color.len(), 1);
    assert_eq!((color[0].frequency, color[0].tag_number), (3, 18));
    let size = find_tags(tags, "size", "");
    assert_eq!(size.len(), 2);
    assert_eq!(size[0].value, "a:b=c");
}

#[test]
fn decode_get_event_tag_format_1() {
    for raw in [
        &fixture!("get_event_tag_format_1")[..],
        &fixture!("get_event_tag_format_1_output_tag_owner_y")[..],
    ] {
        let msg = decode(raw);
        let tags = &msg.event.as_ref().unwrap().tags;
        assert_eq!(tags.len(), 36);
        let records = msg.search_event_records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tags.len(), 36);

        let size = find_tags(tags, "size", "a:b=c");
        assert_eq!(size.len(), 2);
        assert_eq!(size[0].tag_number, 19);
        assert_eq!(size[0].frequency, 5);
        assert_eq!(size[0].timestamp, "1790266974.325930");
        assert_eq!(size[1].timestamp, "1790266974.325980");
        assert_eq!(
            size[0].time(),
            Some(UNIX_EPOCH + Duration::new(1_790_266_974, 325_930_000))
        );

        let w = find_tags(tags, "$W", "");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].value, "TERRA\x0247.6\x02-122.5");
        assert_eq!(w[0].tag_number, 1);

        // This Pod-OS build omits the owner segment on GetEvent even with output_tag_owner=Y.
        assert!(tags.iter().all(|t| t.owner.is_empty()));
    }
}

#[test]
fn decode_get_events_for_tags_buffer_format_0() {
    let msg = decode(fixture!("events_for_tag_buffer_format_0"));
    assert_eq!(msg.search_event_records().len(), 2);
    let target = event_by_key(&msg, TARGET_KEY);
    assert_eq!(target.tags.len(), 21, "duplicate tag:5:size fields must both survive");
    assert_eq!(target.unique_id, TARGET_UID);
    assert_eq!(find_tags(&target.tags, "size", "a:b=c").len(), 2);
    assert!(target.tags.iter().all(|t| t.timestamp.is_empty()));
}

#[test]
fn decode_get_events_for_tags_buffer_format_1() {
    let msg = decode(fixture!("events_for_tag_buffer_format_1"));
    assert_eq!(msg.search_event_records().len(), 2);
    let target = event_by_key(&msg, TARGET_KEY);
    let owned = event_by_key(&msg, OWNED_KEY);
    assert_eq!((target.tags.len(), owned.tags.len()), (21, 18));
    assert_eq!(target.unique_id, TARGET_UID);
    assert_eq!(owned.unique_id, OWNED_UID);

    let color = find_tags(&target.tags, "color", "red");
    assert_eq!(color.len(), 1);
    assert_eq!(color[0].frequency, 3);
    assert_eq!(color[0].timestamp, "1790266973.722260");
    assert_eq!(color[0].owner, "");

    let probe = find_tags(&owned.tags, "probe_sfx", PROBE_VALUE);
    assert_eq!(probe.len(), 1);
    assert_eq!(probe[0].frequency, 4);
    assert_eq!(probe[0].timestamp, "1790266974.024690");
}

#[test]
fn decode_get_events_for_tags_buffer_format_1_owner_event_key() {
    let msg = decode(fixture!("events_for_tag_buffer_format_1_get_tag_owner"));
    let target = event_by_key(&msg, TARGET_KEY);
    let owned = event_by_key(&msg, OWNED_KEY);
    assert_eq!((target.tags.len(), owned.tags.len()), (21, 18));

    let color = find_tags(&target.tags, "color", "red");
    assert_eq!(color.len(), 1);
    assert_eq!(color[0].owner, "", "$sys-owned tag owner must be normalized to empty");

    let size = find_tags(&target.tags, "size", "a:b=c");
    assert_eq!(size.len(), 2);
    assert!(size.iter().all(|s| s.owner == OWNER_KEY));

    let uid = find_tags(&target.tags, "_unique_id", "");
    assert_eq!(uid.len(), 1);
    assert_eq!(uid[0].owner, TARGET_KEY);

    let w = find_tags(&owned.tags, "$W", "");
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].owner, OWNER_KEY);
    assert_eq!(w[0].timestamp, "1790266974.024420");

    for tag in target.tags.iter().chain(owned.tags.iter()) {
        assert!(
            !tag.owner.contains("_event_tag") && !tag.timestamp.contains("owner"),
            "owner segment not separated from next record: {tag:?}"
        );
    }
}

#[test]
fn decode_get_events_for_tags_buffer_format_1_owner_unique_id() {
    let req = Message {
        envelope: Envelope {
            intent: intents::GET_EVENTS_FOR_TAGS.clone(),
            ..Default::default()
        },
        neural_memory: Some(NeuralMemoryFields {
            get_events_for_tags: Some(GetEventsForTagsOptions {
                buffer_format: "1".into(),
                tag_owner_output: TagOwnerOutput::UniqueId,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut msg = decode(fixture!(
        "events_for_tag_buffer_format_1_get_tag_owner_unique_id"
    ));
    apply_tag_owner_output(&req, &mut msg);

    let target = event_by_key(&msg, TARGET_KEY);
    let owned = event_by_key(&msg, OWNED_KEY);

    let color = find_tags(&target.tags, "color", "red");
    assert_eq!(color.len(), 1);
    assert_eq!((color[0].owner.as_str(), color[0].owner_unique_id.as_str()), ("", ""));

    let size = find_tags(&target.tags, "size", "a:b=c");
    assert_eq!(size.len(), 2);
    for s in size {
        assert_eq!(s.owner_unique_id, OWNER_UID);
        assert_eq!(s.owner, "");
    }

    let uid = find_tags(&owned.tags, "_unique_id", "");
    assert_eq!(uid.len(), 1);
    assert_eq!(uid[0].owner_unique_id, OWNED_UID);

    // A second pass is a no-op.
    let before = msg.clone();
    apply_tag_owner_output(&req, &mut msg);
    assert_eq!(
        before.search_event_records()[0].tags,
        msg.search_event_records()[0].tags
    );
}

#[test]
fn apply_tag_owner_output_noop_for_event_key() {
    let req = Message {
        envelope: Envelope {
            intent: intents::GET_EVENT.clone(),
            ..Default::default()
        },
        neural_memory: Some(NeuralMemoryFields {
            get_event: Some(GetEventOptions {
                tag_owner_output: TagOwnerOutput::EventKey,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let tag = TagOutput {
        key: "k".into(),
        owner: OWNER_KEY.into(),
        ..Default::default()
    };
    let mut resp = Message {
        event: Some(EventFields {
            tags: vec![tag.clone()],
            ..Default::default()
        }),
        response: Some(ResponseFields {
            event_records: vec![EventFields {
                tags: vec![tag.clone()],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    apply_tag_owner_output(&req, &mut resp);
    assert_eq!(resp.event.as_ref().unwrap().tags[0], tag);
    assert_eq!(resp.search_event_records()[0].tags[0], tag);
}

#[test]
fn apply_tag_owner_output_get_event_unique_id() {
    let req = Message {
        envelope: Envelope {
            intent: intents::GET_EVENT.clone(),
            ..Default::default()
        },
        neural_memory: Some(NeuralMemoryFields {
            get_event: Some(GetEventOptions {
                tag_format: Some(1),
                tag_owner_output: TagOwnerOutput::UniqueId,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let tag = TagOutput {
        key: "k".into(),
        owner: OWNER_UID.into(),
        ..Default::default()
    };
    let mut resp = Message {
        event: Some(EventFields {
            tags: vec![tag.clone()],
            ..Default::default()
        }),
        response: Some(ResponseFields {
            event_records: vec![EventFields {
                tags: vec![tag],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    apply_tag_owner_output(&req, &mut resp);
    for t in [
        &resp.event.as_ref().unwrap().tags[0],
        &resp.search_event_records()[0].tags[0],
    ] {
        assert_eq!((t.owner.as_str(), t.owner_unique_id.as_str()), ("", OWNER_UID));
    }
}

// ── Header encoding ──────────────────────────────────────────────────────────

fn header_of(msg: &Message) -> String {
    let sm = encode_message(msg, "").expect("encode");
    let raw = sm.as_bytes();
    let hex = |r: std::ops::Range<usize>| {
        let s = std::str::from_utf8(&raw[r]).unwrap();
        usize::from_str_radix(s.trim_start_matches('x'), 16).unwrap()
    };
    let start = 63 + hex(9..18) + hex(18..27);
    String::from_utf8_lossy(&raw[start..start + hex(27..36)]).into_owned()
}

fn get_event_msg(opts: GetEventOptions) -> Message {
    Message {
        envelope: Envelope {
            to: "test@zeroth.pod-os.com".into(),
            from: "c@zeroth.pod-os.com".into(),
            intent: intents::GET_EVENT.clone(),
            message_id: "m1".into(),
            ..Default::default()
        },
        event: Some(EventFields {
            id: "e1".into(),
            ..Default::default()
        }),
        neural_memory: Some(NeuralMemoryFields {
            get_event: Some(opts),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn events_for_tags_msg(opts: GetEventsForTagsOptions) -> Message {
    Message {
        envelope: Envelope {
            to: "test@zeroth.pod-os.com".into(),
            from: "c@zeroth.pod-os.com".into(),
            intent: intents::GET_EVENTS_FOR_TAGS.clone(),
            message_id: "m1".into(),
            ..Default::default()
        },
        payload: Some(PayloadFields {
            data: PayloadData::Text("clause_type:S\tboolean:or\tlow:k=v".into()),
            ..Default::default()
        }),
        neural_memory: Some(NeuralMemoryFields {
            get_events_for_tags: Some(opts),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn has_field(h: &str, field: &str) -> bool {
    h.split('\t').any(|f| f == field)
}

#[test]
fn get_event_header_tag_format() {
    let h = header_of(&get_event_msg(GetEventOptions {
        get_tags: true,
        ..Default::default()
    }));
    assert!(has_field(&h, "tag_format=0") && !h.contains("output_tag_owner"), "{h:?}");

    let h = header_of(&get_event_msg(GetEventOptions {
        get_tags: true,
        tag_format: Some(1),
        ..Default::default()
    }));
    assert!(has_field(&h, "tag_format=1"), "{h:?}");

    let h = header_of(&get_event_msg(GetEventOptions {
        tag_format: Some(1),
        tag_owner_output: TagOwnerOutput::EventKey,
        ..Default::default()
    }));
    assert!(has_field(&h, "output_tag_owner=Y"), "{h:?}");

    let h = header_of(&get_event_msg(GetEventOptions {
        tag_format: Some(1),
        tag_owner_output: TagOwnerOutput::UniqueId,
        ..Default::default()
    }));
    assert!(has_field(&h, "output_tag_owner=N"), "{h:?}");
}

#[test]
fn get_events_for_tags_header_tag_owner() {
    let bf1 = |owner| GetEventsForTagsOptions {
        buffer_format: "1".into(),
        tag_owner_output: owner,
        ..Default::default()
    };
    let h = header_of(&events_for_tags_msg(bf1(TagOwnerOutput::None)));
    assert!(has_field(&h, "buffer_format=1") && !h.contains("get_tag_owner"), "{h:?}");

    let h = header_of(&events_for_tags_msg(bf1(TagOwnerOutput::EventKey)));
    assert!(has_field(&h, "get_tag_owner=Y"), "{h:?}");

    let h = header_of(&events_for_tags_msg(bf1(TagOwnerOutput::UniqueId)));
    assert!(has_field(&h, "get_tag_owner_unique_id=Y"), "{h:?}");
}

// ── Validation ───────────────────────────────────────────────────────────────

fn enable_validation() {
    std::env::set_var("PODOS_VALIDATE", "1");
}

fn find_err<'a>(
    errs: &'a ValidationErrors,
    field: &str,
    severity: &str,
) -> Option<&'a pod_os_client::message::ValidationError> {
    errs.iter()
        .find(|e| e.field == field && e.severity == severity)
}

#[test]
fn validate_tag_format_struct() {
    enable_validation();

    let valid = [
        get_event_msg(GetEventOptions {
            get_tags: true,
            tag_format: Some(1),
            tag_owner_output: TagOwnerOutput::EventKey,
            ..Default::default()
        }),
        events_for_tags_msg(GetEventsForTagsOptions {
            buffer_results: true,
            buffer_format: "1".into(),
            tag_owner_output: TagOwnerOutput::UniqueId,
            ..Default::default()
        }),
    ];
    for msg in &valid {
        let errs = msg.validate();
        assert!(errs.is_empty(), "unexpected validation errors: {errs:#?}");
        let sm = encode_message(msg, "").expect("encode");
        let raw = validate_raw_message(sm.as_bytes());
        assert!(raw.is_empty(), "unexpected wire validation errors: {raw:#?}");
    }

    let cases: Vec<(&str, Message, &str, &str, &str, &str)> = vec![
        (
            "tag_format=2",
            get_event_msg(GetEventOptions {
                get_tags: true,
                tag_format: Some(2),
                ..Default::default()
            }),
            "NeuralMemory.GetEvent.TagFormat",
            "error",
            "tag_format",
            "format",
        ),
        (
            "owner without tag_format=1",
            get_event_msg(GetEventOptions {
                get_tags: true,
                tag_owner_output: TagOwnerOutput::UniqueId,
                ..Default::default()
            }),
            "NeuralMemory.GetEvent.TagOwnerOutput",
            "error",
            "output_tag_owner",
            "semantic",
        ),
        (
            "tag_format=1 without get_tags",
            get_event_msg(GetEventOptions {
                tag_format: Some(1),
                ..Default::default()
            }),
            "NeuralMemory.GetEvent.TagFormat",
            "warn",
            "tag_format",
            "semantic",
        ),
        (
            "buffer_format=2",
            events_for_tags_msg(GetEventsForTagsOptions {
                buffer_format: "2".into(),
                ..Default::default()
            }),
            "NeuralMemory.GetEventsForTags.BufferFormat",
            "error",
            "buffer_format",
            "format",
        ),
        (
            "owner without buffer_format=1",
            events_for_tags_msg(GetEventsForTagsOptions {
                tag_owner_output: TagOwnerOutput::EventKey,
                ..Default::default()
            }),
            "NeuralMemory.GetEventsForTags.TagOwnerOutput",
            "warn",
            "get_tag_owner / get_tag_owner_unique_id",
            "semantic",
        ),
    ];
    for (name, msg, field, severity, wire, rule) in cases {
        let errs = msg.validate();
        let e = find_err(&errs, field, severity)
            .unwrap_or_else(|| panic!("{name}: want {severity} on {field}, got {errs:#?}"));
        assert_eq!((e.wire_field.as_str(), e.rule.as_str()), (wire, rule), "{name}");
        assert!(!e.fix.is_empty() && !e.example_code.is_empty(), "{name}: {e:?}");
    }
}

/// Insert `extra` right after the first occurrence of `after` in the header of an encoded
/// frame, updating the total and header length prefixes.
fn inject_header_field(raw: &[u8], after: &str, extra: &str) -> Vec<u8> {
    let s = String::from_utf8(raw.to_vec()).unwrap();
    let pos = s.find(after).expect("header field present") + after.len();
    let hex = |r: std::ops::Range<usize>| usize::from_str_radix(&s[r][1..], 16).unwrap();
    let total = hex(0..9) + extra.len();
    let header = hex(27..36) + extra.len();
    format!(
        "x{total:08x}{}x{header:08x}{}{extra}{}",
        &s[9..27],
        &s[36..pos],
        &s[pos..]
    )
    .into_bytes()
}

#[test]
fn validate_raw_tag_format_headers() {
    enable_validation();

    let bf1 = events_for_tags_msg(GetEventsForTagsOptions {
        buffer_results: true,
        buffer_format: "1".into(),
        ..Default::default()
    });
    let raw = encode_message(&bf1, "").unwrap();
    let errs = validate_raw_message(&inject_header_field(
        raw.as_bytes(),
        "buffer_format=1",
        "\tget_tag_owner=yes",
    ));
    assert!(
        errs.iter()
            .any(|e| e.wire_field == "get_tag_owner" && e.rule == "header_value"),
        "{errs:#?}"
    );

    let bf0 = events_for_tags_msg(GetEventsForTagsOptions::default());
    let raw = encode_message(&bf0, "").unwrap();
    let errs = validate_raw_message(&inject_header_field(
        raw.as_bytes(),
        "buffer_format=0",
        "\tget_tag_owner_unique_id=Y",
    ));
    assert!(
        errs.iter().any(|e| e.severity == "warn"
            && e.wire_field == "get_tag_owner / get_tag_owner_unique_id"
            && e.rule == "semantic"),
        "{errs:#?}"
    );

    let ge = get_event_msg(GetEventOptions {
        get_tags: true,
        ..Default::default()
    });
    let raw = encode_message(&ge, "").unwrap();
    let errs = validate_raw_message(&inject_header_field(
        raw.as_bytes(),
        "tag_format=0",
        "\toutput_tag_owner=maybe",
    ));
    assert!(
        errs.iter()
            .any(|e| e.wire_field == "output_tag_owner" && e.rule == "header_value"),
        "{errs:#?}"
    );
    assert!(
        errs.iter().any(|e| e.severity == "warn"
            && e.wire_field == "output_tag_owner"
            && e.rule == "semantic"),
        "{errs:#?}"
    );

    let raw = encode_message(&ge, "").unwrap();
    let bad_tf = String::from_utf8(raw.as_bytes().to_vec())
        .unwrap()
        .replacen("tag_format=0", "tag_format=7", 1);
    let errs = validate_raw_message(bad_tf.as_bytes());
    assert!(
        errs.iter()
            .any(|e| e.wire_field == "tag_format" && e.rule == "header_value"),
        "{errs:#?}"
    );
}
