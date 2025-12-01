# Codex Kaioken

Codex Kaioken is a fork of OpenAI’s Codex CLI that focuses on aggressive UX upgrades, multi-agent workflows, and tight integration with developer tooling. The Rust workspace that powers the CLI lives in [`codex-rs/`](./codex-rs), and every binary built from this repo ships as `codex-kaioken` to avoid clashing with upstream `codex`.

> **Why “Kaioken”?** It’s a fork of Codex focused on aggressive UX polish and orchestration so the CLI feels faster and more autonomous out of the box.

## Highlights

- **Plan-first workflow** – toggle plan mode with `/plan` or <kbd>Shift</kbd>+<kbd>Tab</kbd>. The composer turns cyan while your next request is converted into a checklist, and `/settings` lets you choose coarse, detailed, or auto plan granularity.
- **Session settings palette** – quickly toggle plan detail, rate-limit footer visibility, and subagent concurrency (1–8 helpers) from `/settings` instead of hand-editing `config.toml`.
- **Real-time subagent UI** – helper agents stream their tool calls, diffs, and reasoning in dedicated panes so you can see exactly what each agent is doing.
- **Parallel orchestration** – the main session automatically spins up specialized subagents for exploration, execution, or research tasks, and gathers their output back into the primary transcript.
- **Semantic search tool** – when [`sgrep`](https://github.com/Rika-Labs/sgrep) is on `PATH`, Kaioken exposes a `semantic_search` tool for fast ranked code lookups.
- **Snapshot-aware undo & checkpoints** – `/undo` restores the last ghost snapshot, and `/checkpoint save|list|restore` lets you capture and jump to your own save points without touching git history. The inline status indicator now clears the moment a checkpoint completes, so you never get stuck watching a phantom spinner.
- **MCP + sandbox tooling** – everything from upstream Codex (execpolicy, MCP client/server, approvals, sandbox helpers) remains available, but tuned for the Kaioken workflow.

## Quick start

### Install & run

#### Install via npm (prebuilt binaries)

Prefer a zero-build install? The published npm wrapper downloads the correct binary from the latest GitHub release and places it on your `PATH`.

```bash
npm install -g @jayasuryajsk/codex-kaioken
codex-kaioken --version
```

The package version matches this repository (for example `0.1.2`) and automatically fetches the corresponding tarball (`codex-kaioken-<platform>.tar.gz`) that CI attached to the release.

> ⚠️ Windows: the npm package currently ships macOS and Linux binaries. On Windows you’ll need to build from source (or run inside WSL) until Windows release artifacts are added.

#### Build from source

```bash
git clone https://github.com/jayasuryajsk/codex-kaioken.git
cd codex-kaioken/codex-rs
just install-kaioken       # builds once and copies bin into ~/.codex-kaioken/bin
~/.codex-kaioken/bin/codex-kaioken
```

The `just install-kaioken` recipe uses the pinned workspace toolchain (`rust-toolchain.toml`) and `Cargo.lock` for reproducible builds. If you prefer raw Cargo commands:

```bash
cargo build -p codex-cli --bin codex
cp target/debug/codex ~/.codex-kaioken/bin/codex-kaioken
```

Keep `~/.codex-kaioken/bin` ahead of any upstream `codex` install on your `PATH` so you always launch the Kaioken binary.

##### Windows manual build

Until Windows release artifacts land you can compile the CLI yourself:

```powershell
git clone https://github.com/jayasuryajsk/codex-kaioken.git
cd codex-kaioken\codex-rs
cargo build -p codex-cli --bin codex-kaioken --release
New-Item -ItemType Directory -Force $env:USERPROFILE\.codex-kaioken\bin | Out-Null
Copy-Item target\release\codex-kaioken.exe $env:USERPROFILE\.codex-kaioken\bin\
$env:USERPROFILE\.codex-kaioken\bin\codex-kaioken.exe
```

Ensure `%USERPROFILE%\.codex-kaioken\bin` (or `~/.codex-kaioken/bin` in WSL) appears before any upstream `codex` binary on `PATH`.

## Documentation
Most docs live under [`codex-rs/docs/`](./codex-rs/docs):

- [Getting started](./codex-rs/docs/getting-started.md) – walkthrough, slash commands, example prompts.
- [Configuration](./codex-rs/docs/config.md) – sandbox modes, approvals, MCP servers, notifications.
- [Advanced topics](./codex-rs/docs/advanced.md) – tracing, MCP details, semantic search specifics.
- [Execpolicy](./codex-rs/docs/execpolicy.md) and [sandbox](./codex-rs/docs/sandbox.md) – controlling what Codex can run.
- [FAQ](./codex-rs/docs/faq.md) – troubleshooting tips for login, upgrades, etc.

## Repository layout

- [`codex-rs/`](./codex-rs) – Rust workspace with every crate (`codex-core`, `codex-tui`, `codex-cli`, etc.). See [`codex-rs/README.md`](./codex-rs/README.md) for deeper details.
- [`npm/`](./codex-rs/npm) – npm wrapper that downloads the correct `codex-kaioken` binary during `postinstall`. Run `npm publish` from this directory after cutting a GitHub Release so `npm i -g codex-kaioken` can fetch the same artifacts.
- `conductor.json` / `.conductor` – metadata used by the Codex CLI harness while developing Kaioken.
- `.github/` – CI, issue templates, assets used in this README.

When contributing code or docs, work inside `codex-rs`, run the `just` recipes mentioned in that README, and open pull requests against this repository.
