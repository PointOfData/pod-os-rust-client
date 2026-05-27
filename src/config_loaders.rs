//! Environment variable and INI config loaders for Pod-OS client configuration.
//!
//! These mirror the Go SDK's `config/env.go` and `config/ini.go` helpers.

use std::collections::HashMap;
use std::env;
use std::time::Duration;

use crate::config::Config;

/// Builds a [`Config`] from `PODOS_*` environment variables.
///
/// Intended for Category 1 (self-registering) containers that receive
/// gateway connection details via environment rather than INI files.
///
/// Recognized variables:
///
/// - `PODOS_GATEWAY_HOST`, `PODOS_GATEWAY_PORT`, `PODOS_GATEWAY_FQN`
/// - `PODOS_ACTOR_NAME`, `PODOS_PASSCODE`
/// - `PODOS_RECONNECT_ENABLED`, `PODOS_RECONNECT_MAX_RETRIES`,
///   `PODOS_RECONNECT_INITIAL_BACKOFF`, `PODOS_RECONNECT_MAX_BACKOFF`,
///   `PODOS_RECONNECT_BACKOFF_MULTIPLIER`
/// - `PODOS_CONCURRENT_MODE`
/// - `PODOS_DIAL_TIMEOUT`, `PODOS_SEND_TIMEOUT`, `PODOS_RECEIVE_TIMEOUT`
/// - `PODOS_LOG_LEVEL`
///
/// Unset variables are left at their default value. Numeric durations are in seconds.
pub fn config_from_env() -> Config {
    let mut cfg = Config::default();

    cfg.network = "tcp".to_string();

    if let Ok(v) = env::var("PODOS_GATEWAY_HOST") {
        cfg.host = v;
    }
    if let Ok(v) = env::var("PODOS_GATEWAY_PORT") {
        cfg.port = v;
    }
    if let Ok(v) = env::var("PODOS_GATEWAY_FQN") {
        cfg.gateway_actor_name = v;
    }
    if let Ok(v) = env::var("PODOS_ACTOR_NAME") {
        cfg.client_name = v;
    }
    if let Ok(v) = env::var("PODOS_PASSCODE") {
        cfg.passcode = v;
    }

    if let Ok(v) = env::var("PODOS_CONCURRENT_MODE") {
        cfg.enable_concurrent_mode = parse_bool(&v);
    }

    if let Ok(v) = env::var("PODOS_DIAL_TIMEOUT") {
        if let Some(secs) = parse_u64(&v) {
            if secs > 0 {
                cfg.dial_timeout = Duration::from_secs(secs);
            }
        }
    }
    if let Ok(v) = env::var("PODOS_SEND_TIMEOUT") {
        if let Some(secs) = parse_u64(&v) {
            if secs > 0 {
                cfg.send_timeout = Duration::from_secs(secs);
            }
        }
    }
    if let Ok(v) = env::var("PODOS_RECEIVE_TIMEOUT") {
        if let Some(secs) = parse_u64(&v) {
            if secs > 0 {
                cfg.receive_timeout = Duration::from_secs(secs);
            }
        }
    }

    if let Ok(v) = env::var("PODOS_LOG_LEVEL") {
        if let Some(level) = parse_u64(&v) {
            cfg.log_level = level as u8;
        }
    }

    // Reconnection settings
    if let Ok(v) = env::var("PODOS_RECONNECT_ENABLED") {
        cfg.reconnect_config.enabled = Some(parse_bool(&v));
    }
    if let Ok(v) = env::var("PODOS_RECONNECT_MAX_RETRIES") {
        if let Some(n) = parse_u64(&v) {
            cfg.reconnect_config.max_retries = n as usize;
        }
    }
    if let Ok(v) = env::var("PODOS_RECONNECT_INITIAL_BACKOFF") {
        if let Some(secs) = parse_u64(&v) {
            if secs > 0 {
                cfg.reconnect_config.initial_backoff = Duration::from_secs(secs);
            }
        }
    }
    if let Ok(v) = env::var("PODOS_RECONNECT_BACKOFF_MULTIPLIER") {
        if let Some(f) = parse_f64(&v) {
            if f > 0.0 {
                cfg.reconnect_config.backoff_multiplier = f;
            }
        }
    }
    if let Ok(v) = env::var("PODOS_RECONNECT_MAX_BACKOFF") {
        if let Some(secs) = parse_u64(&v) {
            if secs > 0 {
                cfg.reconnect_config.max_backoff = Duration::from_secs(secs);
            }
        }
    }

    cfg
}

