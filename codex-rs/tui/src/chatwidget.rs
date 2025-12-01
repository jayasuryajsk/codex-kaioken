use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use codex_app_server_protocol::AuthMode;
use codex_backend_client::Client as BackendClient;
use codex_core::config::Config;
use codex_core::config::types::Notifications;
use codex_core::config::types::PlanDetailPreference;
use codex_core::config::types::SUBAGENT_LIMIT_HARD_CAP;
use codex_core::config::types::SUBAGENT_LIMIT_MIN;
use codex_core::git_info::current_branch_name;
use codex_core::git_info::local_git_branches;
use codex_core::project_doc::DEFAULT_PROJECT_DOC_FILENAME;
use codex_core::protocol::AgentMessageDeltaEvent;
use codex_core::protocol::AgentMessageEvent;
use codex_core::protocol::AgentReasoningDeltaEvent;
use codex_core::protocol::AgentReasoningEvent;
use codex_core::protocol::AgentReasoningRawContentDeltaEvent;
use codex_core::protocol::AgentReasoningRawContentEvent;
use codex_core::protocol::ApplyPatchApprovalRequestEvent;
use codex_core::protocol::BackgroundEventEvent;
use codex_core::protocol::CheckpointAction;
use codex_core::protocol::CheckpointCreatedEvent;
use codex_core::protocol::CheckpointEntry;
use codex_core::protocol::CheckpointErrorEvent;
use codex_core::protocol::CheckpointListEvent;
use codex_core::protocol::CheckpointRestoredEvent;
use codex_core::protocol::DeprecationNoticeEvent;
use codex_core::protocol::ErrorEvent;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::ExecApprovalRequestEvent;
use codex_core::protocol::ExecCommandBeginEvent;
use codex_core::protocol::ExecCommandEndEvent;
use codex_core::protocol::ExecCommandSource;
use codex_core::protocol::ExitedReviewModeEvent;
use codex_core::protocol::ListCustomPromptsResponseEvent;
use codex_core::protocol::McpListToolsResponseEvent;
use codex_core::protocol::McpStartupCompleteEvent;
use codex_core::protocol::McpStartupStatus;
use codex_core::protocol::McpStartupUpdateEvent;
use codex_core::protocol::McpToolCallBeginEvent;
use codex_core::protocol::McpToolCallEndEvent;
use codex_core::protocol::Op;
use codex_core::protocol::PatchApplyBeginEvent;
use codex_core::protocol::RateLimitSnapshot;
use codex_core::protocol::ReviewRequest;
use codex_core::protocol::StreamErrorEvent;
use codex_core::protocol::SubagentHistoryItemEvent;
use codex_core::protocol::SubagentTaskLogEvent;
use codex_core::protocol::SubagentTaskUpdateEvent;
use codex_core::protocol::TaskCompleteEvent;
use codex_core::protocol::TokenUsage;
use codex_core::protocol::TokenUsageInfo;
use codex_core::protocol::TurnAbortReason;
use codex_core::protocol::TurnDiffEvent;
use codex_core::protocol::UndoCompletedEvent;
use codex_core::protocol::UndoStartedEvent;
use codex_core::protocol::UserMessageEvent;
use codex_core::protocol::ViewImageToolCallEvent;
use codex_core::protocol::WarningEvent;
use codex_core::protocol::WebSearchBeginEvent;
use codex_core::protocol::WebSearchEndEvent;
use codex_protocol::ConversationId;
use codex_protocol::approvals::ElicitationRequestEvent;
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::user_input::UserInput;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use rand::Rng;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tracing::debug;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ApprovalRequest;
use crate::bottom_pane::BottomPane;
use crate::bottom_pane::BottomPaneParams;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::InputResult;
use crate::bottom_pane::PlanReviewView;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::custom_prompt_view::CustomPromptView;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::clipboard_paste::paste_image_to_temp_png;
use crate::diff_render::display_path_for;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::ExecCell;
use crate::exec_cell::new_active_exec_command;
use crate::get_git_diff::get_git_diff;
use crate::history_cell;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::CompositeHistoryCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::McpToolCallCell;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::SubagentTasksCell;
use crate::history_cell::WelcomeSnapshot;
use crate::markdown::append_markdown;
use crate::render::Insets;
use crate::render::line_utils::prefix_lines;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::FlexRenderable;
use crate::render::renderable::Renderable;
use crate::render::renderable::RenderableExt;
use crate::render::renderable::RenderableItem;
use crate::semantic::SemanticStatus;
use crate::semantic::find_sgrep_binary;
use crate::slash_command::SlashCommand;
use crate::status::RateLimitSnapshotDisplay;
use crate::text_formatting::truncate_text;
use crate::tui::FrameRequester;
mod interrupts;
use self::interrupts::InterruptManager;
mod agent;
use self::agent::spawn_agent;
use self::agent::spawn_agent_from_existing;
mod session_header;
use self::session_header::SessionHeader;
use crate::streaming::controller::StreamController;
use std::env;
use std::path::Path;
use std::process::Stdio;

use chrono::Local;
use codex_ansi_escape::ansi_escape_line;
use codex_common::approval_presets::ApprovalPreset;
use codex_common::approval_presets::builtin_approval_presets;
use codex_common::model_presets::ModelPreset;
use codex_common::model_presets::builtin_model_presets;
use codex_core::AuthManager;
use codex_core::CodexAuth;
use codex_core::ConversationManager;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::SandboxPolicy;
use codex_core::protocol_config_types::ReasoningEffort as ReasoningEffortConfig;
use codex_file_search::FileMatch;
use codex_protocol::plan_tool::UpdatePlanArgs;
use strum::IntoEnumIterator;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;

const USER_SHELL_COMMAND_HELP_TITLE: &str = "Prefix a command with ! to run it locally";
const USER_SHELL_COMMAND_HELP_HINT: &str = "Example: !ls";
const PLAN_MODE_PLACEHOLDER: &str = "Plan mode enabled — describe the goal";
const PLAN_FEEDBACK_PLACEHOLDER: &str = "Describe plan adjustments and press Enter";
// Track information about an in-flight exec command.
struct RunningCommand {
    command: Vec<String>,
    parsed_cmd: Vec<ParsedCommand>,
    source: ExecCommandSource,
}

struct UnifiedExecWaitState {
    command_display: String,
}

impl UnifiedExecWaitState {
    fn new(command_display: String) -> Self {
        Self { command_display }
    }

    fn is_duplicate(&self, command_display: &str) -> bool {
        self.command_display == command_display
    }
}

const RATE_LIMIT_WARNING_THRESHOLDS: [f64; 3] = [75.0, 90.0, 95.0];
const NUDGE_MODEL_SLUG: &str = "gpt-5.1-codex-mini";
const RATE_LIMIT_SWITCH_PROMPT_THRESHOLD: f64 = 90.0;

#[derive(Default)]
struct RateLimitWarningState {
    secondary_index: usize,
    primary_index: usize,
}

impl RateLimitWarningState {
    fn take_warnings(
        &mut self,
        secondary_used_percent: Option<f64>,
        secondary_window_minutes: Option<i64>,
        primary_used_percent: Option<f64>,
        primary_window_minutes: Option<i64>,
    ) -> Vec<String> {
        let reached_secondary_cap =
            matches!(secondary_used_percent, Some(percent) if percent == 100.0);
        let reached_primary_cap = matches!(primary_used_percent, Some(percent) if percent == 100.0);
        if reached_secondary_cap || reached_primary_cap {
            return Vec::new();
        }

        let mut warnings = Vec::new();

        if let Some(secondary_used_percent) = secondary_used_percent {
            let mut highest_secondary: Option<f64> = None;
            while self.secondary_index < RATE_LIMIT_WARNING_THRESHOLDS.len()
                && secondary_used_percent >= RATE_LIMIT_WARNING_THRESHOLDS[self.secondary_index]
            {
                highest_secondary = Some(RATE_LIMIT_WARNING_THRESHOLDS[self.secondary_index]);
                self.secondary_index += 1;
            }
            if let Some(threshold) = highest_secondary {
                let limit_label = secondary_window_minutes
                    .map(get_limits_duration)
                    .unwrap_or_else(|| "weekly".to_string());
                warnings.push(format!(
                    "Heads up, you've used over {threshold:.0}% of your {limit_label} limit. Run /status for a breakdown."
                ));
            }
        }

        if let Some(primary_used_percent) = primary_used_percent {
            let mut highest_primary: Option<f64> = None;
            while self.primary_index < RATE_LIMIT_WARNING_THRESHOLDS.len()
                && primary_used_percent >= RATE_LIMIT_WARNING_THRESHOLDS[self.primary_index]
            {
                highest_primary = Some(RATE_LIMIT_WARNING_THRESHOLDS[self.primary_index]);
                self.primary_index += 1;
            }
            if let Some(threshold) = highest_primary {
                let limit_label = primary_window_minutes
                    .map(get_limits_duration)
                    .unwrap_or_else(|| "5h".to_string());
                warnings.push(format!(
                    "Heads up, you've used over {threshold:.0}% of your {limit_label} limit. Run /status for a breakdown."
                ));
            }
        }

        warnings
    }
}

#[derive(Clone)]
struct PlanWorkflow {
    request: UserMessage,
    status: PlanWorkflowStatus,
    last_plan: Option<UpdatePlanArgs>,
    extra_feedback: Vec<String>,
}

impl PlanWorkflow {
    fn new(request: UserMessage) -> Self {
        Self {
            request,
            status: PlanWorkflowStatus::AwaitingPlan,
            last_plan: None,
            extra_feedback: Vec::new(),
        }
    }

    fn awaiting_approval(&self) -> bool {
        matches!(self.status, PlanWorkflowStatus::AwaitingApproval)
    }

    fn request_preview(&self) -> Option<String> {
        self.request
            .visible_text()
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| truncate_text(line, 80))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlanWorkflowStatus {
    AwaitingPlan,
    AwaitingApproval,
}

pub(crate) fn get_limits_duration(windows_minutes: i64) -> String {
    const MINUTES_PER_HOUR: i64 = 60;
    const MINUTES_PER_DAY: i64 = 24 * MINUTES_PER_HOUR;
    const MINUTES_PER_WEEK: i64 = 7 * MINUTES_PER_DAY;
    const MINUTES_PER_MONTH: i64 = 30 * MINUTES_PER_DAY;
    const ROUNDING_BIAS_MINUTES: i64 = 3;

    let windows_minutes = windows_minutes.max(0);

    if windows_minutes <= MINUTES_PER_DAY.saturating_add(ROUNDING_BIAS_MINUTES) {
        let adjusted = windows_minutes.saturating_add(ROUNDING_BIAS_MINUTES);
        let hours = std::cmp::max(1, adjusted / MINUTES_PER_HOUR);
        format!("{hours}h")
    } else if windows_minutes <= MINUTES_PER_WEEK.saturating_add(ROUNDING_BIAS_MINUTES) {
        "weekly".to_string()
    } else if windows_minutes <= MINUTES_PER_MONTH.saturating_add(ROUNDING_BIAS_MINUTES) {
        "monthly".to_string()
    } else {
        "annual".to_string()
    }
}

/// Common initialization parameters shared by all `ChatWidget` constructors.
pub(crate) struct ChatWidgetInit {
    pub(crate) config: Config,
    pub(crate) frame_requester: FrameRequester,
    pub(crate) app_event_tx: AppEventSender,
    pub(crate) initial_prompt: Option<String>,
    pub(crate) initial_images: Vec<PathBuf>,
    pub(crate) enhanced_keys_supported: bool,
    pub(crate) auth_manager: Arc<AuthManager>,
    pub(crate) feedback: codex_feedback::CodexFeedback,
}

#[derive(Default)]
enum RateLimitSwitchPromptState {
    #[default]
    Idle,
    Pending,
    Shown,
}

pub(crate) struct ChatWidget {
    app_event_tx: AppEventSender,
    codex_op_tx: UnboundedSender<Op>,
    bottom_pane: BottomPane,
    active_cell: Option<Box<dyn HistoryCell>>,
    config: Config,
    auth_manager: Arc<AuthManager>,
    session_header: SessionHeader,
    initial_user_message: Option<UserMessage>,
    token_info: Option<TokenUsageInfo>,
    rate_limit_snapshot: Option<RateLimitSnapshotDisplay>,
    rate_limit_warnings: RateLimitWarningState,
    rate_limit_switch_prompt: RateLimitSwitchPromptState,
    rate_limit_poller: Option<JoinHandle<()>>,
    // Stream lifecycle controller
    stream_controller: Option<StreamController>,
    running_commands: HashMap<String, RunningCommand>,
    suppressed_exec_calls: HashSet<String>,
    last_unified_wait: Option<UnifiedExecWaitState>,
    task_complete_pending: bool,
    semantic_warmup_started: bool,
    semantic_watch: Option<tokio::process::Child>,
    mcp_startup_status: Option<HashMap<String, McpStartupStatus>>,
    // Queue of interruptive UI events deferred during an active write cycle
    interrupts: InterruptManager,
    // Accumulates the current reasoning block text to extract a header
    reasoning_buffer: String,
    // Accumulates full reasoning content for transcript-only recording
    full_reasoning_buffer: String,
    // Current status header shown in the status indicator.
    current_status_header: String,
    // Previous status header to restore after a transient stream retry.
    retry_status_header: Option<String>,
    conversation_id: Option<ConversationId>,
    frame_requester: FrameRequester,
    // Whether to include the initial welcome banner on session configured
    show_welcome_banner: bool,
    // When resuming an existing session (selected via resume picker), avoid an
    // immediate redraw on SessionConfigured to prevent a gratuitous UI flicker.
    suppress_session_configured_redraw: bool,
    // User messages queued while a turn is in progress
    queued_user_messages: VecDeque<UserMessage>,
    // Pending notification to show when unfocused on next Draw
    pending_notification: Option<Notification>,
    // Simple review mode flag; used to adjust layout and banners.
    is_review_mode: bool,
    // Snapshot of token usage to restore after review mode exits.
    pre_review_token_info: Option<Option<TokenUsageInfo>>,
    // Whether to add a final message separator after the last message
    needs_final_message_separator: bool,
    // Whether plan-first workflow is enabled.
    plan_mode_enabled: bool,
    // Pending workflow data when plan mode captures a task.
    plan_workflow: Option<PlanWorkflow>,
    // Tracks whether the next submission should be treated as plan feedback.
    plan_feedback_pending: bool,
    // Placeholder shown when plan mode is off.
    default_placeholder: String,

    last_rendered_width: std::cell::Cell<Option<usize>>,
    // Feedback sink for /feedback
    feedback: codex_feedback::CodexFeedback,
    // Current session rollout path (if known)
    current_rollout_path: Option<PathBuf>,
    recent_checkpoints: VecDeque<String>,
}

#[derive(Clone)]
struct UserMessage {
    text: String,
    display_text: Option<String>,
    image_paths: Vec<PathBuf>,
}

impl From<String> for UserMessage {
    fn from(text: String) -> Self {
        Self {
            text,
            display_text: None,
            image_paths: Vec::new(),
        }
    }
}

impl From<&str> for UserMessage {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_string(),
            display_text: None,
            image_paths: Vec::new(),
        }
    }
}

impl UserMessage {
    fn visible_text(&self) -> &str {
        self.display_text.as_deref().unwrap_or(&self.text)
    }
}

enum LocalCommand {
    Checkpoint(String),
    Restore(String),
    List,
}

impl LocalCommand {
    fn parse(text: &str) -> Option<Self> {
        fn tail_segment<'a>(text: &'a str, command: &str) -> Option<&'a str> {
            let rest = text.strip_prefix(command)?;
            if rest.is_empty() {
                Some(rest)
            } else if rest.chars().next().is_some_and(char::is_whitespace) {
                Some(rest.trim_start())
            } else {
                None
            }
        }

        if let Some(rest) = tail_segment(text, "/checkpoint") {
            return Some(LocalCommand::Checkpoint(rest.to_string()));
        }
        if let Some(rest) = tail_segment(text, "/restore") {
            return Some(LocalCommand::Restore(rest.to_string()));
        }
        if text == "/checkpoints" {
            return Some(LocalCommand::List);
        }
        None
    }

    fn name(&self) -> &'static str {
        match self {
            LocalCommand::Checkpoint(_) => "/checkpoint",
            LocalCommand::Restore(_) => "/restore",
            LocalCommand::List => "/checkpoints",
        }
    }
}

const RECENT_CHECKPOINT_LIMIT: usize = 3;

fn create_initial_user_message(text: String, image_paths: Vec<PathBuf>) -> Option<UserMessage> {
    if text.is_empty() && image_paths.is_empty() {
        None
    } else {
        Some(UserMessage {
            text,
            display_text: None,
            image_paths,
        })
    }
}

impl ChatWidget {
    fn welcome_snapshot(&self) -> WelcomeSnapshot {
        let (semantic_status, semantic_message) = self.bottom_pane.semantic_status_snapshot();
        WelcomeSnapshot {
            semantic_status,
            semantic_message,
            checkpoint_names: self.recent_checkpoints.iter().cloned().collect(),
        }
    }

    fn push_recent_checkpoint(&mut self, name: &str) {
        if name.trim().is_empty() {
            return;
        }
        if let Some(pos) = self
            .recent_checkpoints
            .iter()
            .position(|existing| existing == name)
        {
            self.recent_checkpoints.remove(pos);
        }
        self.recent_checkpoints.push_front(name.to_string());
        while self.recent_checkpoints.len() > RECENT_CHECKPOINT_LIMIT {
            self.recent_checkpoints.pop_back();
        }
    }

