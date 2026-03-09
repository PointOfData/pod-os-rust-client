//! Embedded knowledge documents for AI agents and GenAI prompts.
//!
//! These documents are compiled into the binary via `include_str!`.

pub static COMMUNICATION_PROMPTS: &str = include_str!("docs/communication.md");

pub static MESSAGE_HANDLING_PROMPTS: &str = include_str!("docs/message_handling.md");

pub static NEURAL_MEMORY_EVENT_PROMPTS: &str = include_str!("docs/neural_memory.md");

pub static NEURAL_MEMORY_RETRIEVAL_PROMPTS: &str = include_str!("docs/neural_memory_retrieval.md");

/// Retrieve a knowledge document by name.
///
/// Names: `"communication"`, `"message-handling"`,
/// `"neural-memory"`, `"neural-memory-retrieval"`.
pub fn get_document(name: &str) -> Result<&'static str, String> {
    match name {
        "communication" => Ok(COMMUNICATION_PROMPTS),
        "message-handling" => Ok(MESSAGE_HANDLING_PROMPTS),
        "neural-memory" => Ok(NEURAL_MEMORY_EVENT_PROMPTS),
        "neural-memory-retrieval" => Ok(NEURAL_MEMORY_RETRIEVAL_PROMPTS),
        _ => Err(format!(
            "unknown document: '{}'. Available: {}",
            name,
            list_documents().join(", ")
        )),
    }
}

/// List all available document names.
pub fn list_documents() -> Vec<&'static str> {
    vec![
        "communication",
        "message-handling",
        "neural-memory",
        "neural-memory-retrieval",
    ]
}
