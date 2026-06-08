use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use codex_git_utils::get_git_repo_root;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde::Serialize;
use tokio::process::Command;
use tokio::time::timeout;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::kaioken_recall_spec::create_kaioken_recall_tool;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;

pub struct KaiokenRecallHandler;

const RECALL_COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
const EXTERNAL_RECALL_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const SGREP_COMMAND_TIMEOUT: Duration = Duration::from_secs(300);
const RECALL_DEFAULT_LIMIT: usize = 8;
const RECALL_MAX_LIMIT: usize = 20;
const DEFAULT_INCLUDE_GLOB: &str = "**/*.{rs,md,ts,tsx,js,jsx,py,go}";
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

#[derive(Debug, Deserialize)]
struct KaiokenRecallArgs {
    query: String,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    budget: Option<String>,
    #[serde(default)]
    include_tests: Option<bool>,
    #[serde(default)]
    glob: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct SgrepSearchResponse {
    #[serde(default)]
    results: Vec<SgrepSearchResult>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct SgrepSearchResult {
    #[serde(default)]
    path: String,
    #[serde(default)]
    start_line: Option<i64>,
    #[serde(default)]
    end_line: Option<i64>,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    semantic_score: Option<f64>,
    #[serde(default)]
    keyword_score: Option<f64>,
    #[serde(default)]
    snippet: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecallResponse {
    status: String,
    elapsed_ms: u128,
    data: RecallData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecallData {
    query: String,
    needle: Option<String>,
    strategy: String,
    intent: Option<String>,
    budget: Option<String>,
    exact_match_count: usize,
    semantic_result_count: usize,
    output_bytes: usize,
    evidence: Vec<RecallEvidence>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RecallEvidence {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<i64>,
    source: String,
    score: f64,
    exact_matches: usize,
    reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snippet: Option<String>,
}

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for KaiokenRecallHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("kaioken_recall")
    }

    fn spec(&self) -> ToolSpec {
        create_kaioken_recall_tool()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let start = Instant::now();
        let ToolInvocation { payload, turn, .. } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "kaioken_recall handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: KaiokenRecallArgs = parse_arguments(&arguments)?;
        let query = args.query.trim();
        if query.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "query must not be empty".to_string(),
            ));
        }

        let limit = clamp_recall_limit(args.limit)?;
        let cwd = turn
            .environments
            .primary()
            .map(|environment| environment.cwd.as_path())
            .unwrap_or_else(|| {
                #[allow(deprecated)]
                turn.cwd.as_path()
            });
        let search_path = resolve_search_path(cwd, args.path.as_deref());
        verify_path_exists(&search_path).await?;

        let include_tests = args.include_tests.unwrap_or(false);
        let fallback_note =
            match run_external_kaioken_recall(&args, cwd, limit, include_tests).await {
                Ok(Some(result)) => {
                    return Ok(boxed_tool_output(FunctionToolOutput::from_text(
                        result.content,
                        Some(result.success),
                    )));
                }
                Ok(None) => Some(
                    "standalone kaioken-recall binary not found; native fallback used".to_string(),
                ),
                Err(err) => Some(format!(
                    "standalone kaioken-recall failed; native fallback used: {err}"
                )),
            };

        let intent = normalize_optional(args.intent.as_deref());
        let budget = normalize_optional(args.budget.as_deref());
        let is_exact = is_exact_recall_request(query, intent.as_deref());
        let mut data = if is_exact {
            run_exact_recall(query, intent, budget, &search_path, limit, include_tests).await?
        } else {
            run_hybrid_recall(
                query,
                intent,
                budget,
                args.glob.as_deref(),
                &search_path,
                cwd,
                limit,
                include_tests,
            )
            .await?
        };
        if let Some(note) = fallback_note {
            data.notes.insert(0, note);
        }

        data.evidence
            .sort_by(|a, b| b.score.total_cmp(&a.score).then(a.path.cmp(&b.path)));
        data.evidence.truncate(limit);
        let status = if data.strategy == "fast" && data.exact_match_count == 0 {
            "exact_not_found"
        } else if data.evidence.is_empty() {
            "no_matches"
        } else {
            "ok"
        };
        let content = format_recall_response(status, start.elapsed().as_millis(), data)?;

        Ok(boxed_tool_output(FunctionToolOutput::from_text(
            content,
            Some(status == "ok"),
        )))
    }
}

impl CoreToolRuntime for KaiokenRecallHandler {}

struct ExternalRecallResult {
    content: String,
    success: bool,
}