    fn replace_recent_checkpoints(&mut self, entries: &[CheckpointEntry]) {
        self.recent_checkpoints.clear();
        for entry in entries.iter().take(RECENT_CHECKPOINT_LIMIT) {
            self.recent_checkpoints.push_back(entry.name.clone());
        }
    }

    fn flush_answer_stream_with_separator(&mut self) {
        if let Some(mut controller) = self.stream_controller.take()
            && let Some(cell) = controller.finalize()
        {
            self.add_boxed_history(cell);
        }
    }

    fn set_status_header(&mut self, header: String) {
        self.current_status_header = header.clone();
        self.bottom_pane.update_status_header(header);
    }

    // --- Small event handlers ---
    fn on_session_configured(&mut self, event: codex_core::protocol::SessionConfiguredEvent) {
        self.bottom_pane
            .set_history_metadata(event.history_log_id, event.history_entry_count);
        self.conversation_id = Some(event.session_id);
        self.current_rollout_path = Some(event.rollout_path.clone());
        let initial_messages = event.initial_messages.clone();
        let model_for_header = event.model.clone();
        self.session_header.set_model(&model_for_header);
        let welcome_snapshot = self.welcome_snapshot();
        self.add_to_history(history_cell::new_session_info(
            &self.config,
            welcome_snapshot,
            event,
            self.show_welcome_banner,
        ));
        if let Some(messages) = initial_messages {
            self.replay_initial_messages(messages);
        }
        // Ask codex-core to enumerate custom prompts for this session.
        self.submit_op(Op::ListCustomPrompts);
        if let Some(user_message) = self.initial_user_message.take() {
            self.submit_user_message(user_message);
        }
        if !self.suppress_session_configured_redraw {
            self.request_redraw();
        }
    }

    pub(crate) fn open_feedback_note(
        &mut self,
        category: crate::app_event::FeedbackCategory,
        include_logs: bool,
    ) {
        // Build a fresh snapshot at the time of opening the note overlay.
        let snapshot = self.feedback.snapshot(self.conversation_id);
        let rollout = if include_logs {
            self.current_rollout_path.clone()
        } else {
            None
        };
        let view = crate::bottom_pane::FeedbackNoteView::new(
            category,
            snapshot,
            rollout,
            self.app_event_tx.clone(),
            include_logs,
        );
        self.bottom_pane.show_view(Box::new(view));
        self.request_redraw();
    }

    pub(crate) fn open_feedback_consent(&mut self, category: crate::app_event::FeedbackCategory) {
        let params = crate::bottom_pane::feedback_upload_consent_params(
            self.app_event_tx.clone(),
            category,
            self.current_rollout_path.clone(),
        );
        self.bottom_pane.show_selection_view(params);
        self.request_redraw();
    }

    fn on_agent_message(&mut self, message: String) {
        // If we have a stream_controller, then the final agent message is redundant and will be a
        // duplicate of what has already been streamed.
        if self.stream_controller.is_none() {
            self.handle_streaming_delta(message);
        }
        self.flush_answer_stream_with_separator();
        self.handle_stream_finished();
        self.request_redraw();
    }

    fn on_agent_message_delta(&mut self, delta: String) {
        self.handle_streaming_delta(delta);
    }

    fn on_agent_reasoning_delta(&mut self, delta: String) {
        // For reasoning deltas, do not stream to history. Accumulate the
        // current reasoning block and extract the first bold element
        // (between **/**) as the chunk header. Show this header as status.
        self.reasoning_buffer.push_str(&delta);

        if let Some(header) = extract_first_bold(&self.reasoning_buffer) {
            // Update the shimmer header to the extracted reasoning chunk header.
            self.set_status_header(header);
        } else {
            // Fallback while we don't yet have a bold header: leave existing header as-is.
        }
        self.request_redraw();
    }

    fn on_agent_reasoning_final(&mut self) {
        // At the end of a reasoning block, record transcript-only content.
        self.full_reasoning_buffer.push_str(&self.reasoning_buffer);
        if !self.full_reasoning_buffer.is_empty() {
            let cell = history_cell::new_reasoning_summary_block(
                self.full_reasoning_buffer.clone(),
                &self.config,
            );
            self.add_boxed_history(cell);
        }
        self.reasoning_buffer.clear();
        self.full_reasoning_buffer.clear();
        self.request_redraw();
    }

    fn on_reasoning_section_break(&mut self) {
        // Start a new reasoning block for header extraction and accumulate transcript.
        self.full_reasoning_buffer.push_str(&self.reasoning_buffer);
        self.full_reasoning_buffer.push_str("\n\n");
        self.reasoning_buffer.clear();
    }

    // Raw reasoning uses the same flow as summarized reasoning

    fn on_task_started(&mut self) {
        self.bottom_pane.clear_ctrl_c_quit_hint();
        self.bottom_pane.set_task_running(true);
        self.retry_status_header = None;
        self.bottom_pane.set_interrupt_hint_visible(true);
        self.set_status_header(String::from("Working"));
        self.full_reasoning_buffer.clear();
        self.reasoning_buffer.clear();
        self.request_redraw();
    }

    fn on_task_complete(&mut self, last_agent_message: Option<String>) {
        // If a stream is currently active, finalize it.
        self.flush_answer_stream_with_separator();
        // Mark task stopped and request redraw now that all content is in history.
        self.bottom_pane.set_task_running(false);
        self.running_commands.clear();
        self.suppressed_exec_calls.clear();
        self.last_unified_wait = None;
        self.request_redraw();

        // If there is a queued user message, send exactly one now to begin the next turn.
        self.maybe_send_next_queued_input();
        // Emit a notification when the turn completes (suppressed if focused).
        self.notify(Notification::AgentTurnComplete {
            response: last_agent_message.unwrap_or_default(),
        });

        self.maybe_show_pending_rate_limit_prompt();
    }

    pub(crate) fn set_token_info(&mut self, info: Option<TokenUsageInfo>) {
        match info {
            Some(info) => self.apply_token_info(info),
            None => {
                self.bottom_pane.set_context_window_percent(None);
                self.token_info = None;
            }
        }
    }

    fn apply_token_info(&mut self, info: TokenUsageInfo) {
        let percent = self.context_remaining_percent(&info);
        self.bottom_pane.set_context_window_percent(percent);
        self.token_info = Some(info);
    }

    fn context_remaining_percent(&self, info: &TokenUsageInfo) -> Option<i64> {
        info.model_context_window
            .or(self.config.model_context_window)
            .map(|window| {
                info.last_token_usage
                    .percent_of_context_window_remaining(window)
            })
    }

    fn restore_pre_review_token_info(&mut self) {
        if let Some(saved) = self.pre_review_token_info.take() {
            match saved {
                Some(info) => self.apply_token_info(info),
                None => {
                    self.bottom_pane.set_context_window_percent(None);
                    self.token_info = None;
                }
            }
        }
    }

    pub(crate) fn on_rate_limit_snapshot(&mut self, snapshot: Option<RateLimitSnapshot>) {
        if let Some(snapshot) = snapshot {
            let warnings = self.rate_limit_warnings.take_warnings(
                snapshot
                    .secondary
                    .as_ref()
                    .map(|window| window.used_percent),
                snapshot
                    .secondary
                    .as_ref()
                    .and_then(|window| window.window_minutes),
                snapshot.primary.as_ref().map(|window| window.used_percent),
                snapshot
                    .primary
                    .as_ref()
                    .and_then(|window| window.window_minutes),
            );

            let high_usage = snapshot
                .secondary
                .as_ref()
                .map(|w| w.used_percent >= RATE_LIMIT_SWITCH_PROMPT_THRESHOLD)
                .unwrap_or(false)
                || snapshot
                    .primary
                    .as_ref()
                    .map(|w| w.used_percent >= RATE_LIMIT_SWITCH_PROMPT_THRESHOLD)
                    .unwrap_or(false);

            if high_usage
                && !self.rate_limit_switch_prompt_hidden()
                && self.config.model != NUDGE_MODEL_SLUG
                && !matches!(
                    self.rate_limit_switch_prompt,
                    RateLimitSwitchPromptState::Shown
                )
            {
                self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Pending;
            }

            let display = crate::status::rate_limit_snapshot_display(&snapshot, Local::now());
            self.rate_limit_snapshot = Some(display);

            if !warnings.is_empty() {
                for warning in warnings {
                    self.add_to_history(history_cell::new_warning_event(warning));
                }
                self.request_redraw();
            }
        } else {
            self.rate_limit_snapshot = None;
        }
        self.refresh_rate_limit_footer_summary();
    }
    fn refresh_rate_limit_footer_summary(&mut self) {
        if self.config.show_rate_limits_in_footer {
            let summary = self
                .rate_limit_snapshot
                .as_ref()
                .and_then(rate_limit_footer_summary);
            self.bottom_pane.set_rate_limit_summary(summary);
        } else {
            self.bottom_pane.set_rate_limit_summary(None);
        }
    }

    /// Finalize any active exec as failed and stop/clear running UI state.
    fn finalize_turn(&mut self) {
        // Ensure any spinner is replaced by a red ✗ and flushed into history.
        self.finalize_active_cell_as_failed();
        // Reset running state and clear streaming buffers.
        self.bottom_pane.set_task_running(false);
        self.running_commands.clear();
        self.suppressed_exec_calls.clear();
        self.last_unified_wait = None;
        self.stream_controller = None;
        self.maybe_show_pending_rate_limit_prompt();
    }

    fn on_error(&mut self, message: String) {
        self.finalize_turn();
        self.add_to_history(history_cell::new_error_event(message));
        self.request_redraw();

        // After an error ends the turn, try sending the next queued input.
        self.maybe_send_next_queued_input();
    }

    fn on_warning(&mut self, message: impl Into<String>) {
        self.add_to_history(history_cell::new_warning_event(message.into()));
        self.request_redraw();
    }

    fn on_mcp_startup_update(&mut self, ev: McpStartupUpdateEvent) {
        let mut status = self.mcp_startup_status.take().unwrap_or_default();
        if let McpStartupStatus::Failed { error } = &ev.status {
            self.on_warning(error);
        }
        status.insert(ev.server, ev.status);
        self.mcp_startup_status = Some(status);
        self.bottom_pane.set_task_running(true);
        if let Some(current) = &self.mcp_startup_status {
            let total = current.len();
            let mut starting: Vec<_> = current
                .iter()
                .filter_map(|(name, state)| {
                    if matches!(state, McpStartupStatus::Starting) {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect();
            starting.sort();
            if let Some(first) = starting.first() {
                let completed = total.saturating_sub(starting.len());
                let max_to_show = 3;
                let mut to_show: Vec<String> = starting
                    .iter()
                    .take(max_to_show)
                    .map(ToString::to_string)
                    .collect();
                if starting.len() > max_to_show {
                    to_show.push("…".to_string());
                }
                let header = if total > 1 {
                    format!(
                        "Starting MCP servers ({completed}/{total}): {}",
                        to_show.join(", ")
                    )
                } else {
                    format!("Booting MCP server: {first}")
                };
                self.set_status_header(header);
            }
        }
        self.request_redraw();
    }

    fn on_mcp_startup_complete(&mut self, ev: McpStartupCompleteEvent) {
        let mut parts = Vec::new();
        if !ev.failed.is_empty() {
            let failed_servers: Vec<_> = ev.failed.iter().map(|f| f.server.clone()).collect();
            parts.push(format!("failed: {}", failed_servers.join(", ")));
        }
        if !ev.cancelled.is_empty() {
            self.on_warning(format!(
                "MCP startup interrupted. The following servers were not initialized: {}",
                ev.cancelled.join(", ")
            ));
        }
        if !parts.is_empty() {
            self.on_warning(format!("MCP startup incomplete ({})", parts.join("; ")));
        }

        self.mcp_startup_status = None;
        self.bottom_pane.set_task_running(false);
        self.maybe_send_next_queued_input();
        self.request_redraw();
    }

    /// Handle a turn aborted due to user interrupt (Esc).
    /// When there are queued user messages, restore them into the composer
    /// separated by newlines rather than auto‑submitting the next one.
    fn on_interrupted_turn(&mut self, reason: TurnAbortReason) {
        // Finalize, log a gentle prompt, and clear running state.
        self.finalize_turn();

        if reason != TurnAbortReason::ReviewEnded {
            self.add_to_history(history_cell::new_error_event(
                "Conversation interrupted - tell the model what to do differently. Something went wrong? Hit `/feedback` to report the issue.".to_owned(),
            ));
        }

        // If any messages were queued during the task, restore them into the composer.
        if !self.queued_user_messages.is_empty() {
            let queued_text = self
                .queued_user_messages
                .iter()
                .map(|m| m.text.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let existing_text = self.bottom_pane.composer_text();
            let combined = if existing_text.is_empty() {
                queued_text
            } else if queued_text.is_empty() {
                existing_text
            } else {
                format!("{queued_text}\n{existing_text}")
            };
            self.bottom_pane.set_composer_text(combined);
            // Clear the queue and update the status indicator list.
            self.queued_user_messages.clear();
            self.refresh_queued_user_messages();
        }

        self.request_redraw();
    }

    fn on_plan_update(&mut self, update: UpdatePlanArgs) {
        if let Some(workflow) = self.plan_workflow.as_mut() {
            workflow.last_plan = Some(update.clone());
            if matches!(workflow.status, PlanWorkflowStatus::AwaitingPlan) {
                workflow.status = PlanWorkflowStatus::AwaitingApproval;
                self.submit_op(Op::Interrupt);
                self.open_plan_review_popup();
                self.add_info_message(
                    "Plan ready — review it before Codex Kaioken proceeds.".to_string(),
                    None,
                );
            } else if workflow.awaiting_approval() {
                self.open_plan_review_popup();
            }
        }
        self.add_to_history(history_cell::new_plan_update(update));
    }

    fn on_exec_approval_request(&mut self, id: String, ev: ExecApprovalRequestEvent) {
        let id2 = id.clone();
        let ev2 = ev.clone();
        self.defer_or_handle(
            |q| q.push_exec_approval(id, ev),
            |s| s.handle_exec_approval_now(id2, ev2),
        );
    }

    fn on_apply_patch_approval_request(&mut self, id: String, ev: ApplyPatchApprovalRequestEvent) {
        let id2 = id.clone();
        let ev2 = ev.clone();
        self.defer_or_handle(
            |q| q.push_apply_patch_approval(id, ev),
            |s| s.handle_apply_patch_approval_now(id2, ev2),
        );
    }

    fn on_elicitation_request(&mut self, ev: ElicitationRequestEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(
            |q| q.push_elicitation(ev),
            |s| s.handle_elicitation_request_now(ev2),
        );
    }

    fn on_exec_command_begin(&mut self, ev: ExecCommandBeginEvent) {
        self.flush_answer_stream_with_separator();
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_exec_begin(ev), |s| s.handle_exec_begin_now(ev2));
    }

    fn on_exec_command_output_delta(
        &mut self,
        _ev: codex_core::protocol::ExecCommandOutputDeltaEvent,
    ) {
        // TODO: Handle streaming exec output if/when implemented
    }

    fn on_patch_apply_begin(&mut self, event: PatchApplyBeginEvent) {
        self.add_to_history(history_cell::new_patch_event(
            event.changes,
            &self.config.cwd,
        ));
    }

    fn on_view_image_tool_call(&mut self, event: ViewImageToolCallEvent) {
        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_view_image_tool_call(
            event.path,
            &self.config.cwd,
        ));
        self.request_redraw();
    }

    fn on_patch_apply_end(&mut self, event: codex_core::protocol::PatchApplyEndEvent) {
        let ev2 = event.clone();
        self.defer_or_handle(
            |q| q.push_patch_end(event),
            |s| s.handle_patch_apply_end_now(ev2),
        );
    }

    fn on_exec_command_end(&mut self, ev: ExecCommandEndEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_exec_end(ev), |s| s.handle_exec_end_now(ev2));
    }

    fn on_mcp_tool_call_begin(&mut self, ev: McpToolCallBeginEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_mcp_begin(ev), |s| s.handle_mcp_begin_now(ev2));
    }

