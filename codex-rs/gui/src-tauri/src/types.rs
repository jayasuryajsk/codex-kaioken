//! Types for communication between Tauri frontend and backend.
//! These mirror the TypeScript types in src/types.ts

use serde::Deserialize;
use serde::Serialize;

/// Message role in conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

/// Tool execution type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolType {
    Shell,
    Read,
    Write,
    Edit,
    Search,
    Mcp,
    Memory,
}

/// Tool execution status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Pending,
    Running,
    Success,
    Error,
}

/// Tool execution representation for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecution {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: ToolType,
    pub name: String,
    pub status: ToolStatus,
    pub input: serde_json::Value,
    pub output: Option<String>,
    pub error: Option<String>,
    pub start_time: i64, // Unix timestamp ms
    pub end_time: Option<i64>,
}

/// Chat message for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub timestamp: i64, // Unix timestamp ms
    pub tool_calls: Option<Vec<ToolExecution>>,
}

/// Response from send_message command
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResponse {
    pub message: ChatMessage,
    pub token_usage: Option<TokenUsage>,
}

/// Token usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

/// App settings from frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub model: String,
    pub approval_preset: String,
    pub plan_mode: bool,
    pub dark_mode: bool,
}

/// Approval request to show user
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String,
    #[serde(rename = "type")]
    pub approval_type: String,
    pub description: String,
    pub command: Option<String>,
    pub risk: String, // "low" | "medium" | "high"
}

/// Event emitted to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "eventType", rename_all = "camelCase")]
pub enum GuiEvent {
    /// New message in conversation
    Message(ChatMessage),
    /// Tool execution started
    ToolStart(ToolExecution),
    /// Tool execution completed
    ToolEnd(ToolExecution),
    /// Streaming content update
    ContentDelta { message_id: String, delta: String },
    /// Approval requested
    ApprovalNeeded(ApprovalRequest),
    /// Session connected
    Connected { conversation_id: String },
    /// Token usage update
    TokenUsage(serde_json::Value),
    /// Session error
    Error { message: String },
}

// ============================================================================
// Workspace Management Types
// ============================================================================

use std::path::PathBuf;

/// Status of a worktree session
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceStatus {
    Idle,
    Thinking,
    Working,
    Waiting,
    Error,
    Disconnected,
}

impl Default for WorkspaceStatus {
    fn default() -> Self {
        Self::Disconnected
    }
}

/// Git worktree information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    /// Display name (usually branch name)
    pub name: String,
    /// Absolute path to worktree directory
    pub path: PathBuf,
    /// Current git branch
    pub branch: String,
    /// Whether this is the main worktree (repo root)
    pub is_main: bool,
}

/// Repository configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryConfig {
    /// Unique identifier
    pub id: String,
    /// Display name (usually folder name)
    pub name: String,
    /// Path to main git repository
    pub root_path: PathBuf,
    /// Known worktrees for this repo
    pub worktrees: Vec<WorktreeInfo>,
    /// Whether expanded in sidebar
    #[serde(default = "default_true")]
    pub expanded: bool,
}

fn default_true() -> bool {
    true
}

/// Tab configuration (persisted)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabConfig {
    /// Unique tab identifier
    pub id: String,
    /// Tab name (from first message)
    #[serde(default)]
    pub name: String,
    /// Path to rollout file for resuming
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollout_path: Option<PathBuf>,
    /// Backend conversation ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// ISO timestamp of creation
    pub created_at: String,
}

/// Worktree session configuration (persisted)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSessionConfig {
    /// Unique session identifier
    pub id: String,
    /// Parent repository ID
    pub repository_id: String,
    /// Path to worktree
    pub worktree_path: PathBuf,
    /// Worktree name (branch)
    pub worktree_name: String,
    /// ISO timestamp of creation
    pub created_at: String,
    /// ISO timestamp of last activity
    pub last_activity: String,
    /// Path to rollout file for resuming session (deprecated, use tabs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollout_path: Option<PathBuf>,
    /// Backend conversation ID (deprecated, use tabs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// Open tabs
    #[serde(default)]
    pub tabs: Vec<TabConfig>,
    /// Currently active tab ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
    /// Chat history (closed tabs that can be reopened)
    #[serde(default)]
    pub history: Vec<TabConfig>,
}

/// Full workspace configuration (persisted to ~/.codex/gui-workspaces.json)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GuiWorkspacesConfig {
    /// Config file version
    #[serde(default = "default_version")]
    pub version: u32,
    /// Registered repositories
    #[serde(default)]
    pub repositories: Vec<RepositoryConfig>,
    /// Session configurations
    #[serde(default)]
    pub sessions: Vec<WorktreeSessionConfig>,
    /// Currently active session ID
    pub active_session_id: Option<String>,
}

fn default_version() -> u32 {
    1
}

/// Git repository info detected from a path
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedGitInfo {
    /// Whether path is inside a git repo
    pub is_git_repo: bool,
    /// Root path of the git repository
    pub repo_root: Option<PathBuf>,
    /// Current branch name
    pub branch: Option<String>,
    /// Whether this is a worktree (not main repo)
    pub is_worktree: bool,
    /// Main repo path if this is a worktree
    pub main_repo_path: Option<PathBuf>,
}