async fn run_external_kaioken_recall(
    args: &KaiokenRecallArgs,
    cwd: &Path,
    limit: usize,
    include_tests: bool,
) -> Result<Option<ExternalRecallResult>, String> {
    let Some(binary) = find_kaioken_recall_binary() else {
        return Ok(None);
    };

    let (repo, scope) = external_repo_and_scope(cwd, args.path.as_deref());
    let mut command = Command::new(binary);
    command
        .current_dir(cwd)
        .arg("search")
        .arg(args.query.trim())
        .arg("--repo")
        .arg(repo)
        .arg("--intent")
        .arg(standalone_recall_intent(args.intent.as_deref()))
        .arg("--budget")
        .arg(standalone_recall_budget(
            args.intent.as_deref(),
            args.budget.as_deref(),
        ))
        .arg("--limit")
        .arg(limit.to_string())
        .arg("--json");
    if let Some(scope) = scope {
        command.arg("--path").arg(scope);
    }
    if include_tests {
        command.arg("--include-tests");
    }
    if let Some(glob) = args
        .glob
        .as_deref()
        .map(str::trim)
        .filter(|glob| !glob.is_empty())
    {
        command.arg("--glob").arg(glob);
    }

    let output = timeout(EXTERNAL_RECALL_COMMAND_TIMEOUT, command.output())
        .await
        .map_err(|_| "kaioken-recall timed out".to_string())?
        .map_err(|err| format!("failed to launch kaioken-recall: {err}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "kaioken-recall exited with status {:?}: {}",
            output.status.code(),
            truncate_text(stderr.trim(), 800)
        ));
    }
    if stdout.is_empty() {
        return Err("kaioken-recall returned empty stdout".to_string());
    }
    let success = standalone_recall_status_ok(&stdout)?;
    Ok(Some(ExternalRecallResult {
        content: stdout,
        success,
    }))
}

fn find_kaioken_recall_binary() -> Option<PathBuf> {
    for name in ["CODEX_KAIOKEN_RECALL_BIN", "KAIOKEN_RECALL_BIN"] {
        if let Ok(path) = env::var(name) {
            let candidate = PathBuf::from(path);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    if let Ok(path) = which::which("kaioken-recall") {
        return Some(path);
    }

    let home = dirs::home_dir()?;
    for candidate in [
        home.join("Documents/kaioken-recall/target/release/kaioken-recall"),
        home.join(".local/bin/kaioken-recall"),
        home.join(".cargo/bin/kaioken-recall"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn external_repo_and_scope(cwd: &Path, override_path: Option<&str>) -> (PathBuf, Option<PathBuf>) {
    let repo = get_git_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let repo = repo.canonicalize().unwrap_or(repo);
    let Some(path) = override_path.map(str::trim).filter(|path| !path.is_empty()) else {
        return (repo, None);
    };

    let raw_scope = PathBuf::from(path);
    let full_scope = if raw_scope.is_absolute() {
        raw_scope
    } else {
        cwd.join(raw_scope)
    };
    let canonical_scope = full_scope.canonicalize().unwrap_or(full_scope);
    if let Ok(relative) = canonical_scope.strip_prefix(&repo) {
        return (repo, Some(relative.to_path_buf()));
    }
    (repo, Some(canonical_scope))
}

fn standalone_recall_intent(intent: Option<&str>) -> &'static str {
    match intent
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("exact" | "path") => "exact",
        Some("symbol") => "symbol",
        Some("behavior") => "behavior",
        Some("architecture" | "broad" | "deep") => "architecture",
        _ => "auto",
    }
}

fn standalone_recall_budget(intent: Option<&str>, budget: Option<&str>) -> &'static str {
    match budget
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("fast") => "fast",
        Some("deep") => "deep",
        Some("hybrid") => "hybrid",
        _ if matches!(
            intent
                .map(str::trim)
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("broad" | "deep")
        ) =>
        {
            "deep"
        }
        _ => "hybrid",
    }
}

fn standalone_recall_status_ok(stdout: &str) -> Result<bool, String> {
    let value: serde_json::Value = serde_json::from_str(stdout)
        .map_err(|err| format!("kaioken-recall returned invalid JSON: {err}"))?;
    Ok(matches!(
        value.get("status").and_then(|status| status.as_str()),
        Some("ok" | "degraded")
    ))
}

async fn verify_path_exists(path: &Path) -> Result<(), FunctionCallError> {
    tokio::fs::metadata(path).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to access `{}`: {err}", path.display()))
    })?;
    Ok(())
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn clamp_recall_limit(limit: Option<i64>) -> Result<usize, FunctionCallError> {
    let raw = limit.unwrap_or(RECALL_DEFAULT_LIMIT as i64);
    if raw <= 0 {
        return Err(FunctionCallError::RespondToModel(
            "limit must be greater than zero".to_string(),
        ));
    }
    Ok(raw.min(RECALL_MAX_LIMIT as i64) as usize)
}

fn format_recall_response(
    status: &str,
    elapsed_ms: u128,
    data: RecallData,
) -> Result<String, FunctionCallError> {
    let mut response = RecallResponse {
        status: status.to_string(),
        elapsed_ms,
        data,
    };
    let first_pass = serde_json::to_string_pretty(&response).map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to format recall response: {err}"))
    })?;
    response.data.output_bytes = first_pass.len();
    serde_json::to_string_pretty(&response).map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to format recall response: {err}"))
    })
}