    fn on_mcp_tool_call_end(&mut self, ev: McpToolCallEndEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_mcp_end(ev), |s| s.handle_mcp_end_now(ev2));
    }

    fn on_subagent_task_update(&mut self, ev: SubagentTaskUpdateEvent) {
        let needs_new_cell = self
            .active_cell
            .as_ref()
            .and_then(|cell| cell.as_any().downcast_ref::<SubagentTasksCell>())
            .map(|cell| cell.call_id != ev.call_id)
            .unwrap_or(true);

        if needs_new_cell {
            self.flush_active_cell();
            self.active_cell = Some(Box::new(SubagentTasksCell::new(
                ev.call_id.clone(),
                self.config.animations,
            )));
        }

        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<SubagentTasksCell>())
        {
            cell.update_task(ev.task, ev.status, ev.summary, None);
            if !cell.has_running() {
                self.flush_active_cell();
            } else {
                self.request_redraw();
            }
        }
    }

    fn on_subagent_task_log(&mut self, ev: SubagentTaskLogEvent) {
        let needs_new_cell = self
            .active_cell
            .as_ref()
            .and_then(|cell| cell.as_any().downcast_ref::<SubagentTasksCell>())
            .map(|cell| cell.call_id != ev.call_id)
            .unwrap_or(true);

        if needs_new_cell {
            self.flush_active_cell();
            self.active_cell = Some(Box::new(SubagentTasksCell::new(
                ev.call_id.clone(),
                self.config.animations,
            )));
        }

        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<SubagentTasksCell>())
        {
            let agent_label = ev
                .agent_index
                .and_then(|idx| usize::try_from(idx).ok())
                .map(|idx| format!("Agent {} — {}", idx + 1, ev.task));
            cell.append_log(ev.task, ev.line, agent_label);
            self.request_redraw();
        }
    }

    fn on_subagent_history_item(&mut self, ev: SubagentHistoryItemEvent) {
        let needs_new_cell = self
            .active_cell
            .as_ref()
            .and_then(|cell| cell.as_any().downcast_ref::<SubagentTasksCell>())
            .map(|cell| cell.call_id != ev.call_id)
            .unwrap_or(true);

        if needs_new_cell {
            self.flush_active_cell();
            self.active_cell = Some(Box::new(SubagentTasksCell::new(
                ev.call_id.clone(),
                self.config.animations,
            )));
        }

        let agent_label = ev
            .agent_index
            .and_then(|idx| usize::try_from(idx).ok())
            .map(|idx| format!("Agent {} — {}", idx + 1, ev.task))
            .unwrap_or_else(|| ev.task.clone());

        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<SubagentTasksCell>())
        {
            for line in subagent_history_log_lines(&ev.event) {
                cell.append_log(ev.task.clone(), line, Some(agent_label.clone()));
            }
        }

        if build_subagent_history_cell(
            agent_label,
            &ev.event,
            &self.config.cwd,
            self.config.animations,
        )
        .is_some()
        {
            self.request_redraw();
        }
    }

    fn on_web_search_begin(&mut self, _ev: WebSearchBeginEvent) {
        self.flush_answer_stream_with_separator();
    }

    fn on_web_search_end(&mut self, ev: WebSearchEndEvent) {
        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_web_search_call(format!(
            "Searched: {}",
            ev.query
        )));
    }

    fn on_get_history_entry_response(
        &mut self,
        event: codex_core::protocol::GetHistoryEntryResponseEvent,
    ) {
        let codex_core::protocol::GetHistoryEntryResponseEvent {
            offset,
            log_id,
            entry,
        } = event;
        self.bottom_pane
            .on_history_entry_response(log_id, offset, entry.map(|e| e.text));
    }

    fn on_shutdown_complete(&mut self) {
        self.request_exit();
    }

    fn on_turn_diff(&mut self, unified_diff: String) {
        debug!("TurnDiffEvent: {unified_diff}");
    }

    fn on_deprecation_notice(&mut self, event: DeprecationNoticeEvent) {
        let DeprecationNoticeEvent { summary, details } = event;
        self.add_to_history(history_cell::new_deprecation_notice(summary, details));
        self.request_redraw();
    }

    fn on_background_event(&mut self, message: String) {
        debug!("BackgroundEvent: {message}");
        self.bottom_pane.ensure_status_indicator();
        self.bottom_pane.set_interrupt_hint_visible(true);
        self.set_status_header(message);
    }

    fn clear_status_indicator(&mut self) {
        self.bottom_pane.set_interrupt_hint_visible(false);
        self.bottom_pane.hide_status_indicator();
    }

    fn on_undo_started(&mut self, event: UndoStartedEvent) {
        self.bottom_pane.ensure_status_indicator();
        self.bottom_pane.set_interrupt_hint_visible(false);
        let message = event
            .message
            .unwrap_or_else(|| "Undo in progress...".to_string());
        self.set_status_header(message);
    }

    fn on_undo_completed(&mut self, event: UndoCompletedEvent) {
        let UndoCompletedEvent { success, message } = event;
        self.clear_status_indicator();
        let message = message.unwrap_or_else(|| {
            if success {
                "Undo completed successfully.".to_string()
            } else {
                "Undo failed.".to_string()
            }
        });
        if success {
            self.add_info_message(message, None);
        } else {
            self.add_error_message(message);
        }
    }

    fn on_checkpoint_created(&mut self, event: CheckpointCreatedEvent) {
        self.clear_status_indicator();
        let hint = Some(checkpoint_hint(&event.checkpoint));
        self.add_info_message(
            format!("Checkpoint `{}` saved.", event.checkpoint.name),
            hint,
        );
        self.push_recent_checkpoint(&event.checkpoint.name);
    }

    fn on_checkpoint_restored(&mut self, event: CheckpointRestoredEvent) {
        self.clear_status_indicator();
        let hint = Some(checkpoint_hint(&event.checkpoint));
        self.add_info_message(
            format!("Restored checkpoint `{}`.", event.checkpoint.name),
            hint,
        );
        self.push_recent_checkpoint(&event.checkpoint.name);
    }

    fn on_checkpoint_list(&mut self, event: CheckpointListEvent) {
        self.clear_status_indicator();
        self.add_to_history(history_cell::new_checkpoint_list(&event.checkpoints));
        self.replace_recent_checkpoints(&event.checkpoints);
    }

    fn on_checkpoint_error(&mut self, event: CheckpointErrorEvent) {
        let mut prefix = match event.action {
            CheckpointAction::Create => "Checkpoint save failed".to_string(),
            CheckpointAction::Restore => "Checkpoint restore failed".to_string(),
            CheckpointAction::List => "Checkpoint listing failed".to_string(),
        };
        if let Some(name) = event.name {
            prefix.push_str(&format!(" (`{name}`)"));
        }
        let message = format!("{prefix}: {}", event.message);
        self.clear_status_indicator();
        self.add_error_message(message);
    }

    fn on_stream_error(&mut self, message: String) {
        if self.retry_status_header.is_none() {
            self.retry_status_header = Some(self.current_status_header.clone());
        }
        self.set_status_header(message);
    }

    /// Periodic tick to commit at most one queued line to history with a small delay,
    /// animating the output.
    pub(crate) fn on_commit_tick(&mut self) {
        if let Some(controller) = self.stream_controller.as_mut() {
            let (cell, is_idle) = controller.on_commit_tick();
            if let Some(cell) = cell {
                self.bottom_pane.hide_status_indicator();
                self.add_boxed_history(cell);
            }
            if is_idle {
                self.app_event_tx.send(AppEvent::StopCommitAnimation);
            }
        }
    }

    fn flush_interrupt_queue(&mut self) {
        let mut mgr = std::mem::take(&mut self.interrupts);
        mgr.flush_all(self);
        self.interrupts = mgr;
    }

    #[inline]
    fn defer_or_handle(
        &mut self,
        push: impl FnOnce(&mut InterruptManager),
        handle: impl FnOnce(&mut Self),
    ) {
        // Preserve deterministic FIFO across queued interrupts: once anything
        // is queued due to an active write cycle, continue queueing until the
        // queue is flushed to avoid reordering (e.g., ExecEnd before ExecBegin).
        if self.stream_controller.is_some() || !self.interrupts.is_empty() {
            push(&mut self.interrupts);
        } else {
            handle(self);
        }
    }

    fn handle_stream_finished(&mut self) {
        if self.task_complete_pending {
            self.bottom_pane.hide_status_indicator();
            self.task_complete_pending = false;
        }
        // A completed stream indicates non-exec content was just inserted.
        self.flush_interrupt_queue();
    }

    #[inline]
    fn handle_streaming_delta(&mut self, delta: String) {
        // Before streaming agent content, flush any active exec cell group.
        self.flush_active_cell();

        if self.stream_controller.is_none() {
            if self.needs_final_message_separator {
                let elapsed_seconds = self
                    .bottom_pane
                    .status_widget()
                    .map(super::status_indicator_widget::StatusIndicatorWidget::elapsed_seconds);
                self.add_to_history(history_cell::FinalMessageSeparator::new(elapsed_seconds));
                self.needs_final_message_separator = false;
            }
            self.stream_controller = Some(StreamController::new(
                self.last_rendered_width.get().map(|w| w.saturating_sub(2)),
            ));
        }
        if let Some(controller) = self.stream_controller.as_mut()
            && controller.push(&delta)
        {
            self.app_event_tx.send(AppEvent::StartCommitAnimation);
        }
        self.request_redraw();
    }

    pub(crate) fn handle_exec_end_now(&mut self, ev: ExecCommandEndEvent) {
        let running = self.running_commands.remove(&ev.call_id);
        if self.suppressed_exec_calls.remove(&ev.call_id) {
            return;
        }
        let (command, parsed, source) = match running {
            Some(rc) => (rc.command, rc.parsed_cmd, rc.source),
            None => (
                vec![ev.call_id.clone()],
                Vec::new(),
                ExecCommandSource::Agent,
            ),
        };
        let is_unified_exec_interaction =
            matches!(source, ExecCommandSource::UnifiedExecInteraction);

        let needs_new = self
            .active_cell
            .as_ref()
            .map(|cell| cell.as_any().downcast_ref::<ExecCell>().is_none())
            .unwrap_or(true);
        if needs_new {
            self.flush_active_cell();
            self.active_cell = Some(Box::new(new_active_exec_command(
                ev.call_id.clone(),
                command,
                parsed,
                source,
                None,
                self.config.animations,
            )));
        }

        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
        {
            let output = if is_unified_exec_interaction {
                CommandOutput {
                    exit_code: ev.exit_code,
                    formatted_output: String::new(),
                    aggregated_output: String::new(),
                }
            } else {
                CommandOutput {
                    exit_code: ev.exit_code,
                    formatted_output: ev.formatted_output.clone(),
                    aggregated_output: ev.aggregated_output.clone(),
                }
            };
            cell.complete_call(&ev.call_id, output, ev.duration);
            if cell.should_flush() {
                self.flush_active_cell();
            }
        }
    }

    pub(crate) fn handle_patch_apply_end_now(
        &mut self,
        event: codex_core::protocol::PatchApplyEndEvent,
    ) {
        // If the patch was successful, just let the "Edited" block stand.
        // Otherwise, add a failure block.
        if !event.success {
            self.add_to_history(history_cell::new_patch_apply_failure(event.stderr));
        }
    }

    pub(crate) fn handle_exec_approval_now(&mut self, id: String, ev: ExecApprovalRequestEvent) {
        self.flush_answer_stream_with_separator();
        let command = shlex::try_join(ev.command.iter().map(String::as_str))
            .unwrap_or_else(|_| ev.command.join(" "));
        self.notify(Notification::ExecApprovalRequested { command });

        let request = ApprovalRequest::Exec {
            id,
            command: ev.command,
            reason: ev.reason,
            risk: ev.risk,
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
    }

    pub(crate) fn handle_apply_patch_approval_now(
        &mut self,
        id: String,
        ev: ApplyPatchApprovalRequestEvent,
    ) {
        self.flush_answer_stream_with_separator();

        let request = ApprovalRequest::ApplyPatch {
            id,
            reason: ev.reason,
            changes: ev.changes.clone(),
            cwd: self.config.cwd.clone(),
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
        self.notify(Notification::EditApprovalRequested {
            cwd: self.config.cwd.clone(),
            changes: ev.changes.keys().cloned().collect(),
        });
    }

    pub(crate) fn handle_elicitation_request_now(&mut self, ev: ElicitationRequestEvent) {
        self.flush_answer_stream_with_separator();

        self.notify(Notification::ElicitationRequested {
            server_name: ev.server_name.clone(),
        });

        let request = ApprovalRequest::McpElicitation {
            server_name: ev.server_name,
            request_id: ev.id,
            message: ev.message,
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
    }

    pub(crate) fn handle_exec_begin_now(&mut self, ev: ExecCommandBeginEvent) {
        // Ensure the status indicator is visible while the command runs.
        self.running_commands.insert(
            ev.call_id.clone(),
            RunningCommand {
                command: ev.command.clone(),
                parsed_cmd: ev.parsed_cmd.clone(),
                source: ev.source,
            },
        );
        let is_wait_interaction = matches!(ev.source, ExecCommandSource::UnifiedExecInteraction)
            && ev
                .interaction_input
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true);
        let command_display = ev.command.join(" ");
        let should_suppress_unified_wait = is_wait_interaction
            && self
                .last_unified_wait
                .as_ref()
                .is_some_and(|wait| wait.is_duplicate(&command_display));
        if is_wait_interaction {
            self.last_unified_wait = Some(UnifiedExecWaitState::new(command_display));
        } else {
            self.last_unified_wait = None;
        }
        if should_suppress_unified_wait {
            self.suppressed_exec_calls.insert(ev.call_id);
            return;
        }
        let interaction_input = ev.interaction_input.clone();
        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
            && let Some(new_exec) = cell.with_added_call(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd.clone(),
                ev.source,
                interaction_input.clone(),
            )
        {
            *cell = new_exec;
        } else {
            self.flush_active_cell();

            self.active_cell = Some(Box::new(new_active_exec_command(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd,
                ev.source,
                interaction_input,
                self.config.animations,
            )));
        }

        self.request_redraw();
    }

    pub(crate) fn handle_mcp_begin_now(&mut self, ev: McpToolCallBeginEvent) {
        self.flush_answer_stream_with_separator();
        self.flush_active_cell();
        self.active_cell = Some(Box::new(history_cell::new_active_mcp_tool_call(
            ev.call_id,
            ev.invocation,
            self.config.animations,
        )));
        self.request_redraw();
    }
    pub(crate) fn handle_mcp_end_now(&mut self, ev: McpToolCallEndEvent) {
        self.flush_answer_stream_with_separator();

        let McpToolCallEndEvent {
            call_id,
            invocation,
            duration,
            result,
        } = ev;

        let extra_cell = match self
            .active_cell
            .as_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<McpToolCallCell>())
        {
            Some(cell) if cell.call_id() == call_id => cell.complete(duration, result),
            _ => {
                self.flush_active_cell();
                let mut cell = history_cell::new_active_mcp_tool_call(
                    call_id,
                    invocation,
                    self.config.animations,
                );
                let extra_cell = cell.complete(duration, result);
                self.active_cell = Some(Box::new(cell));
                extra_cell
            }
        };

        self.flush_active_cell();
        if let Some(extra) = extra_cell {
            self.add_boxed_history(extra);
        }
    }

    pub(crate) fn new(
        common: ChatWidgetInit,
        conversation_manager: Arc<ConversationManager>,
    ) -> Self {
        let ChatWidgetInit {
            config,
            frame_requester,
            app_event_tx,
            initial_prompt,
            initial_images,
            enhanced_keys_supported,
            auth_manager,
            feedback,
        } = common;
        let mut rng = rand::rng();
        let placeholder = EXAMPLE_PROMPTS[rng.random_range(0..EXAMPLE_PROMPTS.len())].to_string();
        let default_placeholder = placeholder.clone();
        let codex_op_tx = spawn_agent(config.clone(), app_event_tx.clone(), conversation_manager);

        let mut widget = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            codex_op_tx,
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
                animations_enabled: config.animations,
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            session_header: SessionHeader::new(config.model),
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            token_info: None,
            rate_limit_snapshot: None,
            rate_limit_warnings: RateLimitWarningState::default(),
            rate_limit_switch_prompt: RateLimitSwitchPromptState::default(),
            rate_limit_poller: None,
            stream_controller: None,
            running_commands: HashMap::new(),
            suppressed_exec_calls: HashSet::new(),
            last_unified_wait: None,
            task_complete_pending: false,
            semantic_warmup_started: false,
            semantic_watch: None,
            mcp_startup_status: None,
            interrupts: InterruptManager::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: String::from("Working"),
            retry_status_header: None,
            conversation_id: None,
            queued_user_messages: VecDeque::new(),
            show_welcome_banner: true,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            is_review_mode: false,
            pre_review_token_info: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            feedback,
            current_rollout_path: None,
            recent_checkpoints: VecDeque::new(),
            plan_mode_enabled: false,
            plan_workflow: None,
            plan_feedback_pending: false,
            default_placeholder,
        };

        widget.prefetch_rate_limits();
        widget.start_semantic_warmup();

        widget
    }

    /// Create a ChatWidget attached to an existing conversation (e.g., a fork).
    pub(crate) fn new_from_existing(
        common: ChatWidgetInit,
        conversation: std::sync::Arc<codex_core::CodexConversation>,
        session_configured: codex_core::protocol::SessionConfiguredEvent,
    ) -> Self {
        let ChatWidgetInit {
            config,
            frame_requester,
            app_event_tx,
            initial_prompt,
            initial_images,
            enhanced_keys_supported,
            auth_manager,
            feedback,
        } = common;
        let mut rng = rand::rng();
        let placeholder = EXAMPLE_PROMPTS[rng.random_range(0..EXAMPLE_PROMPTS.len())].to_string();
        let default_placeholder = placeholder.clone();

        let codex_op_tx =
            spawn_agent_from_existing(conversation, session_configured, app_event_tx.clone());

        let mut widget = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            codex_op_tx,
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
                animations_enabled: config.animations,
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            session_header: SessionHeader::new(config.model),
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            token_info: None,
            rate_limit_snapshot: None,
            rate_limit_warnings: RateLimitWarningState::default(),
            rate_limit_switch_prompt: RateLimitSwitchPromptState::default(),
            rate_limit_poller: None,
            stream_controller: None,
            running_commands: HashMap::new(),
            suppressed_exec_calls: HashSet::new(),
            last_unified_wait: None,
            task_complete_pending: false,
            semantic_warmup_started: false,
            semantic_watch: None,
            mcp_startup_status: None,
            interrupts: InterruptManager::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: String::from("Working"),
            retry_status_header: None,
            conversation_id: None,
            queued_user_messages: VecDeque::new(),
            show_welcome_banner: true,
            suppress_session_configured_redraw: true,
            pending_notification: None,
            is_review_mode: false,
            pre_review_token_info: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            feedback,
            current_rollout_path: None,
            recent_checkpoints: VecDeque::new(),
            plan_mode_enabled: false,
            plan_workflow: None,
            plan_feedback_pending: false,
            default_placeholder,
        };

        widget.prefetch_rate_limits();
        widget.start_semantic_warmup();

        widget
    }

    pub(crate) fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) && c.eq_ignore_ascii_case(&'c') => {
                self.on_ctrl_c();
                return;
            }
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) && c.eq_ignore_ascii_case(&'v') => {
                match paste_image_to_temp_png() {
                    Ok((path, info)) => {
                        self.attach_image(
                            path,
                            info.width,
                            info.height,
                            info.encoded_format.label(),
                        );
                    }
                    Err(err) => {
                        tracing::warn!("failed to paste image: {err}");
                        self.add_to_history(history_cell::new_error_event(format!(
                            "Failed to paste image: {err}",
                        )));
                    }
                }
                return;
            }
            other if other.kind == KeyEventKind::Press => {
                self.bottom_pane.clear_ctrl_c_quit_hint();
            }
            _ => {}
        }

        match key_event {
            KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::ALT,
                kind: KeyEventKind::Press,
                ..
            } if !self.queued_user_messages.is_empty() => {
                // Prefer the most recently queued item.
                if let Some(user_message) = self.queued_user_messages.pop_back() {
                    self.bottom_pane.set_composer_text(user_message.text);
                    self.refresh_queued_user_messages();
                    self.request_redraw();
                }
            }
            KeyEvent {
                code: KeyCode::BackTab,
                kind: KeyEventKind::Press,
                ..
            } => {
                self.toggle_plan_mode();
            }
            _ => {
                match self.bottom_pane.handle_key_event(key_event) {
                    InputResult::Submitted(text) => {
                        // If a task is running, queue the user input to be sent after the turn completes.
                        let user_message = UserMessage {
                            text,
                            display_text: None,
                            image_paths: self.bottom_pane.take_recent_submission_images(),
                        };
                        self.queue_user_message(user_message);
                    }
                    InputResult::Command(cmd) => {
                        self.dispatch_command(cmd);
                    }
                    InputResult::None => {}
                }
            }
        }
    }

    pub(crate) fn attach_image(
        &mut self,
        path: PathBuf,
        width: u32,
        height: u32,
        format_label: &str,
    ) {
        tracing::info!(
            "attach_image path={path:?} width={width} height={height} format={format_label}",
        );
        self.bottom_pane
            .attach_image(path, width, height, format_label);
        self.request_redraw();
    }

    fn dispatch_command(&mut self, cmd: SlashCommand) {
        if !cmd.available_during_task() && self.bottom_pane.is_task_running() {
            let message = format!(
                "'/{}' is disabled while a task is in progress.",
                cmd.command()
            );
            self.add_to_history(history_cell::new_error_event(message));
            self.request_redraw();
            return;
        }
        match cmd {
            SlashCommand::Feedback => {
                // Step 1: pick a category (UI built in feedback_view)
                let params =
                    crate::bottom_pane::feedback_selection_params(self.app_event_tx.clone());
                self.bottom_pane.show_selection_view(params);
                self.request_redraw();
            }
            SlashCommand::Plan => {
                self.on_plan_command();
            }
            SlashCommand::New => {
                self.app_event_tx.send(AppEvent::NewSession);
            }
            SlashCommand::Init => {
                let init_target = self.config.cwd.join(DEFAULT_PROJECT_DOC_FILENAME);
                if init_target.exists() {
                    let message = format!(
                        "{DEFAULT_PROJECT_DOC_FILENAME} already exists here. Skipping /init to avoid overwriting it."
                    );
                    self.add_info_message(message, None);
                    return;
                }
                const INIT_PROMPT: &str = include_str!("../prompt_for_init_command.md");
                self.submit_user_message(INIT_PROMPT.to_string().into());
            }
            SlashCommand::Compact => {
                self.clear_token_usage();
                self.app_event_tx.send(AppEvent::CodexOp(Op::Compact));
            }
            SlashCommand::Review => {
                self.open_review_popup();
            }
            SlashCommand::Model => {
                self.open_model_popup();
            }
            SlashCommand::Approvals => {
                self.open_approvals_popup();
            }
            SlashCommand::Settings => {
                self.open_settings_popup();
            }
            SlashCommand::Quit | SlashCommand::Exit => {
                self.request_exit();
            }
            SlashCommand::Logout => {
                if let Err(e) = codex_core::auth::logout(
                    &self.config.codex_home,
                    self.config.cli_auth_credentials_store_mode,
                ) {
                    tracing::error!("failed to logout: {e}");
                }
                self.request_exit();
            }
            SlashCommand::Undo => {
                self.app_event_tx.send(AppEvent::CodexOp(Op::Undo));
            }
            SlashCommand::Checkpoint => {
                self.insert_str("/checkpoint ");
                self.request_redraw();
            }
            SlashCommand::RestoreCheckpoint => {
                self.insert_str("/restore ");
                self.request_redraw();
            }
            SlashCommand::ListCheckpoints => {
                self.app_event_tx
                    .send(AppEvent::CodexOp(Op::ListCheckpoints));
            }
            SlashCommand::Diff => {
                self.add_diff_in_progress();
                let tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    let text = match get_git_diff().await {
                        Ok((is_git_repo, diff_text)) => {
                            if is_git_repo {
                                diff_text
                            } else {
                                "`/diff` — _not inside a git repository_".to_string()
                            }
                        }
                        Err(e) => format!("Failed to compute diff: {e}"),
                    };
                    tx.send(AppEvent::DiffResult(text));
                });
            }
            SlashCommand::Mention => {
                self.insert_str("@");
            }
            SlashCommand::Status => {
                self.add_status_output();
            }
            SlashCommand::Mcp => {
                self.add_mcp_output();
            }
            SlashCommand::Rollout => {
                if let Some(path) = self.rollout_path() {
                    self.add_info_message(
                        format!("Current rollout path: {}", path.display()),
                        None,
                    );
                } else {
                    self.add_info_message("Rollout path is not available yet.".to_string(), None);
                }
            }
            SlashCommand::TestApproval => {
                use codex_core::protocol::EventMsg;
                use std::collections::HashMap;

                use codex_core::protocol::ApplyPatchApprovalRequestEvent;
                use codex_core::protocol::FileChange;

                self.app_event_tx.send(AppEvent::CodexEvent(Event {
                    id: "1".to_string(),
                    // msg: EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
                    //     call_id: "1".to_string(),
                    //     command: vec!["git".into(), "apply".into()],
                    //     cwd: self.config.cwd.clone(),
                    //     reason: Some("test".to_string()),
                    // }),
                    msg: EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
                        call_id: "1".to_string(),
                        turn_id: "turn-1".to_string(),
                        changes: HashMap::from([
                            (
                                PathBuf::from("/tmp/test.txt"),
                                FileChange::Add {
                                    content: "test".to_string(),
                                },
                            ),
                            (
                                PathBuf::from("/tmp/test2.txt"),
                                FileChange::Update {
                                    unified_diff: "+test\n-test2".to_string(),
                                    move_path: None,
                                },
                            ),
                        ]),
                        reason: None,
                        grant_root: Some(PathBuf::from("/tmp")),
                    }),
                }));
            }
        }
    }

    fn on_plan_command(&mut self) {
        if let Some(workflow) = self.plan_workflow.as_ref()
            && workflow.awaiting_approval()
        {
            self.open_plan_review_popup();
        } else {
            let enable = !self.plan_mode_enabled;
            self.set_plan_mode(enable);
        }
    }

    fn toggle_plan_mode(&mut self) {
        let enable = !self.plan_mode_enabled;
        self.set_plan_mode(enable);
    }

    fn set_plan_mode(&mut self, enabled: bool) {
        if self.plan_mode_enabled == enabled {
            if enabled && self.plan_review_pending() {
                self.open_plan_review_popup();
            }
            return;
        }

        self.plan_mode_enabled = enabled;
        self.bottom_pane.set_plan_mode_enabled(enabled);
        if enabled {
            self.bottom_pane
                .set_placeholder_text(PLAN_MODE_PLACEHOLDER.to_string());
        } else {
            self.bottom_pane
                .set_placeholder_text(self.default_placeholder.clone());
            self.cancel_plan_workflow();
        }
    }

    fn handle_plan_submission(&mut self, user_message: UserMessage) -> bool {
        if self.plan_feedback_pending {
            self.handle_plan_feedback_submission(user_message);
            return true;
        }
        if !self.plan_mode_enabled {
            return false;
        }
        if self.plan_workflow.is_none() {
            self.start_plan_request(user_message);
            return true;
        }
        if self.plan_review_pending() {
            self.add_info_message(
                "Review or cancel the current plan before starting another request.".to_string(),
                None,
            );
            return true;
        }
        self.add_info_message(
            "Codex Kaioken is still drafting your plan. Wait for the plan to finish or disable plan mode."
                .to_string(),
            None,
        );
        true
    }

    fn plan_review_pending(&self) -> bool {
        self.plan_workflow
            .as_ref()
            .is_some_and(PlanWorkflow::awaiting_approval)
    }

    fn start_plan_request(&mut self, user_message: UserMessage) {
        let original_request = user_message.clone();
        let submission = self.decorate_plan_submission(user_message);
        self.plan_workflow = Some(PlanWorkflow::new(original_request));
        self.plan_feedback_pending = false;
        self.bottom_pane
            .set_placeholder_text(PLAN_MODE_PLACEHOLDER.to_string());
        self.submit_user_message(submission);
        self.add_info_message(
            "Drafting a plan… Codex Kaioken will wait for approval before making changes."
                .to_string(),
            None,
        );
    }

    fn decorate_plan_submission(&self, mut user_message: UserMessage) -> UserMessage {
        let original = user_message.visible_text().to_string();
        let mut submission = String::new();
        submission.push_str(&self.plan_prompt_instructions());
        submission.push_str("\n\nGoal:\n");
        if original.trim().is_empty() {
            submission.push_str("(no detailed goal provided)\n");
        } else {
            submission.push_str(original.trim());
            submission.push('\n');
        }
        user_message.text = submission;
        user_message.display_text = Some(original);
        user_message
    }

    fn plan_prompt_instructions(&self) -> String {
        let mut parts = vec![
            "You are operating in Kaioken's plan-first workflow. Your next response must use the `update_plan` tool only.",
        ];
        match self.config.plan_detail {
            PlanDetailPreference::Auto => parts.push(
                "Decide on the level of detail: keep the plan to roughly 3–4 steps for a narrow change, or expand to 6–10 focused steps when the request spans multiple files or systems.",
            ),
            PlanDetailPreference::Coarse => parts.push(
                "Produce a concise, high-level plan of roughly 3–4 major steps that highlight the main actions.",
            ),
            PlanDetailPreference::Detailed => parts.push(
                "Produce a detailed plan of roughly 6–10 steps that cover implementation, validation, and follow-up tasks.",
            ),
        }
        parts.push(
            "Each step must mention the relevant files/modules or commands plus how you will verify the change (tests, linters, manual QA). Start every step with status `pending` unless progress is already made.",
        );
        parts.join(" ")
    }

    fn open_plan_review_popup(&mut self) {
        if let Some(workflow) = self.plan_workflow.clone()
            && let Some(plan) = workflow.last_plan.clone()
        {
            let goal = workflow.request_preview();
            let feedback = workflow.extra_feedback;
            let view = PlanReviewView::new(self.app_event_tx.clone(), goal, feedback, plan);
            self.bottom_pane.show_view(Box::new(view));
            self.request_redraw();
        }
    }

    pub(crate) fn execute_plan_request(&mut self) {
        let Some(workflow) = self.plan_workflow.take() else {
            self.add_info_message("No pending plan to execute.".to_string(), None);
            return;
        };
        self.plan_feedback_pending = false;
        let mut request = workflow.request;
        let mut final_text = request.text;
        if !workflow.extra_feedback.is_empty() {
            final_text.push_str("\n\nAdditional guidance from planning:\n");
            for entry in workflow.extra_feedback {
                final_text.push_str("• ");
                final_text.push_str(entry.trim());
                final_text.push('\n');
            }
        }
        final_text.push_str("\n\nPlan approved — execute these steps.");
        request.text = final_text;
        self.submit_user_message(request);
        let placeholder = if self.plan_mode_enabled {
            PLAN_MODE_PLACEHOLDER.to_string()
        } else {
            self.default_placeholder.clone()
        };
        self.bottom_pane.set_placeholder_text(placeholder);
        self.add_info_message(
            "Plan approved. Executing the requested work...".to_string(),
            None,
        );
    }

    pub(crate) fn prepare_plan_feedback(&mut self) {
        if !self.plan_review_pending() {
            self.add_info_message("No plan is awaiting feedback.".to_string(), None);
            return;
        }
        self.plan_feedback_pending = true;
        self.bottom_pane
            .set_placeholder_text(PLAN_FEEDBACK_PLACEHOLDER.to_string());
        self.bottom_pane.set_composer_text(String::new());
        self.request_redraw();
    }

    pub(crate) fn cancel_plan_workflow(&mut self) {
        if let Some(workflow) = self.plan_workflow.take() {
            let cancelling_plan = matches!(workflow.status, PlanWorkflowStatus::AwaitingPlan);
            self.plan_feedback_pending = false;
            let placeholder = if self.plan_mode_enabled {
                PLAN_MODE_PLACEHOLDER.to_string()
            } else {
                self.default_placeholder.clone()
            };
            self.bottom_pane.set_placeholder_text(placeholder);
            if cancelling_plan {
                self.submit_op(Op::Interrupt);
            }
            self.add_info_message("Canceled the pending plan workflow.".to_string(), None);
        }
    }

    fn handle_plan_feedback_submission(&mut self, user_message: UserMessage) {
        if !user_message.image_paths.is_empty() {
            self.add_to_history(history_cell::new_error_event(
                "Plan feedback does not support image attachments.".to_string(),
            ));
            return;
        }
        let trimmed = user_message.text.trim();
        if trimmed.is_empty() {
            self.add_to_history(history_cell::new_error_event(
                "Enter some feedback or press Esc to cancel.".to_string(),
            ));
            return;
        }
        let Some(workflow) = self.plan_workflow.as_mut() else {
            self.plan_feedback_pending = false;
            self.bottom_pane
                .set_placeholder_text(self.default_placeholder.clone());
            return;
        };
        workflow.extra_feedback.push(trimmed.to_string());
        workflow.status = PlanWorkflowStatus::AwaitingPlan;
        self.plan_feedback_pending = false;
        self.bottom_pane
            .set_placeholder_text(PLAN_MODE_PLACEHOLDER.to_string());
        let feedback_text =
            format!("Plan feedback:\n{trimmed}\n\nPlease revise the plan and wait for approval.");
        self.submit_user_message(UserMessage {
            text: feedback_text,
            display_text: None,
            image_paths: Vec::new(),
        });
        self.add_info_message("Sent feedback for plan refinement.".to_string(), None);
    }

    pub(crate) fn handle_paste(&mut self, text: String) {
        self.bottom_pane.handle_paste(text);
    }

    // Returns true if caller should skip rendering this frame (a future frame is scheduled).
    pub(crate) fn handle_paste_burst_tick(&mut self, frame_requester: FrameRequester) -> bool {
        if self.bottom_pane.flush_paste_burst_if_due() {
            // A paste just flushed; request an immediate redraw and skip this frame.
            self.request_redraw();
            true
        } else if self.bottom_pane.is_in_paste_burst() {
            // While capturing a burst, schedule a follow-up tick and skip this frame
            // to avoid redundant renders between ticks.
            frame_requester.schedule_frame_in(
                crate::bottom_pane::ChatComposer::recommended_paste_flush_delay(),
            );
            true
        } else {
            false
        }
    }

    fn flush_active_cell(&mut self) {
        if let Some(active) = self.active_cell.take() {
            self.needs_final_message_separator = true;
            self.app_event_tx.send(AppEvent::InsertHistoryCell(active));
        }
    }

    fn add_to_history(&mut self, cell: impl HistoryCell + 'static) {
        self.add_boxed_history(Box::new(cell));
    }

    fn add_boxed_history(&mut self, cell: Box<dyn HistoryCell>) {
        if !cell.display_lines(u16::MAX).is_empty() {
            // Only break exec grouping if the cell renders visible lines.
            self.flush_active_cell();
            self.needs_final_message_separator = true;
        }
        self.app_event_tx.send(AppEvent::InsertHistoryCell(cell));
    }

    fn queue_user_message(&mut self, user_message: UserMessage) {
        if self.handle_plan_submission(user_message.clone()) {
            return;
        }
        if self.bottom_pane.is_task_running() {
            self.queued_user_messages.push_back(user_message);
            self.refresh_queued_user_messages();
        } else {
            self.submit_user_message(user_message);
        }
    }

    fn submit_user_message(&mut self, user_message: UserMessage) {
        let UserMessage {
            text,
            display_text,
            image_paths,
        } = user_message;
        if text.is_empty() && image_paths.is_empty() {
            return;
        }

        let mut items: Vec<UserInput> = Vec::new();

        // Special-case: "!cmd" executes a local shell command instead of sending to the model.
        if let Some(stripped) = text.strip_prefix('!') {
            let cmd = stripped.trim();
            if cmd.is_empty() {
                self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_info_event(
                        USER_SHELL_COMMAND_HELP_TITLE.to_string(),
                        Some(USER_SHELL_COMMAND_HELP_HINT.to_string()),
                    ),
                )));
                return;
            }
            self.submit_op(Op::RunUserShellCommand {
                command: cmd.to_string(),
            });
            return;
        }

        if let Some(command) = LocalCommand::parse(&text) {
            if !image_paths.is_empty() {
                self.add_to_history(history_cell::new_error_event(
                    "Checkpoint commands do not support image attachments.".to_string(),
                ));
                return;
            }
            if matches!(
                command,
                LocalCommand::Checkpoint(_) | LocalCommand::Restore(_)
            ) && self.bottom_pane.is_task_running()
            {
                self.add_to_history(history_cell::new_error_event(format!(
                    "Finish the current task before running '{}'.",
                    command.name()
                )));
                return;
            }
            self.handle_local_command(command);
            return;
        }

        if !text.is_empty() {
            items.push(UserInput::Text { text: text.clone() });
        }

        for path in image_paths {
            items.push(UserInput::LocalImage { path });
        }

        self.codex_op_tx
            .send(Op::UserInput { items })
            .unwrap_or_else(|e| {
                tracing::error!("failed to send message: {e}");
            });

        // Persist the text to cross-session message history.
        let visible_text = display_text.unwrap_or_else(|| text.clone());

        if !visible_text.is_empty() {
            self.codex_op_tx
                .send(Op::AddToHistory {
                    text: visible_text.clone(),
                })
                .unwrap_or_else(|e| {
                    tracing::error!("failed to send AddHistory op: {e}");
                });
        }

        // Only show the text portion in conversation history.
        if !visible_text.is_empty() {
            self.add_to_history(history_cell::new_user_prompt(visible_text));
        }
        self.needs_final_message_separator = false;
    }

    fn handle_local_command(&mut self, command: LocalCommand) {
        match command {
            LocalCommand::Checkpoint(name) => {
                if name.trim().is_empty() {
                    self.add_to_history(history_cell::new_error_event(
                        "Provide a checkpoint name (e.g., `/checkpoint before-fix`).".to_string(),
                    ));
                    return;
                }
                self.submit_op(Op::CreateCheckpoint { name });
            }
            LocalCommand::Restore(name) => {
                if name.trim().is_empty() {
                    self.add_to_history(history_cell::new_error_event(
                        "Provide a checkpoint name to restore (e.g., `/restore before-fix`)."
                            .to_string(),
                    ));
                    return;
                }
                self.submit_op(Op::RestoreCheckpoint { name });
            }
            LocalCommand::List => {
                self.submit_op(Op::ListCheckpoints);
            }
        }
    }

    /// Replay a subset of initial events into the UI to seed the transcript when
    /// resuming an existing session. This approximates the live event flow and
    /// is intentionally conservative: only safe-to-replay items are rendered to
    /// avoid triggering side effects. Event ids are passed as `None` to
    /// distinguish replayed events from live ones.
    fn replay_initial_messages(&mut self, events: Vec<EventMsg>) {
        for msg in events {
            if matches!(msg, EventMsg::SessionConfigured(_)) {
                continue;
            }
            // `id: None` indicates a synthetic/fake id coming from replay.
            self.dispatch_event_msg(None, msg, true);
        }
    }

    pub(crate) fn handle_codex_event(&mut self, event: Event) {
        let Event { id, msg } = event;
        self.dispatch_event_msg(Some(id), msg, false);
    }

    /// Dispatch a protocol `EventMsg` to the appropriate handler.
    ///
    /// `id` is `Some` for live events and `None` for replayed events from
    /// `replay_initial_messages()`. Callers should treat `None` as a "fake" id
    /// that must not be used to correlate follow-up actions.
    fn dispatch_event_msg(&mut self, id: Option<String>, msg: EventMsg, from_replay: bool) {
        match msg {
            EventMsg::AgentMessageDelta(_)
            | EventMsg::AgentReasoningDelta(_)
            | EventMsg::ExecCommandOutputDelta(_) => {}
            _ => {
                tracing::trace!("handle_codex_event: {:?}", msg);
            }
        }

        match msg {
            EventMsg::SessionConfigured(e) => self.on_session_configured(e),
            EventMsg::AgentMessage(AgentMessageEvent { message }) => self.on_agent_message(message),
            EventMsg::AgentMessageDelta(AgentMessageDeltaEvent { delta }) => {
                self.on_agent_message_delta(delta)
            }
            EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent { delta })
            | EventMsg::AgentReasoningRawContentDelta(AgentReasoningRawContentDeltaEvent {
                delta,
            }) => self.on_agent_reasoning_delta(delta),
            EventMsg::AgentReasoning(AgentReasoningEvent { .. }) => self.on_agent_reasoning_final(),
            EventMsg::AgentReasoningRawContent(AgentReasoningRawContentEvent { text }) => {
                self.on_agent_reasoning_delta(text);
                self.on_agent_reasoning_final()
            }
            EventMsg::AgentReasoningSectionBreak(_) => self.on_reasoning_section_break(),
            EventMsg::TaskStarted(_) => self.on_task_started(),
            EventMsg::TaskComplete(TaskCompleteEvent { last_agent_message }) => {
                self.on_task_complete(last_agent_message)
            }
            EventMsg::TokenCount(ev) => {
                self.set_token_info(ev.info);
                self.on_rate_limit_snapshot(ev.rate_limits);
            }
            EventMsg::Warning(WarningEvent { message }) => self.on_warning(message),
            EventMsg::Error(ErrorEvent { message, .. }) => self.on_error(message),
            EventMsg::McpStartupUpdate(ev) => self.on_mcp_startup_update(ev),
            EventMsg::McpStartupComplete(ev) => self.on_mcp_startup_complete(ev),
            EventMsg::TurnAborted(ev) => match ev.reason {
                TurnAbortReason::Interrupted => {
                    self.on_interrupted_turn(ev.reason);
                }
                TurnAbortReason::Replaced => {
                    self.on_error("Turn aborted: replaced by a new task".to_owned())
                }
                TurnAbortReason::ReviewEnded => {
                    self.on_interrupted_turn(ev.reason);
                }
            },
            EventMsg::PlanUpdate(update) => self.on_plan_update(update),
            EventMsg::ExecApprovalRequest(ev) => {
                // For replayed events, synthesize an empty id (these should not occur).
                self.on_exec_approval_request(id.unwrap_or_default(), ev)
            }
            EventMsg::ApplyPatchApprovalRequest(ev) => {
                self.on_apply_patch_approval_request(id.unwrap_or_default(), ev)
            }
            EventMsg::ElicitationRequest(ev) => {
                self.on_elicitation_request(ev);
            }
            EventMsg::ExecCommandBegin(ev) => self.on_exec_command_begin(ev),
            EventMsg::ExecCommandOutputDelta(delta) => self.on_exec_command_output_delta(delta),
            EventMsg::PatchApplyBegin(ev) => self.on_patch_apply_begin(ev),
            EventMsg::PatchApplyEnd(ev) => self.on_patch_apply_end(ev),
            EventMsg::ExecCommandEnd(ev) => self.on_exec_command_end(ev),
            EventMsg::ViewImageToolCall(ev) => self.on_view_image_tool_call(ev),
            EventMsg::McpToolCallBegin(ev) => self.on_mcp_tool_call_begin(ev),
            EventMsg::McpToolCallEnd(ev) => self.on_mcp_tool_call_end(ev),
            EventMsg::SubagentTaskUpdate(ev) => self.on_subagent_task_update(ev),
            EventMsg::SubagentTaskLog(ev) => self.on_subagent_task_log(ev),
            EventMsg::SubagentHistoryItem(ev) => self.on_subagent_history_item(ev),
            EventMsg::WebSearchBegin(ev) => self.on_web_search_begin(ev),
            EventMsg::WebSearchEnd(ev) => self.on_web_search_end(ev),
            EventMsg::GetHistoryEntryResponse(ev) => self.on_get_history_entry_response(ev),
            EventMsg::McpListToolsResponse(ev) => self.on_list_mcp_tools(ev),
            EventMsg::ListCustomPromptsResponse(ev) => self.on_list_custom_prompts(ev),
            EventMsg::ShutdownComplete => self.on_shutdown_complete(),
            EventMsg::TurnDiff(TurnDiffEvent { unified_diff }) => self.on_turn_diff(unified_diff),
            EventMsg::DeprecationNotice(ev) => self.on_deprecation_notice(ev),
            EventMsg::BackgroundEvent(BackgroundEventEvent { message }) => {
                self.on_background_event(message)
            }
            EventMsg::UndoStarted(ev) => self.on_undo_started(ev),
            EventMsg::UndoCompleted(ev) => self.on_undo_completed(ev),
            EventMsg::CheckpointCreated(ev) => self.on_checkpoint_created(ev),
            EventMsg::CheckpointRestored(ev) => self.on_checkpoint_restored(ev),
            EventMsg::CheckpointList(ev) => self.on_checkpoint_list(ev),
            EventMsg::CheckpointError(ev) => self.on_checkpoint_error(ev),
            EventMsg::StreamError(StreamErrorEvent { message, .. }) => {
                self.on_stream_error(message)
            }
            EventMsg::UserMessage(ev) => {
                if from_replay {
                    self.on_user_message_event(ev);
                }
            }
            EventMsg::EnteredReviewMode(review_request) => {
                self.on_entered_review_mode(review_request)
            }
            EventMsg::ExitedReviewMode(review) => self.on_exited_review_mode(review),
            EventMsg::ContextCompacted(_) => self.on_agent_message("Context compacted".to_owned()),
            EventMsg::RawResponseItem(_)
            | EventMsg::ItemStarted(_)
            | EventMsg::ItemCompleted(_)
            | EventMsg::AgentMessageContentDelta(_)
            | EventMsg::ReasoningContentDelta(_)
            | EventMsg::ReasoningRawContentDelta(_) => {}
        }
    }

    fn on_entered_review_mode(&mut self, review: ReviewRequest) {
        // Enter review mode and emit a concise banner
        if self.pre_review_token_info.is_none() {
            self.pre_review_token_info = Some(self.token_info.clone());
        }
        self.is_review_mode = true;
        let banner = format!(">> Code review started: {} <<", review.user_facing_hint);
        self.add_to_history(history_cell::new_review_status_line(banner));
        self.request_redraw();
    }

    fn on_exited_review_mode(&mut self, review: ExitedReviewModeEvent) {
        // Leave review mode; if output is present, flush pending stream + show results.
        if let Some(output) = review.review_output {
            self.flush_answer_stream_with_separator();
            self.flush_interrupt_queue();
            self.flush_active_cell();

            if output.findings.is_empty() {
                let explanation = output.overall_explanation.trim().to_string();
                if explanation.is_empty() {
                    tracing::error!("Reviewer failed to output a response.");
                    self.add_to_history(history_cell::new_error_event(
                        "Reviewer failed to output a response.".to_owned(),
                    ));
                } else {
                    // Show explanation when there are no structured findings.
                    let mut rendered: Vec<ratatui::text::Line<'static>> = vec!["".into()];
                    append_markdown(&explanation, None, &mut rendered);
                    let body_cell = AgentMessageCell::new(rendered, false);
                    self.app_event_tx
                        .send(AppEvent::InsertHistoryCell(Box::new(body_cell)));
                }
            } else {
                let message_text =
                    codex_core::review_format::format_review_findings_block(&output.findings, None);
                let mut message_lines: Vec<ratatui::text::Line<'static>> = Vec::new();
                append_markdown(&message_text, None, &mut message_lines);
                let body_cell = AgentMessageCell::new(message_lines, true);
                self.app_event_tx
                    .send(AppEvent::InsertHistoryCell(Box::new(body_cell)));
            }
        }

        self.is_review_mode = false;
        self.restore_pre_review_token_info();
        // Append a finishing banner at the end of this turn.
        self.add_to_history(history_cell::new_review_status_line(
            "<< Code review finished >>".to_string(),
        ));
        self.request_redraw();
    }

    fn on_user_message_event(&mut self, event: UserMessageEvent) {
        let message = event.message.trim();
        if !message.is_empty() {
            self.add_to_history(history_cell::new_user_prompt(message.to_string()));
        }
    }

    fn request_exit(&self) {
        self.app_event_tx.send(AppEvent::ExitRequest);
    }

    fn request_redraw(&mut self) {
        self.frame_requester.schedule_frame();
    }

    fn notify(&mut self, notification: Notification) {
        if !notification.allowed_for(&self.config.tui_notifications) {
            return;
        }
        self.pending_notification = Some(notification);
        self.request_redraw();
    }

    pub(crate) fn maybe_post_pending_notification(&mut self, tui: &mut crate::tui::Tui) {
        if let Some(notif) = self.pending_notification.take() {
            tui.notify(notif.display());
        }
    }

    /// Mark the active cell as failed (✗) and flush it into history.
    fn finalize_active_cell_as_failed(&mut self) {
        if let Some(mut cell) = self.active_cell.take() {
            // Insert finalized cell into history and keep grouping consistent.
            if let Some(exec) = cell.as_any_mut().downcast_mut::<ExecCell>() {
                exec.mark_failed();
            } else if let Some(tool) = cell.as_any_mut().downcast_mut::<McpToolCallCell>() {
                tool.mark_failed();
            }
            self.add_boxed_history(cell);
        }
    }

    // If idle and there are queued inputs, submit exactly one to start the next turn.
    fn maybe_send_next_queued_input(&mut self) {
        if self.bottom_pane.is_task_running() {
            return;
        }
        if let Some(user_message) = self.queued_user_messages.pop_front() {
            self.submit_user_message(user_message);
        }
        // Update the list to reflect the remaining queued messages (if any).
        self.refresh_queued_user_messages();
    }

    /// Rebuild and update the queued user messages from the current queue.
    fn refresh_queued_user_messages(&mut self) {
        let messages: Vec<String> = self
            .queued_user_messages
            .iter()
            .map(|m| m.text.clone())
            .collect();
        self.bottom_pane.set_queued_user_messages(messages);
    }

    pub(crate) fn add_diff_in_progress(&mut self) {
        self.request_redraw();
    }

    pub(crate) fn on_diff_complete(&mut self) {
        self.request_redraw();
    }

    pub(crate) fn add_status_output(&mut self) {
        let default_usage = TokenUsage::default();
        let (total_usage, context_usage) = if let Some(ti) = &self.token_info {
            (&ti.total_token_usage, Some(&ti.last_token_usage))
        } else {
            (&default_usage, Some(&default_usage))
        };
        self.add_to_history(crate::status::new_status_output(
            &self.config,
            self.auth_manager.as_ref(),
            total_usage,
            context_usage,
            &self.conversation_id,
            self.rate_limit_snapshot.as_ref(),
            Local::now(),
        ));
    }
    fn stop_rate_limit_poller(&mut self) {
        if let Some(handle) = self.rate_limit_poller.take() {
            handle.abort();
        }
    }

    fn prefetch_rate_limits(&mut self) {
        self.stop_rate_limit_poller();

        let Some(auth) = self.auth_manager.auth() else {
            return;
        };
        if auth.mode != AuthMode::ChatGPT {
            return;
        }

        let base_url = self.config.chatgpt_base_url.clone();
        let app_event_tx = self.app_event_tx.clone();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));

            loop {
                if let Some(snapshot) = fetch_rate_limits(base_url.clone(), auth.clone()).await {
                    app_event_tx.send(AppEvent::RateLimitSnapshotFetched(snapshot));
                }
                interval.tick().await;
            }
        });

        self.rate_limit_poller = Some(handle);
    }

    fn start_semantic_warmup(&mut self) {
        if self.semantic_warmup_started {
            return;
        }
        self.semantic_warmup_started = true;

        let Some(sgrep_bin) = find_sgrep_binary() else {
            self.bottom_pane
                .set_semantic_status(SemanticStatus::Missing, Some("sgrep not found".to_string()));
            return;
        };

        self.bottom_pane
            .set_semantic_status(SemanticStatus::Indexing, Some("indexing…".to_string()));

        if let Ok(handle) = Handle::try_current() {
            let app_event_tx = self.app_event_tx.clone();
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let watch_started = self.maybe_start_semantic_watch(sgrep_bin.clone(), cwd.clone());
            if watch_started {
                self.bottom_pane
                    .set_semantic_status(SemanticStatus::Indexing, Some("watching".to_string()));
            }
            handle.spawn(async move {
                run_semantic_warmup(sgrep_bin, cwd, app_event_tx).await;
            });
        } else {
            self.bottom_pane
                .set_semantic_status(SemanticStatus::Ready, None);
        }
    }

    pub(crate) fn set_semantic_status(&mut self, status: SemanticStatus, message: Option<String>) {
        self.bottom_pane.set_semantic_status(status, message);
    }

    pub(crate) fn tick(&mut self) {
        self.bottom_pane.tick();
    }

    fn maybe_start_semantic_watch(&mut self, sgrep_bin: PathBuf, cwd: PathBuf) -> bool {
        if std::env::var("CODEX_SEMANTIC_WATCH").as_deref() == Ok("0") {
            return false;
        }

        let mut command = tokio::process::Command::new(sgrep_bin);
        command.current_dir(cwd).arg("watch").arg("--path").arg(".");
        apply_sgrep_env(&mut command);
        command.stdout(Stdio::null()).stderr(Stdio::null());

        match command.spawn() {
            Ok(child) => {
                self.semantic_watch = Some(child);
                true
            }
            Err(err) => {
                debug!(error = ?err, "failed to start sgrep watch");
                false
            }
        }
    }

    fn stop_semantic_watch(&mut self) {
        if let Some(mut child) = self.semantic_watch.take() {
            let _ = child.start_kill();
        }
    }

    fn lower_cost_preset(&self) -> Option<ModelPreset> {
        let auth_mode = self.auth_manager.auth().map(|auth| auth.mode);
        builtin_model_presets(auth_mode)
            .into_iter()
            .find(|preset| preset.model == NUDGE_MODEL_SLUG)
    }

    fn rate_limit_switch_prompt_hidden(&self) -> bool {
        self.config
            .notices
            .hide_rate_limit_model_nudge
            .unwrap_or(false)
    }

    fn maybe_show_pending_rate_limit_prompt(&mut self) {
        if self.rate_limit_switch_prompt_hidden() {
            self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Idle;
            return;
        }
        if !matches!(
            self.rate_limit_switch_prompt,
            RateLimitSwitchPromptState::Pending
        ) {
            return;
        }
        if let Some(preset) = self.lower_cost_preset() {
            self.open_rate_limit_switch_prompt(preset);
            self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Shown;
        } else {
            self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Idle;
        }
    }

    fn open_rate_limit_switch_prompt(&mut self, preset: ModelPreset) {
        let switch_model = preset.model.to_string();
        let display_name = preset.display_name.to_string();
        let default_effort: ReasoningEffortConfig = preset.default_reasoning_effort;

        let switch_actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                cwd: None,
                approval_policy: None,
                sandbox_policy: None,
                model: Some(switch_model.clone()),
                effort: Some(Some(default_effort)),
                summary: None,
            }));
            tx.send(AppEvent::UpdateModel(switch_model.clone()));
            tx.send(AppEvent::UpdateReasoningEffort(Some(default_effort)));
        })];

        let keep_actions: Vec<SelectionAction> = Vec::new();
        let never_actions: Vec<SelectionAction> = vec![Box::new(|tx| {
            tx.send(AppEvent::UpdateRateLimitSwitchPromptHidden(true));
            tx.send(AppEvent::PersistRateLimitSwitchPromptHidden);
        })];
        let description = if preset.description.is_empty() {
            Some("Uses fewer credits for upcoming turns.".to_string())
        } else {
            Some(preset.description.to_string())
        };

        let items = vec![
            SelectionItem {
                name: format!("Switch to {display_name}"),
                description,
                selected_description: None,
                is_current: false,
                actions: switch_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Keep current model".to_string(),
                description: None,
                selected_description: None,
                is_current: false,
                actions: keep_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Keep current model (never show again)".to_string(),
                description: Some(
                    "Hide future rate limit reminders about switching models.".to_string(),
                ),
                selected_description: None,
                is_current: false,
                actions: never_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Approaching rate limits".to_string()),
            subtitle: Some(format!("Switch to {display_name} for lower credit usage?")),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    /// Open a popup to choose the model (stage 1). After selecting a model,
    /// a second popup is shown to choose the reasoning effort.
    pub(crate) fn open_model_popup(&mut self) {
        let current_model = self.config.model.clone();
        let auth_mode = self.auth_manager.auth().map(|auth| auth.mode);
        let presets: Vec<ModelPreset> = builtin_model_presets(auth_mode);

        let mut items: Vec<SelectionItem> = Vec::new();
        for preset in presets.into_iter() {
            let description = if preset.description.is_empty() {
                None
            } else {
                Some(preset.description.to_string())
            };
            let is_current = preset.model == current_model;
            let single_supported_effort = preset.supported_reasoning_efforts.len() == 1;
            let preset_for_action = preset.clone();
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                let preset_for_event = preset_for_action.clone();
                tx.send(AppEvent::OpenReasoningPopup {
                    model: preset_for_event,
                });
            })];
            items.push(SelectionItem {
                name: preset.display_name.to_string(),
                description,
                is_current,
                actions,
                dismiss_on_select: single_supported_effort,
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select Model and Effort".to_string()),
            subtitle: Some(
                "Access legacy models by running codex -m <model_name> or in your config.toml"
                    .to_string(),
            ),
            footer_hint: Some("Press enter to select reasoning effort, or esc to dismiss.".into()),
            items,
            ..Default::default()
        });
    }

    /// Open a popup to choose the reasoning effort (stage 2) for the given model.
    pub(crate) fn open_reasoning_popup(&mut self, preset: ModelPreset) {
        let default_effort: ReasoningEffortConfig = preset.default_reasoning_effort;
        let supported = preset.supported_reasoning_efforts;

        let warn_effort = if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::XHigh)
        {
            Some(ReasoningEffortConfig::XHigh)
        } else if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::High)
        {
            Some(ReasoningEffortConfig::High)
        } else {
            None
        };
        let warning_text = warn_effort.map(|effort| {
            let effort_label = Self::reasoning_effort_label(effort);
            format!("⚠ {effort_label} reasoning effort can quickly consume Plus plan rate limits.")
        });
        let warn_for_model = preset.model.starts_with("gpt-5.1-codex")
            || preset.model.starts_with("gpt-5.1-codex-max");

        struct EffortChoice {
            stored: Option<ReasoningEffortConfig>,
            display: ReasoningEffortConfig,
        }
        let mut choices: Vec<EffortChoice> = Vec::new();
        for effort in ReasoningEffortConfig::iter() {
            if supported.iter().any(|option| option.effort == effort) {
                choices.push(EffortChoice {
                    stored: Some(effort),
                    display: effort,
                });
            }
        }
        if choices.is_empty() {
            choices.push(EffortChoice {
                stored: Some(default_effort),
                display: default_effort,
            });
        }

        if choices.len() == 1 {
            if let Some(effort) = choices.first().and_then(|c| c.stored) {
                self.apply_model_and_effort(preset.model.to_string(), Some(effort));
            } else {
                self.apply_model_and_effort(preset.model.to_string(), None);
            }
            return;
        }

        let default_choice: Option<ReasoningEffortConfig> = choices
            .iter()
            .any(|choice| choice.stored == Some(default_effort))
            .then_some(Some(default_effort))
            .flatten()
            .or_else(|| choices.iter().find_map(|choice| choice.stored))
            .or(Some(default_effort));

        let model_slug = preset.model.to_string();
        let is_current_model = self.config.model == preset.model;
        let highlight_choice = if is_current_model {
            self.config.model_reasoning_effort
        } else {
            default_choice
        };
        let selection_choice = highlight_choice.or(default_choice);
        let initial_selected_idx = choices
            .iter()
            .position(|choice| choice.stored == selection_choice)
            .or_else(|| {
                selection_choice
                    .and_then(|effort| choices.iter().position(|choice| choice.display == effort))
            });
        let mut items: Vec<SelectionItem> = Vec::new();
        for choice in choices.iter() {
            let effort = choice.display;
            let mut effort_label = Self::reasoning_effort_label(effort).to_string();
            if choice.stored == default_choice {
                effort_label.push_str(" (default)");
            }

            let description = choice
                .stored
                .and_then(|effort| {
                    supported
                        .iter()
                        .find(|option| option.effort == effort)
                        .map(|option| option.description.to_string())
                })
                .filter(|text| !text.is_empty());

            let show_warning = warn_for_model && warn_effort == Some(effort);
            let selected_description = if show_warning {
                warning_text.as_ref().map(|warning_message| {
                    description.as_ref().map_or_else(
                        || warning_message.clone(),
                        |d| format!("{d}\n{warning_message}"),
                    )
                })
            } else {
                None
            };

            let model_for_action = model_slug.clone();
            let effort_for_action = choice.stored;
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                    cwd: None,
                    approval_policy: None,
                    sandbox_policy: None,
                    model: Some(model_for_action.clone()),
                    effort: Some(effort_for_action),
                    summary: None,
                }));
                tx.send(AppEvent::UpdateModel(model_for_action.clone()));
                tx.send(AppEvent::UpdateReasoningEffort(effort_for_action));
                tx.send(AppEvent::PersistModelSelection {
                    model: model_for_action.clone(),
                    effort: effort_for_action,
                });
                tracing::info!(
                    "Selected model: {}, Selected effort: {}",
                    model_for_action,
                    effort_for_action
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "default".to_string())
                );
            })];

            items.push(SelectionItem {
                name: effort_label,
                description,
                selected_description,
                is_current: is_current_model && choice.stored == highlight_choice,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        let mut header = ColumnRenderable::new();
        header.push(Line::from(
            format!("Select Reasoning Level for {model_slug}").bold(),
        ));

        self.bottom_pane.show_selection_view(SelectionViewParams {
            header: Box::new(header),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            initial_selected_idx,
            ..Default::default()
        });
    }

    fn reasoning_effort_label(effort: ReasoningEffortConfig) -> &'static str {
        match effort {
            ReasoningEffortConfig::None => "None",
            ReasoningEffortConfig::Minimal => "Minimal",
            ReasoningEffortConfig::Low => "Low",
            ReasoningEffortConfig::Medium => "Medium",
            ReasoningEffortConfig::High => "High",
            ReasoningEffortConfig::XHigh => "Extra high",
        }
    }

    fn apply_model_and_effort(&self, model: String, effort: Option<ReasoningEffortConfig>) {
        self.app_event_tx
            .send(AppEvent::CodexOp(Op::OverrideTurnContext {
                cwd: None,
                approval_policy: None,
                sandbox_policy: None,
                model: Some(model.clone()),
                effort: Some(effort),
                summary: None,
            }));
        self.app_event_tx.send(AppEvent::UpdateModel(model.clone()));
        self.app_event_tx
            .send(AppEvent::UpdateReasoningEffort(effort));
        self.app_event_tx.send(AppEvent::PersistModelSelection {
            model: model.clone(),
            effort,
        });
        tracing::info!(
            "Selected model: {}, Selected effort: {}",
            model,
            effort
                .map(|e| e.to_string())
                .unwrap_or_else(|| "default".to_string())
        );
    }

    /// Open a popup to choose the approvals mode (ask for approval policy + sandbox policy).
    pub(crate) fn open_approvals_popup(&mut self) {
        let current_approval = self.config.approval_policy;
        let current_sandbox = self.config.sandbox_policy.clone();
        let mut items: Vec<SelectionItem> = Vec::new();
        let presets: Vec<ApprovalPreset> = builtin_approval_presets();
        for preset in presets.into_iter() {
            let is_current =
                Self::preset_matches_current(current_approval, &current_sandbox, &preset);
            let name = preset.label.to_string();
            let description_text = preset.description;
            let description = Some(description_text.to_string());
            let requires_confirmation = preset.id == "full-access"
                && !self
                    .config
                    .notices
                    .hide_full_access_warning
                    .unwrap_or(false);
            let actions: Vec<SelectionAction> = if requires_confirmation {
                let preset_clone = preset.clone();
                vec![Box::new(move |tx| {
                    tx.send(AppEvent::OpenFullAccessConfirmation {
                        preset: preset_clone.clone(),
                    });
                })]
            } else if preset.id == "auto" {
                #[cfg(target_os = "windows")]
                {
                    if codex_core::get_platform_sandbox().is_none() {
                        let preset_clone = preset.clone();
                        vec![Box::new(move |tx| {
                            tx.send(AppEvent::OpenWindowsSandboxEnablePrompt {
                                preset: preset_clone.clone(),
                            });
                        })]
                    } else if let Some((sample_paths, extra_count, failed_scan)) =
                        self.world_writable_warning_details()
                    {
                        let preset_clone = preset.clone();
                        vec![Box::new(move |tx| {
                            tx.send(AppEvent::OpenWorldWritableWarningConfirmation {
                                preset: Some(preset_clone.clone()),
                                sample_paths: sample_paths.clone(),
                                extra_count,
                                failed_scan,
                            });
                        })]
                    } else {
                        Self::approval_preset_actions(preset.approval, preset.sandbox.clone())
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Self::approval_preset_actions(preset.approval, preset.sandbox.clone())
                }
            } else {
                Self::approval_preset_actions(preset.approval, preset.sandbox.clone())
            };
            items.push(SelectionItem {
                name,
                description,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select Approval Mode".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(()),
            ..Default::default()
        });
    }

    pub(crate) fn open_settings_popup(&mut self) {
        let items = self.settings_menu_items();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Settings".to_string()),
            subtitle: Some("Customize the Kaioken TUI experience.".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    fn settings_menu_items(&self) -> Vec<SelectionItem> {
        let mut items = Vec::new();
        let show_footer = self.config.show_rate_limits_in_footer;
        let plan_detail = self.config.plan_detail;
        let subagent_limit = self
            .config
            .subagent_max_tasks
            .clamp(SUBAGENT_LIMIT_MIN, SUBAGENT_LIMIT_HARD_CAP);

        items.push(SelectionItem {
            name: "Show rate limit usage in footer".to_string(),
            description: Some(
                "Display session usage and weekly quota alongside the context indicator."
                    .to_string(),
            ),
            is_current: show_footer,
            actions: self.footer_visibility_actions(true),
            dismiss_on_select: false,
            ..Default::default()
        });

        items.push(SelectionItem {
            name: "Hide rate limit usage in footer".to_string(),
            description: Some("Keep the footer minimal by removing rate limit summaries.".into()),
            is_current: !show_footer,
            actions: self.footer_visibility_actions(false),
            dismiss_on_select: false,
            ..Default::default()
        });

        items.push(SelectionItem {
            name: "Plan detail — auto".to_string(),
            description: Some(
                "Let Kaioken choose between concise (3–4) or detailed (6–10) steps based on scope."
                    .to_string(),
            ),
            is_current: matches!(plan_detail, PlanDetailPreference::Auto),
            actions: self.plan_detail_actions(PlanDetailPreference::Auto),
            dismiss_on_select: false,
            ..Default::default()
        });

        items.push(SelectionItem {
            name: "Plan detail — coarse".to_string(),
            description: Some("Always produce 3–4 high-level steps for quick tasks.".to_string()),
            is_current: matches!(plan_detail, PlanDetailPreference::Coarse),
            actions: self.plan_detail_actions(PlanDetailPreference::Coarse),
            dismiss_on_select: false,
            ..Default::default()
        });

        items.push(SelectionItem {
            name: "Plan detail — detailed".to_string(),
            description: Some(
                "Always produce 6–10 steps with file references, tests, and follow-ups."
                    .to_string(),
            ),
            is_current: matches!(plan_detail, PlanDetailPreference::Detailed),
            actions: self.plan_detail_actions(PlanDetailPreference::Detailed),
            dismiss_on_select: false,
            ..Default::default()
        });

        let concurrency_choices: &[(i64, &str)] = &[
            (1, "Serial mode — run one helper at a time."),
            (
                2,
                "Light concurrency — 2 helpers for smaller repos or laptops.",
            ),
            (4, "Balanced concurrency — default Kaioken throughput."),
            (6, "High concurrency — faster scans, heavier CPU/IO."),
            (
                SUBAGENT_LIMIT_HARD_CAP,
                "Maximum concurrency — expect heavy system load.",
            ),
        ];
        for (limit, description) in concurrency_choices {
            items.push(SelectionItem {
                name: format!("Subagent concurrency — {limit} tasks"),
                description: Some((*description).to_string()),
                is_current: subagent_limit == *limit,
                actions: self.subagent_limit_actions(*limit),
                dismiss_on_select: false,
                ..Default::default()
            });
        }

        items.push(SelectionItem {
            name: "Done".to_string(),
            dismiss_on_select: true,
            ..Default::default()
        });

        items
    }

    fn footer_visibility_actions(&self, show: bool) -> Vec<SelectionAction> {
        vec![Box::new(move |tx| {
            tx.send(AppEvent::UpdateShowRateLimitsInFooter(show));
            tx.send(AppEvent::PersistShowRateLimitsInFooter(show));
        })]
    }

    fn plan_detail_actions(&self, detail: PlanDetailPreference) -> Vec<SelectionAction> {
        vec![Box::new(move |tx| {
            tx.send(AppEvent::UpdatePlanDetailPreference(detail));
            tx.send(AppEvent::PersistPlanDetailPreference(detail));
        })]
    }

    fn subagent_limit_actions(&self, limit: i64) -> Vec<SelectionAction> {
        vec![Box::new(move |tx| {
            tx.send(AppEvent::UpdateSubagentTaskLimit(limit));
            tx.send(AppEvent::PersistSubagentTaskLimit(limit));
        })]
    }

    fn approval_preset_actions(
        approval: AskForApproval,
        sandbox: SandboxPolicy,
    ) -> Vec<SelectionAction> {
        vec![Box::new(move |tx| {
            let sandbox_clone = sandbox.clone();
            tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                cwd: None,
                approval_policy: Some(approval),
                sandbox_policy: Some(sandbox_clone.clone()),
                model: None,
                effort: None,
                summary: None,
            }));
            tx.send(AppEvent::UpdateAskForApprovalPolicy(approval));
            tx.send(AppEvent::UpdateSandboxPolicy(sandbox_clone));
        })]
    }

    fn preset_matches_current(
        current_approval: AskForApproval,
        current_sandbox: &SandboxPolicy,
        preset: &ApprovalPreset,
    ) -> bool {
        if current_approval != preset.approval {
            return false;
        }
        matches!(
            (&preset.sandbox, current_sandbox),
            (SandboxPolicy::ReadOnly, SandboxPolicy::ReadOnly)
                | (
                    SandboxPolicy::DangerFullAccess,
                    SandboxPolicy::DangerFullAccess
                )
                | (
                    SandboxPolicy::WorkspaceWrite { .. },
                    SandboxPolicy::WorkspaceWrite { .. }
                )
        )
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn world_writable_warning_details(&self) -> Option<(Vec<String>, usize, bool)> {
        if self
            .config
            .notices
            .hide_world_writable_warning
            .unwrap_or(false)
        {
            return None;
        }
        let cwd = self.config.cwd.clone();
        let env_map: std::collections::HashMap<String, String> = std::env::vars().collect();
        match codex_windows_sandbox::apply_world_writable_scan_and_denies(
            self.config.codex_home.as_path(),
            cwd.as_path(),
            &env_map,
            &self.config.sandbox_policy,
            Some(self.config.codex_home.as_path()),
        ) {
            Ok(_) => None,
            Err(_) => Some((Vec::new(), 0, true)),
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    pub(crate) fn world_writable_warning_details(&self) -> Option<(Vec<String>, usize, bool)> {
        None
    }

    pub(crate) fn open_full_access_confirmation(&mut self, preset: ApprovalPreset) {
        let approval = preset.approval;
        let sandbox = preset.sandbox;
        let mut header_children: Vec<Box<dyn Renderable>> = Vec::new();
        let title_line = Line::from("Enable full access?").bold();
        let info_line = Line::from(vec![
            "When Codex Kaioken runs with full access, it can edit any file on your computer and run commands with network, without your approval. "
                .into(),
            "Exercise caution when enabling full access. This significantly increases the risk of data loss, leaks, or unexpected behavior."
                .fg(Color::Red),
        ]);
        header_children.push(Box::new(title_line));
        header_children.push(Box::new(
            Paragraph::new(vec![info_line]).wrap(Wrap { trim: false }),
        ));
        let header = ColumnRenderable::with(header_children);

        let mut accept_actions = Self::approval_preset_actions(approval, sandbox.clone());
        accept_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateFullAccessWarningAcknowledged(true));
        }));

        let mut accept_and_remember_actions = Self::approval_preset_actions(approval, sandbox);
        accept_and_remember_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateFullAccessWarningAcknowledged(true));
            tx.send(AppEvent::PersistFullAccessWarningAcknowledged);
        }));

        let deny_actions: Vec<SelectionAction> = vec![Box::new(|tx| {
            tx.send(AppEvent::OpenApprovalsPopup);
        })];

        let items = vec![
            SelectionItem {
                name: "Yes, continue anyway".to_string(),
                description: Some("Apply full access for this session".to_string()),
                actions: accept_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Yes, and don't ask again".to_string(),
                description: Some("Enable full access and remember this choice".to_string()),
                actions: accept_and_remember_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Cancel".to_string(),
                description: Some("Go back without enabling full access".to_string()),
                actions: deny_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn open_world_writable_warning_confirmation(
        &mut self,
        preset: Option<ApprovalPreset>,
        sample_paths: Vec<String>,
        extra_count: usize,
        failed_scan: bool,
    ) {
        let (approval, sandbox) = match &preset {
            Some(p) => (Some(p.approval), Some(p.sandbox.clone())),
            None => (None, None),
        };
        let mut header_children: Vec<Box<dyn Renderable>> = Vec::new();
        let describe_policy = |policy: &SandboxPolicy| match policy {
            SandboxPolicy::WorkspaceWrite { .. } => "Agent mode",
            SandboxPolicy::ReadOnly => "Read-Only mode",
            _ => "Agent mode",
        };
        let mode_label = preset
            .as_ref()
            .map(|p| describe_policy(&p.sandbox))
            .unwrap_or_else(|| describe_policy(&self.config.sandbox_policy));
        let info_line = if failed_scan {
            Line::from(vec![
                "We couldn't complete the world-writable scan, so protections cannot be verified. "
                    .into(),
                format!("The Windows sandbox cannot guarantee protection in {mode_label}.")
                    .fg(Color::Red),
            ])
        } else {
            Line::from(vec![
                "The Windows sandbox cannot protect writes to folders that are writable by Everyone.".into(),
                " Consider removing write access for Everyone from the following folders:".into(),
            ])
        };
        header_children.push(Box::new(
            Paragraph::new(vec![info_line]).wrap(Wrap { trim: false }),
        ));

        if !sample_paths.is_empty() {
            // Show up to three examples and optionally an "and X more" line.
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(""));
            for p in &sample_paths {
                lines.push(Line::from(format!("  - {p}")));
            }
            if extra_count > 0 {
                lines.push(Line::from(format!("and {extra_count} more")));
            }
            header_children.push(Box::new(Paragraph::new(lines).wrap(Wrap { trim: false })));
        }
        let header = ColumnRenderable::with(header_children);

        // Build actions ensuring acknowledgement happens before applying the new sandbox policy,
        // so downstream policy-change hooks don't re-trigger the warning.
        let mut accept_actions: Vec<SelectionAction> = Vec::new();
        // Suppress the immediate re-scan only when a preset will be applied (i.e., via /approvals),
        // to avoid duplicate warnings from the ensuing policy change.
        if preset.is_some() {
            accept_actions.push(Box::new(|tx| {
                tx.send(AppEvent::SkipNextWorldWritableScan);
            }));
        }
        if let (Some(approval), Some(sandbox)) = (approval, sandbox.clone()) {
            accept_actions.extend(Self::approval_preset_actions(approval, sandbox));
        }

        let mut accept_and_remember_actions: Vec<SelectionAction> = Vec::new();
        accept_and_remember_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateWorldWritableWarningAcknowledged(true));
            tx.send(AppEvent::PersistWorldWritableWarningAcknowledged);
        }));
        if let (Some(approval), Some(sandbox)) = (approval, sandbox) {
            accept_and_remember_actions.extend(Self::approval_preset_actions(approval, sandbox));
        }

        let items = vec![
            SelectionItem {
                name: "Continue".to_string(),
                description: Some(format!("Apply {mode_label} for this session")),
                actions: accept_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Continue and don't warn again".to_string(),
                description: Some(format!("Enable {mode_label} and remember this choice")),
                actions: accept_and_remember_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn open_world_writable_warning_confirmation(
        &mut self,
        _preset: Option<ApprovalPreset>,
        _sample_paths: Vec<String>,
        _extra_count: usize,
        _failed_scan: bool,
    ) {
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn open_windows_sandbox_enable_prompt(&mut self, preset: ApprovalPreset) {
        use ratatui_macros::line;

        let mut header = ColumnRenderable::new();
        header.push(*Box::new(
            Paragraph::new(vec![
                line!["Agent mode on Windows uses an experimental sandbox to limit network and filesystem access.".bold()],
                line![
                    "Learn more: https://developers.openai.com/codex/windows"
                ],
            ])
            .wrap(Wrap { trim: false }),
        ));

        let preset_clone = preset;
        let items = vec![
            SelectionItem {
                name: "Enable experimental sandbox".to_string(),
                description: None,
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::EnableWindowsSandboxForAgentMode {
                        preset: preset_clone.clone(),
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Go back".to_string(),
                description: None,
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::OpenApprovalsPopup);
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: None,
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn open_windows_sandbox_enable_prompt(&mut self, _preset: ApprovalPreset) {}

    #[cfg(target_os = "windows")]
    pub(crate) fn maybe_prompt_windows_sandbox_enable(&mut self) {
        if self.config.forced_auto_mode_downgraded_on_windows
            && codex_core::get_platform_sandbox().is_none()
            && let Some(preset) = builtin_approval_presets()
                .into_iter()
                .find(|preset| preset.id == "auto")
        {
            self.open_windows_sandbox_enable_prompt(preset);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn maybe_prompt_windows_sandbox_enable(&mut self) {}

    #[cfg(target_os = "windows")]
    pub(crate) fn clear_forced_auto_mode_downgrade(&mut self) {
        self.config.forced_auto_mode_downgraded_on_windows = false;
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    pub(crate) fn clear_forced_auto_mode_downgrade(&mut self) {}

    /// Set the approval policy in the widget's config copy.
    pub(crate) fn set_approval_policy(&mut self, policy: AskForApproval) {
        self.config.approval_policy = policy;
    }

    /// Set the sandbox policy in the widget's config copy.
    pub(crate) fn set_sandbox_policy(&mut self, policy: SandboxPolicy) {
        #[cfg(target_os = "windows")]
        let should_clear_downgrade = !matches!(policy, SandboxPolicy::ReadOnly)
            || codex_core::get_platform_sandbox().is_some();

        self.config.sandbox_policy = policy;

        #[cfg(target_os = "windows")]
        if should_clear_downgrade {
            self.config.forced_auto_mode_downgraded_on_windows = false;
        }
    }

    pub(crate) fn set_full_access_warning_acknowledged(&mut self, acknowledged: bool) {
        self.config.notices.hide_full_access_warning = Some(acknowledged);
    }

    pub(crate) fn set_world_writable_warning_acknowledged(&mut self, acknowledged: bool) {
        self.config.notices.hide_world_writable_warning = Some(acknowledged);
    }

    pub(crate) fn set_rate_limit_switch_prompt_hidden(&mut self, hidden: bool) {
        self.config.notices.hide_rate_limit_model_nudge = Some(hidden);
        if hidden {
            self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Idle;
        }
    }

    pub(crate) fn set_show_rate_limits_in_footer(&mut self, show: bool) {
        if self.config.show_rate_limits_in_footer == show {
            return;
        }
        self.config.show_rate_limits_in_footer = show;
        self.refresh_rate_limit_footer_summary();
        let message = if show {
            "Rate limit usage will appear in the footer."
        } else {
            "Rate limit usage hidden from the footer (toggle with /settings)."
        };
        self.add_info_message(message.to_string(), None);
    }

    pub(crate) fn set_plan_detail(&mut self, detail: PlanDetailPreference) {
        if self.config.plan_detail == detail {
            return;
        }
        self.config.plan_detail = detail;
        let message = match detail {
            PlanDetailPreference::Auto => {
                "Plan mode will auto-adjust between concise and detailed steps."
            }
            PlanDetailPreference::Coarse => "Plan mode will focus on 3–4 high-level steps.",
            PlanDetailPreference::Detailed => {
                "Plan mode will produce 6–10 implementation-ready steps."
            }
        };
        self.add_info_message(message.to_string(), None);
    }

    pub(crate) fn set_subagent_task_limit(&mut self, limit: i64) {
        let normalized = limit.clamp(SUBAGENT_LIMIT_MIN, SUBAGENT_LIMIT_HARD_CAP);
        if self.config.subagent_max_tasks == normalized {
            return;
        }
        self.config.subagent_max_tasks = normalized;
        let plural = if normalized == 1 { "" } else { "s" };
        let mut message =
            format!("Kaioken will run up to {normalized} subagent task{plural} in parallel.");
        if normalized >= 6 {
            message.push_str(" Higher limits can tax CPU and I/O.");
        }
        self.add_info_message(message, None);
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn world_writable_warning_hidden(&self) -> bool {
        self.config
            .notices
            .hide_world_writable_warning
            .unwrap_or(false)
    }

    /// Set the reasoning effort in the widget's config copy.
    pub(crate) fn set_reasoning_effort(&mut self, effort: Option<ReasoningEffortConfig>) {
        self.config.model_reasoning_effort = effort;
    }

    /// Set the model in the widget's config copy.
    pub(crate) fn set_model(&mut self, model: &str) {
        self.session_header.set_model(model);
        self.config.model = model.to_string();
    }

    pub(crate) fn add_info_message(&mut self, message: String, hint: Option<String>) {
        self.add_to_history(history_cell::new_info_event(message, hint));
        self.request_redraw();
    }

    pub(crate) fn add_plain_history_lines(&mut self, lines: Vec<Line<'static>>) {
        self.add_boxed_history(Box::new(PlainHistoryCell::new(lines)));
        self.request_redraw();
    }

    pub(crate) fn add_error_message(&mut self, message: String) {
        self.add_to_history(history_cell::new_error_event(message));
        self.request_redraw();
    }

    pub(crate) fn add_mcp_output(&mut self) {
        if self.config.mcp_servers.is_empty() {
            self.add_to_history(history_cell::empty_mcp_output());
        } else {
            self.submit_op(Op::ListMcpTools);
        }
    }

    /// Forward file-search results to the bottom pane.
    pub(crate) fn apply_file_search_result(&mut self, query: String, matches: Vec<FileMatch>) {
        self.bottom_pane.on_file_search_result(query, matches);
    }

    /// Handle Ctrl-C key press.
    fn on_ctrl_c(&mut self) {
        if self.bottom_pane.on_ctrl_c() == CancellationEvent::Handled {
            return;
        }

        if self.bottom_pane.is_task_running() {
            self.bottom_pane.show_ctrl_c_quit_hint();
            self.submit_op(Op::Interrupt);
            return;
        }

        self.submit_op(Op::Shutdown);
    }

    pub(crate) fn composer_is_empty(&self) -> bool {
        self.bottom_pane.composer_is_empty()
    }

    /// True when the UI is in the regular composer state with no running task,
    /// no modal overlay (e.g. approvals or status indicator), and no composer popups.
    /// In this state Esc-Esc backtracking is enabled.
    pub(crate) fn is_normal_backtrack_mode(&self) -> bool {
        self.bottom_pane.is_normal_backtrack_mode()
    }

    pub(crate) fn insert_str(&mut self, text: &str) {
        self.bottom_pane.insert_str(text);
    }

    /// Replace the composer content with the provided text and reset cursor.
    pub(crate) fn set_composer_text(&mut self, text: String) {
        self.bottom_pane.set_composer_text(text);
    }

    pub(crate) fn show_esc_backtrack_hint(&mut self) {
        self.bottom_pane.show_esc_backtrack_hint();
    }

    pub(crate) fn clear_esc_backtrack_hint(&mut self) {
        self.bottom_pane.clear_esc_backtrack_hint();
    }
    /// Forward an `Op` directly to codex.
    pub(crate) fn submit_op(&self, op: Op) {
        // Record outbound operation for session replay fidelity.
        crate::session_log::log_outbound_op(&op);
        if let Err(e) = self.codex_op_tx.send(op) {
            tracing::error!("failed to submit op: {e}");
        }
    }

    fn on_list_mcp_tools(&mut self, ev: McpListToolsResponseEvent) {
        self.add_to_history(history_cell::new_mcp_tools_output(
            &self.config,
            ev.tools,
            ev.resources,
            ev.resource_templates,
            &ev.auth_statuses,
        ));
    }

    fn on_list_custom_prompts(&mut self, ev: ListCustomPromptsResponseEvent) {
        let len = ev.custom_prompts.len();
        debug!("received {len} custom prompts");
        // Forward to bottom pane so the slash popup can show them now.
        self.bottom_pane.set_custom_prompts(ev.custom_prompts);
    }

    pub(crate) fn open_review_popup(&mut self) {
        let mut items: Vec<SelectionItem> = Vec::new();

        items.push(SelectionItem {
            name: "Review against a base branch".to_string(),
            description: Some("(PR Style)".into()),
            actions: vec![Box::new({
                let cwd = self.config.cwd.clone();
                move |tx| {
                    tx.send(AppEvent::OpenReviewBranchPicker(cwd.clone()));
                }
            })],
            dismiss_on_select: false,
            ..Default::default()
        });

        items.push(SelectionItem {
            name: "Review uncommitted changes".to_string(),
            actions: vec![Box::new(
                move |tx: &AppEventSender| {
                    tx.send(AppEvent::CodexOp(Op::Review {
                        review_request: ReviewRequest {
                            prompt: "Review the current code changes (staged, unstaged, and untracked files) and provide prioritized findings.".to_string(),
                            user_facing_hint: "current changes".to_string(),
                            append_to_original_thread: true,
                        },
                    }));
                },
            )],
            dismiss_on_select: true,
            ..Default::default()
        });

        // New: Review a specific commit (opens commit picker)
        items.push(SelectionItem {
            name: "Review a commit".to_string(),
            actions: vec![Box::new({
                let cwd = self.config.cwd.clone();
                move |tx| {
                    tx.send(AppEvent::OpenReviewCommitPicker(cwd.clone()));
                }
            })],
            dismiss_on_select: false,
            ..Default::default()
        });

        items.push(SelectionItem {
            name: "Custom review instructions".to_string(),
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::OpenReviewCustomPrompt);
            })],
            dismiss_on_select: false,
            ..Default::default()
        });

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select a review preset".into()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) async fn show_review_branch_picker(&mut self, cwd: &Path) {
        let branches = local_git_branches(cwd).await;
        let current_branch = current_branch_name(cwd)
            .await
            .unwrap_or_else(|| "(detached HEAD)".to_string());
        let mut items: Vec<SelectionItem> = Vec::with_capacity(branches.len());

        for option in branches {
            let branch = option.clone();
            items.push(SelectionItem {
                name: format!("{current_branch} -> {branch}"),
                actions: vec![Box::new(move |tx3: &AppEventSender| {
                    tx3.send(AppEvent::CodexOp(Op::Review {
                        review_request: ReviewRequest {
                            prompt: format!(
                                "Review the code changes against the base branch '{branch}'. Start by finding the merge diff between the current branch and {branch}'s upstream e.g. (`git merge-base HEAD \"$(git rev-parse --abbrev-ref \"{branch}@{{upstream}}\")\"`), then run `git diff` against that SHA to see what changes we would merge into the {branch} branch. Provide prioritized, actionable findings."
                            ),
                            user_facing_hint: format!("changes against '{branch}'"),
                            append_to_original_thread: true,
                        },
                    }));
                })],
                dismiss_on_select: true,
                search_value: Some(option),
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select a base branch".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            is_searchable: true,
            search_placeholder: Some("Type to search branches".to_string()),
            ..Default::default()
        });
    }

    pub(crate) async fn show_review_commit_picker(&mut self, cwd: &Path) {
        let commits = codex_core::git_info::recent_commits(cwd, 100).await;

        let mut items: Vec<SelectionItem> = Vec::with_capacity(commits.len());
        for entry in commits {
            let subject = entry.subject.clone();
            let sha = entry.sha.clone();
            let short = sha.chars().take(7).collect::<String>();
            let search_val = format!("{subject} {sha}");

            items.push(SelectionItem {
                name: subject.clone(),
                actions: vec![Box::new(move |tx3: &AppEventSender| {
                    let hint = format!("commit {short}");
                    let prompt = format!(
                        "Review the code changes introduced by commit {sha} (\"{subject}\"). Provide prioritized, actionable findings."
                    );
                    tx3.send(AppEvent::CodexOp(Op::Review {
                        review_request: ReviewRequest {
                            prompt,
                            user_facing_hint: hint,
                            append_to_original_thread: true,
                        },
                    }));
                })],
                dismiss_on_select: true,
                search_value: Some(search_val),
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select a commit to review".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            is_searchable: true,
            search_placeholder: Some("Type to search commits".to_string()),
            ..Default::default()
        });
    }

    pub(crate) fn show_review_custom_prompt(&mut self) {
        let tx = self.app_event_tx.clone();
        let view = CustomPromptView::new(
            "Custom review instructions".to_string(),
            "Type instructions and press Enter".to_string(),
            None,
            Box::new(move |prompt: String| {
                let trimmed = prompt.trim().to_string();
                if trimmed.is_empty() {
                    return;
                }
                tx.send(AppEvent::CodexOp(Op::Review {
                    review_request: ReviewRequest {
                        prompt: trimmed.clone(),
                        user_facing_hint: trimmed,
                        append_to_original_thread: true,
                    },
                }));
            }),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    pub(crate) fn token_usage(&self) -> TokenUsage {
        self.token_info
            .as_ref()
            .map(|ti| ti.total_token_usage.clone())
            .unwrap_or_default()
    }

    pub(crate) fn conversation_id(&self) -> Option<ConversationId> {
        self.conversation_id
    }

    pub(crate) fn rollout_path(&self) -> Option<PathBuf> {
        self.current_rollout_path.clone()
    }

    /// Return a reference to the widget's current config (includes any
    /// runtime overrides applied via TUI, e.g., model or approval policy).
    pub(crate) fn config_ref(&self) -> &Config {
        &self.config
    }

    pub(crate) fn clear_token_usage(&mut self) {
        self.token_info = None;
    }

    fn as_renderable(&self) -> RenderableItem<'_> {
        let active_cell_renderable = match &self.active_cell {
            Some(cell) => RenderableItem::Borrowed(cell).inset(Insets::tlbr(1, 0, 0, 0)),
            None => RenderableItem::Owned(Box::new(())),
        };
        let mut flex = FlexRenderable::new();
        flex.push(1, active_cell_renderable);
        flex.push(
            0,
            RenderableItem::Borrowed(&self.bottom_pane).inset(Insets::tlbr(1, 0, 0, 0)),
        );
        RenderableItem::Owned(Box::new(flex))
    }
}

impl Drop for ChatWidget {
    fn drop(&mut self) {
        self.stop_rate_limit_poller();
        self.stop_semantic_watch();
    }
}

impl Renderable for ChatWidget {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_renderable().render(area, buf);
        self.last_rendered_width.set(Some(area.width as usize));
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.as_renderable().desired_height(width)
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_renderable().cursor_pos(area)
    }
}

enum Notification {
    AgentTurnComplete { response: String },
    ExecApprovalRequested { command: String },
    EditApprovalRequested { cwd: PathBuf, changes: Vec<PathBuf> },
    ElicitationRequested { server_name: String },
}

impl Notification {
    fn display(&self) -> String {
        match self {
            Notification::AgentTurnComplete { response } => {
                Notification::agent_turn_preview(response)
                    .unwrap_or_else(|| "Agent turn complete".to_string())
            }
            Notification::ExecApprovalRequested { command } => {
                format!("Approval requested: {}", truncate_text(command, 30))
            }
            Notification::EditApprovalRequested { cwd, changes } => {
                format!(
                    "Codex Kaioken wants to edit {}",
                    if changes.len() == 1 {
                        #[allow(clippy::unwrap_used)]
                        display_path_for(changes.first().unwrap(), cwd)
                    } else {
                        format!("{} files", changes.len())
                    }
                )
            }
            Notification::ElicitationRequested { server_name } => {
                format!("Approval requested by {server_name}")
            }
        }
    }

    fn type_name(&self) -> &str {
        match self {
            Notification::AgentTurnComplete { .. } => "agent-turn-complete",
            Notification::ExecApprovalRequested { .. }
            | Notification::EditApprovalRequested { .. }
            | Notification::ElicitationRequested { .. } => "approval-requested",
        }
    }

    fn allowed_for(&self, settings: &Notifications) -> bool {
        match settings {
            Notifications::Enabled(enabled) => *enabled,
            Notifications::Custom(allowed) => allowed.iter().any(|a| a == self.type_name()),
        }
    }

    fn agent_turn_preview(response: &str) -> Option<String> {
        let mut normalized = String::new();
        for part in response.split_whitespace() {
            if !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.push_str(part);
        }
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(truncate_text(trimmed, AGENT_NOTIFICATION_PREVIEW_GRAPHEMES))
        }
    }
}

const AGENT_NOTIFICATION_PREVIEW_GRAPHEMES: usize = 200;

const EXAMPLE_PROMPTS: [&str; 6] = [
    "Explain this codebase",
    "Summarize recent commits",
    "Implement {feature}",
    "Find and fix a bug in @filename",
    "Write tests for @filename",
    "Improve documentation in @filename",
];

// Extract the first bold (Markdown) element in the form **...** from `s`.
// Returns the inner text if found; otherwise `None`.
fn extract_first_bold(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() {
                if bytes[j] == b'*' && bytes[j + 1] == b'*' {
                    // Found closing **
                    let inner = &s[start..j];
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    } else {
                        return None;
                    }
                }
                j += 1;
            }
            // No closing; stop searching (wait for more deltas)
            return None;
        }
        i += 1;
    }
    None
}

async fn run_semantic_warmup(sgrep_bin: PathBuf, cwd: PathBuf, app_event_tx: AppEventSender) {
    let mut command = Command::new(sgrep_bin);
    command
        .current_dir(&cwd)
        .arg("search")
        .arg("--json")
        .arg("--limit")
        .arg("1")
        .arg("--path")
        .arg(&cwd)
        .arg("fn")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_sgrep_env(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            app_event_tx.send(AppEvent::SemanticStatusUpdate(
                SemanticStatus::Missing,
                Some(format!("sgrep failed to start: {err}")),
            ));
            return;
        }
    };

    if let Some(stdout) = child.stdout.take() {
        let tx = app_event_tx.clone();
        tokio::spawn(async move { stream_progress(stdout, tx).await });
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = app_event_tx.clone();
        tokio::spawn(async move { stream_progress(stderr, tx).await });
    }

    let status = tokio::time::timeout(Duration::from_secs(180), child.wait()).await;
    match status {
        Ok(Ok(exit)) if exit.success() => {
            app_event_tx.send(AppEvent::SemanticStatusUpdate(SemanticStatus::Ready, None));
        }
        Ok(Ok(exit)) => {
            app_event_tx.send(AppEvent::SemanticStatusUpdate(
                SemanticStatus::Missing,
                Some(format!("sgrep exited with status {exit}")),
            ));
        }
        Ok(Err(err)) => {
            app_event_tx.send(AppEvent::SemanticStatusUpdate(
                SemanticStatus::Missing,
                Some(format!("sgrep failed: {err}")),
            ));
        }
        Err(_) => {
            app_event_tx.send(AppEvent::SemanticStatusUpdate(
                SemanticStatus::Missing,
                Some("sgrep warmup timed out".to_string()),
            ));
        }
    }
}

async fn stream_progress<R: tokio::io::AsyncRead + Unpin>(reader: R, app_event_tx: AppEventSender) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(clean) = clean_progress_line(&line) {
            app_event_tx.send(AppEvent::SemanticStatusUpdate(
                SemanticStatus::Indexing,
                Some(clean),
            ));
        }
    }
}

