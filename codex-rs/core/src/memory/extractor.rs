//! Memory extraction from agent actions.
//!
//! This module observes tool executions and extracts learnings
//! to store as persistent memories.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::store::MemoryStore;
use super::types::Memory;
use super::types::MemoryType;

/// Tracks recent command failures to detect when they get fixed.
#[derive(Debug)]
struct FailedAttempt {
    command: String,
    error: String,
    timestamp: Instant,
}

/// Extracts memories from agent actions.
pub struct MemoryExtractor {
    store: Arc<MemoryStore>,
    /// Recent failures waiting to be matched with fixes.
    recent_failures: Mutex<HashMap<String, FailedAttempt>>,
    /// Maximum age for a failure to be considered for fix detection.
    max_failure_age_secs: u64,
}

impl MemoryExtractor {
    /// Create a new memory extractor.
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self {
            store,
            recent_failures: Mutex::new(HashMap::new()),
            max_failure_age_secs: 300, // 5 minutes
        }
    }

    /// Called when a command execution completes.
    pub async fn on_exec_complete(
        &self,
        command: &str,
        exit_code: i32,
        stdout: &str,
        stderr: &str,
        cwd: &std::path::Path,
    ) -> Vec<Memory> {
        let mut memories = Vec::new();

        if exit_code == 0 {
            // Success - check if this fixes a recent failure
            if let Some(lesson) = self.check_for_fix(command, stdout).await {
                memories.push(lesson);
            }

            // Extract facts from successful commands
            if let Some(fact) = self.extract_from_success(command, stdout, cwd).await {
                memories.push(fact);
            }
        } else {
            // Failure - record for later fix detection
            self.record_failure(command, stderr).await;

            // Extract lesson from the failure itself
            if let Some(lesson) = self.extract_from_failure(command, stderr, cwd).await {
                memories.push(lesson);
            }
        }

        // Store all extracted memories
        for memory in &memories {
            if let Err(e) = self.store.insert(memory).await {
                warn!("Failed to store memory: {}", e);
            }
        }

        memories
    }

    /// Called when a file is read.
    pub async fn on_file_read(&self, path: &std::path::Path, content: &str) -> Vec<Memory> {
        let mut memories = Vec::new();

        // Extract location memory
        if let Some(location) = self.extract_location_from_file(path, content) {
            if !self
                .store
                .exists_similar(&location.content, MemoryType::Location)
                .await
                .unwrap_or(true)
            {
                memories.push(location);
            }
        }

        // Extract patterns from file content
        for pattern in self.extract_patterns_from_file(path, content) {
            if !self
                .store
                .exists_similar(&pattern.content, MemoryType::Pattern)
                .await
                .unwrap_or(true)
            {
                memories.push(pattern);
            }
        }

        // Store memories
        for memory in &memories {
            if let Err(e) = self.store.insert(memory).await {
                warn!("Failed to store memory from file read: {}", e);
            }
        }

        memories
    }

    /// Called when a file is edited.
    pub async fn on_file_edit(&self, path: &std::path::Path, _diff: &str) -> Vec<Memory> {
        let mut memories = Vec::new();

        // Extract location if this is a significant file
        let location = Memory::new(
            MemoryType::Location,
            format!("{} was edited", path.display()),
        )
        .with_source_file(path.to_path_buf())
        .with_context("from file edit");

        // Only store if not already known
        if !self
            .store
            .exists_similar(&location.content, MemoryType::Location)
            .await
            .unwrap_or(true)
        {
            if let Err(e) = self.store.insert(&location).await {
                warn!("Failed to store edit location: {}", e);
            } else {
                memories.push(location);
            }
        }

        memories
    }

    /// Called when the user explicitly requests to remember something.
    pub async fn on_user_remember(&self, input: &str) -> anyhow::Result<Memory> {
        // Parse the input to determine memory type
        let (memory_type, content) = self.parse_user_memory(input);

        let memory = Memory::new(memory_type, content)
            .with_context("explicitly remembered by user")
            .with_importance(0.9); // User-explicit memories are important

        self.store.insert(&memory).await?;

        info!("Stored user memory: {}", memory.id);
        Ok(memory)
    }

    /// Called when the user corrects the agent.
    pub async fn on_user_correction(&self, original: &str, correction: &str) -> Option<Memory> {
        let content = format!(
            "User corrected: '{}' should be '{}'",
            truncate(original, 100),
            truncate(correction, 100)
        );

        let memory = Memory::new(MemoryType::Preference, content)
            .with_context(format!("Original: {}", original))
            .with_importance(0.85);

        if let Err(e) = self.store.insert(&memory).await {
            warn!("Failed to store correction memory: {}", e);
            return None;
        }

        Some(memory)
    }

    /// Record a command failure for later fix detection.
    async fn record_failure(&self, command: &str, error: &str) {
        let mut failures = self.recent_failures.lock().await;

        // Clean up old failures
        let now = Instant::now();
        failures
            .retain(|_, f| now.duration_since(f.timestamp).as_secs() < self.max_failure_age_secs);

        // Compute a key based on the command type
        let key = self.failure_key(command);

        failures.insert(
            key,
            FailedAttempt {
                command: command.to_string(),
                error: error.to_string(),
                timestamp: now,
            },
        );

        debug!("Recorded failure: {}", truncate(command, 50));
    }

    /// Check if a successful command fixes a recent failure.
    async fn check_for_fix(&self, command: &str, _output: &str) -> Option<Memory> {
        let mut failures = self.recent_failures.lock().await;
        let key = self.failure_key(command);

        if let Some(failed) = failures.remove(&key) {
            // This command succeeded where a similar one failed
            let lesson_content = format!(
                "When '{}' fails with '{}', try '{}'",
                truncate(&failed.command, 50),
                truncate(&failed.error, 100),
                truncate(command, 50)
            );

            let memory = Memory::new(MemoryType::Lesson, lesson_content)
                .with_context(format!(
                    "Failed command: {}\nError: {}\nFix: {}",
                    failed.command, failed.error, command
                ))
                .with_importance(0.95); // Lessons from fixes are very important

            info!(
                "Learned fix: {} -> {}",
                truncate(&failed.command, 30),
                truncate(command, 30)
            );
            return Some(memory);
        }

        None
    }

    /// Generate a key for matching similar commands.
    fn failure_key(&self, command: &str) -> String {
        // Extract the base command (first word or two)
        let parts: Vec<&str> = command.split_whitespace().take(2).collect();
        parts.join(" ")
    }

    /// Extract a fact from a successful command.
    async fn extract_from_success(
        &self,
        command: &str,
        output: &str,
        cwd: &std::path::Path,
    ) -> Option<Memory> {
        // Detect package manager
        if command.starts_with("npm ")
            || command.starts_with("pnpm ")
            || command.starts_with("yarn ")
        {
            let pm = command.split_whitespace().next()?;
            let content = format!("Project uses {} as package manager", pm);

            if !self
                .store
                .exists_similar(&content, MemoryType::Fact)
                .await
                .ok()?
            {
                return Some(
                    Memory::new(MemoryType::Fact, content)
                        .with_source_file(cwd.to_path_buf())
                        .with_context(format!("detected from running: {}", truncate(command, 50))),
                );
            }
        }

        // Detect build tools
        if command.starts_with("cargo ") {
            let content = "Project uses Cargo/Rust".to_string();
            if !self
                .store
                .exists_similar(&content, MemoryType::Fact)
                .await
                .ok()?
            {
                return Some(
                    Memory::new(MemoryType::Fact, content).with_source_file(cwd.to_path_buf()),
                );
            }
        }

        // Detect test frameworks from output
        if command.contains("test") {
            if output.contains("jest") || output.contains("PASS") || output.contains("FAIL") {
                let content = "Project uses Jest for testing".to_string();
                if !self
                    .store
                    .exists_similar(&content, MemoryType::Fact)
                    .await
                    .ok()?
                {
                    return Some(Memory::new(MemoryType::Fact, content));
                }
            }
            if output.contains("pytest") || output.contains("collected") {
                let content = "Project uses pytest for testing".to_string();
                if !self
                    .store
                    .exists_similar(&content, MemoryType::Fact)
                    .await
                    .ok()?
                {
                    return Some(Memory::new(MemoryType::Fact, content));
                }
            }
        }

        None
    }

    /// Extract a lesson from a command failure.
    async fn extract_from_failure(
        &self,
        command: &str,
        stderr: &str,
        _cwd: &std::path::Path,
    ) -> Option<Memory> {
        // Common failure patterns
        let content = if stderr.contains("ENOENT") || stderr.contains("not found") {
            Some(format!(
                "Command '{}' failed: required binary or file not found",
                truncate(command, 30)
            ))
        } else if stderr.contains("permission denied") {
            Some(format!(
                "Command '{}' requires elevated permissions",
                truncate(command, 30)
            ))
        } else if stderr.contains("ECONNREFUSED") || stderr.contains("connection refused") {
            Some(format!(
                "Command '{}' failed: service not running or connection refused",
                truncate(command, 30)
            ))
        } else {
            None
        };

        content.map(|c| {
            Memory::new(MemoryType::Lesson, c)
                .with_context(format!("Error: {}", truncate(stderr, 200)))
                .with_importance(0.8)
        })
    }

    /// Extract location information from a file.
    fn extract_location_from_file(&self, path: &std::path::Path, content: &str) -> Option<Memory> {
        let filename = path.file_name()?.to_string_lossy();

        // Detect file purpose from name and content
        let purpose = if filename.contains("test")
            || content.contains("#[test]")
            || content.contains("describe(")
        {
            Some("tests")
        } else if filename.contains("config")
            || filename.ends_with(".toml")
            || filename.ends_with(".json")
        {
            Some("configuration")
        } else if filename.contains("route")
            || filename.contains("handler")
            || content.contains("Router")
        {
            Some("routing/handlers")
        } else if filename.contains("model")
            || content.contains("struct") && content.contains("derive")
        {
            Some("data models")
        } else if filename.contains("auth")
            || content.contains("authenticate")
            || content.contains("login")
        {
            Some("authentication")
        } else {
            None
        }?;

        let dir = path.parent()?.to_string_lossy();
        let content_str = format!("{} contains {}", dir, purpose);

        Some(
            Memory::new(MemoryType::Location, content_str)
                .with_source_file(path.to_path_buf())
                .with_context(format!("detected from {}", filename)),
        )
    }

    /// Extract patterns from file content.
    fn extract_patterns_from_file(&self, path: &std::path::Path, content: &str) -> Vec<Memory> {
        let mut patterns = Vec::new();

        // Detect test location patterns
        if path.to_string_lossy().contains("__tests__") {
            patterns.push(
                Memory::new(
                    MemoryType::Pattern,
                    "Tests are in __tests__ directories".to_string(),
                )
                .with_source_file(path.to_path_buf()),
            );
        }

        // Detect Result pattern in Rust
        if path.extension().map_or(false, |e| e == "rs") {
            if content.contains("-> Result<") {
                let func_count = content.matches("-> Result<").count();
                if func_count >= 3 {
                    patterns.push(
                        Memory::new(
                            MemoryType::Pattern,
                            "Functions return Result<T, E> for error handling".to_string(),
                        )
                        .with_source_file(path.to_path_buf()),
                    );
                }
            }
        }

        // Detect async patterns
        if content.contains("async fn") || content.contains("async function") {
            patterns.push(
                Memory::new(
                    MemoryType::Pattern,
                    "Codebase uses async/await patterns".to_string(),
                )
                .with_source_file(path.to_path_buf()),
            );
        }

        patterns
    }

    /// Parse user input for /remember command.
    fn parse_user_memory(&self, input: &str) -> (MemoryType, String) {
        let input = input.trim();

        // Check for type prefixes
        if let Some(rest) = input.strip_prefix("decision:") {
            return (MemoryType::Decision, rest.trim().to_string());
        }
        if let Some(rest) = input.strip_prefix("lesson:") {
            return (MemoryType::Lesson, rest.trim().to_string());
        }
        if let Some(rest) = input.strip_prefix("preference:") {
            return (MemoryType::Preference, rest.trim().to_string());
        }
        if let Some(rest) = input.strip_prefix("pattern:") {
            return (MemoryType::Pattern, rest.trim().to_string());
        }
        if let Some(rest) = input.strip_prefix("location:") {
            return (MemoryType::Location, rest.trim().to_string());
        }

        // Infer type from content
        let lower = input.to_lowercase();
        if lower.contains("always ") || lower.contains("never ") || lower.contains("should ") {
            (MemoryType::Preference, input.to_string())
        } else if lower.contains("because") || lower.contains("decided") || lower.contains("chose")
        {
            (MemoryType::Decision, input.to_string())
        } else if lower.contains("learned")
            || lower.contains("don't forget")
            || lower.contains("remember to")
        {
            (MemoryType::Lesson, input.to_string())
        } else if lower.contains(" in ") && (lower.contains("src/") || lower.contains("lib/")) {
            (MemoryType::Location, input.to_string())
        } else {
            (MemoryType::Fact, input.to_string())
        }
    }
}

