//! LLM-powered memory extraction.
//!
//! This module uses the LLM to intelligently extract memories from
//! conversation turns, capturing learnings, patterns, and insights
//! that would be missed by rule-based extraction.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use tokio::time::timeout;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::AuthManager;
use crate::ModelProviderInfo;
use crate::client::ModelClient;
use crate::client_common::Prompt;
use crate::client_common::ResponseEvent;
use crate::config::Config;
use codex_otel::otel_event_manager::OtelEventManager;
use codex_protocol::ConversationId;
use codex_protocol::config_types::ReasoningEffort;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionSource;

use super::store::MemoryStore;
use super::types::Memory;
use super::types::MemoryType;

/// Model to use for memory extraction.
pub const MEMORY_EXTRACTION_MODEL: &str = "gpt-5.1-codex-mini";

/// Reasoning effort for memory extraction.
const MEMORY_EXTRACTION_REASONING: ReasoningEffort = ReasoningEffort::Medium;

/// Timeout for memory extraction calls.
const MEMORY_EXTRACTION_TIMEOUT: Duration = Duration::from_secs(30);

/// LLM-based memory extractor.
pub struct LlmMemoryExtractor {
    store: Arc<MemoryStore>,
}

/// Extracted memory from LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedMemory {
    #[serde(rename = "type")]
    pub memory_type: String,
    pub content: String,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub importance: Option<f64>,
}

/// Response from memory extraction LLM call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResponse {
    #[serde(default)]
    pub memories: Vec<ExtractedMemory>,
}

impl LlmMemoryExtractor {
    /// Create a new LLM memory extractor.
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }

    /// Extract memories from a completed turn.
    ///
    /// This should be called after a turn completes to analyze:
    /// - What was learned from the conversation
    /// - Patterns observed
    /// - Decisions made
    /// - Lessons learned from errors/fixes
    pub async fn extract_from_turn(
        &self,
        config: Arc<Config>,
        provider: ModelProviderInfo,
        auth_manager: Arc<AuthManager>,
        otel: &OtelEventManager,
        conversation_id: ConversationId,
        session_source: SessionSource,
        turn_summary: &str,
    ) -> Vec<Memory> {
        let extracted = match self
            .call_extraction_model(
                config,
                provider,
                auth_manager,
                otel,
                conversation_id,
                session_source,
                turn_summary,
            )
            .await
        {
            Ok(response) => response.memories,
            Err(e) => {
                warn!("Memory extraction failed: {}", e);
                return Vec::new();
            }
        };

        let mut memories = Vec::new();
        for em in extracted {
            // Convert string type to MemoryType
            let memory_type = match em.memory_type.to_lowercase().as_str() {
                "lesson" => MemoryType::Lesson,
                "decision" => MemoryType::Decision,
                "pattern" => MemoryType::Pattern,
                "fact" => MemoryType::Fact,
                "preference" => MemoryType::Preference,
                "location" => MemoryType::Location,
                _ => MemoryType::Fact,
            };

            // Check for semantic duplicates before adding
            let is_duplicate = self
                .store
                .exists_semantically_similar(&em.content, memory_type, 0.85)
                .await
                .unwrap_or(false);

            if !is_duplicate {
                let mut memory = Memory::new(memory_type, em.content.clone());
                if let Some(ctx) = em.context {
                    memory = memory.with_context(&ctx);
                }
                if let Some(imp) = em.importance {
                    memory = memory.with_importance(imp);
                }

                // Store the memory
                if let Err(e) = self.store.insert(&memory).await {
                    warn!("Failed to store extracted memory: {}", e);
                } else {
                    info!(
                        "Extracted and stored memory: {} ({})",
                        truncate(&em.content, 50),
                        memory_type.as_str()
                    );
                    memories.push(memory);
                }
            } else {
                debug!("Skipping duplicate memory: {}", truncate(&em.content, 50));
            }
        }

        memories
    }

    /// Call the LLM to extract memories from turn content.
    async fn call_extraction_model(
        &self,
        config: Arc<Config>,
        provider: ModelProviderInfo,
        auth_manager: Arc<AuthManager>,
        otel: &OtelEventManager,
        conversation_id: ConversationId,
        session_source: SessionSource,
        turn_summary: &str,
    ) -> anyhow::Result<ExtractionResponse> {
        let system_prompt = EXTRACTION_SYSTEM_PROMPT.to_string();
        let user_prompt = format!(
            "Extract memories from this conversation turn:\n\n{}",
            turn_summary
        );

        let prompt = Prompt {
            input: vec![ResponseItem::Message {
                id: None,
                role: "user".to_string(),
                content: vec![ContentItem::InputText { text: user_prompt }],
            }],
            tools: Vec::new(),
            parallel_tool_calls: false,
            base_instructions_override: Some(system_prompt),
            output_schema: Some(extraction_schema()),
        };

        // Create a modified config with our extraction model
        let mut extraction_config = (*config).clone();
        extraction_config.model = MEMORY_EXTRACTION_MODEL.to_string();

        let child_otel = otel.with_model(
            MEMORY_EXTRACTION_MODEL,
            &extraction_config.model_family.slug,
        );

        let client = ModelClient::new(
            Arc::new(extraction_config),
            Some(auth_manager),
            child_otel,
            provider,
            Some(MEMORY_EXTRACTION_REASONING),
            config.model_reasoning_summary,
            conversation_id,
            session_source,
        );

        let result = timeout(MEMORY_EXTRACTION_TIMEOUT, async move {
            let mut stream = client.stream(&prompt).await?;
            let mut last_json: Option<String> = None;

            while let Some(event) = stream.next().await {
                match event {
                    Ok(ResponseEvent::OutputItemDone(item)) => {
                        if let Some(text) = response_item_text(&item) {
                            last_json = Some(text);
                        }
                    }
                    Ok(ResponseEvent::Completed { .. }) => break,
                    Ok(_) => continue,
                    Err(err) => return Err(err),
                }
            }

            Ok(last_json)
        })
        .await;

        match result {
            Ok(Ok(Some(json_str))) => {
                let response: ExtractionResponse = serde_json::from_str(json_str.trim())?;
                Ok(response)
            }
            Ok(Ok(None)) => {
                anyhow::bail!("No response from memory extraction model")
            }
            Ok(Err(e)) => {
                anyhow::bail!("Memory extraction model error: {}", e)
            }
            Err(_) => {
                anyhow::bail!("Memory extraction timed out")
            }
        }
    }

    /// Build a summary of a turn for extraction.
    pub fn build_turn_summary(
        user_message: &str,
        agent_response: &str,
        tool_calls: &[(String, String, bool)], // (tool_name, summary, success)
        files_touched: &[String],
    ) -> String {
        let mut summary = String::new();

        summary.push_str("## User Request\n");
        summary.push_str(user_message);
        summary.push_str("\n\n");

        if !tool_calls.is_empty() {
            summary.push_str("## Tool Calls\n");
            for (name, call_summary, success) in tool_calls {
                let status = if *success { "SUCCESS" } else { "FAILED" };
                summary.push_str(&format!("- {} ({}): {}\n", name, status, call_summary));
            }
            summary.push('\n');
        }

        if !files_touched.is_empty() {
            summary.push_str("## Files Touched\n");
            for file in files_touched {
                summary.push_str(&format!("- {}\n", file));
            }
            summary.push('\n');
        }

        summary.push_str("## Agent Response\n");
        summary.push_str(agent_response);

        summary
    }
}

