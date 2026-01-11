//! Workspace manager for multi-repo, multi-worktree session management.
//!
//! Each worktree session runs as a separate codex process.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use tauri::Emitter;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::types::DetectedGitInfo;
use crate::types::GuiWorkspacesConfig;
use crate::types::RepositoryConfig;
use crate::types::WorkspaceStatus;
use crate::types::WorktreeInfo;
use crate::types::WorktreeSessionConfig;

/// Live session state (in-memory only)
pub struct LiveSession {
    pub config: WorktreeSessionConfig,
    pub status: WorkspaceStatus,
    pub process: Option<Child>,
    pub current_task: Option<String>,
}

/// Manages multiple worktree sessions
pub struct WorkspaceManager {
    /// Path to codex home (~/.codex)
    codex_home: PathBuf,
    /// Path to config file
    config_path: PathBuf,
    /// Persisted workspace configuration
    config: RwLock<GuiWorkspacesConfig>,
    /// Live sessions indexed by session ID
    sessions: RwLock<HashMap<String, Arc<RwLock<LiveSession>>>>,
}

impl WorkspaceManager {
    /// Create a new workspace manager
    pub fn new(codex_home: PathBuf) -> Self {
        let config_path = codex_home.join("gui-workspaces.json");
        let config = Self::load_config_sync(&config_path).unwrap_or_default();

        Self {
            codex_home,
            config_path,
            config: RwLock::new(config),
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Load config from disk (sync version for initialization)
    fn load_config_sync(path: &PathBuf) -> Option<GuiWorkspacesConfig> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Load config from disk
    pub async fn load_config(&self) -> Result<(), String> {
        let content = tokio::fs::read_to_string(&self.config_path)
            .await
            .map_err(|e| format!("Failed to read config: {}", e))?;
        let config: GuiWorkspacesConfig =
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;
        *self.config.write().await = config;
        Ok(())
    }

    /// Save config to disk
    pub async fn save_config(&self) -> Result<(), String> {
        let config = self.config.read().await;
        let json = serde_json::to_string_pretty(&*config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        tokio::fs::write(&self.config_path, json)
            .await
            .map_err(|e| format!("Failed to write config: {}", e))?;
        Ok(())
    }

    /// Get current workspace config
    pub async fn get_config(&self) -> GuiWorkspacesConfig {
        self.config.read().await.clone()
    }

    /// Add a repository to the workspace
    pub async fn add_repository(&self, path: PathBuf) -> Result<RepositoryConfig, String> {
        // Detect git info
        let git_info = detect_git_info(&path).await?;
        if !git_info.is_git_repo {
            return Err("Not a git repository".to_string());
        }

        let repo_root = git_info.repo_root.ok_or("Could not determine repo root")?;
        let name = repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Just use the current folder as the single worktree (no multi-worktree detection)
        let branch = git_info.branch.unwrap_or_else(|| "main".to_string());
        let worktrees = vec![WorktreeInfo {
            name: name.clone(),
            path: repo_root.clone(),
            branch,
            is_main: true,
        }];

        let repo = RepositoryConfig {
            id: Uuid::new_v4().to_string(),
            name,
            root_path: repo_root,
            worktrees,
            expanded: true,
        };

        // Add to config
        let mut config = self.config.write().await;

        // Check for duplicate
        if config
            .repositories
            .iter()
            .any(|r| r.root_path == repo.root_path)
        {
            return Err("Repository already added".to_string());
        }

        config.repositories.push(repo.clone());
        drop(config);

        self.save_config().await?;
        Ok(repo)
    }

    /// Remove a repository from the workspace
    pub async fn remove_repository(&self, repository_id: &str) -> Result<(), String> {
        let mut config = self.config.write().await;

        // Remove associated sessions
        config.sessions.retain(|s| s.repository_id != repository_id);

        // Remove repository
        config.repositories.retain(|r| r.id != repository_id);

        drop(config);
        self.save_config().await
    }

    /// Create a new git worktree
    pub async fn create_worktree(
        &self,
        repository_id: &str,
        branch_name: &str,
        worktree_path: PathBuf,
    ) -> Result<WorktreeInfo, String> {
        let config = self.config.read().await;
        let repo = config
            .repositories
            .iter()
            .find(|r| r.id == repository_id)
            .ok_or("Repository not found")?
            .clone();
        drop(config);

        // Run git worktree add
        let output = Command::new("git")
            .args(["worktree", "add", "-b", branch_name])
            .arg(&worktree_path)
            .current_dir(&repo.root_path)
            .output()
            .await
            .map_err(|e| format!("Failed to run git worktree add: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git worktree add failed: {}", stderr));
        }

        let worktree = WorktreeInfo {
            name: branch_name.to_string(),
            path: worktree_path,
            branch: branch_name.to_string(),
            is_main: false,
        };

        // Update config
        let mut config = self.config.write().await;
        if let Some(repo) = config
            .repositories
            .iter_mut()
            .find(|r| r.id == repository_id)
        {
            repo.worktrees.push(worktree.clone());
        }
        drop(config);

        self.save_config().await?;
        Ok(worktree)
    }

    /// Start a session for a worktree (spawns codex process)
    pub async fn start_session(
        &self,
        session_id: &str,
        worktree_path: PathBuf,
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        eprintln!(
            "[workspace] Starting session {} at {:?}",
            session_id, worktree_path
        );

        // Find or create session config
        let mut config = self.config.write().await;
        let session_config =
            if let Some(existing) = config.sessions.iter().find(|s| s.id == session_id) {
                existing.clone()
            } else {
                // Find repository for this worktree
                let repo = config
                    .repositories
                    .iter()
                    .find(|r| r.worktrees.iter().any(|w| w.path == worktree_path))
                    .ok_or("Worktree not found in any repository")?;

                let worktree = repo
                    .worktrees
                    .iter()
                    .find(|w| w.path == worktree_path)
                    .ok_or("Worktree not found")?;

                let new_session = WorktreeSessionConfig {
                    id: session_id.to_string(),
                    repository_id: repo.id.clone(),
                    worktree_path: worktree_path.clone(),
                    worktree_name: worktree.name.clone(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    last_activity: chrono::Utc::now().to_rfc3339(),
                    rollout_path: None, // Set by frontend after init_session returns
                    conversation_id: None, // Set by frontend after init_session returns
                    tabs: Vec::new(),
                    active_tab_id: None,
                    history: Vec::new(),
                };

                config.sessions.push(new_session.clone());
                new_session
            };

        config.active_session_id = Some(session_id.to_string());
        drop(config);
        self.save_config().await?;

        // Spawn codex process
        // For now, we'll spawn codex-kaioken CLI with --cd flag
        // Communication will be via stdout/stderr
        let mut child = Command::new("codex-kaioken")
            .args(["--cd", worktree_path.to_str().unwrap_or(".")])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn codex process: {}", e))?;

        // Set up stdout reader for events
        let stdout = child.stdout.take();
        let session_id_clone = session_id.to_string();
        let app_clone = app.clone();

        if let Some(stdout) = stdout {
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    // Emit event to frontend with session ID
                    let _ = app_clone.emit(
                        "kaioken-session-event",
                        serde_json::json!({
                            "sessionId": session_id_clone,
                            "event": line,
                        }),
                    );
                }
            });
        }

        // Create live session
        let live_session = LiveSession {
            config: session_config,
            status: WorkspaceStatus::Idle,
            process: Some(child),
            current_task: None,
        };

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.to_string(), Arc::new(RwLock::new(live_session)));

