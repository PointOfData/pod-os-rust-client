//! Embedded knowledge documents for AI agents and GenAI prompts.
//!
//! These documents are compiled into the binary via `include_str!`.

use std::fmt;

pub static COMMUNICATION_PROMPTS: &str = include_str!("docs/communication.md");

pub static MESSAGE_HANDLING_PROMPTS: &str = include_str!("docs/message_handling.md");

pub static NEURAL_MEMORY_EVENT_PROMPTS: &str = include_str!("docs/neural_memory.md");

pub static NEURAL_MEMORY_RETRIEVAL_PROMPTS: &str = include_str!("docs/neural_memory_retrieval.md");

pub static INTENT_FIELD_VALIDATION_PLAN: &str =
    include_str!("docs/intent_field_validation.plan.md");

#[derive(Debug)]
pub struct DocumentNotFound {
    pub name: String,
    pub available: Vec<&'static str>,
}

impl fmt::Display for DocumentNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown document: '{}'. Available: {}",
            self.name,
            self.available.join(", ")
        )
    }
}

impl std::error::Error for DocumentNotFound {}

/// Retrieve a knowledge document by name.
///
/// Retrieve a knowledge document by name.
///
/// Names: `"communication"`, `"message-handling"`,
/// `"neural-memory"`, `"neural-memory-retrieval"`,
/// `"intent-field-validation-plan"`.
pub fn get_document(name: &str) -> Result<&'static str, DocumentNotFound> {
    match name {
        "communication" => Ok(COMMUNICATION_PROMPTS),
        "message-handling" => Ok(MESSAGE_HANDLING_PROMPTS),
        "neural-memory" => Ok(NEURAL_MEMORY_EVENT_PROMPTS),
        "neural-memory-retrieval" => Ok(NEURAL_MEMORY_RETRIEVAL_PROMPTS),
        "intent-field-validation-plan" => Ok(INTENT_FIELD_VALIDATION_PLAN),
        _ => Err(DocumentNotFound {
            name: name.to_string(),
            available: list_documents(),
        }),
    }
}

/// List all available document names.
pub fn list_documents() -> Vec<&'static str> {
    vec![
        "communication",
        "message-handling",
        "neural-memory",
        "neural-memory-retrieval",
        "intent-field-validation-plan",
    ]
}
