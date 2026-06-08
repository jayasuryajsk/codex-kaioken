<p align="center"><strong>Codex CLI</strong> is a coding agent from OpenAI that runs locally on your computer.
<p align="center">
  <img src="https://github.com/openai/codex/blob/main/.github/codex-cli-splash.png" alt="Codex CLI splash" width="80%" />
</p>
</br>
If you want Codex in your code editor (VS Code, Cursor, Windsurf), <a href="https://developers.openai.com/codex/ide">install in your IDE.</a>
</br>If you want the desktop app experience, run <code>codex app</code> or visit <a href="https://chatgpt.com/codex?app-landing-page=true">the Codex App page</a>.
</br>If you are looking for the <em>cloud-based agent</em> from OpenAI, <strong>Codex Web</strong>, go to <a href="https://chatgpt.com/codex">chatgpt.com/codex</a>.</p>

---

## Kaioken Recall v5 fork

This fork tracks the current upstream OpenAI Codex codebase and adds **Kaioken Recall v5**, a retrieval bridge for Codex agents.

Kaioken Recall gives Codex a compact ranked evidence pack before it starts broad shell search or noisy file reads. The implementation adds a `kaioken_recall` tool to Codex, routes it to a standalone `kaioken-recall` engine when available, and keeps a native fallback path inside Codex.

Key files:

- [`docs/kaioken-recall.md`](./docs/kaioken-recall.md) - architecture, usage, and verification notes.
- [`codex-rs/core/src/tools/handlers/kaioken_recall.rs`](./codex-rs/core/src/tools/handlers/kaioken_recall.rs) - Codex handler bridge and fallback retrieval logic.
- [`codex-rs/core/src/tools/handlers/kaioken_recall_spec.rs`](./codex-rs/core/src/tools/handlers/kaioken_recall_spec.rs) - tool schema.
- [`codex-rs/core/src/tools/spec_plan.rs`](./codex-rs/core/src/tools/spec_plan.rs) - default tool-plan registration.

Verified local proof on the upstream-based branch:

- Standalone Kaioken Recall tests: `35/35` passed.
- Codex Kaioken handler tests: `7/7` passed.
- `codex-exec` build: passed.
- Black-box smoke with shell disabled: returned `nested/proof/needle.txt` via retrieval.
- 100-query CodeSearchNet/MTEB-style retrieval benchmark: `Recall@10 0.87`, `nDCG@10 0.6993`, `MRR@10 0.6442`.
- 20-query Codex-agent comparison against regular Codex: equal `hit@5` at `20/20`, higher MRR for Kaioken (`0.9750` vs `0.9417`), lower uncached token use (`296,932` vs `573,606`), and lower elapsed time (`348.1s` vs `859.2s`).

## Quickstart

### Installing and running Codex CLI

Run the following on Mac or Linux to install Codex CLI:

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | sh
```

Run the following on Windows to install Codex CLI:

```
powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"
```

Codex CLI can also be installed via the following package managers:

```shell
# Install using npm
npm install -g @openai/codex
```

```shell
# Install using Homebrew
brew install --cask codex
```

Then simply run `codex` to get started.

<details>
<summary>You can also go to the <a href="https://github.com/openai/codex/releases/latest">latest GitHub Release</a> and download the appropriate binary for your platform.</summary>

Each GitHub Release contains many executables, but in practice, you likely want one of these:

- macOS
  - Apple Silicon/arm64: `codex-aarch64-apple-darwin.tar.gz`
  - x86_64 (older Mac hardware): `codex-x86_64-apple-darwin.tar.gz`
- Linux
  - x86_64: `codex-x86_64-unknown-linux-musl.tar.gz`
  - arm64: `codex-aarch64-unknown-linux-musl.tar.gz`

Each archive contains a single entry with the platform baked into the name (e.g., `codex-x86_64-unknown-linux-musl`), so you likely want to rename it to `codex` after extracting it.

</details>

### Using Codex with your ChatGPT plan

Run `codex` and select **Sign in with ChatGPT**. We recommend signing into your ChatGPT account to use Codex as part of your Plus, Pro, Business, Edu, or Enterprise plan. [Learn more about what's included in your ChatGPT plan](https://help.openai.com/en/articles/11369540-codex-in-chatgpt).

You can also use Codex with an API key, but this requires [additional setup](https://developers.openai.com/codex/auth#sign-in-with-an-api-key).

## Docs

- [**Codex Documentation**](https://developers.openai.com/codex)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)
- [**Open source fund**](./docs/open-source-fund.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
