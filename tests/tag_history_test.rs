use pod_os_client::message::{
    decode_message, field_history, latest_by_key, suggest_delta, tag_search_clauses, TagOutput,
    TagTerm,
};

fn tag(key: &str, value: &str, ts: &str, n: i64) -> TagOutput {
    TagOutput {
        key: key.to_string(),
        value: value.to_string(),
        timestamp: ts.to_string(),
        tag_number: n,
        frequency: 1,
        ..Default::default()
    }
}

#[test]
fn latest_by_key_timestamp_then_tag_number() {
    let tags = vec![
        tag("hue.alpha", "old", "0000000001.000000", 9),
        tag("hue.alpha", "mid", "0000000002.000000", 1),
        tag("hue.alpha", "tie-low", "0000000002.000000", 4),
        tag("hue.beta", "only", "0000000001.000000", 1),
        tag("hue.alpha", "tie-high", "0000000002.000000", 4),
    ];
    let got = latest_by_key(&tags);
    assert_eq!(got["hue.alpha"].value, "tie-high");
    assert_eq!(got["hue.beta"].value, "only");

    let later = latest_by_key(&[
        tag("k", "low-number", "0000000005.000000", 1),
        tag("k", "high-number", "0000000005.000000", 8),
    ]);
    assert_eq!(later["k"].value, "high-number");

    let earlier_ts = latest_by_key(&[
        tag("k", "new-ts", "0000000009.000000", 1),
        tag("k", "old-ts", "0000000001.000000", 99),
    ]);
    assert_eq!(earlier_ts["k"].value, "new-ts");
}

#[test]
fn field_history_zip_and_edges() {
    let tags = vec![
        tag("paint.beta", "b0", "0000000002.000000", 1),
        tag("paint.alpha", "a-low", "0000000002.000000", 2),
        tag("paint.alpha", "a-tie", "0000000002.000000", 9),
        tag("other.alpha", "nope", "0000000009.000000", 1),
        tag("paint", "bare", "0000000009.000000", 1),
    ];
    let got = field_history(&tags, "paint");
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].index, 0);
    assert_eq!(got[0].facets["alpha"].value, "a-low");
    assert!(!got[0].facets.contains_key("beta"));
    assert_eq!(got[1].facets["alpha"].value, "a-tie");
    assert_eq!(got[1].facets["beta"].value, "b0");
    assert!(field_history(&tags, "").is_empty());
    assert!(field_history(&tags, "pai").is_empty());

    let dotted = field_history(
        &[
            tag("a.b.c", "yes", "0000000001.000000", 1),
            tag("a.bc", "no", "0000000001.000000", 1),
            tag("a.b", "bare", "0000000001.000000", 1),
        ],
        "a.b",
    );
    assert_eq!(dotted.len(), 1);
    assert_eq!(dotted[0].facets["c"].value, "yes");
    assert_eq!(dotted[0].facets.len(), 1);
}

#[test]
fn conventional_facet_names_are_ordinary_keys() {
    let tags = vec![
        tag("stage.value", "seed", "0000000001.000000", 1),
        tag("stage.event_timestamp", "2020", "0000000001.000001", 2),
        tag("stage.delta_type", "initial", "0000000001.000002", 3),
        tag("stage.confidence", "0.5", "0000000001.000003", 4),
        tag("stage.evidence", "", "0000000001.000004", 5),
        tag("stage.source", "doc-1", "0000000001.000005", 6),
        tag("stage.value", "series-a", "0000000002.000000", 7),
        tag("stage.source", "doc-2", "0000000002.000005", 12),
    ];
    let got = field_history(&tags, "stage");
    assert_eq!(got[0].facets["value"].value, "seed");
    assert_eq!(got[1].facets["source"].value, "doc-2");
}

#[test]
fn suggest_delta_and_clauses() {
    assert_eq!(suggest_delta(false, "", "seed"), "initial");
    assert_eq!(suggest_delta(true, "seed", "seed"), "confirmed");
    assert_eq!(suggest_delta(true, "seed", "series-a"), "modified");

    let got = tag_search_clauses(&[
        TagTerm {
            key: "hue.alpha".into(),
            value: "red=blue".into(),
        },
        TagTerm {
            key: "hue.beta".into(),
            value: "1".into(),
        },
    ]);
    let lines: Vec<&str> = got.split('\n').collect();
    assert_eq!(
        lines[0],
        "clause_type:S\tboolean:or\tlow:hue.alpha=red=blue"
    );
    assert_eq!(lines[1], "clause_type:S\tboolean:and\tlow:hue.beta=1");
}

