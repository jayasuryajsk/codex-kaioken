```
   ██╗  ██╗ █████╗ ██╗ ██████╗ ██╗  ██╗███████╗███╗   ██╗
   ██║ ██╔╝██╔══██╗██║██╔═══██╗██║ ██╔╝██╔════╝████╗  ██║
   █████╔╝ ███████║██║██║   ██║█████╔╝ █████╗  ██╔██╗ ██║
   ██╔═██╗ ██╔══██║██║██║   ██║██╔═██╗ ██╔══╝  ██║╚██╗██║
   ██║  ██╗██║  ██║██║╚██████╔╝██║  ██╗███████╗██║ ╚████║
   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝ ╚═════╝ ╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝

                  ▂▃▄▅▆▇██ POWER ██▇▆▅▄▃▂
```

# Codex Kaioken

Codex Kaioken is a fork of OpenAI's Codex CLI that focuses on aggressive UX upgrades, multi-agent workflows, persistent memory, and tight integration with developer tooling. The Rust workspace that powers the CLI lives in [`codex-rs/`](./codex-rs), and every binary built from this repo ships as `codex-kaioken` to avoid clashing with upstream `codex`.

> **Why "Kaioken"?** It's a fork of Codex that layers on aggressive UX polish, persistent learning, and orchestration so the CLI feels faster and more autonomous out of the box.

## System Requirements

| Platform | Supported |
|----------|-----------|
| macOS (Apple Silicon) | ✅ |
| Linux x64 (glibc 2.35+) | ✅ Ubuntu 22.04+, Debian 12+, Fedora 36+ |
| Windows x64 | ✅ |
| macOS Intel | ❌ |
| Linux arm64 | ❌ |
| Older Linux | ❌ Ubuntu 20.04, RHEL 7/8, CentOS 7, Debian 11 |

Check your Linux glibc version: `ldd --version`

## Highlights

### Memory System

Kaioken remembers things across sessions:

- **Lessons** – Mistakes and fixes (never forgets)
- **Decisions** – Why you chose X over Y (never forgets)
- **Locations** – Where stuff is in your codebase
- **Patterns** – Your coding style and preferences

Stored in `.kaioken/memory/`. Agent uses `memory_recall` and `memory_save` tools.

### Multi-Agent

- Subagents stream in real-time in dedicated panes
- Parallel task execution (exploration, research, etc.)
- No artificial timeouts

### Planning

- `/plan` or `Shift+Tab` to enter plan mode
- `/settings` for plan granularity, concurrency (1-8 agents)
- `/undo` and `/checkpoint` for snapshots without touching git

### Tools

- Semantic search via [`sgrep`](https://github.com/Rika-Labs/sgrep) if installed
- MCP + sandbox from upstream Codex
- Generous timeouts (5min shell, 10min MCP)

## Install

### npm (recommended)

```bash
npm install -g @jayasuryajsk/codex-kaioken
codex-kaioken --version
```

### Build from source

```bash
git clone https://github.com/jayasuryajsk/codex-kaioken.git
cd codex-kaioken/codex-rs
just install-kaioken
~/.codex-kaioken/bin/codex-kaioken
```

Or with cargo directly:

```bash
cargo build -p codex-cli --bin codex-kaioken --release
```

### Windows

```powershell
git clone https://github.com/jayasuryajsk/codex-kaioken.git
cd codex-kaioken\codex-rs
cargo build -p codex-cli --bin codex-kaioken --release
```

## Docs

See [`codex-rs/docs/`](./codex-rs/docs) for configuration, sandbox, MCP, and more.