fn clean_progress_line(line: &str) -> Option<String> {
    let stripped = ansi_escape_line(&line.replace('\r', "\n"));
    let mut text = String::new();
    for span in stripped.spans {
        text.push_str(&span.content);
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(idx) = trimmed.find("Indexing files (") {
        let short = &trimmed[idx..];
        return Some(short.to_string());
    }

    if trimmed.contains("no index found")
        || trimmed.contains("Building index")
        || trimmed.contains("Full indexing")
    {
        return Some("indexing…".to_string());
    }

    None
}

fn apply_sgrep_env(command: &mut Command) {
    for key in [
        "SGREP_CPU_PRESET",
        "SGREP_DEVICE",
        "SGREP_EMBEDDER_POOL_SIZE",
        "SGREP_MAX_THREADS",
    ] {
        if let Ok(value) = env::var(key) {
            command.env(key, value);
        }
    }
}

async fn fetch_rate_limits(base_url: String, auth: CodexAuth) -> Option<RateLimitSnapshot> {
    match BackendClient::from_auth(base_url, &auth).await {
        Ok(client) => match client.get_rate_limits().await {
            Ok(snapshot) => Some(snapshot),
            Err(err) => {
                debug!(error = ?err, "failed to fetch rate limits from /usage");
                None
            }
        },
        Err(err) => {
            debug!(error = ?err, "failed to construct backend client for rate limits");
            None
        }
    }
}

#[cfg(test)]
pub(crate) fn show_review_commit_picker_with_entries(
    chat: &mut ChatWidget,
    entries: Vec<codex_core::git_info::CommitLogEntry>,
) {
    let mut items: Vec<SelectionItem> = Vec::with_capacity(entries.len());
    for entry in entries {
        let subject = entry.subject.clone();
        let sha = entry.sha.clone();
        let short = sha.chars().take(7).collect::<String>();
        let search_val = format!("{subject} {sha}");

        items.push(SelectionItem {
            name: subject.clone(),
            actions: vec![Box::new(move |tx3: &AppEventSender| {
                let hint = format!("commit {short}");
                let prompt = format!(
                    "Review the code changes introduced by commit {sha} (\"{subject}\"). Provide prioritized, actionable findings."
                );
                tx3.send(AppEvent::CodexOp(Op::Review {
                    review_request: ReviewRequest {
                        prompt,
                        user_facing_hint: hint,
                        append_to_original_thread: true,
                    },
                }));
            })],
            dismiss_on_select: true,
            search_value: Some(search_val),
            ..Default::default()
        });
    }

    chat.bottom_pane.show_selection_view(SelectionViewParams {
        title: Some("Select a commit to review".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        is_searchable: true,
        search_placeholder: Some("Type to search commits".to_string()),
        ..Default::default()
    });
}

fn build_subagent_history_cell(
    label: String,
    event: &EventMsg,
    cwd: &Path,
    animations_enabled: bool,
) -> Option<Box<dyn HistoryCell>> {
    let cell: Option<Box<dyn HistoryCell>> = match event {
        EventMsg::ExecCommandEnd(ev) => {
            let mut cell = new_active_exec_command(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd.clone(),
                ev.source,
                ev.interaction_input.clone(),
                animations_enabled,
            );
            let aggregated_output = if ev.aggregated_output.is_empty() {
                let mut combined = String::new();
                if !ev.stdout.is_empty() {
                    combined.push_str(&ev.stdout);
                }
                if !ev.stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&ev.stderr);
                }
                combined
            } else {
                ev.aggregated_output.clone()
            };
            let output = CommandOutput {
                exit_code: ev.exit_code,
                formatted_output: ev.formatted_output.clone(),
                aggregated_output,
            };
            cell.complete_call(&ev.call_id, output, ev.duration);
            Some(Box::new(cell))
        }
        EventMsg::PatchApplyBegin(ev) => Some(Box::new(history_cell::new_patch_event(
            ev.changes.clone(),
            cwd,
        ))),
        EventMsg::PatchApplyEnd(ev) => {
            if ev.success {
                None
            } else {
                Some(Box::new(history_cell::new_patch_apply_failure(
                    ev.stderr.clone(),
                )))
            }
        }
        EventMsg::McpToolCallEnd(ev) => {
            let mut cell = McpToolCallCell::new(
                ev.call_id.clone(),
                ev.invocation.clone(),
                animations_enabled,
            );
            let image_cell = cell.complete(ev.duration, ev.result.clone());
            let mut parts: Vec<Box<dyn HistoryCell>> = vec![Box::new(cell)];
            if let Some(img) = image_cell {
                parts.push(img);
            }
            Some(Box::new(CompositeHistoryCell::new(parts)))
        }
        EventMsg::WebSearchEnd(ev) => Some(Box::new(history_cell::new_web_search_call(format!(
            "Searched: {}",
            ev.query
        )))),
        EventMsg::AgentMessage(AgentMessageEvent { message }) => {
            let mut lines = Vec::new();
            append_markdown(message, None, &mut lines);
            Some(Box::new(AgentMessageCell::new(lines, true)))
        }
        EventMsg::TaskComplete(ev) => ev.last_agent_message.as_ref().map(|message| {
            let mut lines: Vec<Line<'static>> = Vec::new();
            append_markdown(message, None, &mut lines);
            Box::new(AgentMessageCell::new(lines, true)) as Box<dyn HistoryCell>
        }),
        _ => None,
    };

    cell.map(|inner| Box::new(SubagentHistoryCell { label, inner }) as Box<dyn HistoryCell>)
}