        Ok(())
    }

    /// Stop a session (kills codex process)
    pub async fn stop_session(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.remove(session_id) {
            let mut session = session.write().await;
            if let Some(mut process) = session.process.take() {
                let _ = process.kill().await;
            }
        }

        Ok(())
    }

    /// Get session status
    pub async fn get_session_status(&self, session_id: &str) -> Option<WorkspaceStatus> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            Some(session.read().await.status.clone())
        } else {
            None
        }
    }

    /// Update session status
    pub async fn set_session_status(&self, session_id: &str, status: WorkspaceStatus) {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            session.write().await.status = status;
        }
    }

    /// Set current task description for a session
    pub async fn set_current_task(&self, session_id: &str, task: Option<String>) {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            session.write().await.current_task = task;
        }
    }

    /// Update session with rollout path and conversation ID (for persistence)
    pub async fn update_session_rollout(
        &self,
        session_id: &str,
        rollout_path: Option<PathBuf>,
        conversation_id: Option<String>,
    ) -> Result<(), String> {
        let mut config = self.config.write().await;
        if let Some(session) = config.sessions.iter_mut().find(|s| s.id == session_id) {
            session.rollout_path = rollout_path;
            session.conversation_id = conversation_id;
            session.last_activity = chrono::Utc::now().to_rfc3339();
        }
        drop(config);
        self.save_config().await
    }

    /// Add or update a session entry and persist to disk
    pub async fn upsert_session(
        &self,
        session_id: String,
        repository_id: String,
        worktree_path: PathBuf,
        worktree_name: String,
        rollout_path: Option<PathBuf>,
        conversation_id: Option<String>,
    ) -> Result<(), String> {
        let mut config = self.config.write().await;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(session) = config.sessions.iter_mut().find(|s| s.id == session_id) {
            session.repository_id = repository_id;
            session.worktree_path = worktree_path;
            session.worktree_name = worktree_name;
            session.rollout_path = rollout_path;
            session.conversation_id = conversation_id;
            session.last_activity = now.clone();
        } else {
            config.sessions.push(WorktreeSessionConfig {
                id: session_id.clone(),
                repository_id,
                worktree_path,
                worktree_name,
                created_at: now.clone(),
                last_activity: now.clone(),
                rollout_path,
                conversation_id,
                tabs: Vec::new(),
                active_tab_id: None,
                history: Vec::new(),
            });
        }

        config.active_session_id = Some(session_id);
        drop(config);
        self.save_config().await
    }

    /// Get session rollout path for resuming
    pub async fn get_session_rollout(&self, session_id: &str) -> Option<PathBuf> {
        let config = self.config.read().await;
        config
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .and_then(|s| s.rollout_path.clone())
    }

    /// Update session tabs (save open tabs and active tab)
    pub async fn update_session_tabs(
        &self,
        session_id: &str,
        tabs: Vec<crate::types::TabConfig>,
        active_tab_id: Option<String>,
    ) -> Result<(), String> {
        let mut config = self.config.write().await;
        if let Some(session) = config.sessions.iter_mut().find(|s| s.id == session_id) {
            session.tabs = tabs;
            session.active_tab_id = active_tab_id;
            session.last_activity = chrono::Utc::now().to_rfc3339();
        }
        drop(config);
        self.save_config().await
    }

    /// Add tab to session history (when closing a tab with content)
    pub async fn add_to_history(
        &self,
        session_id: &str,
        tab: crate::types::TabConfig,
    ) -> Result<(), String> {
        let mut config = self.config.write().await;
        if let Some(session) = config.sessions.iter_mut().find(|s| s.id == session_id) {
            // Add to front of history (most recent first)
            session.history.insert(0, tab);
            // Keep only last 20 history items
            session.history.truncate(20);
            session.last_activity = chrono::Utc::now().to_rfc3339();
        }
        drop(config);
        self.save_config().await
    }

    /// Get session history
    pub async fn get_session_history(
        &self,
        session_id: &str,
    ) -> Vec<crate::types::TabConfig> {
        let config = self.config.read().await;
        config
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .map(|s| s.history.clone())
            .unwrap_or_default()
    }
}

