use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::process::Command;
use tokio::time::timeout;

use crate::function_tool::FunctionCallError;
use crate::git_info::get_git_repo_root;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct SemanticSearchHandler;

const DEFAULT_LIMIT: i64 = 25;
const MAX_LIMIT: i64 = 50;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const INDEX_TIMEOUT: Duration = Duration::from_secs(180);

const DEFAULT_INCLUDE_GLOB: &str = "**/*.{rs,md,ts,tsx,py,go}";
const DEFAULT_EXCLUDE_GLOBS: &[&str] = &[
    "!target/**",
    "!node_modules/**",
    "!dist/**",
    "!.git/**",
    "!coverage/**",
    "!snapshots/**",
    "!**/*.snap",
    "!**/*.snap.new",
    "!.next/**",
    "!.turbo/**",
];

#[derive(Deserialize)]
struct SemanticSearchArgs {
    query: String,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    context: bool,
    #[serde(default)]
    filters: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, Default)]
struct SgrepSearchResponse {
    #[serde(default)]
    results: Vec<SgrepSearchResult>,
}

#[derive(Debug, Deserialize, Default)]
struct SgrepSearchResult {
    #[serde(default)]
    path: String,
    #[serde(default)]
    start_line: Option<i64>,
    #[serde(default)]
    end_line: Option<i64>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    semantic_score: Option<f64>,
    #[serde(default)]
    keyword_score: Option<f64>,
    #[serde(default)]
    snippet: Option<String>,
}

#[async_trait]
impl ToolHandler for SemanticSearchHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation { payload, turn, .. } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "semantic_search handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: SemanticSearchArgs = serde_json::from_str(&arguments).map_err(|err| {
            FunctionCallError::RespondToModel(format!("failed to parse function arguments: {err}"))
        })?;

        let query = args.query.trim();
        if query.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "query must not be empty".to_string(),
            ));
        }

        let limit = clamp_limit(args.limit)?;
        let search_path = resolve_search_path(&turn.cwd, args.path.as_deref());

        verify_path_exists(&search_path).await?;

        let glob = default_glob(args.glob.as_deref());

        let Some(sgrep_bin) = find_sgrep_binary() else {
            return Err(FunctionCallError::RespondToModel(
                "sgrep not found; install it or ensure it is on PATH".to_string(),
            ));
        };

        let results = run_sgrep_search(
            query,
            glob.as_deref(),
            &search_path,
            limit,
            args.context,
            args.filters.as_ref(),
            &sgrep_bin,
            &turn.cwd,
        )
        .await?;

        if results.is_empty() {
            if let Some(refreshed) = try_reindex_and_search(
                query,
                glob.as_deref(),
                &search_path,
                limit,
                args.context,
                args.filters.as_ref(),
                &sgrep_bin,
                &turn.cwd,
            )
            .await?
            {
                return Ok(ToolOutput::Function {
                    content: refreshed.join("\n"),
                    content_items: None,
                    success: Some(true),
                });
            }

            return Ok(ToolOutput::Function {
                content: "No semantic matches found (index refreshed).".to_string(),
                content_items: None,
                success: Some(false),
            });
        }

        let formatted = format_results(&results);
        Ok(ToolOutput::Function {
            content: formatted.join("\n"),
            content_items: None,
            success: Some(true),
        })
    }
}

async fn verify_path_exists(path: &Path) -> Result<(), FunctionCallError> {
    tokio::fs::metadata(path).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to access `{}`: {err}", path.display()))
    })?;
    Ok(())
}

fn clamp_limit(limit: Option<i64>) -> Result<usize, FunctionCallError> {
    let raw = limit.unwrap_or(DEFAULT_LIMIT);
    if raw <= 0 {
        return Err(FunctionCallError::RespondToModel(
            "limit must be greater than zero".to_string(),
        ));
    }
    Ok(raw.min(MAX_LIMIT) as usize)
}

async fn run_sgrep_search(
    query: &str,
    glob: Option<&str>,
    search_path: &Path,
    limit: usize,
    include_context: bool,
    filters: Option<&HashMap<String, String>>,
    sgrep_bin: &Path,
    cwd: &Path,
) -> Result<Vec<SgrepSearchResult>, FunctionCallError> {
    let mut command = Command::new(sgrep_bin);
    command
        .current_dir(cwd)
        .arg("search")
        .arg("--json")
        .arg("--limit")
        .arg(limit.to_string());
    apply_sgrep_env(&mut command);

    if include_context {
        command.arg("--context");
    }

    if let Some(glob) = glob {
        command.arg("--glob").arg(glob);
    }
    for exclude in DEFAULT_EXCLUDE_GLOBS {
        command.arg("--glob").arg(exclude);
    }

    if let Some(filters) = filters {
        for (key, value) in filters {
            command.arg("--filters").arg(format!("{key}={value}"));
        }
    }

    command.arg("--path").arg(search_path).arg(query);

    let output = timeout(COMMAND_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            FunctionCallError::RespondToModel("sgrep timed out after 120 seconds".to_string())
        })?
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to launch sgrep: {err}. Ensure sgrep is installed and on PATH."
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FunctionCallError::RespondToModel(format!(
            "sgrep failed: {stderr}"
        )));
    }

    let response: SgrepSearchResponse = serde_json::from_slice(&output.stdout).map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to parse sgrep output as JSON: {err}"))
    })?;

    Ok(response.results)
}