fn subagent_history_log_lines(event: &EventMsg) -> Vec<String> {
    match event {
        EventMsg::ExecCommandBegin(_) | EventMsg::ExecCommandOutputDelta(_) => Vec::new(),
        EventMsg::ExecCommandEnd(ev) => {
            if let Some(lines) = explore_lines(&ev.parsed_cmd) {
                let mut out = Vec::new();
                out.push("**Explored**".to_string());
                out.extend(lines);
                return out;
            }
            let cmd = ev.command.join(" ");
            vec![format!(
                "**Exec** {cmd} \u{2192} exit {} ({:.1}s)",
                ev.exit_code,
                ev.duration.as_secs_f32()
            )]
        }
        EventMsg::PatchApplyBegin(ev) => {
            vec![format!(
                "**apply_patch** begin: {} change(s)",
                ev.changes.len()
            )]
        }
        EventMsg::PatchApplyEnd(ev) => {
            if ev.success {
                vec![format!(
                    "**apply_patch** success: {} change(s)",
                    ev.changes.len()
                )]
            } else if ev.stderr.trim().is_empty() {
                vec![format!(
                    "**apply_patch** failed: {} change(s)",
                    ev.changes.len()
                )]
            } else {
                let snippet = truncate_text(&ev.stderr, 120);
                vec![format!("**apply_patch** failed: {snippet}")]
            }
        }
        EventMsg::McpToolCallEnd(ev) => {
            let status = if ev.is_success() { "ok" } else { "err" };
            vec![format!(
                "**MCP** {status}: {}::{}",
                ev.invocation.server, ev.invocation.tool
            )]
        }
        EventMsg::WebSearchEnd(ev) => vec![format!("**Web search** {}", ev.query)],
        EventMsg::AgentMessage(_) | EventMsg::TaskComplete(_) => Vec::new(),
        _ => Vec::new(),
    }
}

