use std::sync::Arc;

/// Logging verbosity levels, matching the Go client's log levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Disabled = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
}

impl From<u8> for Level {
    fn from(v: u8) -> Self {
        match v {
            0 => Level::Disabled,
            1 => Level::Error,
            2 => Level::Warn,
            3 => Level::Info,
            _ => Level::Debug,
        }
    }
}

/// Logger interface — an `Arc<dyn Logger>` can be passed across tasks.
pub trait Logger: Send + Sync + 'static {
    fn enabled(&self, level: Level) -> bool;
    fn debug(&self, msg: &str, fields: &[(&str, &dyn std::fmt::Display)]);
    fn info(&self, msg: &str, fields: &[(&str, &dyn std::fmt::Display)]);
    fn warn(&self, msg: &str, fields: &[(&str, &dyn std::fmt::Display)]);
    fn error(&self, msg: &str, fields: &[(&str, &dyn std::fmt::Display)]);
}

/// Logger that discards every message.
#[derive(Clone, Default, Debug)]
pub struct NoOpLogger;

impl Logger for NoOpLogger {
    fn enabled(&self, _level: Level) -> bool {
        false
    }
    fn debug(&self, _msg: &str, _fields: &[(&str, &dyn std::fmt::Display)]) {}
    fn info(&self, _msg: &str, _fields: &[(&str, &dyn std::fmt::Display)]) {}
    fn warn(&self, _msg: &str, _fields: &[(&str, &dyn std::fmt::Display)]) {}
    fn error(&self, _msg: &str, _fields: &[(&str, &dyn std::fmt::Display)]) {}
}

/// Returns the supplied logger, or a `NoOpLogger` if `None`.
pub fn logger_or_noop(l: Option<Arc<dyn Logger>>) -> Arc<dyn Logger> {
    l.unwrap_or_else(|| Arc::new(NoOpLogger))
}

/// `tracing`-backed logger that honours a minimum level.
#[derive(Clone)]
pub struct TracingLogger {
    level: Level,
}

impl TracingLogger {
    pub fn build(level: Level) -> Arc<dyn Logger> {
        Arc::new(Self { level })
    }
}

impl Logger for TracingLogger {
    fn enabled(&self, level: Level) -> bool {
        level <= self.level && self.level != Level::Disabled
    }

    fn debug(&self, msg: &str, fields: &[(&str, &dyn std::fmt::Display)]) {
        if self.enabled(Level::Debug) {
            let kv: Vec<String> = fields.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            tracing::debug!("{} {}", msg, kv.join(" "));
        }
    }
    fn info(&self, msg: &str, fields: &[(&str, &dyn std::fmt::Display)]) {
        if self.enabled(Level::Info) {
            let kv: Vec<String> = fields.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            tracing::info!("{} {}", msg, kv.join(" "));
        }
    }
    fn warn(&self, msg: &str, fields: &[(&str, &dyn std::fmt::Display)]) {
        if self.enabled(Level::Warn) {
            let kv: Vec<String> = fields.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            tracing::warn!("{} {}", msg, kv.join(" "));
        }
    }
    fn error(&self, msg: &str, fields: &[(&str, &dyn std::fmt::Display)]) {
        if self.enabled(Level::Error) {
            let kv: Vec<String> = fields.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            tracing::error!("{} {}", msg, kv.join(" "));
        }
    }
}
