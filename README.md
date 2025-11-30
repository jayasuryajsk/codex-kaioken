# Codex Kaioken

Codex Kaioken is a fork of OpenAI’s Codex CLI that focuses on aggressive UX upgrades, multi-agent workflows, and tight integration with developer tooling. The Rust workspace that powers the CLI lives in [`codex-rs/`](./codex-rs), and every binary built from this repo ships as `codex-kaioken` to avoid clashing with upstream `codex`.

> **Why “Kaioken”?** It is our “power-up” harness: we keep stacking capabilities (parallel subagents, real-time streaming, semantic search, MCP integrations, etc.) so that Codex feels faster and more autonomous without any extra setup from the user.

## Highlights

- **Real-time subagent UI** – helper agents stream their tool calls, diffs, and reasoning in dedicated panes so you can see exactly what each agent is doing.
- **Parallel orchestration** – the main session automatically spins up specialized subagents for exploration, execution, or research tasks, and gathers their output back into the primary transcript.
- **Semantic search tool** – when [`sgrep`](https://github.com/Rika-Labs/sgrep) is on `PATH`, Kaioken exposes a `semantic_search` tool for fast ranked code lookups.
- **Snapshot-aware undo & checkpoints** – `/undo` restores the last ghost snapshot, and `/checkpoint <name>` / `/restore <name>` let you capture and jump to your own save points without touching git history. The inline status indicator now clears the moment a checkpoint completes, so you no longer get stuck with a lingering “Saving…” spinner after the snapshot is already available.
- **MCP + sandbox tooling** – everything from upstream Codex (execpolicy, MCP client/server, approvals, sandbox helpers) remains available, but tuned for the Kaioken workflow.

## Quick start

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

### Install via npm (prebuilt binaries)

Prefer a zero-build install? The published npm wrapper downloads the correct binary from the latest GitHub release and places it on your `PATH`.

```bash
npm install -g @jayasuryajsk/codex-kaioken
codex-kaioken --version
```

The package version matches this repository (for example `0.1.2`) and automatically fetches the corresponding tarball (`codex-kaioken-<platform>.tar.gz`) that CI attached to the release.

## Documentation

Most docs live under [`codex-rs/docs/`](./codex-rs/docs):

- [Getting started](./codex-rs/docs/getting-started.md) – walkthrough, slash commands, example prompts.
- [Configuration](./codex-rs/docs/config.md) – sandbox modes, approvals, MCP servers, notifications.
- [Advanced topics](./codex-rs/docs/advanced.md) – tracing, MCP details, semantic search specifics.
- [Execpolicy](./codex-rs/docs/execpolicy.md) and [sandbox](./codex-rs/docs/sandbox.md) – controlling what Codex can run.
- [FAQ](./codex-rs/docs/faq.md) – troubleshooting tips for login, upgrades, etc.

## Repository layout

- [`codex-rs/`](./codex-rs) – Rust workspace with every crate (`codex-core`, `codex-tui`, `codex-cli`, etc.). See [`codex-rs/README.md`](./codex-rs/README.md) for deeper details.
- `conductor.json` / `.conductor` – metadata used by the Codex CLI harness while developing Kaioken.
- `.github/` – CI, issue templates, assets used in this README.

When contributing code or docs, work inside `codex-rs`, run the `just` recipes mentioned in that README, and open pull requests against this repository.

## License

Codex Kaioken inherits the upstream [Apache-2.0 License](./codex-rs/LICENSE). Any new changes in this fork remain under the same license.