fn explore_lines(parsed: &[ParsedCommand]) -> Option<Vec<String>> {
    if parsed.is_empty()
        || parsed.iter().any(|p| {
            !matches!(
                p,
                ParsedCommand::Read { .. }
                    | ParsedCommand::ListFiles { .. }
                    | ParsedCommand::Search { .. }
                    | ParsedCommand::Unknown { .. }
            )
        })
    {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();
    for cmd in parsed {
        match cmd {
            ParsedCommand::Read { name, .. } => lines.push(format!("**Read** {name}")),
            ParsedCommand::ListFiles { cmd, path } => {
                let label = path.clone().unwrap_or(cmd.clone());
                lines.push(format!("**List** {label}"));
            }
            ParsedCommand::Search { cmd, query, path } => {
                if let Some(q) = query {
                    match path {
                        Some(p) => lines.push(format!("**Search** {q} in {p}")),
                        None => lines.push(format!("**Search** {q}")),
                    }
                } else {
                    lines.push(format!("**Search** {cmd}"));
                }
            }
            ParsedCommand::Unknown { cmd } => lines.push(format!("**Run** {cmd}")),
        }
    }

    if lines.is_empty() { None } else { Some(lines) }
}

#[derive(Debug)]
struct SubagentHistoryCell {
    label: String,
    inner: Box<dyn HistoryCell>,
}

impl HistoryCell for SubagentHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> =
            vec![vec!["Subagent".bold(), " ".into(), self.label.clone().bold()].into()];
        let inner_width = width.saturating_sub(4);
        let inner_lines = self.inner.display_lines(inner_width);
        if !inner_lines.is_empty() {
            lines.extend(prefix_lines(inner_lines, "  └ ".dim(), "    ".into()));
        }
        lines
    }
}

fn checkpoint_hint(entry: &CheckpointEntry) -> String {
    let short_id = short_commit_id(&entry.commit_id);
    entry
        .created_at
        .as_deref()
        .map(|ts| format!("{short_id} · {ts}"))
        .unwrap_or(short_id)
}

fn short_commit_id(commit_id: &str) -> String {
    commit_id.chars().take(7).collect()
}

fn rate_limit_footer_summary(snapshot: &RateLimitSnapshotDisplay) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(primary) = snapshot.primary.as_ref() {
        let remaining = (100.0 - primary.used_percent).clamp(0.0, 100.0);
        let mut text = format!("Limits: {remaining:.0}% left");
        if let Some(reset) = primary.resets_at.as_ref() {
            text.push_str(&format!(" (reset {reset})"));
        }
        parts.push(text);
    }

    if let Some(secondary) = snapshot.secondary.as_ref() {
        let remaining = (100.0 - secondary.used_percent).clamp(0.0, 100.0);
        let mut text = format!("Weekly left: {remaining:.0}%");
        if let Some(reset) = secondary.resets_at.as_ref() {
            text.push_str(&format!(" (reset {reset})"));
        }
        parts.push(text);
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

#[cfg(test)]
pub(crate) mod tests;
