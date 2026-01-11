//! Memory tool handlers for Kaioken's persistent memory system.
//!
//! Provides tools for the model to recall and save memories:
//! - `memory_recall`: Search and retrieve relevant memories
//! - `memory_save`: Explicitly save new memories (lessons, decisions, etc.)

use std::collections::BTreeMap;
use std::sync::LazyLock;

use async_trait::async_trait;
use serde::Deserialize;

use crate::client_common::tools::ResponsesApiTool;
use crate::client_common::tools::ToolSpec;
use crate::function_tool::FunctionCallError;
use crate::memory::types::MemoryType;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use crate::tools::spec::JsonSchema;

// ============================================================================
// memory_recall tool
// ============================================================================

pub static MEMORY_RECALL_TOOL: LazyLock<ToolSpec> = LazyLock::new(|| {
    let mut properties = BTreeMap::new();
    properties.insert(
        "query".to_string(),
        JsonSchema::String {
            description: Some("Search query to find relevant memories".to_string()),
        },
    );
    properties.insert(
        "memory_type".to_string(),
        JsonSchema::String {
            description: Some(
                "Optional filter by memory type: lesson, decision, pattern, fact, preference, location"
                    .to_string(),
            ),
        },
    );
    properties.insert(
        "limit".to_string(),
        JsonSchema::Number {
            description: Some("Maximum number of memories to return (default: 5)".to_string()),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: "memory_recall".to_string(),
        description: r#"Search and retrieve relevant memories from the project's persistent memory.
Use this to recall:
- Lessons learned from past mistakes
- Decisions made and their rationale
- Code patterns and conventions used in this project
- Important facts about the codebase
- User preferences and coding style
- Locations of key files and components

Call this when you need context about the project that you may have learned before."#
            .to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["query".to_string()]),
            additional_properties: Some(false.into()),
        },
    })
});

#[derive(Debug, Deserialize)]
struct MemoryRecallArgs {
    query: String,
    memory_type: Option<String>,
    limit: Option<usize>,
}

pub struct MemoryRecallHandler;

#[async_trait]
impl ToolHandler for MemoryRecallHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            session, payload, ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "memory_recall handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: MemoryRecallArgs = serde_json::from_str(&arguments).map_err(|e| {
            FunctionCallError::RespondToModel(format!("failed to parse arguments: {e}"))
        })?;

        // Get memory manager from session
        let memory_manager = match session.memory_manager() {
            Some(mm) => mm,
            None => {
                return Ok(ToolOutput::Function {
                    content: "Memory system is not enabled for this project.".to_string(),
                    content_items: None,
                    success: Some(true),
                });
            }
        };

        // Parse memory type filter if provided
        let type_filter = args.memory_type.as_ref().and_then(|t| parse_memory_type(t));

        // Search memories
        let limit = args.limit.unwrap_or(5).min(20);
        let memories = memory_manager.search(&args.query, type_filter, limit).await;

        if memories.is_empty() {
            return Ok(ToolOutput::Function {
                content: format!("No memories found matching query: \"{}\"", args.query),
                content_items: None,
                success: Some(true),
            });
        }

        // Format memories for the model
        let mut output = format!("Found {} relevant memories:\n\n", memories.len());
        for (i, scored) in memories.iter().enumerate() {
            let mem = &scored.memory;
            let type_label = format!("{:?}", mem.memory_type).to_uppercase();
            output.push_str(&format!("{}. [{}] {}\n", i + 1, type_label, mem.content));
            if let Some(ref ctx) = mem.context {
                output.push_str(&format!("   Context: {}\n", ctx));
            }
            if let Some(ref path) = mem.source_file {
                output.push_str(&format!("   Source: {}\n", path.display()));
            }
            output.push('\n');
        }

        Ok(ToolOutput::Function {
            content: output,
            content_items: None,
            success: Some(true),
        })
    }
}

// ============================================================================
// memory_save tool
// ============================================================================

