use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current time as a Pod-OS POSIX timestamp:
/// `"+<seconds>.<microseconds>"` — e.g. `"+1741388400.123456"`.
pub fn get_timestamp() -> String {
    timestamp_from_duration(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default(),
    )
}

/// Same format for an arbitrary `std::time::SystemTime`.
pub fn get_timestamp_from_time(t: SystemTime) -> String {
    timestamp_from_duration(t.duration_since(UNIX_EPOCH).unwrap_or_default())
}

fn timestamp_from_duration(d: std::time::Duration) -> String {
    let secs  = d.as_secs();
    let usecs = d.subsec_micros();
    format!("+{}.{:06}", secs, usecs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_correct() {
        let ts = get_timestamp();
        assert!(ts.starts_with('+'), "must start with '+', got: {ts}");
        let dot = ts.find('.').expect("must contain '.'");
        let frac = &ts[dot + 1..];
        assert_eq!(frac.len(), 6, "must have 6 fractional digits, got: {ts}");
    }
}