/// Detect git info for a path
pub async fn detect_git_info(path: &PathBuf) -> Result<DetectedGitInfo, String> {
    // Check if inside a git repo
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        return Ok(DetectedGitInfo {
            is_git_repo: false,
            repo_root: None,
            branch: None,
            is_worktree: false,
            main_repo_path: None,
        });
    }

    // Get repo root
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {}", e))?;

    let repo_root = if output.status.success() {
        Some(PathBuf::from(
            String::from_utf8_lossy(&output.stdout).trim(),
        ))
    } else {
        None
    };

    // Get current branch
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(path)
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {}", e))?;

    let branch = if output.status.success() {
        let b = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if b.is_empty() { None } else { Some(b) }
    } else {
        None
    };

    // Check if worktree
    let output = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(path)
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {}", e))?;

    let (is_worktree, main_repo_path) = if output.status.success() {
        let common_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        let git_dir_output = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .output()
            .await
            .ok();

        if let Some(git_dir_out) = git_dir_output {
            let git_dir = PathBuf::from(String::from_utf8_lossy(&git_dir_out.stdout).trim());
            // If git-dir != git-common-dir, it's a worktree
            let is_wt = git_dir != common_dir && !common_dir.ends_with(".git");
            let main_path = if is_wt {
                common_dir.parent().map(|p| p.to_path_buf())
            } else {
                None
            };
            (is_wt, main_path)
        } else {
            (false, None)
        }
    } else {
        (false, None)
    };

    Ok(DetectedGitInfo {
        is_git_repo: true,
        repo_root,
        branch,
        is_worktree,
        main_repo_path,
    })
}

/// List all worktrees for a repository
pub async fn list_git_worktrees(repo_path: &PathBuf) -> Result<Vec<WorktreeInfo>, String> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_path)
        .output()
        .await
        .map_err(|e| format!("Failed to run git worktree list: {}", e))?;

    if !output.status.success() {
        return Err("git worktree list failed".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            // Save previous worktree if exists
            if let Some(path) = current_path.take() {
                let branch = current_branch.take().unwrap_or_else(|| "HEAD".to_string());
                let name = branch.split('/').last().unwrap_or(&branch).to_string();
                let is_main = path == *repo_path;
                worktrees.push(WorktreeInfo {
                    name,
                    path,
                    branch,
                    is_main,
                });
            }
            current_path = Some(PathBuf::from(path));
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(branch.to_string());
        }
    }

    // Don't forget the last one
    if let Some(path) = current_path {
        let branch = current_branch.unwrap_or_else(|| "HEAD".to_string());
        let name = branch.split('/').last().unwrap_or(&branch).to_string();
        let is_main = path == *repo_path;
        worktrees.push(WorktreeInfo {
            name,
            path,
            branch,
            is_main,
        });
    }

    Ok(worktrees)
}
