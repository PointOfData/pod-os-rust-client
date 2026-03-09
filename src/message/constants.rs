use std::sync::atomic::{AtomicI64, Ordering};

/// Maximum allowed message size (payload or total). Mutable so tests can lower it.
static MAX_MESSAGE_SIZE_BYTES: AtomicI64 = AtomicI64::new(2 * 1024 * 1024 * 1024);

pub fn max_message_size() -> i64 {
    MAX_MESSAGE_SIZE_BYTES.load(Ordering::Relaxed)
}

/// Override the limit — intended for tests only.
pub fn set_max_message_size(bytes: i64) {
    MAX_MESSAGE_SIZE_BYTES.store(bytes, Ordering::Relaxed);
}

/// Struct-tag key used in the Go client's reflection-based header builder.
/// Kept as a constant for documentation purposes.
pub const SOCKET_FIELD_TAG: &str = "podos";
