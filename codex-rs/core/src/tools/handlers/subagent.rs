use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SessionSource;
use codex_protocol::user_input::UserInput;
use futures::future::join_all;
use serde::Deserialize;
use tokio::time::timeout;
use tracing::warn;

use crate::codex::Codex;
use crate::codex::CodexSpawnOk;
use crate::config::Config;
use crate::config::types::SUBAGENT_LIMIT_HARD_CAP;
use crate::config::types::SUBAGENT_LIMIT_MIN;
use crate::function_tool::FunctionCallError;
use crate::protocol::AskForApproval;
use crate::protocol::EventMsg;
use crate::protocol::SubagentTaskStatus;
use crate::protocol::SubagentTaskUpdateEvent;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct SubagentHandler;

#[derive(Debug, Deserialize)]
struct SubagentArgs {
    tasks: Vec<SubagentTask>,
}

#[derive(Debug, Deserialize)]
struct SubagentTask {
    name: String,
    prompt: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_ms: Option<i64>,
}

#[derive(Debug)]
struct SubagentResult {
    name: String,
    status: String,
    output: Option<String>,
    error: Option<String>,
}

const DEFAULT_CHILD_TIMEOUT: Option<Duration> = None;

#[async_trait]
impl ToolHandler for SubagentHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            payload,
            turn,
            session,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "subagent_run handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: SubagentArgs = serde_json::from_str(&arguments).map_err(|err| {
            FunctionCallError::RespondToModel(format!("failed to parse function arguments: {err}"))
        })?;

        if args.tasks.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "tasks must not be empty".to_string(),
            ));
        }

        let auth_manager = turn.client.get_auth_manager().ok_or_else(|| {
            FunctionCallError::RespondToModel("auth manager unavailable".to_string())
        })?;
        let parent_config = session
            .clone_original_config()
            .await
            .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;
        let task_limit = effective_subagent_limit(parent_config.subagent_max_tasks);
        if args.tasks.len() > task_limit {
            return Err(FunctionCallError::RespondToModel(format!(
                "too many subagent tasks (limit {task_limit})"
            )));
        }

        let session_source = turn.client.get_session_source();
        let parent_cwd = turn.cwd.clone();
        let call_id = invocation.call_id.clone();
        let session = session.clone();
        let turn = turn.clone();

        let futures = args.tasks.into_iter().enumerate().map(|(idx, task)| {
            let auth_manager = auth_manager.clone();
            let parent_config = parent_config.clone();
            let parent_cwd = parent_cwd.clone();
            let session_source = session_source.clone();
            let session = session.clone();
            let turn = turn.clone();
            let call_id = call_id.clone();
            let agent_index = idx;
            async move {
                send_subagent_update(
                    &session,
                    &turn,
                    SubagentTaskStatus::Running,
                    &call_id,
                    &task.name,
                    None,
                )
                .await;
                run_subagent_task(
                    task,
                    parent_config,
                    auth_manager,
                    session_source,
                    parent_cwd,
                    session,
                    turn,
                    call_id,
                    agent_index,
                )
                .await
            }
        });

        let results = join_all(futures).await;

        let mut lines: Vec<String> = Vec::new();
        for result in results {
            match result {
                Ok(res) => {
                    lines.push(format!("[{}] {}", res.name, res.status));
                    if let Some(output) = res.output {
                        lines.push(output);
                    }
                    if let Some(err) = res.error {
                        lines.push(format!("error: {err}"));
                    }
                }
                Err(err) => {
                    lines.push(format!("task failed: {err}"));
                }
            }
        }

        Ok(ToolOutput::Function {
            content: lines.join("\n"),
            content_items: None,
            success: Some(true),
        })
    }
}

fn effective_subagent_limit(raw_limit: i64) -> usize {
    let clamped = raw_limit.clamp(SUBAGENT_LIMIT_MIN, SUBAGENT_LIMIT_HARD_CAP);
    if raw_limit != clamped {
        warn!(
            configured = raw_limit,
            applied = clamped,
            hard_cap = SUBAGENT_LIMIT_HARD_CAP,
            "subagent limit outside supported range; clamping"
        );
    }
    clamped as usize
}