pub static MEMORY_SAVE_TOOL: LazyLock<ToolSpec> = LazyLock::new(|| {
    let mut properties = BTreeMap::new();
    properties.insert(
        "memory_type".to_string(),
        JsonSchema::String {
            description: Some(
                "Type of memory: lesson (learned from mistake), decision (choice made), pattern (code convention), fact (project info), preference (user style), location (where things are)"
                    .to_string(),
            ),
        },
    );
    properties.insert(
        "content".to_string(),
        JsonSchema::String {
            description: Some("The memory content to save".to_string()),
        },
    );
    properties.insert(
        "context".to_string(),
        JsonSchema::String {
            description: Some("Optional additional context about this memory".to_string()),
        },
    );
    properties.insert(
        "source_file".to_string(),
        JsonSchema::String {
            description: Some("Optional file path this memory relates to".to_string()),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: "memory_save".to_string(),
        description: r#"Save a new memory to the project's persistent memory system.
Use this to remember:
- LESSON: Something learned from a mistake (e.g., "Must run npm install before tests")
- DECISION: A choice made and why (e.g., "Chose Axum over Actix for simpler async")
- PATTERN: A code pattern or convention (e.g., "Tests go in __tests__ folders")
- FACT: Important project information (e.g., "Project uses React 18")
- PREFERENCE: User's coding style preference (e.g., "Prefers explicit error handling")
- LOCATION: Where important code lives (e.g., "Auth logic in src/auth/")

Lessons and decisions are never forgotten. Other memories decay over time if unused."#
            .to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["memory_type".to_string(), "content".to_string()]),
            additional_properties: Some(false.into()),
        },
    })
});

#[derive(Debug, Deserialize)]
struct MemorySaveArgs {
    memory_type: String,
    content: String,
    context: Option<String>,
    source_file: Option<String>,
}

pub struct MemorySaveHandler;

#[async_trait]
impl ToolHandler for MemorySaveHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            session, payload, ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "memory_save handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: MemorySaveArgs = serde_json::from_str(&arguments).map_err(|e| {
            FunctionCallError::RespondToModel(format!("failed to parse arguments: {e}"))
        })?;

        // Get memory manager from session
        let memory_manager = match session.memory_manager() {
            Some(mm) => mm,
            None => {
                return Ok(ToolOutput::Function {
                    content: "Memory system is not enabled for this project.".to_string(),
                    content_items: None,
                    success: Some(true),
                });
            }
        };

        // Parse memory type
        let memory_type = match parse_memory_type(&args.memory_type) {
            Some(t) => t,
            None => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "Invalid memory type '{}'. Use: lesson, decision, pattern, fact, preference, or location",
                    args.memory_type
                )));
            }
        };

        // Save the memory
        let source_path = args.source_file.map(std::path::PathBuf::from);
        match memory_manager
            .save_explicit(
                memory_type,
                &args.content,
                args.context.as_deref(),
                source_path.as_deref(),
            )
            .await
        {
            Ok(_) => Ok(ToolOutput::Function {
                content: format!(
                    "Memory saved: [{:?}] {}",
                    memory_type,
                    truncate_for_display(&args.content, 100)
                ),
                content_items: None,
                success: Some(true),
            }),
            Err(e) => Ok(ToolOutput::Function {
                content: format!("Failed to save memory: {}", e),
                content_items: None,
                success: Some(false),
            }),
        }
    }
}

// ============================================================================
// Helper functions
// ============================================================================

fn parse_memory_type(s: &str) -> Option<MemoryType> {
    match s.to_lowercase().as_str() {
        "lesson" => Some(MemoryType::Lesson),
        "decision" => Some(MemoryType::Decision),
        "pattern" => Some(MemoryType::Pattern),
        "fact" => Some(MemoryType::Fact),
        "preference" => Some(MemoryType::Preference),
        "location" => Some(MemoryType::Location),
        _ => None,
    }
}

fn truncate_for_display(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}
