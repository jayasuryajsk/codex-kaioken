# Kaioken Recall

Kaioken Recall is a native retrieval layer built into the Codex-Kaioken fork. Its goal is simple: make the agent find the right files faster, with fewer tool calls and less context pollution than default `rg`-only exploration.

It is not just semantic search. It is a routing system for code retrieval.

```text
kaioken_recall
├─ fast path
│  └─ exact symbol / path / error / rg-style lookup
├─ hybrid path
│  └─ behavior query -> lexical search + path/role ranking + related files
└─ deep path
   └─ broad architecture query -> semantic fallback when useful
```

The important design choice is that Kaioken Recall does not blindly run semantic search every time. That was the old bottleneck. Semantic search had a noticeable fixed cost. Recall now uses semantic retrieval only when the query actually needs it.

For exact lookups like:

```text
find where KaiokenRecallHandler is registered
find dispatchInboundMessage
where is src/tools/spec_plan.rs
```

Recall uses the fast path. It behaves closer to `rg`: exact, deterministic, and quick.

For behavior questions like:

```text
where are inbound channel messages routed into an agent reply?
where is model fallback handled?
how are plugin HTTP routes wired?
```

Recall uses the hybrid path. It combines cheap lexical evidence, filename/path boosts, role boosts, and compact evidence rows. The point is to return the likely implementation files without dumping hundreds of noisy `rg` matches into the model context.

For broad questions like:

```text
explain the gateway plugin architecture
map the codebase routing flow
show the main config validation system
```

Recall can use a deeper path and optionally semantic fallback.

## Why It Exists

Default Codex is very good, but its local code retrieval is still mostly tool-driven exploration: search, read, refine, search again, read more. That works, but it can waste:

- tool calls
- model context
- transcript tokens
- time spent reading irrelevant matches

Kaioken Recall tries to compress that exploration into one high-signal retrieval call.

Instead of giving the model a giant grep dump, it returns compact structured evidence:

```json
{
  "status": "ok",
  "strategy": "fast",
  "exactMatchCount": 6,
  "semanticResultCount": 0,
  "evidence": [
    {
      "path": "codex-rs/core/src/tools/spec_plan.rs",
      "startLine": 648,
      "source": "exact_rg",
      "reasons": ["exact_match"]
    }
  ]
}
```

That makes retrieval easier for the model to reason about.

## Current Installed Behavior

The current installed Kaioken Recall has been tested through `codex-kaioken exec`.

Test prompt:

```text
Use kaioken_recall first. Find where KaiokenRecallHandler is registered into the default tool plan.
```

Result:

```text
Kaioken recall status: ok
strategy: fast
intent: exact
exact matches: 6
semantic fallback skipped
```

It found the registration at:

```text
codex-rs/core/src/tools/spec_plan.rs:648
planned_tools.add(KaiokenRecallHandler);
```

During testing, Recall exposed and fixed a real runtime issue: Codex's runtime PATH did not expose `rg`. Recall now resolves `rg` through:

```text
CODEX_KAIOKEN_RG
PATH
~/.cargo/bin/rg
/opt/homebrew/bin/rg
/usr/local/bin/rg
/opt/local/bin/rg
```

## How It Compares To Cursor's Proprietary Retriever

The exact internals of Cursor's retriever are proprietary, so the comparison has to be based on public claims and observed product behavior.

Cursor's public semantic-search writeup says its agent uses semantic search alongside grep-style search, that Cursor trained its own embedding model, built indexing pipelines for fast retrieval, and maintains an evaluation set called Cursor Context Bench. Cursor's codebase indexing docs also describe background indexing, embeddings, automatic sync, ignore rules, and returning relevant code chunks to the agent.

So the strongest version of Cursor's system is not "just embeddings." It is closer to:

```text
Cursor proprietary retrieval
├─ background codebase indexing
├─ trained code embedding model
├─ fast semantic search over code chunks
├─ grep/symbol-style tools
├─ IDE state: open files, selections, edits, diagnostics
├─ context packing/ranking
└─ internal evals such as Cursor Context Bench
```

Kaioken Recall is not yet as broad as that. It does not have a trained proprietary embedding model, a mature IDE state layer, or Cursor-scale ranking data.

The Kaioken angle is different:

- Cursor's retriever is hidden inside Cursor's IDE and agent loop.
- Kaioken Recall is an explicit native Codex tool with model-visible strategy, status, evidence, and match counts.
- Cursor optimizes the full IDE experience: autocomplete, chat, agent, open tabs, edits, symbols, diagnostics.
- Kaioken Recall optimizes the Codex CLI agent's first retrieval move.
- Cursor likely wins today on mature semantic quality and IDE context integration.
- Kaioken can win on inspectability, local hackability, benchmark transparency, and precise routing between exact, hybrid, and deep search.

The practical comparison:

```text
Cursor: proprietary context engine for an AI IDE.
Kaioken Recall: open fork-level retrieval router for a Codex CLI agent.
```

Cursor's bet is "index everything and use a strong internal retriever." Kaioken's bet is "make retrieval strategy explicit and cheap: exact when exact, hybrid when behavioral, deep only when needed."

## How It Compares To Windsurf's Fast Context

This is the more direct comparison.

Windsurf's docs describe Fast Context as a specialized subagent that retrieves relevant code up to 20x faster than traditional agentic search. The docs say it uses SWE-grep models for rapid code retrieval and triggers automatically when Cascade needs code search. Windsurf frames the benefit as saving Cascade's context budget and intelligence for the actual coding task rather than wasting turns on search.

That is very close to the Kaioken Recall thesis.

Windsurf Fast Context appears to be:

```text
Windsurf Fast Context
├─ specialized retrieval subagent
├─ SWE-grep model based retrieval
├─ automatic trigger inside Cascade
├─ codebase-level relevant file/snippet retrieval
├─ optimized for speed versus traditional agent search
└─ integrated into Windsurf's broader IDE context engine
```

Kaioken Recall is similar in goal, but different in implementation and positioning:

- Windsurf Fast Context is a proprietary specialized retrieval subagent.
- Kaioken Recall is a native Codex tool, not a hidden subagent.
- Windsurf emphasizes model-powered retrieval speed with SWE-grep.
- Kaioken Recall emphasizes a deterministic router: fast exact path, hybrid lexical/path-ranking path, and deep semantic fallback.
- Windsurf's retrieval is automatic but opaque.
- Kaioken Recall is automatic by instruction/default behavior, but its output exposes `strategy`, `status`, `exactMatchCount`, `semanticResultCount`, evidence paths, reasons, and snippets.
- Windsurf likely has stronger trained retrieval models today.
- Kaioken can iterate faster locally because the ranking rules and tool behavior are fork-owned.

The practical comparison:

```text
Windsurf Fast Context: proprietary retrieval subagent using SWE-grep models.
Kaioken Recall: transparent retrieval router plus local exact/hybrid/deep search paths.
```

Kaioken Recall should not claim it already beats Windsurf's proprietary retriever. The honest claim is narrower and stronger:

```text
Kaioken Recall brings the Fast Context idea into Codex as a native, inspectable, hackable fork feature.
```

That is a credible differentiation because default Codex does not expose an equivalent first-class retrieval router today.

## The Differentiator

The core differentiator is this:

```text
Cursor/Windsurf: editor-native codebase context engines.
Kaioken Recall: agent-native retrieval router for Codex.
```

It is built for the CLI agent loop, not for autocomplete.

Kaioken Recall should become the default first move when Codex needs to understand code:

```text
Unknown behavior? Use kaioken_recall first.
Exact symbol? Recall fast path.
Broad architecture? Recall deep path.
Need proof? Then use rg/read to verify.
```

That gives Kaioken its edge: not "semantic search exists," but "the agent retrieves code like a senior engineer would: exact when exact, hybrid when behavioral, deep only when necessary."

## References

- [Cursor: Improving agent with semantic search](https://cursor.com/blog/semsearch)
- [Cursor codebase indexing](https://cursordocs.com/en/docs/context/codebase-indexing)
- [Cursor secure codebase indexing](https://cursor.com/blog/secure-codebase-indexing)
- [Cursor security](https://www.cursor.com/security)
- [Windsurf Fast Context](https://docs.windsurf.com/context-awareness/fast-context)
- [Windsurf context awareness](https://docs.windsurf.com/context-awareness/overview)
- [Windsurf Cascade](https://docs.windsurf.com/windsurf/cascade)