async fn try_reindex_and_search(
    query: &str,
    glob: Option<&str>,
    search_path: &Path,
    limit: usize,
    include_context: bool,
    filters: Option<&HashMap<String, String>>,
    sgrep_bin: &Path,
    cwd: &Path,
) -> Result<Option<Vec<String>>, FunctionCallError> {
    run_sgrep_index(search_path, sgrep_bin, cwd).await?;
    let refreshed = run_sgrep_search(
        query,
        glob,
        search_path,
        limit,
        include_context,
        filters,
        sgrep_bin,
        cwd,
    )
    .await?;
    if refreshed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format_results(&refreshed)))
    }
}

async fn run_sgrep_index(
    search_path: &Path,
    sgrep_bin: &Path,
    cwd: &Path,
) -> Result<(), FunctionCallError> {
    let mut command = Command::new(sgrep_bin);
    command
        .current_dir(cwd)
        .arg("index")
        .arg("--path")
        .arg(search_path);
    apply_sgrep_env(&mut command);

    let output = timeout(INDEX_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            FunctionCallError::RespondToModel("sgrep index timed out after 180 seconds".to_string())
        })?
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to launch sgrep index: {err}. Ensure sgrep is installed and on PATH."
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FunctionCallError::RespondToModel(format!(
            "sgrep index failed: {stderr}"
        )));
    }

    Ok(())
}

fn default_glob(glob: Option<&str>) -> Option<String> {
    glob.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| Some(DEFAULT_INCLUDE_GLOB.to_string()))
}

fn resolve_search_path(cwd: &Path, override_path: Option<&str>) -> PathBuf {
    if let Some(custom) = override_path {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return cwd.join(trimmed);
        }
    }

    get_git_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf())
}

fn find_sgrep_binary() -> Option<PathBuf> {
    bundled_sgrep_path()
        .filter(|path| path.is_file())
        .or_else(|| which::which("sgrep").ok())
}

fn bundled_sgrep_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".codex-kaioken/bin/sgrep"))
}

fn format_results(results: &[SgrepSearchResult]) -> Vec<String> {
    results.iter().map(format_result).collect()
}

fn format_result(result: &SgrepSearchResult) -> String {
    let span = format_span(result.start_line, result.end_line);
    let language = result
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let snippet = result
        .snippet
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|text| text.replace('\n', " "));
    let score = best_score(result);

    let mut parts = Vec::new();
    if let Some(score) = score {
        parts.push(format!("score={score:.3}"));
    }
    if let Some(language) = language {
        parts.push(format!("[{language}]"));
    }
    if let Some(snippet) = snippet {
        parts.push(snippet);
    }

    if parts.is_empty() {
        format!("{path}{span}", path = result.path)
    } else {
        format!(
            "{path}{span} {details}",
            path = result.path,
            details = parts.join(" ")
        )
    }
}

fn format_span(start: Option<i64>, end: Option<i64>) -> String {
    match (start, end) {
        (Some(begin), Some(finish)) if finish > begin => format!(":{begin}-{finish}"),
        (Some(begin), _) => format!(":{begin}"),
        _ => String::new(),
    }
}

fn best_score(result: &SgrepSearchResult) -> Option<f64> {
    result
        .score
        .or(result.semantic_score)
        .or(result.keyword_score)
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn formats_result_with_span_and_score() {
        let result = SgrepSearchResult {
            path: "src/lib.rs".to_string(),
            start_line: Some(10),
            end_line: Some(20),
            language: Some("rust".to_string()),
            score: Some(0.9123),
            semantic_score: Some(0.8),
            keyword_score: Some(0.5),
            snippet: Some("fn demo() { println!(\"hi\"); }".to_string()),
        };

        let formatted = format_result(&result);
        assert_eq!(
            formatted,
            "src/lib.rs:10-20 score=0.912 [rust] fn demo() { println!(\"hi\"); }"
        );
    }

    #[test]
    fn formats_result_without_optional_fields() {
        let result = SgrepSearchResult {
            path: "src/main.rs".to_string(),
            start_line: None,
            end_line: None,
            language: None,
            score: None,
            semantic_score: None,
            keyword_score: None,
            snippet: None,
        };

        let formatted = format_result(&result);
        assert_eq!(formatted, "src/main.rs");
    }
}