async fn run_exact_recall(
    query: &str,
    intent: Option<String>,
    budget: Option<String>,
    search_path: &Path,
    limit: usize,
    include_tests: bool,
) -> Result<RecallData, FunctionCallError> {
    let needle = exact_query_needle(query);
    let (exact_match_count, mut evidence) =
        run_rg_exact_matches(search_path, &needle, limit, include_tests).await?;
    let mut notes = vec![
        "Kaioken Recall fast path used direct rg-style exact search; semantic fallback skipped."
            .to_string(),
    ];

    if exact_match_count == 0 {
        notes.push(format!(
            "Exact needle `{needle}` was not found. Returning fuzzy file/name suggestions."
        ));
        evidence =
            fuzzy_name_suggestions(search_path, query, &needle, limit, include_tests).await?;
    }

    Ok(RecallData {
        query: query.to_string(),
        needle: Some(needle),
        strategy: "fast".to_string(),
        intent,
        budget,
        exact_match_count,
        semantic_result_count: 0,
        output_bytes: 0,
        evidence,
        notes,
    })
}

async fn run_hybrid_recall(
    query: &str,
    intent: Option<String>,
    budget: Option<String>,
    glob: Option<&str>,
    search_path: &Path,
    cwd: &Path,
    limit: usize,
    include_tests: bool,
) -> Result<RecallData, FunctionCallError> {
    let mut notes = Vec::new();
    let mut evidence = run_hot_recall(query, search_path, include_tests).await?;
    let mut semantic_result_count = 0;
    let needs_deep = is_deep_recall_request(query, intent.as_deref(), budget.as_deref());
    let low_confidence = recall_low_confidence(query, &evidence, needs_deep);
    let mut strategy = if low_confidence || needs_deep {
        if needs_deep { "deep" } else { "hybrid" }
    } else {
        "hybrid-fast"
    };

    if low_confidence || needs_deep {
        if let Some(sgrep_bin) = find_sgrep_binary() {
            let semantic_limit = if needs_deep {
                limit.max(12).min(RECALL_MAX_LIMIT)
            } else {
                limit
            };
            match run_sgrep_search(
                query,
                default_glob(glob).as_deref(),
                search_path,
                semantic_limit,
                &sgrep_bin,
                cwd,
            )
            .await
            {
                Ok(results) => {
                    semantic_result_count = results.len();
                    for result in results {
                        merge_recall_evidence(
                            &mut evidence,
                            recall_evidence_from_semantic(result, search_path),
                        );
                    }
                    notes.push("Semantic fallback was used after hot retrieval.".to_string());
                }
                Err(err) => {
                    notes.push(format!(
                        "Semantic fallback failed; returning hot retrieval only: {err}"
                    ));
                    strategy = "hybrid-fast";
                }
            }
        } else {
            notes.push(
                "sgrep was not available; returning hot lexical/path retrieval only.".to_string(),
            );
            strategy = "hybrid-fast";
        }
    } else {
        notes.push(
            "Kaioken Recall returned from hot lexical/path retrieval without semantic fallback."
                .to_string(),
        );
    }

    evidence.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.path.cmp(&b.path)));
    evidence.truncate(limit);

    Ok(RecallData {
        query: query.to_string(),
        needle: None,
        strategy: strategy.to_string(),
        intent,
        budget,
        exact_match_count: evidence.iter().map(|item| item.exact_matches).sum(),
        semantic_result_count,
        output_bytes: 0,
        evidence,
        notes,
    })
}

async fn run_hot_recall(
    query: &str,
    search_path: &Path,
    include_tests: bool,
) -> Result<Vec<RecallEvidence>, FunctionCallError> {
    let terms = query_terms(query);
    let files = list_source_files(search_path).await?;
    let mut evidence = Vec::new();

    for path in files.iter() {
        if !is_source_path(path, include_tests) {
            continue;
        }
        let (score, reasons) = path_recall_score(path, &terms, query);
        if score >= 0.65 {
            merge_recall_evidence(
                &mut evidence,
                RecallEvidence {
                    path: path.clone(),
                    start_line: None,
                    end_line: None,
                    source: "path_role".to_string(),
                    score,
                    exact_matches: 0,
                    reasons,
                    snippet: None,
                },
            );
        }
    }

    for rg_hit in run_rg_term_matches(search_path, &terms, include_tests).await? {
        merge_recall_evidence(&mut evidence, rg_hit);
    }

    expand_related_files(&mut evidence, &files, &terms, query, include_tests);
    Ok(evidence)
}

