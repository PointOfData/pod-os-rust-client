use serde::{Deserialize, Serialize};
use crate::message::intents::Intent;

// ─────────────────────────────────────────────
// Primitive helpers
// ─────────────────────────────────────────────

/// Nullable integer — `None` means "not set / omit from wire".
pub type NullInt = Option<i64>;

/// Raw payload data — string, bytes, or multiple strings.
#[derive(Debug, Clone, Default)]
pub enum PayloadData {
    #[default]
    Empty,
    Text(String),
    Binary(Vec<u8>),
    Lines(Vec<String>),
}

impl PayloadData {
    pub fn is_empty(&self) -> bool {
        matches!(self, PayloadData::Empty)
    }
}

/// Payload encoding type (only RAW = 0 is currently supported).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    #[default]
    Raw = 0,
}

impl DataType {
    pub fn as_wire_int(self) -> i32 { self as i32 }
    pub fn from_wire_int(v: i32) -> Self {
        match v { _ => DataType::Raw }
    }
}

// ─────────────────────────────────────────────
// Date / time
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct DateTimeObject {
    pub year:  i32,
    pub month: i32,
    pub day:   i32,
    pub hour:  i32,
    pub min:   i32,
    pub sec:   i32,
    pub usec:  i32,
}

// ─────────────────────────────────────────────
// Tag types
// ─────────────────────────────────────────────

/// Structured tag value that mirrors Go's `Tag.Value: any`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TagValue {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Json(serde_json::Value),
}

impl Default for TagValue {
    fn default() -> Self { TagValue::Text(String::new()) }
}

impl TagValue {
    pub fn as_str(&self) -> Option<&str> {
        if let TagValue::Text(s) = self { Some(s.as_str()) } else { None }
    }
    pub fn as_int(&self) -> Option<i64> {
        if let TagValue::Int(n) = self { Some(*n) } else { None }
    }
    pub fn as_float(&self) -> Option<f64> {
        if let TagValue::Float(f) = self { Some(*f) } else { None }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let TagValue::Bool(b) = self { Some(*b) } else { None }
    }
}

/// Tag as used in `NeuralMemoryFields` and batch operations.
#[derive(Debug, Clone, Default)]
pub struct Tag {
    pub frequency: i32,
    pub key:       String,
    pub value:     TagValue,
    pub timestamp: String,
    pub id:        String,
}

/// Ordered list of tags.
pub type TagList = Vec<Tag>;

/// Tag as returned in decoded responses.
#[derive(Debug, Clone, Default)]
pub struct TagOutput {
    pub frequency:    i32,
    pub category:     String,
    pub key:          String,
    pub value:        String,
    pub owner:        String,
    pub timestamp:    String,
    pub target_tag_id: String,
}

// ─────────────────────────────────────────────
// Link / event reference
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct LinkFields {
    pub unique_id:            String,
    pub id:                   String,
    pub local_id:             String,
    pub owner:                String,
    pub timestamp:            String,
    pub date_time:            DateTimeObject,
    pub location:             String,
    pub location_separator:   String,
    pub event_a:              String,
    pub event_b:              String,
    pub unique_id_a:          String,
    pub unique_id_b:          String,
    pub strength_a:           f64,
    pub strength_b:           f64,
    pub category:             String,
    pub r#type:               String,
    pub owner_unique_id:      String,
    pub owner_id:             String,
    pub tags:                 Vec<TagOutput>,
    pub target_tags:          Vec<TagOutput>,
    pub status:               String,
    pub message:              String,
}

// ─────────────────────────────────────────────
// Event fields
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct EventFields {
    pub unique_id:          String,
    pub id:                 String,
    pub local_id:           String,
    pub owner:              String,
    pub owner_unique_id:    String,
    pub timestamp:          String,
    pub date_time:          DateTimeObject,
    pub location:           String,
    pub location_separator: String,
    pub r#type:             String,
    pub tags:               Vec<TagOutput>,
    pub links:              Vec<LinkFields>,
    pub payload_data:       PayloadFields,
    pub status:             String,
    pub hits:               i32,
}

// ─────────────────────────────────────────────
// Payload
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct PayloadFields {
    pub data:      PayloadData,
    pub data_type: DataType,
    pub mime_type: String,
    pub data_size: i32,
}

// ─────────────────────────────────────────────
// NeuralMemory query options
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct GetEventOptions {
    pub send_data:           bool,
    pub local_id_only:       bool,
    pub get_tags:            bool,
    pub get_links:           bool,
    pub get_link_tags:       bool,
    pub get_target_tags:     bool,
    pub tag_format:          NullInt,
    pub request_format:      i32,
    pub first_link:          i32,
    pub link_count:          i32,
    pub event_facet_filter:  String,
    pub link_facet_filter:   String,
    pub target_facet_filter: String,
    pub category_filter:     String,
    pub tag_filter:          String,
}

