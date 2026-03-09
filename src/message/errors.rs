use thiserror::Error;

/// Error codes for message-level encode / decode failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgErrCode {
    // Decode errors (1000+)
    DecodeMessageTooShort          = 1000,
    DecodeInvalidSizeParam         = 1001,
    DecodeInvalidHeader            = 1002,
    DecodeInvalidMessageType       = 1003,
    DecodeInvalidDataType          = 1004,
    DecodePayloadTooLarge          = 1005,
    DecodeHeaderTransformFailed    = 1006,
    // Encode errors (1007+)
    EncodeNilMessage               = 1007,
    EncodePayloadTooLarge          = 1008,
    EncodeInvalidData              = 1009,
    EncodeInvalidFromAddress       = 1010,
    EncodeInvalidGatewayName       = 1011,
    EncodeInvalidActorName         = 1012,
    EncodeInvalidDomainName        = 1013,
    EncodeInvalidToAddress         = 1014,
}

impl std::fmt::Display for MsgErrCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}({})", self, *self as u32)
    }
}

#[derive(Debug, Error)]
#[error("DecodeError[{code}] field={field:?}: {message}")]
pub struct DecodeError {
    pub code:    MsgErrCode,
    pub message: String,
    pub field:   Option<String>,
    #[source]
    pub source:  Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl DecodeError {
    pub fn new(code: MsgErrCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), field: None, source: None }
    }
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into()); self
    }
    pub fn wrap(code: MsgErrCode, message: impl Into<String>, err: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self { code, message: message.into(), field: None, source: Some(Box::new(err)) }
    }
}

#[derive(Debug, Error)]
#[error("EncodeError[{code}] field={field:?}: {message}")]
pub struct EncodeError {
    pub code:    MsgErrCode,
    pub message: String,
    pub field:   Option<String>,
    #[source]
    pub source:  Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl EncodeError {
    pub fn new(code: MsgErrCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), field: None, source: None }
    }
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into()); self
    }
}

pub fn is_decode_error(err: &(dyn std::error::Error + 'static)) -> bool {
    err.downcast_ref::<DecodeError>().is_some()
}

pub fn is_encode_error(err: &(dyn std::error::Error + 'static)) -> bool {
    err.downcast_ref::<EncodeError>().is_some()
}