async fn run_rg_exact_matches(
    root: &Path,
    needle: &str,
    limit: usize,
    include_tests: bool,
) -> Result<(usize, Vec<RecallEvidence>), FunctionCallError> {
    let mut command = new_rg_command()?;
    command
        .current_dir(root)
        .arg("--fixed-strings")
        .arg("--case-sensitive")
        .arg("--line-number")
        .arg("--no-heading")
        .arg("--color")
        .arg("never")
        .arg("--max-count")
        .arg("24");
    apply_rg_excludes(&mut command, include_tests);
    command.arg(needle).arg(".");

    let output = run_recall_command(command, "rg exact search").await?;
    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FunctionCallError::RespondToModel(format!(
            "rg exact search failed: {stderr}"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut total = 0;
    let mut evidence = Vec::new();
    for line in stdout.lines() {
        if let Some((path, line_number, text)) = parse_rg_line(line) {
            if !is_source_path(&path, include_tests) {
                continue;
            }
            total += 1;
            if evidence.len() < limit {
                evidence.push(RecallEvidence {
                    path,
                    start_line: Some(line_number),
                    end_line: Some(line_number),
                    source: "exact_rg".to_string(),
                    score: 10.0,
                    exact_matches: 1,
                    reasons: vec!["exact_match".to_string()],
                    snippet: Some(truncate_text(text.trim(), 280)),
                });
            }
        }
    }

    Ok((total, evidence))
}

async fn run_rg_term_matches(
    root: &Path,
    terms: &[String],
    include_tests: bool,
) -> Result<Vec<RecallEvidence>, FunctionCallError> {
    let selected = important_terms(terms);
    if selected.is_empty() {
        return Ok(Vec::new());
    }

    let pattern = selected.join("|");
    let mut command = new_rg_command()?;
    command
        .current_dir(root)
        .arg("--ignore-case")
        .arg("--line-number")
        .arg("--no-heading")
        .arg("--color")
        .arg("never")
        .arg("--max-count")
        .arg("5")
        .arg("--max-columns")
        .arg("240")
        .arg("--max-columns-preview");
    apply_rg_excludes(&mut command, include_tests);
    command.arg("-e").arg(pattern).arg(".");

    let output = run_recall_command(command, "rg lexical search").await?;
    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FunctionCallError::RespondToModel(format!(
            "rg lexical search failed: {stderr}"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut evidence = Vec::new();
    for line in stdout.lines().take(180) {
        if let Some((path, line_number, text)) = parse_rg_line(line) {
            if !is_source_path(&path, include_tests) {
                continue;
            }
            let matches = count_term_matches(&text, &selected);
            let (path_score, mut reasons) = path_recall_score(&path, terms, "");
            reasons.push("scoped_rg".to_string());
            evidence.push(RecallEvidence {
                path,
                start_line: Some(line_number),
                end_line: Some(line_number),
                source: "scoped_rg".to_string(),
                score: 1.0 + path_score + (matches as f64 * 0.25),
                exact_matches: matches,
                reasons,
                snippet: Some(truncate_text(text.trim(), 280)),
            });
        }
    }

    Ok(evidence)
}

async fn run_recall_command(
    mut command: Command,
    label: &str,
) -> Result<std::process::Output, FunctionCallError> {
    timeout(RECALL_COMMAND_TIMEOUT, command.output())
        .await
        .map_err(|_| FunctionCallError::RespondToModel(format!("{label} timed out")))?
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!("failed to launch {label}: {err}"))
        })
}

fn apply_rg_excludes(command: &mut Command, include_tests: bool) {
    for glob in [
        "!target/**",
        "!node_modules/**",
        "!dist/**",
        "!.git/**",
        "!coverage/**",
        "!.next/**",
        "!.turbo/**",
        "!**/*.snap",
        "!**/*.snap.new",
    ] {
        command.arg("--glob").arg(glob);
    }
    if !include_tests {
        for glob in [
            "!**/*.test.*",
            "!**/*.spec.*",
            "!**/__tests__/**",
            "!docs/**",
            "!generated/**",
        ] {
            command.arg("--glob").arg(glob);
        }
    }
}

async fn list_source_files(root: &Path) -> Result<Vec<String>, FunctionCallError> {
    let mut git = Command::new("git");
    git.current_dir(root).arg("ls-files");
    if let Ok(output) = run_recall_command(git, "git ls-files").await
        && output.status.success()
    {
        let files = parse_file_list(&output.stdout);
        if !files.is_empty() {
            return Ok(files);
        }
    }

    let mut rg = new_rg_command()?;
    rg.current_dir(root).arg("--files");
    let output = run_recall_command(rg, "rg --files").await?;
    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FunctionCallError::RespondToModel(format!(
            "file listing failed: {stderr}"
        )));
    }
    Ok(parse_file_list(&output.stdout))
}

fn parse_file_list(stdout: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(normalize_relative_path)
        .filter(|path| !path.is_empty())
        .collect()
}

async fn fuzzy_name_suggestions(
    root: &Path,
    query: &str,
    needle: &str,
    limit: usize,
    include_tests: bool,
) -> Result<Vec<RecallEvidence>, FunctionCallError> {
    let mut terms = query_terms(query);
    terms.extend(query_terms(needle));
    terms.sort();
    terms.dedup();
    let files = list_source_files(root).await?;
    let mut suggestions = Vec::new();
    for path in files {
        if !is_source_path(&path, include_tests) {
            continue;
        }
        let (score, reasons) = path_recall_score(&path, &terms, query);
        if score >= 0.5 {
            suggestions.push(RecallEvidence {
                path,
                start_line: None,
                end_line: None,
                source: "fuzzy_name".to_string(),
                score,
                exact_matches: 0,
                reasons,
                snippet: None,
            });
        }
    }
    suggestions.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.path.cmp(&b.path)));
    suggestions.truncate(limit);
    Ok(suggestions)
}