/// Truncate a string to a maximum length.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len { s } else { &s[..max_len] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::MemoryConfig;
    use tempfile::TempDir;

    async fn create_test_extractor() -> (MemoryExtractor, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = MemoryStore::init(temp_dir.path(), MemoryConfig::default())
            .await
            .unwrap();
        let extractor = MemoryExtractor::new(Arc::new(store));
        (extractor, temp_dir)
    }

    #[tokio::test]
    async fn test_parse_user_memory() {
        let (extractor, _dir) = create_test_extractor().await;

        let (mem_type, content) = extractor.parse_user_memory("decision: use Axum for HTTP");
        assert_eq!(mem_type, MemoryType::Decision);
        assert_eq!(content, "use Axum for HTTP");

        let (mem_type, _) = extractor.parse_user_memory("always use explicit errors");
        assert_eq!(mem_type, MemoryType::Preference);

        let (mem_type, _) = extractor.parse_user_memory("auth code in src/auth/");
        assert_eq!(mem_type, MemoryType::Location);
    }

    #[tokio::test]
    async fn test_failure_tracking() {
        let (extractor, _dir) = create_test_extractor().await;

        // Record a failure
        extractor
            .on_exec_complete(
                "npm install",
                1,
                "",
                "ENOENT: npm not found",
                std::path::Path::new("/"),
            )
            .await;

        // Check that it was recorded
        let failures = extractor.recent_failures.lock().await;
        assert!(!failures.is_empty());
    }
}