/// Populates a [`Config`] from INI key-value pairs as parsed by Pod-OS actor binaries
/// (flat key=value format, no section headers).
///
/// Recognized keys:
///
/// - `host`, `port`, `agent` (gateway_actor_name), `client` (client_name)
/// - `stream_messages`, `concurrent_mode`
/// - `reconnect_enabled`, `reconnect_max_retries`, `reconnect_initial_backoff`,
///   `reconnect_backoff_multiplier`, `reconnect_max_backoff`
/// - `dial_timeout`, `send_timeout`, `receive_timeout`
/// - `retry_count`, `retry_backoff`, `retry_backoff_multiplier`
/// - `passcode`, `log_level`
///
/// Unrecognized keys are silently ignored. Numeric durations are in seconds.
pub fn config_from_ini(kvs: &HashMap<String, String>) -> Config {
    let mut cfg = Config::default();
    cfg.network = "tcp".to_string();

    for (key, value) in kvs {
        match key.trim().to_lowercase().as_str() {
            "host" => cfg.host = value.clone(),
            "port" => cfg.port = value.clone(),
            "agent" => cfg.gateway_actor_name = value.clone(),
            "client" => cfg.client_name = value.clone(),
            "passcode" => cfg.passcode = value.clone(),
            "stream_messages" => {
                cfg.enable_streaming = Some(parse_bool_ini(value));
            }
            "concurrent_mode" => {
                cfg.enable_concurrent_mode = parse_bool_ini(value);
            }
            "dial_timeout" => {
                if let Some(secs) = parse_u64(value) {
                    if secs > 0 {
                        cfg.dial_timeout = Duration::from_secs(secs);
                    }
                }
            }
            "send_timeout" => {
                if let Some(secs) = parse_u64(value) {
                    if secs > 0 {
                        cfg.send_timeout = Duration::from_secs(secs);
                    }
                }
            }
            "receive_timeout" => {
                if let Some(secs) = parse_u64(value) {
                    if secs > 0 {
                        cfg.receive_timeout = Duration::from_secs(secs);
                    }
                }
            }
            "log_level" => {
                if let Some(level) = parse_u64(value) {
                    cfg.log_level = level as u8;
                }
            }

            // Reconnection settings
            "reconnect_enabled" => {
                cfg.reconnect_config.enabled = Some(parse_bool_ini(value));
            }
            "reconnect_max_retries" => {
                if let Some(n) = parse_u64(value) {
                    cfg.reconnect_config.max_retries = n as usize;
                }
            }
            "reconnect_initial_backoff" => {
                if let Some(secs) = parse_u64(value) {
                    if secs > 0 {
                        cfg.reconnect_config.initial_backoff = Duration::from_secs(secs);
                    }
                }
            }
            "reconnect_backoff_multiplier" => {
                if let Some(f) = parse_f64(value) {
                    if f > 0.0 {
                        cfg.reconnect_config.backoff_multiplier = f;
                    }
                }
            }
            "reconnect_max_backoff" => {
                if let Some(secs) = parse_u64(value) {
                    if secs > 0 {
                        cfg.reconnect_config.max_backoff = Duration::from_secs(secs);
                    }
                }
            }

            // Retry settings (initial dial)
            "retry_count" => {
                if let Some(n) = parse_u64(value) {
                    cfg.retry_config.retries = n as usize;
                }
            }
            "retry_backoff" => {
                if let Some(secs) = parse_u64(value) {
                    if secs > 0 {
                        cfg.retry_config.backoff = Duration::from_secs(secs);
                    }
                }
            }
            "retry_backoff_multiplier" => {
                if let Some(f) = parse_f64(value) {
                    if f > 0.0 {
                        cfg.retry_config.backoff_multiplier = f;
                    }
                }
            }

            _ => {} // Unrecognized keys are silently ignored
        }
    }

    cfg
}

fn parse_bool(v: &str) -> bool {
    let lower = v.trim().to_lowercase();
    matches!(lower.as_str(), "true" | "1" | "yes" | "y")
}

fn parse_bool_ini(v: &str) -> bool {
    let upper = v.trim().to_uppercase();
    matches!(upper.as_str(), "Y" | "YES" | "TRUE" | "1")
}

fn parse_u64(v: &str) -> Option<u64> {
    v.trim().parse::<u64>().ok()
}

fn parse_f64(v: &str) -> Option<f64> {
    v.trim().parse::<f64>().ok()
}
