#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod types;
mod workspace;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use codex_backend_client::Client as BackendClient;
use codex_core::AuthManager;
use codex_core::CodexConversation;
use codex_core::ConversationManager;
use codex_core::NewConversation;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::Op;
use codex_core::protocol::SandboxPolicy;
use codex_protocol::config_types::ReasoningEffort;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::protocol::SessionSource;
use codex_protocol::user_input::UserInput;
use tauri::Emitter;
use tokio::sync::RwLock;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;
use types::ChatMessage;
use types::MessageRole;
use types::SendMessageResponse;

/// Per-session state (conversation + messages)
struct SessionState {
    /// The conversation for this session
    conversation: Arc<CodexConversation>,
    /// Channel to send ops to this conversation
    op_tx: UnboundedSender<Op>,
    /// Working directory for this session
    cwd: PathBuf,
    /// Message history for this session
    messages: Vec<ChatMessage>,
}

/// Application state
pub struct AppState {
    /// Conversation manager initialized from stored auth/config (shared across sessions)
    conversation_manager: RwLock<Option<Arc<ConversationManager>>>,
    /// Active sessions keyed by session ID
    sessions: RwLock<HashMap<String, SessionState>>,
    /// Default working directory (for backward compat)
    default_cwd: PathBuf,
    /// Current model (shared across sessions)
    model: RwLock<String>,
    /// Current reasoning effort (shared across sessions)
    reasoning_effort: RwLock<ReasoningEffort>,
    /// Current approval policy (shared across sessions)
    approval_policy: RwLock<AskForApproval>,
    /// Current sandbox policy (shared across sessions)
    sandbox_policy: RwLock<SandboxPolicy>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn generate_id() -> String {
    format!("msg-{}", now_ms())
}

/// A restored message from rollout file
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RestoredMessage {
    role: String,
    content: String,
}

/// Response from init_session containing conversation info for persistence
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InitSessionResponse {
    /// Whether the session was successfully initialized
    success: bool,
    /// The backend conversation ID (UUID) - store this for resuming
    conversation_id: Option<String>,
    /// Path to the rollout file - store this for resuming
    rollout_path: Option<String>,
    /// Restored messages from rollout (only when resuming)
    messages: Vec<RestoredMessage>,
}

/// Parse a rollout JSONL file and extract user/assistant messages
async fn parse_rollout_messages(rollout_path: &str) -> Vec<RestoredMessage> {
    let mut messages = Vec::new();

    let content = match tokio::fs::read_to_string(rollout_path).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[kaioken-gui] Failed to read rollout file: {}", e);
            return messages;
        }
    };

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let json: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Look for response_item with type=message
        if json.get("type").and_then(|t| t.as_str()) == Some("response_item") {
            if let Some(payload) = json.get("payload") {
                if payload.get("type").and_then(|t| t.as_str()) == Some("message") {
                    let role = payload.get("role").and_then(|r| r.as_str()).unwrap_or("");

                    // Extract text content from content array
                    if let Some(content_arr) = payload.get("content").and_then(|c| c.as_array()) {
                        for item in content_arr {
                            // Check for input_text (user) or output_text (assistant)
                            let text = item.get("text").and_then(|t| t.as_str());
                            let item_type = item.get("type").and_then(|t| t.as_str());

                            if let Some(text) = text {
                                // Skip system messages (instructions, environment context, etc.)
                                if role == "user" {
                                    // Skip if it looks like system injection
                                    if text.starts_with("# AGENTS.md")
                                        || text.starts_with("<environment_context>")
                                        || text.starts_with("<INSTRUCTIONS>")
                                    {
                                        continue;
                                    }
                                }

                                // Only include input_text for user, output_text for assistant
                                let is_user_input =
                                    role == "user" && item_type == Some("input_text");
                                let is_assistant_output =
                                    role == "assistant" && item_type == Some("output_text");

                                if is_user_input || is_assistant_output {
                                    messages.push(RestoredMessage {
                                        role: role.to_string(),
                                        content: text.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    eprintln!(
        "[kaioken-gui] Parsed {} messages from rollout",
        messages.len()
    );
    messages
}

/// Initialize a session with a specific ID and working directory
/// If rollout_path is provided, resumes from that session instead of creating new
#[tauri::command]
async fn init_session(
    session_id: String,
    cwd: String,
    rollout_path: Option<String>,
    state: tauri::State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<InitSessionResponse, String> {
    eprintln!(
        "[kaioken-gui] init_session called: session_id={}, cwd={}, rollout_path={:?}",
        session_id, cwd, rollout_path
    );

    // Check if session already exists
    {
        let sessions = state.sessions.read().await;
        if sessions.contains_key(&session_id) {
            eprintln!("[kaioken-gui] Session {} already exists", session_id);
            return Ok(InitSessionResponse {
                success: true,
                conversation_id: None,
                rollout_path: None,
                messages: Vec::new(),
            });
        }
    }

    // Parse messages from rollout if resuming
    let restored_messages = if let Some(ref rollout) = rollout_path {
        parse_rollout_messages(rollout).await
    } else {
        Vec::new()
    };

    let model = state.model.read().await.clone();
    let cwd_path = PathBuf::from(&cwd);
    eprintln!("[kaioken-gui] Model: {model}, CWD: {cwd_path:?}");

    // Load config from ~/.codex/config.toml with overrides
    let overrides = ConfigOverrides {
        model: Some(model.clone()),
        cwd: Some(cwd_path.clone()),
        ..Default::default()
    };

    eprintln!("[kaioken-gui] Loading config...");
    let config = Config::load_with_cli_overrides(vec![], overrides.clone())
        .await
        .map_err(|e| {
            eprintln!("[kaioken-gui] Config load error: {e}");
            format!("Failed to load config: {e}")
        })?;
    eprintln!("[kaioken-gui] Config loaded successfully");

    let conversation_manager = {
        let mut manager_guard = state.conversation_manager.write().await;
        if let Some(manager) = manager_guard.clone() {
            manager
        } else {
            let auth_manager = AuthManager::shared(
                config.codex_home.clone(),
                false,
                config.cli_auth_credentials_store_mode,
            );
            if auth_manager.auth().is_none() {
                return Err(
                    "No Codex credentials found. Run `codex-kaioken login` to authenticate."
                        .to_string(),
                );
            }
            let manager = Arc::new(ConversationManager::new(auth_manager, SessionSource::Exec));
            *manager_guard = Some(manager.clone());
            manager
        }
    };

    // Either resume existing conversation or create new one
    let NewConversation {
        conversation_id,
        conversation,
        session_configured,
    } = if let Some(ref rollout) = rollout_path {
        // Resume from existing rollout file
        eprintln!(
            "[kaioken-gui] Resuming conversation from rollout: {}",
            rollout
        );
        let auth_manager = AuthManager::shared(
            config.codex_home.clone(),
            false,
            config.cli_auth_credentials_store_mode,
        );
        // Need to reload config for resume since we consumed it
        let resume_config = Config::load_with_cli_overrides(vec![], overrides)
            .await
            .map_err(|e| format!("Failed to reload config: {e}"))?;
        conversation_manager
            .resume_conversation_from_rollout(resume_config, PathBuf::from(rollout), auth_manager)
            .await
            .map_err(|e| {
                eprintln!("[kaioken-gui] resume_conversation error: {e}");
                format!("Failed to resume conversation: {e}")
            })?
    } else {
        // Create new conversation
        eprintln!(
            "[kaioken-gui] Creating new conversation for session {}...",
            session_id
        );
        conversation_manager
            .new_conversation(config)
            .await
            .map_err(|e| {
                eprintln!("[kaioken-gui] new_conversation error: {e}");
                format!("Failed to create conversation: {e}")
            })?
    };

    let conv_id_str = conversation_id.to_string();
    let rollout_path_str = session_configured.rollout_path.display().to_string();
    eprintln!(
        "[kaioken-gui] Conversation ready: id={}, rollout={}",
        conv_id_str, rollout_path_str
    );

    // Create channel for ops
    let (op_tx, mut op_rx) = unbounded_channel::<Op>();

    // Store session state
    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(
            session_id.clone(),
            SessionState {
                conversation: Arc::clone(&conversation),
                op_tx,
                cwd: cwd_path,
                messages: Vec::new(),
            },
        );
    }

    // Emit session configured with session ID
    eprintln!("[kaioken-gui] Model: {}", session_configured.model);
    let _ = app.emit(
        "kaioken-session-event",
        serde_json::json!({
            "sessionId": session_id,
            "type": "sessionConfigured",
            "model": session_configured.model,
        }),
    );

    // Spawn op forwarding task
    let conv_for_ops = conversation.clone();
    tokio::spawn(async move {
        eprintln!("[kaioken-gui] Op task started for session");
        while let Some(op) = op_rx.recv().await {
            eprintln!("[kaioken-gui] Submitting op...");
            if let Err(e) = conv_for_ops.submit(op).await {
                eprintln!("[kaioken-gui] Submit error: {e}");
            } else {
                eprintln!("[kaioken-gui] Op submitted OK");
            }
        }
    });

    // Spawn event receiving task with session ID
    let app_clone = app.clone();
    let conv_for_events = conversation.clone();
    let session_id_for_events = session_id.clone();
    tokio::spawn(async move {
        eprintln!(
            "[kaioken-gui] Event task started for session {}",
            session_id_for_events
        );
        loop {
            match conv_for_events.next_event().await {
                Ok(event) => {
                    eprintln!(
                        "[kaioken-gui] Got event for session {}: {:?}",
                        session_id_for_events,
                        std::mem::discriminant(&event.msg)
                    );
                    emit_session_event(&app_clone, &session_id_for_events, event);
                }
                Err(e) => {
                    eprintln!(
                        "[kaioken-gui] Event error for session {}: {e}",
                        session_id_for_events
                    );
                    break;
                }
            }
        }
    });

    eprintln!("[kaioken-gui] Session {} init complete", session_id);
    Ok(InitSessionResponse {
        success: true,
        conversation_id: Some(conv_id_str),
        rollout_path: Some(rollout_path_str),
        messages: restored_messages,
    })
}

/// Initialize conversation with codex-core (backward compat - uses "default" session)
#[tauri::command]
async fn init_conversation(
    state: tauri::State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let cwd = state.default_cwd.to_string_lossy().to_string();
    let result = init_session("default".to_string(), cwd, None, state, app).await?;
    Ok(result.success)
}

/// Emit codex event to frontend with session ID
fn emit_session_event(app: &tauri::AppHandle, session_id: &str, event: Event) {
    eprintln!(
        "[kaioken-gui] emit_session_event[{}]: {:?}",
        session_id, &event.msg
    );

    match &event.msg {
        // === Agent Messages ===
        EventMsg::AgentMessage(msg) => {
            // Skip - we use streaming deltas instead to avoid duplicates
            eprintln!(
                "[kaioken-gui] AgentMessage (skipped, using deltas): {}",
                msg.message
            );
        }
        EventMsg::AgentMessageDelta(delta) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "contentDelta",
                    "delta": delta.delta,
                }),
            );
        }

        // === Reasoning Events ===
        EventMsg::AgentReasoningDelta(delta) => {
            eprintln!(
                "[kaioken-gui] EMITTING reasoningDelta: {} chars",
                delta.delta.len()
            );
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "reasoningDelta",
                    "delta": delta.delta,
                }),
            );
        }
        EventMsg::AgentReasoning(reasoning) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "reasoning",
                    "text": reasoning.text,
                }),
            );
        }
        EventMsg::AgentReasoningRawContentDelta(delta) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "reasoningRawDelta",
                    "delta": delta.delta,
                }),
            );
        }

        // === Task Lifecycle ===
        EventMsg::TaskStarted(task) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "taskStarted",
                    "contextWindow": task.model_context_window,
                }),
            );
        }
        EventMsg::TaskComplete(task) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "taskComplete",
                    "lastMessage": task.last_agent_message,
                }),
            );
        }
        EventMsg::TurnAborted(aborted) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "turnAborted",
                    "reason": format!("{:?}", aborted.reason),
                }),
            );
        }

        // === Token Usage ===
        EventMsg::TokenCount(tc) => {
            if let Some(info) = &tc.info {
                let _ = app.emit(
                    "kaioken-session-event",
                    serde_json::json!({
                        "sessionId": session_id,
                        "type": "tokenUsage",
                        "total": {
                            "input": info.total_token_usage.input_tokens,
                            "output": info.total_token_usage.output_tokens,
                            "cached": info.total_token_usage.cached_input_tokens,
                            "reasoning": info.total_token_usage.reasoning_output_tokens,
                            "total": info.total_token_usage.total_tokens,
                        },
                        "last": {
                            "input": info.last_token_usage.input_tokens,
                            "output": info.last_token_usage.output_tokens,
                            "total": info.last_token_usage.total_tokens,
                        },
                        "contextWindow": info.model_context_window,
                    }),
                );
            }
        }

        // === Shell Command Events ===
        EventMsg::ExecCommandBegin(cmd) => {
            eprintln!(
                "[kaioken-gui] EMITTING toolStart (shell): {:?}",
                cmd.command
            );
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "toolStart",
                    "id": cmd.call_id,
                    "toolType": "shell",
                    "name": "shell",
                    "status": "running",
                    "input": {"command": cmd.command, "cwd": cmd.cwd},
                    "output": null,
                    "error": null,
                    "startTime": now_ms(),
                    "endTime": null,
                }),
            );
        }
        EventMsg::ExecCommandOutputDelta(delta) => {
            let stream = match delta.stream {
                codex_core::protocol::ExecOutputStream::Stdout => "stdout",
                codex_core::protocol::ExecOutputStream::Stderr => "stderr",
            };
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "execOutputDelta",
                    "callId": delta.call_id,
                    "stream": stream,
                    "chunk": String::from_utf8_lossy(&delta.chunk),
                }),
            );
        }
        EventMsg::ExecCommandEnd(cmd) => {
            let status = if cmd.exit_code == 0 {
                "success"
            } else {
                "error"
            };
            let error_val: Option<&str> = if cmd.stderr.is_empty() {
                None
            } else {
                Some(&cmd.stderr)
            };
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "toolEnd",
                    "id": cmd.call_id,
                    "toolType": "shell",
                    "name": "shell",
                    "status": status,
                    "input": null,
                    "output": cmd.aggregated_output,
                    "error": error_val,
                    "startTime": now_ms(),
                    "endTime": now_ms(),
                }),
            );
        }

        // === Approval Requests ===
        EventMsg::ExecApprovalRequest(req) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "approvalRequest",
                    "kind": "exec",
                    "id": req.call_id,
                    "command": req.command,
                    "cwd": req.cwd,
                    "reasoning": req.reason,
                }),
            );
        }
        EventMsg::ApplyPatchApprovalRequest(req) => {
            let files: Vec<String> = req
                .changes
                .keys()
                .map(|p| p.display().to_string())
                .collect();
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "approvalRequest",
                    "kind": "patch",
                    "id": req.call_id,
                    "files": files,
                    "reasoning": req.reason,
                }),
            );
        }

        // === MCP Tool Calls ===
        EventMsg::McpToolCallBegin(mcp) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "toolStart",
                    "id": mcp.call_id,
                    "toolType": "mcp",
                    "name": format!("{}:{}", mcp.invocation.server, mcp.invocation.tool),
                    "status": "running",
                    "input": mcp.invocation.arguments.clone().unwrap_or(serde_json::Value::Null),
                    "output": null,
                    "error": null,
                    "startTime": now_ms(),
                    "endTime": null,
                }),
            );
        }
        EventMsg::McpToolCallEnd(mcp) => {
            let (status, output, error): (&str, Option<String>, Option<String>) = match &mcp.result
            {
                Ok(result) => {
                    let is_err = result.is_error.unwrap_or(false);
                    // Extract actual text content from MCP result
                    let content_str = result
                        .content
                        .iter()
                        .filter_map(|c| {
                            // Extract text from different MCP content types
                            use mcp_types::{ContentBlock, EmbeddedResourceResource};
                            match c {
                                ContentBlock::TextContent(text) => Some(text.text.clone()),
                                ContentBlock::ImageContent(_) => Some("[image]".to_string()),
                                ContentBlock::AudioContent(_) => Some("[audio]".to_string()),
                                ContentBlock::ResourceLink(res) => Some(format!("[resource: {}]", res.uri)),
                                ContentBlock::EmbeddedResource(res) => {
                                    match &res.resource {
                                        EmbeddedResourceResource::TextResourceContents(text) => Some(text.text.clone()),
                                        EmbeddedResourceResource::BlobResourceContents(blob) => Some(format!("[blob: {}]", blob.uri)),
                                    }
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if is_err {
                        ("error", None, Some(content_str))
                    } else {
                        ("success", Some(content_str), None)
                    }
                }
                Err(e) => ("error", None, Some(e.clone())),
            };
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "toolEnd",
                    "id": mcp.call_id,
                    "toolType": "mcp",
                    "name": format!("{}:{}", mcp.invocation.server, mcp.invocation.tool),
                    "status": status,
                    "input": null,
                    "output": output,
                    "error": error,
                    "startTime": now_ms(),
                    "endTime": now_ms(),
                }),
            );
        }
        EventMsg::McpStartupUpdate(update) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "mcpStartup",
                    "server": update.server,
                    "status": format!("{:?}", update.status),
                }),
            );
        }

        // === Web Search ===
        EventMsg::WebSearchBegin(ws) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "toolStart",
                    "id": ws.call_id,
                    "toolType": "search",
                    "name": "web_search",
                    "status": "running",
                    "input": null,
                    "output": null,
                    "error": null,
                    "startTime": now_ms(),
                    "endTime": null,
                }),
            );
        }
        EventMsg::WebSearchEnd(ws) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "toolEnd",
                    "id": ws.call_id,
                    "toolType": "search",
                    "name": "web_search",
                    "status": "success",
                    "input": {"query": ws.query},
                    "output": format!("Searched: {}", ws.query),
                    "error": null,
                    "startTime": now_ms(),
                    "endTime": now_ms(),
                }),
            );
        }

        // === Patch/File Operations ===
        EventMsg::PatchApplyBegin(patch) => {
            let files: Vec<String> = patch
                .changes
                .keys()
                .map(|p| p.display().to_string())
                .collect();
            eprintln!("[kaioken-gui] EMITTING toolStart (edit): {:?}", files);
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "toolStart",
                    "id": patch.call_id,
                    "toolType": "edit",
                    "name": "edit",
                    "status": "running",
                    "input": {"files": files, "autoApproved": patch.auto_approved},
                    "output": null,
                    "error": null,
                    "startTime": now_ms(),
                    "endTime": null,
                }),
            );
        }
        EventMsg::PatchApplyEnd(patch) => {
            let status = if patch.success { "success" } else { "error" };
            let error_val: Option<&str> = if patch.stderr.is_empty() {
                None
            } else {
                Some(&patch.stderr)
            };
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "toolEnd",
                    "id": patch.call_id,
                    "toolType": "edit",
                    "name": "edit",
                    "status": status,
                    "input": null,
                    "output": patch.stdout,
                    "error": error_val,
                    "startTime": now_ms(),
                    "endTime": now_ms(),
                }),
            );
        }

        // === Subagent Tasks ===
        EventMsg::SubagentTaskUpdate(update) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "subagentUpdate",
                    "callId": update.call_id,
                    "task": update.task,
                    "agentIndex": update.agent_index,
                    "status": format!("{:?}", update.status),
                    "summary": update.summary,
                }),
            );
        }
        EventMsg::SubagentTaskLog(log) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "subagentLog",
                    "callId": log.call_id,
                    "task": log.task,
                    "agentIndex": log.agent_index,
                    "line": log.line,
                }),
            );
        }
        EventMsg::SubagentHistoryItem(ev) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "subagentHistory",
                    "callId": ev.call_id,
                    "task": ev.task,
                    "event": ev.event,
                    "agentIndex": ev.agent_index,
                }),
            );
        }

        // === Errors & Warnings ===
        EventMsg::Error(err) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "error",
                    "message": err.message,
                    "info": format!("{:?}", err.codex_error_info),
                }),
            );
        }
        EventMsg::Warning(warn) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "warning",
                    "message": warn.message,
                }),
            );
        }
        EventMsg::StreamError(err) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "streamError",
                    "message": err.message,
                }),
            );
        }

        // === Background & Context ===
        EventMsg::BackgroundEvent(bg) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "background",
                    "message": bg.message,
                }),
            );
        }
        EventMsg::ContextCompacted(_) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "contextCompacted",
                }),
            );
        }

        // === Plan Updates ===
        EventMsg::PlanUpdate(plan) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "planUpdate",
                    "plan": serde_json::to_value(plan).unwrap_or_default(),
                }),
            );
        }

        // === Memory Events ===
        EventMsg::MemoryRememberResponse(resp) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "memoryResponse",
                    "success": resp.success,
                    "memoryId": resp.memory_id,
                    "error": resp.error,
                }),
            );
        }

        // === Turn Diff (file changes) ===
        EventMsg::TurnDiff(diff) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "turnDiff",
                    "diff": diff.unified_diff,
                }),
            );
        }

        // === View Image ===
        EventMsg::ViewImageToolCall(img) => {
            let _ = app.emit(
                "kaioken-session-event",
                serde_json::json!({
                    "sessionId": session_id,
                    "type": "toolEnd",
                    "id": img.call_id,
                    "toolType": "read",
                    "name": "view_image",
                    "status": "success",
                    "input": {"path": img.path.display().to_string()},
                    "output": format!("Viewing: {}", img.path.display()),
                    "error": null,
                    "startTime": now_ms(),
                    "endTime": now_ms(),
                }),
            );
        }

        // Other events we acknowledge but don't need special handling
        _ => {
            eprintln!(
                "[kaioken-gui] Unhandled event type: {:?}",
                std::mem::discriminant(&event.msg)
            );
        }
    }
}