async fn run_subagent_task(
    task: SubagentTask,
    parent_config: Arc<Config>,
    auth_manager: Arc<crate::AuthManager>,
    session_source: SessionSource,
    parent_cwd: PathBuf,
    session: Arc<crate::codex::Session>,
    turn: Arc<crate::codex::TurnContext>,
    call_id: String,
    agent_index: usize,
) -> Result<SubagentResult, String> {
    let timeout_duration = task
        .timeout_ms
        .and_then(|ms| u64::try_from(ms).ok())
        .map(Duration::from_millis)
        .or(DEFAULT_CHILD_TIMEOUT);

    let task_name = task.name.clone();
    let task_name_for_timeout = task_name.clone();
    let resolved_cwd = resolve_child_cwd(&parent_cwd, task.cwd);

    let child_config = make_child_config(parent_config, resolved_cwd);

    let fut_task_name = task_name.clone();
    let session_for_result = session.clone();
    let turn_for_result = turn.clone();
    let call_id_for_result = call_id.clone();

    let fut = async move {
        let CodexSpawnOk { codex, .. } = Codex::spawn(
            (*child_config).clone(),
            auth_manager,
            InitialHistory::New,
            session_source,
        )
        .await
        .map_err(|err| format!("failed to spawn subagent: {err}"))?;

        let op = Op::UserInput {
            items: vec![UserInput::Text {
                text: task.prompt.clone(),
            }],
        };
        codex
            .submit(op)
            .await
            .map_err(|err| format!("failed to submit subagent task: {err}"))?;

        let mut last_message: Option<String> = None;
        while let Ok(event) = codex.rx_event.recv().await {
            let child_msg = event.msg.clone();
            match &child_msg {
                crate::protocol::EventMsg::AgentMessage(ev) => {
                    last_message = Some(ev.message.clone());
                    send_subagent_update(
                        &session,
                        &turn,
                        SubagentTaskStatus::Running,
                        &call_id,
                        &fut_task_name,
                        Some(ev.message.clone()),
                    )
                    .await;
                }
                crate::protocol::EventMsg::AgentMessageContentDelta(_)
                | crate::protocol::EventMsg::ReasoningContentDelta(_) => {}
                crate::protocol::EventMsg::TaskComplete(ev) => {
                    last_message = ev.last_agent_message.clone();
                    send_subagent_history_item(
                        &session,
                        &turn,
                        &call_id,
                        &fut_task_name,
                        Some(agent_index),
                        child_msg.clone(),
                    )
                    .await;
                    break;
                }
                crate::protocol::EventMsg::ExecCommandBegin(_)
                | crate::protocol::EventMsg::ExecCommandOutputDelta(_)
                | crate::protocol::EventMsg::ExecCommandEnd(_)
                | crate::protocol::EventMsg::PatchApplyBegin(_)
                | crate::protocol::EventMsg::PatchApplyEnd(_)
                | crate::protocol::EventMsg::McpToolCallBegin(_)
                | crate::protocol::EventMsg::McpToolCallEnd(_)
                | crate::protocol::EventMsg::WebSearchBegin(_)
                | crate::protocol::EventMsg::WebSearchEnd(_) => {
                    send_subagent_history_item(
                        &session,
                        &turn,
                        &call_id,
                        &fut_task_name,
                        Some(agent_index),
                        child_msg.clone(),
                    )
                    .await;
                }
                _ => {}
            }
        }

        if last_message.is_none() {
            last_message = Some("no result produced".to_string());
        }

        Ok(SubagentResult {
            name: fut_task_name,
            status: "done".to_string(),
            output: last_message,
            error: None,
        })
    };

    let result: Result<SubagentResult, String> = if let Some(duration) = timeout_duration {
        match timeout(duration, fut).await {
            Ok(inner) => inner,
            Err(_) => Ok(SubagentResult {
                name: task_name_for_timeout.clone(),
                status: "timeout".to_string(),
                output: None,
                error: Some(format!("timed out after {}s", duration.as_secs())),
            }),
        }
    } else {
        fut.await
    };

    let (status, summary, name) = match &result {
        Ok(res) if res.status == "done" => (
            SubagentTaskStatus::Done,
            res.output.clone(),
            res.name.clone(),
        ),
        Ok(res) if res.status == "timeout" => (
            SubagentTaskStatus::Timeout,
            res.error.clone().or_else(|| res.output.clone()),
            res.name.clone(),
        ),
        Ok(res) => (
            SubagentTaskStatus::Failed,
            res.error.clone().or_else(|| res.output.clone()),
            res.name.clone(),
        ),
        Err(err) => (
            SubagentTaskStatus::Failed,
            Some(err.clone()),
            task_name_for_timeout.clone(),
        ),
    };

    send_subagent_update(
        &session_for_result,
        &turn_for_result,
        status,
        &call_id_for_result,
        &name,
        summary,
    )
    .await;

    result
}

fn resolve_child_cwd(parent_cwd: &Path, maybe_cwd: Option<String>) -> PathBuf {
    if let Some(cwd) = maybe_cwd {
        let trimmed = cwd.trim();
        if trimmed.is_empty() {
            return parent_cwd.to_path_buf();
        }
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            return path;
        }
        return parent_cwd.join(path);
    }
    parent_cwd.to_path_buf()
}

fn make_child_config(parent: Arc<Config>, cwd: PathBuf) -> Arc<Config> {
    let mut config = (*parent).clone();
    config.cwd = cwd;
    config.approval_policy = AskForApproval::Never;
    Arc::new(config)
}

async fn send_subagent_update(
    session: &Arc<crate::codex::Session>,
    turn: &Arc<crate::codex::TurnContext>,
    status: SubagentTaskStatus,
    call_id: &str,
    task_name: &str,
    summary: Option<String>,
) {
    let event = EventMsg::SubagentTaskUpdate(SubagentTaskUpdateEvent {
        call_id: call_id.to_string(),
        task: task_name.to_string(),
        status,
        summary,
    });
    session.send_event(turn.as_ref(), event).await;
}

async fn send_subagent_history_item(
    session: &Arc<crate::codex::Session>,
    turn: &Arc<crate::codex::TurnContext>,
    call_id: &str,
    task_name: &str,
    agent_index: Option<usize>,
    event: crate::protocol::EventMsg,
) {
    let event = EventMsg::SubagentHistoryItem(crate::protocol::SubagentHistoryItemEvent {
        call_id: call_id.to_string(),
        task: task_name.to_string(),
        agent_index: agent_index.and_then(|idx| i64::try_from(idx).ok()),
        event: Box::new(event),
    });
    session.send_event(turn.as_ref(), event).await;
}