fn wire_frame(header: &str, payload: &str) -> Vec<u8> {
    let to = b"mem@gateway.example.com";
    let from = b"client@gateway.example.com";
    let hdr = header.as_bytes();
    let pay = payload.as_bytes();
    let total = 63 + to.len() + from.len() + hdr.len() + pay.len();
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(format!("x{total:08x}").as_bytes());
    buf.extend_from_slice(format!("x{:08x}", to.len()).as_bytes());
    buf.extend_from_slice(format!("x{:08x}", from.len()).as_bytes());
    buf.extend_from_slice(format!("x{:08x}", hdr.len()).as_bytes());
    buf.extend_from_slice(format!("{:09}", 1001).as_bytes());
    buf.extend_from_slice(format!("{:09}", 0).as_bytes());
    buf.extend_from_slice(format!("x{:08x}", pay.len()).as_bytes());
    buf.extend_from_slice(to);
    buf.extend_from_slice(from);
    buf.extend_from_slice(hdr);
    buf.extend_from_slice(pay);
    buf
}

#[test]
fn decode_get_event_link_and_target_tag_history() {
    let header = "_type=get\t_status=OK\t_event_id=evt_a\t_unique_id=eo_seed";
    let payload = [
        "_link=link1\tunique_id=lk_1\ttarget_event=evt_n\ttarget_unique_id=eo_nbr\tstrength=0.5\tcategory=rel",
        "_linktag\tevent_id=link1\tunique=lk_1\tfreq=1\ttimestamp=0000000002.000000\tvalue=paint.alpha=later",
        "_linktag\tevent_id=link1\tunique=lk_1\tfreq=1\ttimestamp=0000000001.000000\tvalue=paint.alpha=earlier",
        "_target_event_tag\tevent_id=evt_n\tfreq=1\ttimestamp=0000000002.000000\tvalue=paint.alpha=n-later",
        "_target_event_tag\tevent_id=evt_n\tfreq=1\ttimestamp=0000000001.000000\tvalue=paint.alpha=n-earlier",
    ]
    .join("\n");
    let decoded = decode_message(&wire_frame(header, &payload)).expect("decode");
    let event = decoded.event.expect("event");
    assert_eq!(event.links.len(), 1);
    let link = &event.links[0];
    assert_eq!(link.tags.len(), 2);
    assert!(!link.tags[0].timestamp.is_empty());
    assert!(!link.tags[1].timestamp.is_empty());
    assert_eq!(link.target_tags.len(), 2);
    let link_hist = field_history(&link.tags, "paint");
    assert_eq!(link_hist[0].facets["alpha"].value, "earlier");
    assert_eq!(link_hist[1].facets["alpha"].value, "later");
    let target_hist = field_history(&link.target_tags, "paint");
    assert_eq!(target_hist[0].facets["alpha"].value, "n-earlier");
    assert_eq!(target_hist[1].facets["alpha"].value, "n-later");
}

#[test]
fn link_tags_join_by_unique_and_stay_off_target_tags() {
    let header = "_type=get\t_status=OK\t_event_id=evt_a\t_unique_id=eo_seed";
    let payload = [
        "_link=linkA\tunique_id=lk\ttarget_event=evt_n\ttarget_unique_id=eo_nbr\tstrength=0.5\tcategory=rel",
        "_linktag\tevent_id=linkA\tunique=lk\tfreq=1\ttimestamp=0000000001.000000\tvalue=paint.alpha=la1",
        "_target_event_tag\tevent_id=evt_n\tfreq=1\ttimestamp=0000000001.000000\tvalue=paint.alpha=na1",
    ]
    .join("\n");
    let decoded = decode_message(&wire_frame(header, &payload)).expect("decode");
    let link = &decoded.event.expect("event").links[0];
    assert_eq!(link.tags.len(), 1);
    assert_eq!(link.tags[0].value, "la1");
    assert_eq!(link.target_tags.len(), 1);
    assert_eq!(link.target_tags[0].value, "na1");
}
