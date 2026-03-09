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
}