/// Send a message to a specific session
#[tauri::command(rename_all = "camelCase")]
async fn send_message(
    session_id: Option<String>,
    message: String,
    state: tauri::State<'_, Arc<AppState>>,
    _app: tauri::AppHandle,
) -> Result<SendMessageResponse, String> {
    let session_id = session_id.unwrap_or_else(|| "default".to_string());
    eprintln!("[kaioken-gui] send_message[{}]: {message}", session_id);

    // Check if session exists
    {
        let sessions = state.sessions.read().await;
        if !sessions.contains_key(&session_id) {
            eprintln!(
                "[kaioken-gui] Session {} doesn't exist - must call init_session first",
                session_id
            );
            return Err(format!(
                "Session {} not initialized. Call init_session first.",
                session_id
            ));
        }
    }

    // Create user message
    let user_msg = ChatMessage {
        id: generate_id(),
        role: MessageRole::User,
        content: message.clone(),
        timestamp: now_ms(),
        tool_calls: None,
    };

    // Get session and send op
    let sessions = state.sessions.read().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    // Store message in session
    // Note: We need write access, so we'll clone the op_tx and cwd first
    let op_tx = session.op_tx.clone();
    let cwd = session.cwd.clone();
    drop(sessions);

    // Store user message
    {
        let mut sessions = state.sessions.write().await;
        if let Some(session) = sessions.get_mut(&session_id) {
            session.messages.push(user_msg.clone());
        }
    }

    // Send user turn
    let model = state.model.read().await.clone();
    let reasoning_effort = *state.reasoning_effort.read().await;
    let approval_policy = *state.approval_policy.read().await;
    let sandbox_policy = state.sandbox_policy.read().await.clone();

    let op = Op::UserTurn {
        items: vec![UserInput::Text { text: message }],
        cwd,
        approval_policy,
        sandbox_policy,
        model,
        effort: Some(reasoning_effort),
        summary: ReasoningSummary::Auto,
        final_output_json_schema: None,
    };

    op_tx.send(op).map_err(|e| format!("Failed to send: {e}"))?;

    Ok(SendMessageResponse {
        message: user_msg,
        token_usage: None,
    })
}

