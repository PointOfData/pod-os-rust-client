use once_cell::sync::Lazy;
use std::collections::HashMap;

/// A Pod-OS intent descriptor, encoding all wire-level discriminants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Intent {
    pub name: &'static str,
    pub routing_message_type: &'static str,
    pub neural_memory_command: &'static str,
    pub message_type: i32,
}

impl Intent {
    const fn new(
        name: &'static str,
        routing_message_type: &'static str,
        neural_memory_command: &'static str,
        message_type: i32,
    ) -> Self {
        Self {
            name,
            routing_message_type,
            neural_memory_command,
            message_type,
        }
    }
    pub fn is_zero(&self) -> bool {
        self.name.is_empty()
    }
}

// ── All known intents ────────────────────────────────────────────────────────

pub static STORE_EVENT: Intent = Intent::new("StoreEvent", "MEM_REQ", "store", 1000);
pub static STORE_DATA: Intent = Intent::new("StoreData", "MEM_REQ", "store_data", 1000);
pub static STORE_BATCH_EVENTS: Intent =
    Intent::new("StoreBatchEvents", "MEM_REQ", "store_batch", 1000);
pub static STORE_BATCH_TAGS: Intent =
    Intent::new("StoreBatchTags", "MEM_REQ", "tag_store_batch", 1000);
pub static GET_EVENT: Intent = Intent::new("GetEvent", "MEM_REQ", "get", 1000);
pub static GET_EVENTS_FOR_TAGS: Intent =
    Intent::new("GetEventsForTags", "MEM_REQ", "events_for_tag", 1000);
pub static LINK_EVENT: Intent = Intent::new("LinkEvent", "MEM_REQ", "link", 1000);
pub static UNLINK_EVENT: Intent = Intent::new("UnlinkEvent", "MEM_REQ", "unlink", 1000);
pub static STORE_BATCH_LINKS: Intent =
    Intent::new("StoreBatchLinks", "MEM_REQ", "link_batch", 1000);

pub static STORE_EVENT_RESPONSE: Intent =
    Intent::new("StoreEventResponse", "MEM_REPLY", "store", 1001);
pub static STORE_DATA_RESPONSE: Intent =
    Intent::new("StoreDataResponse", "MEM_REPLY", "store_data", 1001);
pub static STORE_BATCH_EVENTS_RESPONSE: Intent =
    Intent::new("StoreBatchEventsResponse", "MEM_REPLY", "store_batch", 1001);
pub static STORE_BATCH_TAGS_RESPONSE: Intent = Intent::new(
    "StoreBatchTagsResponse",
    "MEM_REPLY",
    "tag_store_batch",
    1001,
);
pub static GET_EVENT_RESPONSE: Intent = Intent::new("GetEventResponse", "MEM_REPLY", "get", 1001);
pub static GET_EVENTS_FOR_TAGS_RESPONSE: Intent = Intent::new(
    "GetEventsForTagsResponse",
    "MEM_REPLY",
    "events_for_tag",
    1001,
);
pub static LINK_EVENT_RESPONSE: Intent =
    Intent::new("LinkEventResponse", "MEM_REPLY", "link", 1001);
pub static UNLINK_EVENT_RESPONSE: Intent =
    Intent::new("UnlinkEventResponse", "MEM_REPLY", "unlink", 1001);
pub static STORE_BATCH_LINKS_RESPONSE: Intent =
    Intent::new("StoreBatchLinksResponse", "MEM_REPLY", "link_batch", 1001);

