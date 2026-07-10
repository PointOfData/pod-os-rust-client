use thiserror::Error;

/// Numeric error codes for gateway-level errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ErrCode {
    Unknown = 0,
    ClientReceiveFailed = 1,
    ClientSendFailed = 2,
    ClientNotConnected = 3,
    ClientConnectionFailed = 4,
    ClientDialFailed = 5,
    ClientCloseFailed = 6,
    ClientReconnectFailed = 7,
    ResolveFailed = 8,
    PoolExhausted = 9,
    PoolConnectionFailed = 10,
    PoolInitializationFailed = 11,
    RetryFailed = 12,
    RetriesExhausted = 13,
    NilPointer = 14,
    ValidationFailed = 15,
    AuthenticationFailed = 16,
    NotAuthenticated = 17,
    GatewayError = 18,
    GatewayTimeout = 19,
    GatewayDisconnected = 20,
    InvalidMessage = 21,
    InvalidAddress = 22,
    InvalidConfig = 23,
    InvalidResponse = 24,
    InvalidIntent = 25,
    InvalidPayload = 26,
    InvalidHeader = 27,
    InvalidLength = 28,
    InvalidNetwork = 29,
    SerializationFailed = 30,
    DeserializationFailed = 31,
    CompressionFailed = 32,
    DecompressionFailed = 33,
    EncryptionFailed = 34,
    DecryptionFailed = 35,
    SignatureFailed = 36,
    VerificationFailed = 37,
    NotFound = 38,
    AlreadyExists = 39,
    PermissionDenied = 40,
    QuotaExceeded = 41,
    RateLimitExceeded = 42,
    Unavailable = 43,
    Unimplemented = 44,
    InternalError = 45,
    DataCorruption = 46,
    StorageFailed = 47,
    NetworkFailed = 48,
    NoLoadBalancerRules = 49,
    /// Fatal, unrecoverable-on-this-socket condition (hard I/O error, mid-frame
    /// read timeout, or framing desync). The transport is marked disconnected
    /// and the caller should reconnect/retry.
    ConnectionLost = 50,
    /// Benign idle read timeout: no frame bytes were pending, so the connection
    /// is still considered healthy.
    ReceiveIdleTimeout = 51,
}

impl std::fmt::Display for ErrCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}({})", self, *self as u32)
    }
}

/// Primary error type for all gateway operations, mirroring Go's `GatewayDError`.
#[derive(Debug, Error)]
#[error("GatewayDError[{code}]: {message}")]
pub struct GatewayDError {
    pub code: ErrCode,
    pub message: String,
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl GatewayDError {
    pub fn new(code: ErrCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            source: None,
        }
    }

    pub fn wrap(
        code: ErrCode,
        message: impl Into<String>,
        err: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            source: Some(Box::new(err)),
        }
    }

    pub fn with_source(mut self, err: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(err));
        self
    }

    /// Check if the underlying source is a timeout I/O error.
    pub fn is_timeout(&self) -> bool {
        if let Some(source) = &self.source {
            if let Some(io_err) = source.downcast_ref::<std::io::Error>() {
                return io_err.kind() == std::io::ErrorKind::TimedOut;
            }
        }
        false
    }

    /// Whether this is a fatal connection-lost error (the socket is dead and
    /// must be reconnected). True for the explicit `ConnectionLost`/
    /// `GatewayDisconnected` codes or any wrapped connection-class I/O error.
    pub fn is_connection_lost(&self) -> bool {
        matches!(self.code, ErrCode::ConnectionLost | ErrCode::GatewayDisconnected)
            || self.is_io_connection_error()
    }

    /// Whether this is a benign idle receive timeout (connection still healthy).
    pub fn is_idle_timeout(&self) -> bool {
        self.code == ErrCode::ReceiveIdleTimeout
    }

    /// Check if the underlying source is a connection-class I/O error.
    pub fn is_io_connection_error(&self) -> bool {
        if let Some(source) = &self.source {
            if let Some(io_err) = source.downcast_ref::<std::io::Error>() {
                return matches!(
                    io_err.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof
                );
            }
        }
        false
    }
}