fn merge_recall_evidence(items: &mut Vec<RecallEvidence>, incoming: RecallEvidence) {
    if incoming.path.is_empty() {
        return;
    }
    if let Some(existing) = items.iter_mut().find(|item| item.path == incoming.path) {
        existing.score += incoming.score;
        existing.exact_matches += incoming.exact_matches;
        if existing.start_line.is_none() {
            existing.start_line = incoming.start_line;
            existing.end_line = incoming.end_line;
        }
        if existing.snippet.is_none() {
            existing.snippet = incoming.snippet;
        }
        let mut seen: HashSet<String> = existing.reasons.iter().cloned().collect();
        for reason in incoming.reasons {
            if seen.insert(reason.clone()) {
                existing.reasons.push(reason);
            }
        }
        if existing.source != incoming.source {
            existing.source = "hybrid".to_string();
        }
    } else {
        items.push(incoming);
    }
}

fn recall_evidence_from_semantic(result: SgrepSearchResult, root: &Path) -> RecallEvidence {
    RecallEvidence {
        path: normalize_result_path(&result.path, root),
        start_line: result.start_line,
        end_line: result.end_line.or(result.start_line),
        source: "semantic_seed".to_string(),
        score: 1.5 + best_score(&result).unwrap_or(0.0),
        exact_matches: 0,
        reasons: vec!["semantic_seed".to_string()],
        snippet: result
            .snippet
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_text(value, 360)),
    }
}

fn expand_related_files(
    evidence: &mut Vec<RecallEvidence>,
    files: &[String],
    terms: &[String],
    query: &str,
    include_tests: bool,
) {
    let mut top_paths: Vec<String> = evidence.iter().map(|item| item.path.clone()).collect();
    top_paths.sort();
    top_paths.dedup();
    top_paths.truncate(8);

    for hit_path in top_paths {
        let Some(hit_dir) = parent_dir(&hit_path) else {
            continue;
        };
        let hit_parent = parent_dir(&hit_dir);
        let mut additions = 0;
        for path in files {
            if additions >= 18 {
                break;
            }
            if !is_source_path(path, include_tests) {
                continue;
            }
            let same_dir = parent_dir(path).as_deref() == Some(hit_dir.as_str());
            let parent_neighbor = hit_parent
                .as_ref()
                .is_some_and(|parent| path.starts_with(parent));
            if !same_dir && !parent_neighbor {
                continue;
            }
            let (boost, mut reasons) = role_filename_boost(path, terms, query);
            if boost <= 0.0 {
                continue;
            }
            reasons.push("related_file_expansion".to_string());
            merge_recall_evidence(
                evidence,
                RecallEvidence {
                    path: path.clone(),
                    start_line: None,
                    end_line: None,
                    source: "related_file".to_string(),
                    score: 0.55 + boost,
                    exact_matches: 0,
                    reasons,
                    snippet: None,
                },
            );
            additions += 1;
        }
    }
}

fn recall_low_confidence(query: &str, evidence: &[RecallEvidence], needs_deep: bool) -> bool {
    if evidence.is_empty() {
        return true;
    }
    let top_score = evidence
        .iter()
        .map(|item| item.score)
        .fold(0.0_f64, f64::max);
    let has_role_hit = evidence.iter().any(|item| {
        item.reasons
            .iter()
            .any(|reason| reason.contains("boost") || reason.contains("spine"))
    });
    let broad = is_broad_query(query);

    evidence.len() < 3
        || top_score < 1.6
        || (broad && !has_role_hit)
        || (needs_deep && top_score < 3.0)
}

fn is_exact_recall_request(query: &str, intent: Option<&str>) -> bool {
    if matches!(intent, Some("exact" | "symbol" | "path")) {
        return true;
    }
    let lower = query.to_ascii_lowercase();
    if lower.contains(" exact") || lower.contains("symbol") || lower.contains("where is ") {
        return true;
    }
    tokenize_preserve(query).iter().any(|token| {
        token.len() >= 4
            && (token.contains("::")
                || token.contains('_')
                || token.contains('/')
                || token.chars().any(|ch| ch.is_ascii_uppercase()))
    })
}

fn is_deep_recall_request(query: &str, intent: Option<&str>, budget: Option<&str>) -> bool {
    matches!(intent, Some("deep" | "architecture" | "broad"))
        || matches!(budget, Some("deep"))
        || is_broad_query(query)
}