/// System prompt for memory extraction.
const EXTRACTION_SYSTEM_PROMPT: &str = r#"You are a memory extraction assistant. Analyze the conversation turn and extract valuable memories that would help in future similar tasks.

Extract memories of these types:
- lesson: Things learned from mistakes or successes (e.g., "must mock Redis before running tests")
- decision: Important choices made (e.g., "chose Axum over Actix for simpler async")
- pattern: Recurring code patterns observed (e.g., "tests follow __tests__/Name.test.tsx convention")
- fact: Project knowledge (e.g., "uses React 18 with TypeScript")
- preference: User preferences (e.g., "prefers explicit error handling")
- location: Where things are (e.g., "auth code in src/auth/")

Guidelines:
- Only extract genuinely useful information
- Be concise but specific
- Focus on actionable knowledge
- Ignore trivial or temporary information
- Don't extract what's already obvious from the file structure

Return a JSON object with a "memories" array. Each memory has:
- type: One of the types above
- content: The memory content (1-2 sentences)
- context: Optional context about when this applies
- importance: 0.0-1.0 based on how valuable this is (lessons/decisions should be high)
"#;

/// JSON schema for extraction response.
fn extraction_schema() -> serde_json::Value {
    json!({
        "name": "memory_extraction",
        "strict": true,
        "schema": {
            "type": "object",
            "properties": {
                "memories": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["lesson", "decision", "pattern", "fact", "preference", "location"]
                            },
                            "content": {
                                "type": "string",
                                "description": "The memory content (1-2 sentences)"
                            },
                            "context": {
                                "type": ["string", "null"],
                                "description": "Optional context about when this applies"
                            },
                            "importance": {
                                "type": ["number", "null"],
                                "description": "Importance score from 0.0 to 1.0"
                            }
                        },
                        "required": ["type", "content"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["memories"],
            "additionalProperties": false
        }
    })
}

/// Extract text from a response item.
fn response_item_text(item: &ResponseItem) -> Option<String> {
    match item {
        ResponseItem::Message { content, .. } => {
            for c in content {
                if let ContentItem::OutputText { text } = c {
                    return Some(text.clone());
                }
            }
            None
        }
        _ => None,
    }
}

/// Truncate a string for logging.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len { s } else { &s[..max_len] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_turn_summary() {
        let summary = LlmMemoryExtractor::build_turn_summary(
            "How do I run the tests?",
            "You can run tests with npm test",
            &[("bash".to_string(), "npm test".to_string(), true)],
            &["package.json".to_string()],
        );

        assert!(summary.contains("User Request"));
        assert!(summary.contains("How do I run the tests?"));
        assert!(summary.contains("Tool Calls"));
        assert!(summary.contains("npm test"));
        assert!(summary.contains("SUCCESS"));
        assert!(summary.contains("Files Touched"));
        assert!(summary.contains("package.json"));
    }

    #[test]
    fn test_extraction_response_parsing() {
        let json = r#"{
            "memories": [
                {
                    "type": "lesson",
                    "content": "Must run npm install before npm test",
                    "importance": 0.9
                },
                {
                    "type": "fact",
                    "content": "Project uses Jest for testing"
                }
            ]
        }"#;

        let response: ExtractionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.memories.len(), 2);
        assert_eq!(response.memories[0].memory_type, "lesson");
        assert_eq!(response.memories[0].importance, Some(0.9));
        assert!(response.memories[1].importance.is_none());
    }
}
