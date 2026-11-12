//! Tag version history. Field and facet names are caller-defined.
//! Version order is storage timestamp, then tag number.

use std::collections::HashMap;

use crate::message::types::TagOutput;

/// One version index across the facets of a field.
#[derive(Debug, Clone, PartialEq)]
pub struct TagSighting {
    pub index: usize,
    pub facets: HashMap<String, TagOutput>,
}

/// One search term. `key` is the full tag key; `value` may contain `=`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagTerm {
    pub key: String,
    pub value: String,
}

/// Whether [`crate::Client::append_tags`] wrote a version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendResult {
    pub wrote: bool,
}

fn tag_before(a: &TagOutput, b: &TagOutput) -> bool {
    if a.timestamp != b.timestamp {
        return a.timestamp < b.timestamp;
    }
    a.tag_number < b.tag_number
}

/// Latest row for each full tag key.
/// The winner is the greatest storage timestamp, then the greatest tag number.
/// When both are equal, the later input row wins.
pub fn latest_by_key(tags: &[TagOutput]) -> HashMap<String, TagOutput> {
    let mut out = HashMap::with_capacity(tags.len());
    for tag in tags {
        match out.get(&tag.key) {
            Some(prev) if tag_before(tag, prev) => {}
            _ => {
                out.insert(tag.key.clone(), tag.clone());
            }
        }
    }
    out
}

/// Zip versions of keys that start with `field + "."`.
/// A facet added in a later batch is absent from earlier sightings.
/// An empty field returns no sightings.
pub fn field_history(tags: &[TagOutput], field: &str) -> Vec<TagSighting> {
    if field.is_empty() {
        return Vec::new();
    }
    let prefix = format!("{field}.");
    let mut groups: HashMap<String, Vec<TagOutput>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for tag in tags {
        if !tag.key.starts_with(&prefix) || tag.key.len() == prefix.len() {
            continue;
        }
        let facet = tag.key[prefix.len()..].to_string();
        if !groups.contains_key(&facet) {
            order.push(facet.clone());
        }
        groups.entry(facet).or_default().push(tag.clone());
    }
    if groups.is_empty() {
        return Vec::new();
    }
    let mut max_n = 0usize;
    for facet in &order {
        let rows = groups.get_mut(facet).expect("facet inserted");
        rows.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then(a.tag_number.cmp(&b.tag_number)));
        max_n = max_n.max(rows.len());
    }
    let mut sightings: Vec<TagSighting> = (0..max_n)
        .map(|index| TagSighting {
            index,
            facets: HashMap::new(),
        })
        .collect();
    for facet in &order {
        let rows = &groups[facet];
        let offset = max_n - rows.len();
        for (i, row) in rows.iter().enumerate() {
            sightings[offset + i]
                .facets
                .insert(facet.clone(), row.clone());
        }
    }
    sightings
}

/// Classify a new value. `append_tags` does not call this.
pub fn suggest_delta(has_previous: bool, previous: &str, next: &str) -> &'static str {
    if !has_previous {
        return "initial";
    }
    if previous == next {
        "confirmed"
    } else {
        "modified"
    }
}

/// One clause per term. The first line is `boolean:or`; later lines are `boolean:and`.
pub fn tag_search_clauses(terms: &[TagTerm]) -> String {
    let mut lines = Vec::with_capacity(terms.len());
    for (i, term) in terms.iter().enumerate() {
        let op = if i == 0 { "or" } else { "and" };
        lines.push(format!(
            "clause_type:S\tboolean:{op}\tlow:{}={}",
            term.key, term.value
        ));
    }
    lines.join("\n")
}