fn is_broad_query(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    [
        "architecture",
        "overview",
        "map",
        "explain",
        "codebase",
        "flow",
        "lifecycle",
        "system",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

fn exact_query_needle(query: &str) -> String {
    if let Some(quoted) = first_quoted(query)
        && !quoted.trim().is_empty()
    {
        return quoted.trim().to_string();
    }

    let tokens = tokenize_preserve(query);
    if let Some(symbol) = tokens
        .iter()
        .filter(|token| token.len() >= 3)
        .filter(|token| {
            token.contains("::")
                || token.contains('_')
                || token.contains('/')
                || token.chars().any(|ch| ch.is_ascii_uppercase())
        })
        .max_by_key(|token| token.len())
    {
        return symbol.to_string();
    }

    tokens
        .into_iter()
        .filter(|token| !is_stopword(token))
        .max_by_key(|token| token.len())
        .unwrap_or_else(|| query.trim().to_string())
}

fn first_quoted(query: &str) -> Option<String> {
    for quote in ['"', '\'', '`'] {
        let start = query.find(quote)?;
        let rest = &query[start + quote.len_utf8()..];
        if let Some(end) = rest.find(quote) {
            return Some(rest[..end].to_string());
        }
    }
    None
}

fn tokenize_preserve(query: &str) -> Vec<String> {
    query
        .split(|ch: char| {
            !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':' | '/' | '.' | '-'))
        })
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.trim_matches(|ch: char| matches!(ch, '.' | ',' | ':' | ';')))
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn query_terms(query: &str) -> Vec<String> {
    let mut expanded = String::with_capacity(query.len() * 2);
    let mut previous_lower = false;
    for ch in query.chars() {
        if ch.is_ascii_uppercase() && previous_lower {
            expanded.push(' ');
        }
        if ch.is_ascii_alphanumeric() {
            expanded.push(ch.to_ascii_lowercase());
            previous_lower = ch.is_ascii_lowercase();
        } else {
            expanded.push(' ');
            previous_lower = false;
        }
    }

    let mut terms: Vec<String> = expanded
        .split_whitespace()
        .filter(|term| term.len() >= 3)
        .filter(|term| !is_stopword(term))
        .map(ToString::to_string)
        .collect();
    terms.sort();
    terms.dedup();
    terms
}

fn important_terms(terms: &[String]) -> Vec<String> {
    let mut selected: Vec<String> = terms
        .iter()
        .filter(|term| term.len() >= 4)
        .filter(|term| !matches!(term.as_str(), "where" | "find" | "code" | "file"))
        .take(10)
        .cloned()
        .collect();
    if selected.is_empty() {
        selected = terms.iter().take(6).cloned().collect();
    }
    selected
}

fn is_stopword(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "the"
            | "and"
            | "for"
            | "with"
            | "where"
            | "what"
            | "when"
            | "how"
            | "this"
            | "that"
            | "from"
            | "into"
            | "code"
            | "file"
            | "files"
            | "find"
            | "show"
            | "tell"
            | "exact"
            | "symbol"
            | "lookup"
    )
}

fn path_recall_score(path: &str, terms: &[String], query: &str) -> (f64, Vec<String>) {
    let mut score = 0.0;
    let mut reasons = Vec::new();
    let lower = path.to_ascii_lowercase();
    for term in terms {
        if lower.contains(term) {
            score += 0.28;
            reasons.push(format!("path_token:{term}"));
        }
    }
    let (boost, boost_reasons) = role_filename_boost(path, terms, query);
    score += boost;
    reasons.extend(boost_reasons);
    (score, reasons)
}

fn role_filename_boost(path: &str, terms: &[String], query: &str) -> (f64, Vec<String>) {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(lower.as_str());
    let mut score = 0.0;
    let mut reasons = Vec::new();
    let query_lower = query.to_ascii_lowercase();

    let broad = is_broad_query(query);
    if broad
        && [
            "loader.ts",
            "registry.ts",
            "runtime.ts",
            "server.ts",
            "server.impl.ts",
            "http-registry.ts",
            "command-registry.ts",
            "dispatch.ts",
        ]
        .iter()
        .any(|spine| name == *spine)
    {
        score += 1.1;
        reasons.push("architecture_spine_boost".to_string());
    }
    if broad && (name.starts_with("get-") || name.starts_with("build-")) {
        score += 0.75;
        reasons.push("architecture_bridge_boost".to_string());
    }

    if contains_any(terms, &["dispatch", "inbound", "reply", "agent", "runner"])
        || query_lower.contains("message")
    {
        for marker in ["dispatch", "get-reply", "runner", "route-reply", "session"] {
            if lower.contains(marker) {
                score += 0.75;
                reasons.push("reply_flow_boost".to_string());
                break;
            }
        }
    }

    if contains_any(terms, &["plugin", "http", "route", "gateway", "registry"]) {
        for marker in [
            "plugin",
            "plugins-http",
            "http-registry",
            "registry",
            "loader",
        ] {
            if lower.contains(marker) {
                score += 0.7;
                reasons.push("plugin_route_boost".to_string());
                break;
            }
        }
    }

    if contains_any(terms, &["config", "schema", "validation", "validate"]) {
        for marker in ["schema", "zod-schema", "validation", "validator", "io.ts"] {
            if lower.contains(marker) {
                score += 0.7;
                reasons.push("config_schema_boost".to_string());
                break;
            }
        }
    }

    if contains_any(
        terms,
        &["channel", "session", "routing", "route", "delivery"],
    ) {
        for marker in [
            "session",
            "session-key",
            "resolve-route",
            "route",
            "delivery",
            "channel",
        ] {
            if lower.contains(marker) {
                score += 0.7;
                reasons.push("routing_boost".to_string());
                break;
            }
        }
    }

    if contains_any(terms, &["pairing", "allowlist", "approve", "gate"]) {
        for marker in ["pairing", "allowlist", "approval", "gate"] {
            if lower.contains(marker) {
                score += 0.7;
                reasons.push("pairing_gate_boost".to_string());
                break;
            }
        }
    }

    (score, reasons)
}

