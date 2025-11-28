use std::sync::OnceLock;

/// The current Codex CLI version as embedded at compile time.
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The display version, optionally annotated with build metadata (git hash or timestamp).
pub fn display_version() -> &'static str {
    static DISPLAY: OnceLock<String> = OnceLock::new();
    DISPLAY.get_or_init(|| {
        let build = option_env!("CODEX_BUILD_HASH")
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(build) = build {
            format!("{CODEX_CLI_VERSION}+{build}")
        } else {
            CODEX_CLI_VERSION.to_string()
        }
    })
}