pub static ACTOR_ECHO: Intent = Intent::new("ActorEcho", "ECHO", "", 2);
pub static ACTOR_START: Intent = Intent::new("ActorStart", "START", "", 1);
pub static STATUS: Intent = Intent::new("Status", "STATUS", "", 3);
pub static GATEWAY_STATUS: Intent = Intent::new("GatewayStatus", "STATUS", "", 3);
pub static ACTOR_REQUEST: Intent = Intent::new("ActorRequest", "REQUEST", "", 4);
pub static ACTOR_RESPONSE: Intent = Intent::new("ActorResponse", "REPLY", "", 30);
pub static GATEWAY_ID: Intent = Intent::new("GatewayId", "ID", "", 5);
pub static GATEWAY_DISCONNECT: Intent = Intent::new("GatewayDisconnect", "DISCONNECT", "", 6);
pub static GATEWAY_SEND_NEXT: Intent = Intent::new("GatewaySendNext", "NEXT", "", 7);
pub static GATEWAY_NO_SEND: Intent = Intent::new("GatewayNoSend", "NO_SEND", "", 8);
pub static GATEWAY_STREAM_OFF: Intent = Intent::new("GatewayStreamOff", "STREAM_OFF", "", 9);
pub static GATEWAY_STREAM_ON: Intent = Intent::new("GatewayStreamOn", "STREAM_ON", "", 10);
pub static ACTOR_RECORD: Intent = Intent::new("ActorRecord", "RECORD", "", 11);
pub static GATEWAY_BATCH_START: Intent = Intent::new("GatewayBatchStart", "BATCH_START", "", 12);
pub static GATEWAY_BATCH_END: Intent = Intent::new("GatewayBatchEnd", "BATCH_END", "", 13);
pub static QUEUE_NEXT_REQUEST: Intent = Intent::new("QueueNextRequest", "QUEUE_NEXT", "", 14);
pub static QUEUE_ALL_REQUEST: Intent = Intent::new("QueueAllRequest", "QUEUE_ALL", "", 15);
pub static QUEUE_COUNT_REQUEST: Intent = Intent::new("QueueCountRequest", "QUEUE_COUNT", "", 16);
pub static QUEUE_EMPTY: Intent = Intent::new("QueueEmpty", "QUEUE_EMPTY", "", 17);
pub static KEEPALIVE: Intent = Intent::new("Keepalive", "KEEPALIVE", "", 18);
pub static ACTOR_REPORT: Intent = Intent::new("ActorReport", "REPORT", "", 19);
pub static REPORT_REQUEST: Intent = Intent::new("ReportRequest", "REPORT_REQUEST", "", 20);
pub static INFORMATION_REPORT: Intent = Intent::new("InformationReport", "INFO_REPORT", "", 21);
pub static AUTH_ADD_USER: Intent = Intent::new("AuthAddUser", "AUTH_ADD_USER", "", 100);
pub static AUTH_UPDATE_USER: Intent = Intent::new("AuthUpdateUser", "AUTH_UPDATE_USER", "", 101);
pub static AUTH_USER_LIST: Intent = Intent::new("AuthUserList", "AUTH_USER_LIST", "", 102);
pub static AUTH_DISABLE_USER: Intent = Intent::new("AuthDisableUser", "AUTH_DISABLE_USER", "", 103);
pub static ACTOR_HALT: Intent = Intent::new("ActorHalt", "HALT", "", 99);
pub static STATUS_REQUEST: Intent = Intent::new("StatusRequest", "STATUS_REQ", "", 110);
pub static ACTOR_USER: Intent = Intent::new("ActorUser", "USER", "", 65536);
pub static ROUTE_ANY_MESSAGE: Intent = Intent::new("RouteAnyMessage", "ANY", "", 0);
pub static ROUTE_USER_ONLY_MESSAGE: Intent = Intent::new("RouteUserOnlyMessage", "USERONLY", "", 0);

// ── Lookup indices ───────────────────────────────────────────────────────────

/// (message_type, neural_memory_command) → &'static Intent  [for request intents]
static BY_TYPE_AND_CMD: Lazy<HashMap<(i32, &'static str), &'static Intent>> = Lazy::new(|| {
    let mut m = HashMap::new();
    for intent in ALL_INTENTS {
        if !intent.neural_memory_command.is_empty() {
            m.insert((intent.message_type, intent.neural_memory_command), *intent);
        }
    }
    m
});

/// message_type → &'static Intent  [for non-NeuralMemory intents]
static BY_MESSAGE_TYPE: Lazy<HashMap<i32, &'static Intent>> = Lazy::new(|| {
    let mut m = HashMap::new();
    for intent in ALL_INTENTS {
        if intent.neural_memory_command.is_empty() {
            m.entry(intent.message_type).or_insert(*intent);
        }
    }
    m
});

