# Codex CLI (Rust Implementation)

Fork branding: this distribution ships as `codex-kaioken` only. Install it to a path that won't shadow upstream `codex` (e.g., a separate prefix or a dedicated `bin` directory on your PATH).

### Highlights

- **Plan-first workflow** – toggle `/plan` (or press <kbd>Shift</kbd>+<kbd>Tab</kbd>) to force Codex to propose a checklist before it touches your repo. The composer turns cyan so you know the next prompt will draft a plan, and `/settings` lets you choose coarse, detailed, or auto plan granularity.
- **Session settings palette** – `/settings` exposes Kaioken-only switches for plan detail, rate-limit footer visibility, and subagent concurrency (1–8 helpers) without hand-editing `config.toml`.
- **Real-time subagent UI** – helper agents stream their tool calls, diffs, and reasoning into dedicated panes so you can watch exploration tasks unfold turn by turn.
- **Parallel orchestration** – the main session can spin up specialized subagents (explore, infra, tests, etc.) and merge their findings back into the primary transcript automatically.
- **Semantic search tool** – when [`sgrep`](https://github.com/Rika-Labs/sgrep) is on `PATH`, Kaioken registers `semantic_search` for fast ranked code lookups.
- **Snapshot-aware undo & checkpoints** – `/undo` rolls back the last ghost snapshot, and `/checkpoint save|list|restore` gives you named save points without touching git. The inline status indicator clears as soon as checkpoints finish so you aren’t stuck watching a phantom spinner.
- **MCP + sandbox tooling** – everything from upstream Codex (execpolicy, MCP client/server, approvals, sandbox helpers) remains available, but defaults are tuned for Kaioken’s power workflows.

### Installing without colliding with upstream `codex`

- Package names should differ from upstream (e.g., `codex-kaioken` for brew/npm); do not reuse the `codex` package name.
- The installed binary is `codex-kaioken` only; no `codex` alias is shipped.
- Manual install example to keep it isolated:
  ```
  mkdir -p ~/.local/bin
  mv codex-kaioken ~/.local/bin/
  export PATH="$HOME/.local/bin:$PATH"
  ```
- Faster rebuilds without reinstalling dependencies: `just install-kaioken` builds once with the workspace target dir and copies the binary into `~/.codex-kaioken/bin` (honors the workspace `Cargo.lock` via `--locked`).

### Semantic search

If [`sgrep`](https://github.com/Rika-Labs/sgrep) is installed and on `PATH`, Codex Kaioken exposes a `semantic_search` tool that shells out to `sgrep search --json` for ranked code results. When `sgrep` is absent, the tool is not registered.

### Plan-first workflow

Codex Kaioken can capture a plan before it edits your tree. Press <kbd>Shift</kbd>+<kbd>Tab</kbd> (or run `/plan`) to toggle Plan Mode. When enabled, the next prompt is held while Codex drafts a checklist via the `update_plan` tool. The plan appears in a modal with three actions:

- <kbd>Enter</kbd>: approve and execute the plan.
- <kbd>F</kbd>: provide feedback; the modal closes so you can type adjustments, then Codex refreshes the plan.
- <kbd>Esc</kbd>: cancel the workflow entirely.

Use `/plan` again at any time to pop the review UI back up or simply disable the workflow.

Plan fidelity is configurable. Open `/settings` and choose whether Kaioken should generate coarse (3–4) steps, detailed (6–10) implementation tasks, or auto-select based on the request. The composer lights up in cyan while plan mode is active so you can see at a glance when a request will be routed through the planning loop.

### Live subagent harness

The `subagent_run` tool is surfaced prominently in Kaioken. When Codex spins up helper agents (“Read the repo with 3 subagents”), each child session streams its own exec/patch/log history into the conversation so you can watch every tool call, diff, and summary in real time. By default Kaioken allows four concurrent helpers; power users can raise or lower that ceiling (1–8) in `/settings` under “Subagent concurrency” to balance parallelism with local resource limits.

### Session settings palette

Use `/settings` to toggle several Kaioken-only UX affordances without editing `config.toml` by hand:

- Show or hide the session usage / rate-limit footer.
- Pick the default plan detail preference (auto, coarse, detailed).
- Adjust the maximum number of concurrent subagent tasks.

All of these toggles persist by writing to `~/.codex/config.toml`, so your preferences survive future upgrades.

### Checkpoints

Kaioken exposes `/checkpoint save <name>`, `/checkpoint list`, and `/checkpoint restore <name>` built on the CLI’s checkpoint protocol. Saving captures the current repository state instantly (the heavy lifting happens inside Codex), and restore applies the stored patch set back onto your workspace. This makes it easy to stage experiments, roll back multi-file edits, or hand teammates a reproducible starting point without juggling manual `git stash` stacks.

We provide Codex CLI as a standalone, native executable to ensure a zero-dependency install.

## Installing Codex

Today, the easiest way to install Codex is via `npm`:

```shell
npm i -g @openai/codex
codex
```

You can also install via Homebrew (`brew install --cask codex`) or download a platform-specific release directly from our [GitHub Releases](https://github.com/openai/codex/releases).

## Documentation quickstart

- First run with Codex? Follow the walkthrough in [`docs/getting-started.md`](../docs/getting-started.md) for prompts, keyboard shortcuts, and session management.
- Already shipping with Codex and want deeper control? Jump to [`docs/advanced.md`](../docs/advanced.md) and the configuration reference at [`docs/config.md`](../docs/config.md).

## What's new in the Rust CLI

The Rust implementation is now the maintained Codex CLI and serves as the default experience. It includes a number of features that the legacy TypeScript CLI never supported.

### Config

Codex supports a rich set of configuration options. Note that the Rust CLI uses `config.toml` instead of `config.json`. See [`docs/config.md`](../docs/config.md) for details.

### Model Context Protocol Support

#### MCP client

Codex CLI functions as an MCP client that allows the Codex CLI and IDE extension to connect to MCP servers on startup. See the [`configuration documentation`](../docs/config.md#mcp_servers) for details.

#### MCP server (experimental)

Codex can be launched as an MCP _server_ by running `codex mcp-server`. This allows _other_ MCP clients to use Codex as a tool for another agent.

Use the [`@modelcontextprotocol/inspector`](https://github.com/modelcontextprotocol/inspector) to try it out:

```shell
npx @modelcontextprotocol/inspector codex mcp-server
```

Use `codex mcp` to add/list/get/remove MCP server launchers defined in `config.toml`, and `codex mcp-server` to run the MCP server directly.

### Notifications

You can enable notifications by configuring a script that is run whenever the agent finishes a turn. The [notify documentation](../docs/config.md#notify) includes a detailed example that explains how to get desktop notifications via [terminal-notifier](https://github.com/julienXX/terminal-notifier) on macOS.

### `codex exec` to run Codex programmatically/non-interactively

To run Codex non-interactively, run `codex exec PROMPT` (you can also pass the prompt via `stdin`) and Codex will work on your task until it decides that it is done and exits. Output is printed to the terminal directly. You can set the `RUST_LOG` environment variable to see more about what's going on.

### Experimenting with the Codex Sandbox

To test to see what happens when a command is run under the sandbox provided by Codex, we provide the following subcommands in Codex CLI:

```
# macOS
codex sandbox macos [--full-auto] [--log-denials] [COMMAND]...

# Linux
codex sandbox linux [--full-auto] [COMMAND]...

# Windows
codex sandbox windows [--full-auto] [COMMAND]...

# Legacy aliases
codex debug seatbelt [--full-auto] [--log-denials] [COMMAND]...
codex debug landlock [--full-auto] [COMMAND]...
```

### Selecting a sandbox policy via `--sandbox`

The Rust CLI exposes a dedicated `--sandbox` (`-s`) flag that lets you pick the sandbox policy **without** having to reach for the generic `-c/--config` option:

```shell
# Run Codex with the default, read-only sandbox
codex --sandbox read-only

# Allow the agent to write within the current workspace while still blocking network access
codex --sandbox workspace-write

# Danger! Disable sandboxing entirely (only do this if you are already running in a container or other isolated env)
codex --sandbox danger-full-access
```

The same setting can be persisted in `~/.codex/config.toml` via the top-level `sandbox_mode = "MODE"` key, e.g. `sandbox_mode = "workspace-write"`.

## Code Organization

This folder is the root of a Cargo workspace. It contains quite a bit of experimental code, but here are the key crates:

- [`core/`](./core) contains the business logic for Codex. Ultimately, we hope this to be a library crate that is generally useful for building other Rust/native applications that use Codex.
- [`exec/`](./exec) "headless" CLI for use in automation.
- [`tui/`](./tui) CLI that launches a fullscreen TUI built with [Ratatui](https://ratatui.rs/).
- [`cli/`](./cli) CLI multitool that provides the aforementioned CLIs via subcommands.