fn contains_any(terms: &[String], needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| terms.iter().any(|term| term == needle))
}

fn count_term_matches(text: &str, terms: &[String]) -> usize {
    let lower = text.to_ascii_lowercase();
    terms.iter().filter(|term| lower.contains(*term)).count()
}

fn parse_rg_line(line: &str) -> Option<(String, i64, String)> {
    let mut parts = line.splitn(3, ':');
    let path = normalize_relative_path(parts.next()?);
    let line_number = parts.next()?.parse::<i64>().ok()?;
    let text = parts.next().unwrap_or_default().to_string();
    Some((path, line_number, text))
}

fn normalize_relative_path(path: &str) -> String {
    path.trim()
        .trim_start_matches("./")
        .replace('\\', "/")
        .trim()
        .to_string()
}

fn normalize_result_path(path: &str, root: &Path) -> String {
    let raw = Path::new(path);
    if raw.is_absolute()
        && let Ok(relative) = raw.strip_prefix(root)
    {
        return normalize_relative_path(&relative.to_string_lossy());
    }
    normalize_relative_path(path)
}

fn parent_dir(path: &str) -> Option<String> {
    path.rsplit_once('/').map(|(dir, _)| dir.to_string())
}

fn is_source_path(path: &str, include_tests: bool) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    if lower.is_empty()
        || lower.starts_with(".git/")
        || lower.starts_with("target/")
        || lower.starts_with("node_modules/")
        || lower.starts_with("dist/")
        || lower.starts_with("coverage/")
        || lower.starts_with(".next/")
        || lower.starts_with(".turbo/")
        || lower.contains("/node_modules/")
        || lower.contains("/target/")
    {
        return false;
    }
    if !include_tests
        && (lower.contains(".test.")
            || lower.contains(".spec.")
            || lower.contains("/__tests__/")
            || lower.starts_with("docs/")
            || lower.starts_with("generated/"))
    {
        return false;
    }
    let Some(extension) = Path::new(&lower)
        .extension()
        .and_then(|value| value.to_str())
    else {
        return false;
    };
    matches!(
        extension,
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "py"
            | "go"
            | "java"
            | "kt"
            | "swift"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "cs"
            | "rb"
            | "php"
            | "scala"
            | "clj"
            | "sh"
            | "toml"
            | "yaml"
            | "yml"
            | "json"
    )
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for ch in text.chars().take(max_chars) {
        output.push(ch);
    }
    if text.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