#[derive(Debug, Clone, Default)]
pub struct GetEventsForTagsOptions {
    pub event_pattern:           String,
    pub event_pattern_high:      String,
    pub include_brief_hits:      bool,
    pub get_all_data:            bool,
    pub first_link:              i32,
    pub link_count:              i32,
    pub events_per_message:      i32,
    pub start_result:            i32,
    pub end_result:              i32,
    pub min_event_hits:          i32,
    pub count_only:              bool,
    pub get_match_links:         bool,
    pub count_match_links:       bool,
    pub get_link_tags:           bool,
    pub get_target_tags:         bool,
    pub link_tag_filter:         String,
    pub linked_events_filter:    String,
    pub link_category:           String,
    pub owner:                   String,
    pub owner_unique_id:         String,
    pub get_event_object_count:  bool,
    pub buffer_results:          bool,
    pub include_tag_stats:       bool,
    pub invert_hit_tag_filter:   bool,
    pub hit_tag_filter:          String,
    pub buffer_format:           String,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub pattern: String,
}

// ─────────────────────────────────────────────
// Response fields
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ResponseFields {
    pub status:               String,
    pub message:              String,
    pub tag_count:            i32,
    pub link_count:           i32,
    pub link_id:              String,
    pub date_time:            DateTimeObject,
    pub total_events:         i32,
    pub returned_events:      i32,
    pub start_result:         i32,
    pub end_result:           i32,
    pub storage_error_count:  i32,
    pub storage_success_count: i32,

    pub event_records:              Vec<EventFields>,
    pub store_link_batch_event_record: StoreLinkBatchEventRecord,
    pub store_batch_event_record:   StoreBatchEventRecord,

    pub match_term_count: i32,
    pub is_buffered:      bool,
    pub brief_hits:       Vec<BriefHitRecord>,
}

// ─────────────────────────────────────────────
// Batch types
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct BatchEventSpec {
    pub event: EventFields,
    pub tags:  TagList,
}

#[derive(Debug, Clone, Default)]
pub struct BatchLinkEventSpec {
    pub event: EventFields,
    pub link:  LinkFields,
}

#[derive(Debug, Clone, Default)]
pub struct BriefHitRecord {
    pub event_id:   String,
    pub total_hits: i32,
}

#[derive(Debug, Clone, Default)]
pub struct StoreBatchEventRecord {
    pub status:       String,
    pub message:      String,
    pub event_count:  i32,
    pub event_results: Vec<EventFields>,
}

#[derive(Debug, Clone, Default)]
pub struct StoreLinkBatchEventRecord {
    pub status:                   String,
    pub message:                  String,
    pub total_link_requests_found: i32,
    pub links_ok:                 i32,
    pub links_with_errors:        i32,
    pub link_results:             Vec<LinkFields>,
}

// ─────────────────────────────────────────────
// NeuralMemory fields
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct NeuralMemoryFields {
    pub get_event:          Option<GetEventOptions>,
    pub get_events_for_tags: Option<GetEventsForTagsOptions>,
    pub search:             Option<SearchOptions>,
    pub link:               Option<LinkFields>,
    pub batch_links:        Vec<BatchLinkEventSpec>,
    pub unlink:             Option<LinkFields>,
    pub tags:               TagList,
    pub batch_events:       Vec<BatchEventSpec>,
}

// ─────────────────────────────────────────────
// Envelope + Message
// ─────────────────────────────────────────────

/// Routing envelope — present in every message.
#[derive(Debug, Clone, Default)]
pub struct Envelope {
    /// `"actor@gateway.domain"` — destination address.
    pub to:          String,
    /// `"clientName@gateway.domain"` — source address.
    pub from:        String,
    pub intent:      Intent,
    pub client_name: String,
    pub message_id:  String,
    pub passcode:    String,
    pub user_name:   String,
}

/// Top-level message, combining envelope with optional payload sections.
#[derive(Debug, Clone, Default)]
pub struct Message {
    pub envelope:      Envelope,
    pub event:         Option<EventFields>,
    pub payload:       Option<PayloadFields>,
    pub neural_memory: Option<NeuralMemoryFields>,
    pub response:      Option<ResponseFields>,
    pub public_key:    Option<Vec<u8>>,
}

impl Message {
    // ── Envelope delegates ──────────────────────────────────────

    pub fn to(&self)          -> &str { &self.envelope.to }
    pub fn from(&self)        -> &str { &self.envelope.from }
    pub fn intent(&self)      -> &Intent { &self.envelope.intent }
    pub fn client_name(&self) -> &str { &self.envelope.client_name }
    pub fn message_id(&self)  -> &str { &self.envelope.message_id }

    // ── Convenience accessors ────────────────────────────────────

    pub fn event_id(&self) -> &str {
        self.event.as_ref().map_or("", |e| &e.id)
    }

    pub fn event_unique_id(&self) -> &str {
        self.event.as_ref().map_or("", |e| &e.unique_id)
    }

    pub fn payload_data(&self) -> Option<&PayloadData> {
        self.payload.as_ref().map(|p| &p.data)
    }

    pub fn payload_mime_type(&self) -> &str {
        self.payload.as_ref().map_or("", |p| &p.mime_type)
    }