/// Get messages for a specific session
#[tauri::command(rename_all = "camelCase")]
async fn get_messages(
    session_id: Option<String>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<ChatMessage>, String> {
    let session_id = session_id.unwrap_or_else(|| "default".to_string());
    let sessions = state.sessions.read().await;
    if let Some(session) = sessions.get(&session_id) {
        Ok(session.messages.clone())
    } else {
        Ok(Vec::new())
    }
}

/// Get status - returns true if session exists
#[tauri::command(rename_all = "camelCase")]
async fn get_status(
    session_id: Option<String>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    let session_id = session_id.unwrap_or_else(|| "default".to_string());
    let sessions = state.sessions.read().await;
    Ok(sessions.contains_key(&session_id))
}

/// Clear conversation for a specific session
#[tauri::command(rename_all = "camelCase")]
async fn clear_conversation(
    session_id: Option<String>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let session_id = session_id.unwrap_or_else(|| "default".to_string());
    let mut sessions = state.sessions.write().await;
    if let Some(session) = sessions.get_mut(&session_id) {
        session.messages.clear();
    }
    Ok(())
}

/// Set model
#[tauri::command]
async fn set_model(model: String, state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    *state.model.write().await = model;
    Ok(())
}

/// Set reasoning effort
#[tauri::command(rename_all = "camelCase")]
async fn set_reasoning_effort(
    reasoning_effort: ReasoningEffort,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    *state.reasoning_effort.write().await = reasoning_effort;
    Ok(())
}

/// Set approval mode preset
/// Presets (matching TUI):
/// - "read-only" -> OnRequest + ReadOnly sandbox
/// - "auto" -> OnRequest + WorkspaceWrite sandbox
/// - "full-access" -> Never + DangerFullAccess sandbox
#[tauri::command(rename_all = "camelCase")]
async fn set_approval_mode(
    preset_id: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let (approval, sandbox) = match preset_id.as_str() {
        "read-only" => (AskForApproval::OnRequest, SandboxPolicy::ReadOnly),
        "auto" => (
            AskForApproval::OnRequest,
            SandboxPolicy::new_workspace_write_policy(),
        ),
        "full-access" => (AskForApproval::Never, SandboxPolicy::DangerFullAccess),
        _ => return Err(format!("Unknown approval preset: {}", preset_id)),
    };
    eprintln!(
        "[kaioken-gui] Setting approval mode to: {} (approval={:?})",
        preset_id, approval
    );
    *state.approval_policy.write().await = approval;
    *state.sandbox_policy.write().await = sandbox;
    Ok(())
}

/// Interrupt a specific session
#[tauri::command(rename_all = "camelCase")]
async fn interrupt(
    session_id: Option<String>,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let session_id = session_id.unwrap_or_else(|| "default".to_string());
    let sessions = state.sessions.read().await;
    if let Some(session) = sessions.get(&session_id) {
        let _ = session.op_tx.send(Op::Interrupt);
    }
    Ok(())
}

/// Send approval decision for a specific session
#[tauri::command(rename_all = "camelCase")]
async fn send_approval(
    session_id: Option<String>,
    id: String,
    kind: String,
    approved: bool,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let session_id = session_id.unwrap_or_else(|| "default".to_string());
    let sessions = state.sessions.read().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    let decision = if approved {
        codex_core::protocol::ReviewDecision::Approved
    } else {
        codex_core::protocol::ReviewDecision::Denied
    };

    let op = match kind.as_str() {
        "exec" => Op::ExecApproval { id, decision },
        "patch" => Op::PatchApproval { id, decision },
        _ => return Err(format!("Unknown approval kind: {}", kind)),
    };

    session
        .op_tx
        .send(op)
        .map_err(|e| format!("Failed to send approval: {e}"))?;
    Ok(())
}

// ============================================================================
// Workspace Management Commands
// ============================================================================

use types::DetectedGitInfo;
use types::GuiWorkspacesConfig;
use types::RepositoryConfig;
use types::WorktreeInfo;
use workspace::WorkspaceManager;

/// Get workspace configuration
#[tauri::command]
async fn get_workspace_config(
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<GuiWorkspacesConfig, String> {
    Ok(workspace.get_config().await)
}

/// Add a repository to workspace
#[tauri::command]
async fn add_repository(
    path: String,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<RepositoryConfig, String> {
    workspace.add_repository(PathBuf::from(path)).await
}

/// Remove a repository from workspace
#[tauri::command]
async fn remove_repository(
    repository_id: String,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<(), String> {
    workspace.remove_repository(&repository_id).await
}

/// List git worktrees for a repository path
#[tauri::command]
async fn list_git_worktrees(repo_path: String) -> Result<Vec<WorktreeInfo>, String> {
    workspace::list_git_worktrees(&PathBuf::from(repo_path)).await
}

/// Create a new git worktree
#[tauri::command]
async fn create_worktree(
    repository_id: String,
    branch_name: String,
    worktree_path: String,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<WorktreeInfo, String> {
    workspace
        .create_worktree(&repository_id, &branch_name, PathBuf::from(worktree_path))
        .await
}

/// Detect git info for a path
#[tauri::command]
async fn detect_git_info(path: String) -> Result<DetectedGitInfo, String> {
    workspace::detect_git_info(&PathBuf::from(path)).await
}

/// Add or update a worktree session entry without spawning a process
#[tauri::command]
async fn upsert_worktree_session(
    session_id: String,
    repository_id: String,
    worktree_path: String,
    worktree_name: String,
    rollout_path: Option<String>,
    conversation_id: Option<String>,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<(), String> {
    workspace
        .upsert_session(
            session_id,
            repository_id,
            PathBuf::from(worktree_path),
            worktree_name,
            rollout_path.map(PathBuf::from),
            conversation_id,
        )
        .await
}

/// Start a worktree session (spawns codex process)
#[tauri::command]
async fn start_worktree_session(
    session_id: String,
    worktree_path: String,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    workspace
        .start_session(&session_id, PathBuf::from(worktree_path), app)
        .await
}

/// Update session with rollout path and conversation ID for persistence
#[tauri::command]
async fn update_session_rollout(
    session_id: String,
    rollout_path: Option<String>,
    conversation_id: Option<String>,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<(), String> {
    workspace
        .update_session_rollout(
            &session_id,
            rollout_path.map(PathBuf::from),
            conversation_id,
        )
        .await
}

/// Get session rollout path for resuming
#[tauri::command]
async fn get_session_rollout(
    session_id: String,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<Option<String>, String> {
    Ok(workspace
        .get_session_rollout(&session_id)
        .await
        .map(|p| p.display().to_string()))
}

/// Tab config for saving/loading
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TabConfigInput {
    id: String,
    name: String,
    rollout_path: Option<String>,
    conversation_id: Option<String>,
    created_at: String,
}

impl From<TabConfigInput> for types::TabConfig {
    fn from(input: TabConfigInput) -> Self {
        Self {
            id: input.id,
            name: input.name,
            rollout_path: input.rollout_path.map(PathBuf::from),
            conversation_id: input.conversation_id,
            created_at: input.created_at,
        }
    }
}

impl From<types::TabConfig> for TabConfigInput {
    fn from(config: types::TabConfig) -> Self {
        Self {
            id: config.id,
            name: config.name,
            rollout_path: config.rollout_path.map(|p| p.display().to_string()),
            conversation_id: config.conversation_id,
            created_at: config.created_at,
        }
    }
}

/// Update session tabs (save open tabs and active tab)
#[tauri::command]
async fn update_session_tabs(
    session_id: String,
    tabs: Vec<TabConfigInput>,
    active_tab_id: Option<String>,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<(), String> {
    let tabs: Vec<types::TabConfig> = tabs.into_iter().map(Into::into).collect();
    workspace
        .update_session_tabs(&session_id, tabs, active_tab_id)
        .await
}

/// Add tab to session history (when closing a tab)
#[tauri::command]
async fn add_tab_to_history(
    session_id: String,
    tab: TabConfigInput,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<(), String> {
    workspace
        .add_to_history(&session_id, tab.into())
        .await
}

/// Get session chat history (closed tabs)
#[tauri::command]
async fn get_session_history(
    session_id: String,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<Vec<TabConfigInput>, String> {
    let history = workspace.get_session_history(&session_id).await;
    Ok(history.into_iter().map(Into::into).collect())
}

/// Stop a worktree session
#[tauri::command]
async fn stop_worktree_session(
    session_id: String,
    workspace: tauri::State<'_, Arc<WorkspaceManager>>,
) -> Result<(), String> {
    workspace.stop_session(&session_id).await
}

/// Rate limit window info for frontend
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitWindowResponse {
    used_percent: f64,
    window_minutes: Option<i64>,
    resets_at: Option<i64>,
}

/// Rate limits response for frontend
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitsResponse {
    primary: Option<RateLimitWindowResponse>,
    secondary: Option<RateLimitWindowResponse>,
    has_credits: Option<bool>,
    credits_balance: Option<String>,
}

/// List files in a session's working directory for @-mention autocomplete
#[tauri::command]
async fn list_files(
    session_id: String,
    query: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<String>, String> {
    // Get session's cwd
    let cwd = {
        let sessions = state.sessions.read().await;
        sessions
            .get(&session_id)
            .map(|s| s.cwd.clone())
            .unwrap_or_else(|| state.default_cwd.clone())
    };

    let query_lower = query.to_lowercase();
    let mut files = Vec::new();

    // Walk directory (max 1000 files, max depth 5)
    fn walk_dir(
        dir: &std::path::Path,
        base: &std::path::Path,
        files: &mut Vec<String>,
        query: &str,
        depth: usize,
    ) {
        if depth > 5 || files.len() >= 1000 {
            return;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files and common ignore patterns
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "dist"
                || name == "build"
                || name == "__pycache__"
                || name == ".git"
            {
                continue;
            }

            if path.is_file() {
                if let Ok(rel) = path.strip_prefix(base) {
                    let rel_str = rel.to_string_lossy().to_string();
                    // Filter by query if provided
                    if query.is_empty() || rel_str.to_lowercase().contains(query) {
                        files.push(rel_str);
                    }
                }
            } else if path.is_dir() {
                walk_dir(&path, base, files, query, depth + 1);
            }
        }
    }

    walk_dir(&cwd, &cwd, &mut files, &query_lower, 0);

    // Sort by relevance (exact filename match first, then by path length)
    files.sort_by(|a, b| {
        let a_name = a.split('/').last().unwrap_or(a).to_lowercase();
        let b_name = b.split('/').last().unwrap_or(b).to_lowercase();
        let a_exact = a_name.contains(&query_lower);
        let b_exact = b_name.contains(&query_lower);

        match (a_exact, b_exact) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.len().cmp(&b.len()),
        }
    });

    // Limit to 50 results
    files.truncate(50);

    Ok(files)
}

/// Fetch rate limits from the API
#[tauri::command]
async fn get_rate_limits() -> Result<RateLimitsResponse, String> {
    eprintln!("[kaioken-gui] Fetching rate limits...");

    // Load config to get base URL and auth settings
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let config = Config::load_with_cli_overrides(
        vec![],
        ConfigOverrides {
            cwd: Some(cwd),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| format!("Failed to load config: {}", e))?;

    // Create auth manager
    let auth_manager = AuthManager::shared(
        config.codex_home.clone(),
        false,
        config.cli_auth_credentials_store_mode,
    );

    // Get auth credentials
    let auth = auth_manager
        .auth()
        .ok_or_else(|| "Not authenticated. Run `codex-kaioken login` first.".to_string())?;

    // Create backend client and fetch rate limits
    let client = BackendClient::from_auth(&config.chatgpt_base_url, &auth)
        .await
        .map_err(|e| format!("Failed to create backend client: {}", e))?;

    let snapshot = client
        .get_rate_limits()
        .await
        .map_err(|e| format!("Failed to fetch rate limits: {}", e))?;

    eprintln!("[kaioken-gui] Rate limits fetched successfully");

    // Convert to response format
    Ok(RateLimitsResponse {
        primary: snapshot.primary.map(|w| RateLimitWindowResponse {
            used_percent: w.used_percent,
            window_minutes: w.window_minutes,
            resets_at: w.resets_at,
        }),
        secondary: snapshot.secondary.map(|w| RateLimitWindowResponse {
            used_percent: w.used_percent,
            window_minutes: w.window_minutes,
            resets_at: w.resets_at,
        }),
        has_credits: snapshot.credits.as_ref().map(|c| c.has_credits),
        credits_balance: snapshot.credits.and_then(|c| c.balance),
    })
}

/// Memory entry for UI display
#[derive(Debug, Clone, serde::Serialize)]
struct MemoryEntry {
    id: String,
    memory_type: String,
    content: String,
    importance: f64,
    created_at: i64,
}

/// Get memories from the project's memory database
#[tauri::command]
async fn get_memories(cwd: String) -> Result<Vec<MemoryEntry>, String> {
    let db_path = PathBuf::from(&cwd)
        .join(".kaioken")
        .join("memory")
        .join("memories.db");

    if !db_path.exists() {
        return Ok(vec![]);
    }

    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("Failed to open memory database: {}", e))?;

    let mut stmt = conn
        .prepare("SELECT id, type, content, importance, created_at FROM memories ORDER BY importance DESC, last_used DESC LIMIT 20")
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let memories = stmt
        .query_map([], |row| {
            Ok(MemoryEntry {
                id: row.get(0)?,
                memory_type: row.get(1)?,
                content: row.get(2)?,
                importance: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|e| format!("Failed to query memories: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(memories)
}

fn main() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Initialize workspace manager with codex home directory
    let codex_home = dirs::home_dir()
        .map(|h| h.join(".codex"))
        .unwrap_or_else(|| PathBuf::from(".codex"));
    let workspace_manager = Arc::new(WorkspaceManager::new(codex_home));

    let app_state = Arc::new(AppState {
        conversation_manager: RwLock::new(None),
        sessions: RwLock::new(HashMap::new()),
        default_cwd: cwd,
        model: RwLock::new("gpt-5.2-codex".to_string()),
        reasoning_effort: RwLock::new(ReasoningEffort::Medium),
        // Default to "auto" preset (Agent mode with workspace write)
        approval_policy: RwLock::new(AskForApproval::OnRequest),
        sandbox_policy: RwLock::new(SandboxPolicy::new_workspace_write_policy()),
    });

    if let Err(err) = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .manage(workspace_manager)
        .invoke_handler(tauri::generate_handler![
            // Session commands
            init_session,
            init_conversation,
            send_message,
            get_messages,
            get_status,
            clear_conversation,
            set_model,
            set_reasoning_effort,
            set_approval_mode,
            interrupt,
            send_approval,
            // Workspace management commands
            get_workspace_config,
            add_repository,
            remove_repository,
            list_git_worktrees,
            create_worktree,
            detect_git_info,
            upsert_worktree_session,
            start_worktree_session,
            stop_worktree_session,
            update_session_rollout,
            get_session_rollout,
            update_session_tabs,
            add_tab_to_history,
            get_session_history,
            get_rate_limits,
            get_memories,
            list_files,
        ])
        .run(tauri::generate_context!())
    {
        eprintln!("error running tauri app: {err}");
    }
}