async fn run_sgrep_search(
    query: &str,
    glob: Option<&str>,
    search_path: &Path,
    limit: usize,
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

    if let Some(glob) = glob {
        command.arg("--glob").arg(glob);
    }
    for exclude in DEFAULT_EXCLUDE_GLOBS {
        command.arg("--glob").arg(exclude);
    }

    command.arg("--path").arg(search_path).arg(query);

    let output = timeout(SGREP_COMMAND_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            FunctionCallError::RespondToModel("sgrep timed out after 300 seconds".to_string())
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
    if let Ok(path) = env::var("CODEX_KAIOKEN_SGREP") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    dirs::home_dir()
        .map(|home| home.join(".codex-kaioken/bin/sgrep"))
        .filter(|path| path.is_file())
        .or_else(|| which::which("sgrep").ok())
}

fn new_rg_command() -> Result<Command, FunctionCallError> {
    Ok(Command::new(find_rg_binary()?))
}

fn find_rg_binary() -> Result<PathBuf, FunctionCallError> {
    if let Ok(path) = env::var("CODEX_KAIOKEN_RG") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Ok(path) = which::which("rg") {
        return Ok(path);
    }

    if let Some(path) = dirs::home_dir()
        .map(|home| home.join(".cargo/bin/rg"))
        .filter(|path| path.is_file())
    {
        return Ok(path);
    }

    for path in [
        "/opt/homebrew/bin/rg",
        "/usr/local/bin/rg",
        "/opt/local/bin/rg",
        "/usr/bin/rg",
    ] {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(FunctionCallError::RespondToModel(
        "rg was not found. Install ripgrep or set CODEX_KAIOKEN_RG to its absolute path."
            .to_string(),
    ))
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

    #[cfg(unix)]
    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    #[cfg(unix)]
    impl EnvVarGuard {
        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = env::var_os(key);
            // Tests run with a single test thread when this guard is used.
            unsafe {
                env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    #[cfg(unix)]
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(previous) = &self.previous {
                    env::set_var(self.key, previous);
                } else {
                    env::remove_var(self.key);
                }
            }
        }
    }

    #[test]
    fn recall_detects_exact_symbol_queries() {
        assert!(is_exact_recall_request(
            "find where dispatchInboundMessage is defined",
            None,
        ));
        assert!(is_exact_recall_request("anything", Some("symbol")));
        assert!(!is_exact_recall_request(
            "where are inbound messages routed into replies",
            Some("behavior"),
        ));
    }

    #[test]
    fn recall_extracts_symbol_needle() {
        assert_eq!(
            exact_query_needle("find where loadOpenClawPlugins is defined"),
            "loadOpenClawPlugins",
        );
        assert_eq!(
            exact_query_needle("find `src/gateway/server-methods.ts`"),
            "src/gateway/server-methods.ts",
        );
    }

    #[test]
    fn recall_scores_architecture_spine_files() {
        let terms = query_terms("map gateway plugin channel architecture");
        let (score, reasons) =
            role_filename_boost("src/plugins/loader.ts", &terms, "map architecture");
        assert!(score >= 1.0);
        assert!(reasons.iter().any(|reason| reason.contains("spine")));
    }

    #[test]
    fn standalone_bridge_maps_tool_hints_to_supported_cli_values() {
        assert_eq!(standalone_recall_intent(None), "auto");
        assert_eq!(standalone_recall_intent(Some("path")), "exact");
        assert_eq!(standalone_recall_intent(Some("broad")), "architecture");
        assert_eq!(standalone_recall_budget(None, None), "hybrid");
        assert_eq!(standalone_recall_budget(Some("broad"), None), "deep");
        assert_eq!(standalone_recall_budget(None, Some("fast")), "fast");
    }

    #[test]
    fn standalone_bridge_resolves_relative_and_absolute_scope() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let file = src.join("lib.rs");
        std::fs::write(&file, "fn target() {}\n").unwrap();
        let canonical_repo = temp.path().canonicalize().unwrap();

        let (repo, scope) = external_repo_and_scope(temp.path(), Some("src/lib.rs"));
        assert_eq!(repo, canonical_repo);
        assert_eq!(scope.unwrap(), PathBuf::from("src/lib.rs"));

        let (repo, scope) = external_repo_and_scope(temp.path(), Some(&file.display().to_string()));
        assert_eq!(repo, canonical_repo);
        assert_eq!(scope.unwrap(), PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn standalone_bridge_reads_json_status() {
        assert!(standalone_recall_status_ok(r#"{"status":"ok"}"#).unwrap());
        assert!(standalone_recall_status_ok(r#"{"status":"degraded"}"#).unwrap());
        assert!(!standalone_recall_status_ok(r#"{"status":"miss"}"#).unwrap());
        assert!(standalone_recall_status_ok("not json").is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn standalone_bridge_invokes_external_binary_with_supported_args() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "fn target() {}\n").unwrap();

        let argv_file = temp.path().join("argv.txt");
        let argv_file_quoted = argv_file.to_string_lossy().replace('\'', "'\\''");
        let fake_binary = temp.path().join("kaioken-recall");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s\\n' '{{\"status\":\"ok\",\"evidence\":[{{\"path\":\"src/lib.rs\",\"start_line\":1}}]}}'\n",
            argv_file_quoted
        );
        std::fs::write(&fake_binary, script).unwrap();
        let mut permissions = std::fs::metadata(&fake_binary).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_binary, permissions).unwrap();

        let _guard = EnvVarGuard::set_path("CODEX_KAIOKEN_RECALL_BIN", &fake_binary);
        let args = KaiokenRecallArgs {
            query: "where target lives".to_string(),
            limit: None,
            path: Some("src/lib.rs".to_string()),
            intent: Some("broad".to_string()),
            budget: None,
            include_tests: None,
            glob: Some("src/**/*.rs".to_string()),
        };

        let result = run_external_kaioken_recall(&args, &repo, 3, true)
            .await
            .unwrap()
            .unwrap();

        assert!(result.success);
        assert!(result.content.contains(r#""path":"src/lib.rs""#));
        let argv = std::fs::read_to_string(argv_file).unwrap();
        let argv_lines = argv.lines().collect::<Vec<_>>();
        assert_eq!(argv_lines[0], "search");
        assert_eq!(argv_lines[1], "where target lives");
        assert_eq!(
            value_after(&argv_lines, "--repo"),
            repo.canonicalize().unwrap()
        );
        assert_eq!(
            value_after(&argv_lines, "--intent"),
            PathBuf::from("architecture")
        );
        assert_eq!(value_after(&argv_lines, "--budget"), PathBuf::from("deep"));
        assert_eq!(value_after(&argv_lines, "--limit"), PathBuf::from("3"));
        assert!(argv_lines.contains(&"--json"));
        assert_eq!(
            value_after(&argv_lines, "--path"),
            PathBuf::from("src/lib.rs")
        );
        assert!(argv_lines.contains(&"--include-tests"));
        assert_eq!(
            value_after(&argv_lines, "--glob"),
            PathBuf::from("src/**/*.rs")
        );
    }

    #[cfg(unix)]
    fn value_after(lines: &[&str], flag: &str) -> PathBuf {
        let index = lines
            .iter()
            .position(|line| *line == flag)
            .unwrap_or_else(|| panic!("missing {flag}"));
        PathBuf::from(lines[index + 1])
    }
}