    pub fn processing_status(&self) -> &str {
        self.response.as_ref().map_or("", |r| &r.status)
    }

    pub fn processing_message(&self) -> &str {
        self.response.as_ref().map_or("", |r| &r.message)
    }

    pub fn tags(&self) -> Vec<&TagOutput> {
        self.event.as_ref().map_or_else(Vec::new, |e| e.tags.iter().collect())
    }

    pub fn link(&self) -> Option<&LinkFields> {
        self.neural_memory.as_ref().and_then(|n| n.link.as_ref())
    }

    pub fn get_event_opts(&self) -> Option<&GetEventOptions> {
        self.neural_memory.as_ref().and_then(|n| n.get_event.as_ref())
    }

    pub fn get_events_for_tags_opts(&self) -> Option<&GetEventsForTagsOptions> {
        self.neural_memory.as_ref().and_then(|n| n.get_events_for_tags.as_ref())
    }
}

// ─────────────────────────────────────────────
// Raw socket message (pre-encoded bytes)
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SocketMessage {
    pub data: Vec<u8>,
}

impl SocketMessage {
    pub fn new(data: Vec<u8>) -> Self { Self { data } }
    pub fn as_bytes(&self) -> &[u8] { &self.data }
    pub fn into_bytes(self) -> Vec<u8> { self.data }
    pub fn len(&self) -> usize { self.data.len() }
    pub fn is_empty(&self) -> bool { self.data.is_empty() }
}

// ─────────────────────────────────────────────
// Gateway configuration structs
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PodOsConfiguration {
    pub gateways: Vec<GatewayDefinition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatewayDefinition {
    pub name:          String,
    pub host:          String,
    pub port:          String,
    pub network:       String,
    pub actors:        Vec<String>,
    pub shell_actors:  Vec<ShellActor>,
    pub neural_memory_actors: Vec<NeuralMemoryActor>,
    pub peer_actors:   Vec<PeerActor>,
    pub script_actors: Vec<ScriptActor>,
    pub mailbox_actors: Vec<MailboxActor>,
    pub socket_actors: Vec<SocketActor>,
    pub router_actors: Vec<RouterActor>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShellActor {
    pub name:    String,
    pub command: String,
    pub args:    Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NeuralMemoryActor {
    pub name:             String,
    pub storage_path:     String,
    pub max_memory_bytes: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PeerActor {
    pub name:     String,
    pub host:     String,
    pub port:     String,
    pub network:  String,
    pub actor:    String,
    pub passcode: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptActor {
    pub name:   String,
    pub script: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MailboxActor {
    pub name:         String,
    pub storage_path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocketActor {
    pub name:    String,
    pub host:    String,
    pub port:    String,
    pub network: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouterActor {
    pub name:   String,
    pub routes: Vec<RouteDefinition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouteDefinition {
    pub name:        String,
    pub r#type:      String,
    pub test:        String,
    pub test_value:  String,
    pub action:      String,
    pub action_value: String,
}

// ── Routing constants ────────────────────────────────────────────────────────

pub const ROUTING_TEST_TYPE_NONE:   &str = "NONE";
pub const ROUTING_TEST_TYPE_EQ:     &str = "EQ";
pub const ROUTING_TEST_TYPE_NE:     &str = "NE";
pub const ROUTING_TEST_TYPE_LT:     &str = "LT";
pub const ROUTING_TEST_TYPE_LE:     &str = "LE";
pub const ROUTING_TEST_TYPE_GT:     &str = "GT";
pub const ROUTING_TEST_TYPE_GE:     &str = "GE";
pub const ROUTING_TEST_TYPE_RANGE:  &str = "RANGE";
pub const ROUTING_TEST_TYPE_EXCL:   &str = "EXCL";
pub const ROUTING_TEST_TYPE_REGEXP: &str = "REGEXP";
pub const ROUTING_TEST_TYPE_NUM_EQ: &str = "#EQ";
pub const ROUTING_TEST_TYPE_NUM_NE: &str = "#NE";
pub const ROUTING_TEST_TYPE_NUM_LT: &str = "#LT";
pub const ROUTING_TEST_TYPE_NUM_LE: &str = "#LE";
pub const ROUTING_TEST_TYPE_NUM_GT: &str = "#GT";
pub const ROUTING_TEST_TYPE_NUM_GE: &str = "#GE";
pub const ROUTING_TEST_TYPE_NUM_RANGE: &str = "#RANGE";
pub const ROUTING_TEST_TYPE_NUM_EXCL:  &str = "#EXCL";

pub const ROUTING_ACTION_TYPE_NONE:      &str = "NONE";
pub const ROUTING_ACTION_TYPE_ROUTE:     &str = "ROUTE";
pub const ROUTING_ACTION_TYPE_DISCARD:   &str = "DISCARD";
pub const ROUTING_ACTION_TYPE_CHANGE:    &str = "CHANGE";
pub const ROUTING_ACTION_TYPE_DUPLICATE: &str = "DUPLICATE";
