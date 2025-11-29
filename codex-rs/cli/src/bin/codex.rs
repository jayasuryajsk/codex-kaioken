// Thin wrapper to build the legacy `codex` binary alongside `codex-kaioken`.
// We reuse the main from src/main.rs to avoid code drift.
#[cfg(not(windows))]
#[path = "../mcp_cmd.rs"]
mod mcp_cmd;
#[cfg(not(windows))]
#[path = "../wsl_paths.rs"]
mod wsl_paths;

#[path = "../main.rs"]
mod main_bin;

fn main() -> anyhow::Result<()> {
    main_bin::real_main()
}