/// routing_message_type (command string) → &'static Intent
static BY_COMMAND: Lazy<HashMap<&'static str, &'static Intent>> = Lazy::new(|| {
    let mut m = HashMap::new();
    for intent in ALL_INTENTS {
        m.entry(intent.routing_message_type).or_insert(*intent);
    }
    m
});

const ALL_INTENTS: &[&Intent] = &[
    &STORE_EVENT,
    &STORE_DATA,
    &STORE_BATCH_EVENTS,
    &STORE_BATCH_TAGS,
    &GET_EVENT,
    &GET_EVENTS_FOR_TAGS,
    &LINK_EVENT,
    &UNLINK_EVENT,
    &STORE_BATCH_LINKS,
    &STORE_EVENT_RESPONSE,
    &STORE_DATA_RESPONSE,
    &STORE_BATCH_EVENTS_RESPONSE,
    &STORE_BATCH_TAGS_RESPONSE,
    &GET_EVENT_RESPONSE,
    &GET_EVENTS_FOR_TAGS_RESPONSE,
    &LINK_EVENT_RESPONSE,
    &UNLINK_EVENT_RESPONSE,
    &STORE_BATCH_LINKS_RESPONSE,
    &ACTOR_ECHO,
    &ACTOR_START,
    &STATUS,
    &ACTOR_REQUEST,
    &ACTOR_RESPONSE,
    &GATEWAY_ID,
    &GATEWAY_DISCONNECT,
    &GATEWAY_SEND_NEXT,
    &GATEWAY_NO_SEND,
    &GATEWAY_STREAM_OFF,
    &GATEWAY_STREAM_ON,
    &ACTOR_RECORD,
    &GATEWAY_BATCH_START,
    &GATEWAY_BATCH_END,
    &QUEUE_NEXT_REQUEST,
    &QUEUE_ALL_REQUEST,
    &QUEUE_COUNT_REQUEST,
    &QUEUE_EMPTY,
    &KEEPALIVE,
    &ACTOR_REPORT,
    &REPORT_REQUEST,
    &INFORMATION_REPORT,
    &AUTH_ADD_USER,
    &AUTH_UPDATE_USER,
    &AUTH_USER_LIST,
    &AUTH_DISABLE_USER,
    &ACTOR_HALT,
    &STATUS_REQUEST,
    &ACTOR_USER,
    &ROUTE_ANY_MESSAGE,
    &ROUTE_USER_ONLY_MESSAGE,
];

// ── Public lookup functions ──────────────────────────────────────────────────

/// Resolve an intent from `(message_type, neural_memory_command)`.
/// Falls back to lookup by message_type alone when command is empty.
pub fn intent_from_message_type_and_command(
    message_type: i32,
    command: &str,
) -> Option<&'static Intent> {
    if !command.is_empty() {
        if let Some(i) = BY_TYPE_AND_CMD.get(&(message_type, command)) {
            return Some(i);
        }
    }
    BY_MESSAGE_TYPE.get(&message_type).copied()
}

/// Resolve from a routing command string (e.g. `"MEM_REQ"`).
pub fn intent_from_command(command: &str) -> Option<&'static Intent> {
    BY_COMMAND.get(command).copied()
}

/// Resolve a *response* intent from a neural_memory_command.
pub fn intent_from_response_command(command: &str) -> Option<&'static Intent> {
    intent_from_message_type_and_command(1001, command)
}

/// Resolve from message_type only.
pub fn intent_from_message_type(message_type: i32) -> Option<&'static Intent> {
    BY_MESSAGE_TYPE.get(&message_type).copied()
}
