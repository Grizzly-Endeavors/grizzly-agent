# grizzly-agent workspace — Design

> Systems level only. No file or line references. This document must stand on its own: it will be implemented later in fresh sessions that have only this doc, phases.md, and the codebase — not the conversation that produced it.

## Goal & context

`grizzly-agent` is the one dependency a Rust project pulls in to build an agent or otherwise work with LLMs, so that each new project reuses what the last one built instead of re-solving it. Its admission bar is **"would I want to use this again?"** — not "do two projects already need it." Building agents is the most common thing done across these projects, so waiting for a second consumer only guarantees the second project starts with a migration and a drifted copy.

Today the crate holds only conversation types and an error taxonomy. Meanwhile two sibling projects have built the pieces this crate is missing, each locked inside its own repo:

- **`grizzly-gameservers`' `prompt-lib`** compiles Markdown-plus-frontmatter prompt files into typed Rust at build time — prompt renderers, tool specs, and tool parameter structs — and has a test-time pass that cross-checks each prompt's declared call site against the source tree. It has no coupling to gameservers.
- **`gantry`'s eval suite** measures an agent harness with live model calls: single-call "role" cases scored against a known answer, and whole-run "harness" cases scored against outcomes and hidden checks. It is deeply coupled to gantry's domain types, but its skeleton — repeats, pass thresholds, failure categories, aggregation, a self-explaining report — is the proven eval shape this crate was missing.

This work turns `grizzly-agent` into a Cargo workspace, gives it a provider layer, a tool model, a turn loop, prompts, skills, and evals, and moves both sibling projects onto it (gantry partially — see Integration).

### Out of scope

Transcript trimming and session storage, telemetry/recording, structured-output repair (JSON repair, parse-retry gateways), memory and retrieval, MCP (consumers use `rmcp` directly), token counting, and cost tracking. Each is a candidate for later work; none is needed for the shape here to be complete.

## Shape

### Workspace layout

One git repository, one Cargo workspace, six packages, all versioned together:

| Package | Responsibility | Used at |
| --- | --- | --- |
| `grizzly-agent` | **Facade.** Re-exports the runtime crates behind features. The one line a consumer adds for runtime use. | runtime |
| `grizzly-agent-core` | Conversation types, errors, the provider trait and `Model`, the tool model, the turn loop and `Agent`, retry. No HTTP. | runtime |
| `grizzly-agent-providers` | Concrete provider clients: Anthropic and OpenAI-compatible, each behind its own feature. Depends on core. | runtime |
| `grizzly-agent-prompts` | Frontmatter parsing (default); prompt codegen (`codegen` feature); call-site verification (`verify` feature). Does **not** depend on core. | build / dev (frontmatter: runtime, via skills) |
| `grizzly-agent-skills` | Agent Skills: format, index, activation. Depends on core and on prompts (default features only). | runtime |
| `grizzly-agent-eval` | ResponseEval and AgentEval over a shared scoring and report core. Depends on core. | test / dev |

Dependency direction is strictly: `providers → core`, `skills → core + prompts`, `eval → core`, `facade → everything runtime`. Nothing depends on the facade. Core depends on no other workspace member.

**Facade features:** `anthropic` and `openai` (enable providers with that provider), `skills`, `eval`, and `test-support` (forwarded to core). Default features are empty: a bare `grizzly-agent` dependency is core alone. The facade re-exports at its root so consumers write `grizzly_agent::Model`, `grizzly_agent::Agent`, etc. Re-exports are **curated, item by item** (`pub use` of named items with `#[doc(inline)]`, each gated on the feature that brings its crate in) — never glob re-exports, which would let two members' same-named items silently collide or shadow. Documentation lives on the item in its member crate; the facade adds none of its own beyond a crate-level overview. Each member likewise re-exports its own public surface at its root, keeping its module tree private. Adding a public item to a member therefore means adding its facade re-export in the same change; the facade's tests reference every re-exported item so a missing one is caught.

**Prompts are not behind the facade.** Codegen runs in a consumer's build script and verify in its tests. Routing those through the facade would compile core (and whatever runtime features are on) for the host just to run a parser. A consumer that uses prompt files therefore adds `grizzly-agent-prompts` directly on its build edge (feature `codegen`) and dev edge (feature `verify`), alongside `grizzly-agent` on its normal edge. This is a deliberate second dependency line in exchange for a light build script.

**Lints, toolchain, deny policy** live once at the workspace root. Every member inherits the workspace lint table unchanged. The existing strict lint set — including `missing_docs` and `unreachable_pub` at deny — applies to every member.

### Core: conversation types and errors

The existing block-structured `Message` / `Content` / `Role` / `ToolUse` / `ToolResult` types are kept and become core's. Content is a sequence of blocks (text, reasoning, tool use, tool result); reasoning is never folded into text.

Errors keep the existing split:

- **`ProviderFailure`** — a model call failed. Classified into transport, HTTP status (with `Retry-After`), decode, and configuration. Exposes whether a retry can help and how long to wait.
- **`ToolFailure`** — a tool did not produce a useful result. Always converts into a tool result the model reads; never ends a turn.
- **`RunFailure`** (replacing `TurnFailure`) — the run could not be carried out: the system broke, as distinct from the agent behaving in some way. Two variants: **`InvalidConversation`** (the caller's conversation broke a rule, detected before any work; names the rule) and **`Provider`** (a model call failed after retries were spent; carries the `ProviderFailure` as its error `source()`, so the cause chain works with `?` and `anyhow`, plus the **`RunTrace`** of everything completed before the failure). Running out of rounds and stalling are not failures — they move out of this type and become run endings (see the loop), because they describe what the agent did, not a breakage.

`ProviderFailure` gains one variant, **`InvalidRequest`** (a message naming the violated rule), for a `CompletionRequest` that breaks the request invariants below. It is not retryable.

### Core: model calls

**`Provider`** is an object-safe async trait with one required operation: given a `CompletionRequest`, open a **completion stream** — a boxed, `Send` stream of `CompletionEvent`s, each item a `Result` with `ProviderFailure` as its error — or fail with a `ProviderFailure` before the stream opens. Streaming is the primitive because every provider streams on the wire; a whole `Completion` is derived from the stream in core, not re-implemented per provider. It is object-safe so providers can be chosen at runtime and held as `Arc<dyn Provider>`.

**`CompletionEvent`** is a closed enum describing one step of a streamed completion in provider-neutral terms:

- a **text delta** (appended to the current text block);
- a **reasoning delta** (appended to the current reasoning block);
- a **tool-use start** (the call's id and tool name, opening a tool-use block);
- a **tool-use arguments delta** (a fragment of that call's JSON arguments, addressed by the call's id);
- **usage** (any usage the provider reports, possibly more than once; later values supersede earlier ones field by field);
- **finished** (the stop reason, raw and classified, and the reported model identifier) — always the last event of a successful stream.

A new text or reasoning delta after a different kind of block starts a new block, so block order in the reassembled message follows the stream. A stream that ends without `finished` has failed: the stream yields a retryable transport-class `ProviderFailure` as its final item. Providers are responsible for turning their wire format into this sequence, including reassembling tool calls delivered as indexed fragments into id-addressed events.

**`CompletionAccumulator`** (core) folds a sequence of `CompletionEvent`s into a `Completion`, parsing each tool call's concatenated arguments as JSON when the stream finishes. Unparseable arguments are a decode-class `ProviderFailure` — except when the stop reason is max-tokens, where a tool call cut off mid-arguments is expected: the accumulator drops that incomplete tool-use block and returns the rest of the completion. It is public so a consumer rendering a stream live can also keep the assembled result without re-implementing reassembly.

**`Model`** is what the rest of the crate and every consumer passes around: a shared provider, a model identifier, default request parameters, and a retry policy. "Use this model from this provider" is constructing a `Model`; nothing downstream of that knows which provider it is. `Model` is cheap to clone. It offers two calls:

- **`complete`** — returns a whole `Completion`: opens the stream and drives it through the accumulator. What most callers, and every eval, use.
- **`stream`** — returns the completion stream itself, for callers that render output as it arrives.

**Reasoning blocks carry an optional provider signature.** `Content::Reasoning` holds the reasoning text and an optional opaque signature string. Providers that require prior reasoning to be sent back verbatim and signed (Anthropic's extended thinking with tool use) populate it on the way in and require it on the way out; providers that do not use one leave it empty.

**`CompletionRequest`** carries: the messages, the tool specs being advertised, optional sampling parameters (max output tokens, temperature), and an optional **response format** requesting schema-constrained JSON output (a name and a JSON Schema). Unset parameters fall through to the `Model`'s defaults, then to the provider's.

**Request invariants.** `Model` validates every request before it reaches a provider and rejects violations with `ProviderFailure::InvalidRequest`, so providers may assume them:

- system-role messages appear only as a contiguous run at the head of the list and contain only text blocks (providers that take a single system field join them with a blank line);
- tool-use and reasoning blocks appear only on assistant messages;
- tool-result blocks appear only on user messages; a user message may mix tool results and text;
- every tool result answers a tool use in an earlier assistant message.

Beyond these, any mix is valid and each provider maps it deterministically: the OpenAI-compatible provider emits a user message's tool results as consecutive tool-role messages followed by one user message holding its remaining text (if any), and drops reasoning blocks from outgoing history; the Anthropic provider maps blocks natively and sends reasoning back only when it carries a signature, dropping unsigned reasoning.

**`Completion`** carries: the assistant message (as content blocks, including any reasoning and tool uses), **usage** with every field optional (a provider that reports no count reads as *unknown*, never zero), a **stop reason** as a small closed enum (end of turn, tool use, max tokens, other) plus the provider's raw string, and the model identifier the provider reports.

**Retry** lives in core as two parts: a pure decision function (attempt number, the failure, any `Retry-After` → wait this long, or stop) and a combinator that applies it with full-jitter backoff. `Model` applies its retry policy by default; the policy is configurable and can be set to "no retries." The default policy is gantry's proven one: **5 total attempts**, exponential backoff starting at **500 ms** and doubling, each delay capped at **10 s**, with full jitter (the delay actually waited is uniform between zero and the computed delay). When a failure carries `Retry-After`, the wait is the larger of that value and the computed delay. Sleep and jitter are injectable so tests run on a deterministic schedule without real time. Only failures the classification marks retryable are retried; `Retry-After` is honoured when present and caps nothing downward. Where a retry is allowed depends on the call:

- **`complete`** buffers the whole stream before returning, so a failure at any point — before the stream opens or mid-stream — retries the whole request.
- **`stream`** retries only failures that happen **before the first event is yielded** to the caller. Once any event has been delivered, a later failure is yielded as the stream's final item and not retried, because the caller has already consumed partial output that a retry could not take back.

**Timeouts** are per call and belong to the caller: `Model` exposes an optional per-call timeout covering the whole call (for `stream`, from the call until the stream ends) that, when it fires, produces a transport-class `ProviderFailure` naming the timeout. Providers additionally bound idle gaps between wire reads (below).

**Test support.** Behind core's `test-support` feature: a scripted provider that plays back a queued sequence of scripted responses — a whole completion (emitted as a well-formed event sequence), an explicit event sequence (to test partial and broken streams), or a failure before the stream opens — and records every request it received. Called after its queue is empty, it fails with a non-retryable `ProviderFailure::Configuration` whose message says the script was exhausted and how many calls it served, so an under-scripted test fails legibly. Every consumer needs this to test anything built on a model; it is part of the product.

### Providers

Each provider translates between core's canonical types and its wire format with pure, I/O-free conversion functions, so translation is tested without a network. The HTTP client is `reqwest` with rustls; it is compiled only when a provider feature is on.

- **OpenAI-compatible** — targets the chat-completions API. Takes a base URL and an optional API key, so the same client serves OpenAI, vLLM, Ollama, and any OpenAI-compatible gateway. Flattens content blocks to the OpenAI message shape on the way out (tool results become tool-role messages; reasoning is dropped from outgoing history) and rebuilds blocks on the way in, mapping the reasoning field under any of its spellings (`reasoning_content`, `reasoning`, `thinking`) to the reasoning block.
- **Anthropic** — targets the Messages API. Content blocks map natively; system messages are lifted to the top-level system field.

**Both providers always stream on the wire** and translate the wire stream into core's `CompletionEvent` sequence. There is no non-streaming request path. Besides enabling the public streaming API, this keeps connections alive through tunnels and proxies that drop idle connections — a non-streamed request is silent for the whole generation. Each provider must:

- request usage reporting in the stream where the API requires opting in, and emit it as usage events;
- end with `finished` only when the wire stream carried a finish/stop signal; a wire stream that ends without one yields a **retryable transport failure** as the final item, never a (truncated) success;
- bound the gap between reads with an idle timeout, reset on every byte, **defaulting to 45 s** and configurable on the provider's constructor; expiry is a retryable transport failure;
- turn an error payload delivered mid-stream into a failure item, and a malformed chunk into a decode failure;
- translate tool calls delivered as indexed fragments into id-addressed tool-use events;
- handle chunk boundaries anywhere, including mid-line and mid-UTF-8 sequence;
- honour the request's response format through the API's native structured-output mechanism — the OpenAI-compatible `response_format` of type `json_schema` (strict), and Anthropic's native structured-output parameter, whose current wire form is verified against Anthropic's API documentation when the provider is built. A provider never silently ignores a response format: if the endpoint or model rejects it, the API's error surfaces as a `Status` failure.

Provider-specific configuration (base URL, API key, API version header, idle timeout) is the provider constructor's business; none of it leaks into `Model` or `CompletionRequest`.

### Core: tools

The tool model is `prompt-lib`'s, promoted to the crate's single definition of a tool.

**`ToolSpec`** — the advertisement sent to the model: a wire name, a description, and a JSON Schema for parameters as a JSON value. Name and description are `Cow<'static, str>`: generated tools use borrowed statics at zero cost, and tools built at runtime (skills, MCP-backed tools a consumer wraps, anything discovered from config) own their strings.

**`ToolDefinition`** — a trait binding a tool type to its spec and its **parameter type**: an associated `Params` type that deserializes from the model's arguments, plus a function returning the `ToolSpec`. Codegen emits this impl for every generated tool, so the schema advertised and the type the arguments parse into come from the same prompt file and cannot drift. Tools with no parameters use core's `NoParams` (an empty struct that rejects unknown fields).

**`ToolHandler`** — what actually runs a tool. An object-safe async trait with two methods: one returning its `ToolSpec`, and a call method taking **the raw JSON arguments and a shared reference to the run's `ToolContext`**, returning a tool result string or a `ToolFailure`. Consumer state (clients, handles, configuration) is held by the handler value itself — captured when the handler is built — not passed per call. Core provides a typed adapter: given a `ToolDefinition` and an async function taking the tool's `Params` and the `ToolContext`, it produces a `ToolHandler` that parses the arguments into `Params` and turns a parse error into `ToolFailure::InvalidArguments` phrased for the model. The function is typically a closure capturing the consumer's state. Hand-written runtime tools implement `ToolHandler` directly.

**`ToolSet`** — the registry the loop uses: an ordered collection of handlers keyed by wire name. It is the single source for both the tool list advertised to the model and dispatch. Registering two tools with the same wire name is an error at construction. Dispatching an unknown name yields `ToolFailure::Unknown` listing the registered names.

**Stop requests.** A tool can ask for the run to end after the current round — the "hand this to a human" exit — by recording a stop request (a reply for the user and a reason for whoever handles it) on the run's context. The loop checks after each round of tool results and ends the run with that request as its ending. Handlers make that request through the **`ToolContext`**, created fresh by the loop for each run and passed to every call in that run. It is the typed side channel between tools and the loop; it exposes the run's cancellation token and the stop-request slot. Cancellation everywhere in the crate is a `tokio_util::sync::CancellationToken` — the caller keeps a clone and cancels it; the loop and tools observe it. If more than one tool records a stop request in the same round, the first one recorded wins.

### Core: the turn loop and `Agent`

**`Agent`** is a configured, reusable harness: a `Model`, a `ToolSet`, a system prompt, and limits. It is the concrete type AgentEval evaluates; there is no agent trait — harnesses are built on this loop. An `Agent` is `Send + Sync` and `run` takes `&self`, so one `Agent` may serve many concurrent runs (one per conversation); nothing in the `Agent` is mutated by a run. Per-run state — the tool context, stall counter, and observer — belongs to the run.

**System prompt as sections.** The system prompt is an ordered list of sections, each either static text or a **dynamic section** — a trait object rendered fresh before every model call. Before each model call the loop renders every section in list order, drops sections whose rendering is empty or whitespace-only, and joins the rest with a blank line (`\n\n`) into a single system message at the head of the request. This is the seam skills uses to inject its index and active skills, and the general way any consumer adds context that changes mid-run.

**Limits:** a maximum number of model rounds (the last-resort backstop), and a stall threshold, **default 4**. The loop tracks individual dispatched tool calls in dispatch order across rounds: each call is compared to the one dispatched immediately before it by tool name and by structural equality of the parsed arguments value (so key order and whitespace do not matter). An identical call increments a counter, any different call resets it to one; when the counter reaches the threshold the run ends as stalled, before the next model call. The round limit defaults to 16.

**`Agent::run`** takes the conversation so far (the caller's history plus the new user input), a cancellation token, and an optional run observer for this run. The conversation must not contain system-role messages — the `Agent`'s sections are the only system prompt — and must satisfy the request invariants; `run` checks this before doing anything and returns an **`InvalidConversation`** error naming the violated rule. `run` returns `Result<RunRecord, RunFailure>`, and the dividing line is: **`Err` means the system broke; `Ok` means the loop ran to a defined stopping point, whatever the agent decided.** Once the upfront check passes, it drives the loop — call the model with the rendered system prompt, the conversation and the advertised tools; decide from the completion's stop reason and content what happens next; if the reply requests tools, dispatch each through the `ToolSet` in the order requested, append one user message holding all the round's results, check for a stop request and for a stall, repeat — and returns a **`RunRecord`**, or `RunFailure::Provider` if a model call fails after retries.

What a completion means to the loop:

- stop reason **max-tokens** → the run ends as `Truncated`; no tool calls from that completion are dispatched, even complete ones, since the model did not finish deciding. The assistant message is appended to the record with its tool-use blocks removed (text and reasoning kept), so the record's transcript never holds an unanswered tool use and a caller can resume the conversation — for example with a larger token limit — by passing it back to `run`;
- otherwise, if the completion contains tool uses → dispatch them and continue;
- otherwise → the run ends as `Completed`, whatever the stop reason (end-of-turn or other).

Cancellation is checked before each model call and before each tool dispatch; a cancelled run stops there without starting further work.

**`RunTrace`** is the progress of a run — what happened, independent of how it ended: the messages appended, the per-round records, and total usage. **`RunRecord`** is a finished run: its ending, its final reply, and its `RunTrace`. The trace is what `RunFailure::Provider` carries, so a failure still hands back every completed round. In detail:

- the **ending** (record only): `Completed` (the model answered without requesting tools), `Truncated` (the model hit its output-token limit), `StopRequested` (a tool asked; carries the request), `RoundsExhausted`, `Stalled` (carries the repeated tool name and count), or `Cancelled`;
- the **final reply text**, when there is one (record only);
- the **full message sequence** appended during the run (assistant messages and tool results), so the caller can persist it and a scorer can inspect it;
- a **per-round record**: the completion's usage, stop reason, and latency, and each tool call made with its arguments, whether it failed, and its latency;
- **total usage** summed across rounds, with a field unknown in the total if any round reported it unknown.

Tool failures never end a run; they are tool results the model reads and can correct from. They are still visible: each is flagged in its round record, reported to the observer, and logged as a `tracing` warning naming the tool and the failure. A provider failure ends the run with `Err(RunFailure::Provider)`, so `?` propagates it like any other error; a caller that persists transcripts takes the `RunTrace` out of the failure first and loses nothing. The loop logs a `tracing` error event whenever a run fails, so failures reach logs even when a caller mishandles the `Err`.

**Observation hook and live output.** Each run optionally takes a run observer — a trait notified of every `CompletionEvent` as it streams from the model, of each round as it completes, and of each tool call as it completes — so both a live UI and telemetry are structural rather than something a caller must remember to wrap around the loop. The loop always calls the model through `Model::stream` and assembles each round with the accumulator, forwarding events to the observer as they arrive. The observer cannot change the run. Because the loop consumes the stream itself, a mid-stream failure after events were forwarded ends the run with `RunFailure::Provider` rather than being retried; failures before the first event are retried per the `Model`'s policy.

### Prompts

The `prompt-lib` crate moves into the workspace as `grizzly-agent-prompts`, with three faces:

- **Default (frontmatter):** split a Markdown file into its YAML frontmatter and body, and parse the frontmatter into a `yaml-rust2` document. No codegen, no filesystem walking. This is what skills builds on.
- **`codegen`:** load a prompt directory, validate it, and emit the generated Rust module; plus a build-script entry point that also registers the tree for rebuild-on-change. The prompt file format — ids, the `prompt` / `tool` / `params` types, placeholders, inline and shared parameter schemas, enum parameters, annotations — is **unchanged**, so existing prompt trees work as they are.
- **`verify`:** cross-check every prompt's declared call sites and id against a source tree, as today.

Changes from `prompt-lib`:

- **YAML parsing moves from `serde_yaml_ng` to `yaml-rust2`**, with the frontmatter fields mapped by hand. Field order in parameter schemas must still follow the file (`yaml-rust2`'s mapping preserves insertion order), so generated struct fields and schema properties keep their current order. Validation rules and error messages are preserved, including naming the offending file.
- **Generated code targets core's types through a configurable crate path**, defaulting to `grizzly_agent` (the facade). The generated module references `ToolSpec`, `ToolDefinition`, `NoParams`, and the `serde_json` re-export via that path. The build-script entry point keeps its current one-argument form (prompt directory in, default path) so existing build scripts are unchanged; a builder — constructed from the prompt directory, with a method setting the crate path, and a method that emits — covers the non-default case. A consumer depending on core directly sets the path to `grizzly_agent_core`.
- **`spec()` bodies build owned-or-borrowed strings**: name and description are emitted as borrowed statics wrapped for `ToolSpec`'s `Cow` fields. The `NAME` constant stays a plain static string.
- **Generated tools implement `ToolDefinition`**, binding each tool's spec to its params type (inline, shared via `params_from`, or `NoParams`). The existing generated items — the tool unit struct, its `NAME` constant, its `spec()` function, params structs, enums, and prompt renderers — remain, so existing call sites keep compiling.
- The `ToolSpec` type itself leaves this crate; it is core's.

### Skills

`grizzly-agent-skills` loads and runs Agent Skills per the open specification, in three layers:

- **Format:** parse a skill's `SKILL.md` (via prompts' frontmatter) into its six specified fields — `name`, `description`, `license`, `compatibility`, `metadata`, `allowed-tools` — validated to the spec's constraints (name 1–64 chars of lowercase alphanumerics and single hyphens, no leading/trailing hyphen, equal to its directory name; description 1–1024 chars; compatibility ≤ 500 chars; metadata a flat string map; `allowed-tools` a list of tool names, accepted either as the spec's space-delimited string or as a YAML list of strings). Unknown frontmatter keys are preserved, not rejected. `allowed-tools` is **parsed and exposed, not enforced**: activating a skill does not change the advertised tool set. Tool-permission policy is the consumer's, and a consumer that wants it reads the field. Vendor-specific rules (reserved words, etc.) are not enforced.
- **Index:** scan an ordered list of skill directories; each subdirectory with a `SKILL.md` is a skill. Earlier directories take precedence on a name collision (later duplicates are reported and skipped). An invalid skill is reported and skipped, never fatal to the scan. The index renders a compact name-plus-description listing for the system prompt.
- **Activation:** a shared skill state (index plus the set of active skills) with two tools — activate a skill by name (loads its body) and deactivate one — implemented as `ToolHandler`s, and a **dynamic system-prompt section** that renders the index and the bodies of active skills. A consumer adds the two tools to its `ToolSet` and the section to its `Agent` and gets progressive disclosure with no further code. A consumer wanting a different policy uses the format and index layers alone.

Skills are read from disk at runtime; the crate does not cache across processes.

### Eval

`grizzly-agent-eval` answers two questions: *how does a model do at this single call?* (**ResponseEval**) and *how does this agent harness do at this task?* (**AgentEval**). Both are provider-blind — the provider is chosen when the `Model` or `Agent` is built — and both feed one shared scoring and report core.

**Shared core** (public, so a consumer with a bespoke runner can report through it):

- **Case metadata:** name, repeats (optional; the suite default applies otherwise), minimum pass rate (optional; default two thirds), and a **canary** flag that pins the threshold at 1.0. Deserializable, so consumers embed it in their own case files.
- **Repeat verdict:** passed or not, a **category** — `Pass`, `Wrong` (a readable answer that was not the expected one), `Unparseable` (the model answered and the parser could not read it), `Unavailable` (no answer came back: timeout, transport failure, rejected request), `Failed` (harness or infrastructure trouble; nothing was measured) — a human-readable reason, and optional latency and usage. `Unparseable` and `Unavailable` are always misses and are never merged: one is a finding about the model, the other about the endpoint.
- **Aggregation:** per case, repeats run, passes, pass rate, threshold, whether the threshold was met, counts per category, and latency and usage summaries.
- **Report:** one JSON document per invocation with a schema version, an invocation id (UUID v7, so reports sort by time), start time, consumer-supplied settings (an arbitrary JSON object the consumer uses to record models, filters, and anything else needed for a report to explain itself), and per-case aggregates with every repeat. A repeat carries a consumer-supplied JSON **detail** — for a miss, enough to diagnose it without a rerun (ResponseEval records the raw reply on a miss). A plain-text summary table renders from the report. Where the report is written is the consumer's choice; a helper creates a per-invocation directory under a given root.
- **Check result:** the outcome of one post-run check — a name, whether it passed, and a detail string (for example, a command's exit status and output tail). AgentEval's check step produces these, and they are recorded in the repeat's report detail. A consumer that needs richer check data keeps it in the detail text.
- **Suite result:** whether every case met its threshold, so a consumer can map it to an exit code.

Everything that would fail every repeat alike — a malformed case, an unwritable report location — is the consumer's to check before running; once a suite runs, nothing aborts it. A failed judgment is a finding, recorded as a verdict.

**ResponseEval.** A consumer defines a case type implementing a trait with three parts: build the `CompletionRequest` for this case, parse the completion into the consumer's answer type (or a parse failure), and score an answer against the case's expectation (a verdict with a reason). The case trait also has an optional fourth part, the case's **timeout**: by default it defers to the runner's default timeout; a case may return its own duration or no timeout at all, so a consumer can apply each production call's real bound. The runner, given a `Model`, a default timeout, a concurrency limit and the cases, runs every repeat of every case: it calls the model under the case's timeout, times the call, classifies any provider failure or timeout as `Unavailable`, any parse failure as `Unparseable`, and otherwise hands the parsed answer to the scorer. It records the raw reply text and usage on every repeat and keeps the raw reply in the report detail on a miss. Repeats run concurrently up to the limit. The request builder and parser are meant to be the consumer's **production** builder and parser — the eval deliberately bypasses any production fallback that would turn a dead endpoint into a plausible default answer.

**AgentEval.** A consumer defines a case type implementing a trait with four parts: build a fresh `Agent` for a repeat, **set up** the environment the agent will act on (for example: create a scratch directory and seed it) returning the task input, **check** the environment after the run (for example: run hidden commands whose results the agent never saw), and score the `RunRecord` together with the check results. The runner drives each repeat through set up → `Agent::run` → check → score. A run that returns `RunFailure::Provider` is `Unavailable` (check and score are skipped, and the failure's trace goes in the report detail); an `InvalidConversation` is `Failed`, since it is a case bug; an error in set up or check is `Failed`; otherwise the verdict is the scorer's. AgentEval repeats run one at a time by default (they are heavy and usually share an endpoint), with the limit configurable. The report detail for an AgentEval repeat includes the run's ending, total usage, round count, and check results; the full transcript is kept on a miss.

## Reasoning & alternatives

**Workspace over a single feature-gated crate.** A single crate with a feature per component was considered and rejected. Crate boundaries make layering compiler-enforced — eval cannot reach into loop internals, the loop cannot reach into a provider's — where inside one crate `pub(crate)` is visible everywhere and layering is convention. A single crate with six-plus features has hundreds of feature combinations, and `--all-features` CI hides a component that silently needs another one enabled; per-crate manifests make each dependency explicit. And prompt codegen runs in consumers' build scripts, where compiling the whole crate for the host is pure cost. The price — a shared lint table members cannot extend individually, more manifests, and shared internals having to become documented public API — is small while the codebase is small, and grows the longer the split is put off.

**Providers as their own crate, not inside core.** Considered folding providers into core behind features, since everything uses a model. But what everything depends on is the `Provider` trait and canonical types, which are core's; concrete clients are leaves nothing in core should call. A crate boundary guarantees the loop only ever sees the trait, keeps HTTP out of core entirely, and keeps provider churn from rebuilding core.

**Prompts outside the facade.** Putting codegen behind a facade feature gives consumers one dependency name but compiles core for the host inside every build script. Build scripts run on every clean build and in CI; a second dependency line costs once.

**Frontmatter parsing in prompts, and skills depending on it.** Skills and prompt files are both Markdown with YAML frontmatter, but only the split-and-parse step is shared — their schemas have nothing in common. Duplicating ~30 lines was considered; placing the shared step in prompts' default face costs skills one light dependency and keeps a single YAML dependency and a single frontmatter implementation in the workspace. Skills was kept out of the prompts crate because it runs at runtime and needs core's tool and loop types, while prompts runs at build time and must stay free of core.

**`prompt-lib`'s tool model over a schemars-derived one.** A struct deriving `JsonSchema` generates its own schema, but schema output then needs attribute nudging to read well to a model, pulls `schemars` into every consumer, and puts model-facing descriptions in Rust attributes instead of in prose files where prompts live. `prompt-lib` keeps a tool's description and parameters in the same reviewed prompt file as the rest of the model-facing text, and — with the generated `ToolDefinition` binding — closes the schema/parser drift the schemars approach was chosen to close. `schemars` is dropped. Runtime tools remain possible because `ToolSpec` owns its strings when it needs to and `ToolHandler` takes raw JSON.

**`yaml-rust2` over a serde YAML crate.** The serde-integrated YAML crates are deprecated, forks with stale release cadence, or pre-1.0. `yaml-rust2` is maintained and stable; hand-mapping a closed frontmatter schema is a small, one-time cost. `prompt-lib`'s use of `serde_yaml_ng` goes away with the move.

**Streaming as the provider primitive, and public.** Every provider streams on the wire anyway — to keep connections alive through proxies with idle timeouts, a real failure gantry hit, where long reasoning generations were killed as idle and retried to exhaustion. Given that, making the event stream the provider contract costs nothing extra and moves reassembly out of every provider into one accumulator in core. Exposing it publicly (on `Model` and through the run observer) serves interactive agents that render output live. The alternative — streaming as a hidden transport detail with only whole completions public — was the earlier plan; it was dropped because adding the public API later would mean changing the provider contract every provider implements, which is cheaper to do before there are any. The retry rule for streams (only before the first delivered event) is the unavoidable cost: partial output a caller has consumed cannot be un-sent.

**Agent behaviour is an ending; system breakage is an error.** A run that ran out of rounds, stalled, was truncated, or was handed off by a tool is the loop working correctly on an agent that behaved a certain way — a normal outcome a caller acts on and an eval scores, so it is an `Ok` ending. A provider failure means the system broke, so it is an `Err` that propagates through `?` and error chains exactly like any other failure; agent loops are not a special debugging case. Making provider failure an ending was considered, to preserve the rounds completed before it, and rejected: it puts a failure inside a successful return, where `let record = agent.run(..)?` compiles and silently proceeds. Carrying the `RunTrace` inside the error keeps both properties — loud failures and no lost progress.

**`Agent` is concrete; AgentEval evaluates harnesses built on this loop.** An agent trait would let AgentEval evaluate foreign harnesses, but the purpose of the eval feature is measuring harnesses built with this crate. A concrete type keeps the eval's contract — the `RunRecord` — owned by the same crate that produces it.

**Two eval shapes over a generic runner trait.** A single "run one repeat, return an observation" trait was considered as the only abstraction. It pushes timeouts, timing, and the `Unavailable`/`Unparseable` split onto every consumer — the parts most easily gotten wrong. Two concrete shapes handle those in the crate; the shared scoring and report core stays public for anything bespoke.

**Retry inside `Model` by default.** Every consumer surveyed either retried or listed the missing retry as a gap. Defaulting it on, with a structured decision that honours `Retry-After`, removes a class of hand-rolled substring-matching retry loops. It is configurable to off for callers (like evals measuring endpoint health) that want the raw outcome.

## External touchpoints

**LLM provider APIs (via `grizzly-agent-providers`).**
- *OpenAI-compatible chat completions:* `POST {base_url}/chat/completions` with `stream: true` and usage requested in the stream; bearer auth when a key is configured. Server-sent events terminated by `[DONE]`; tool calls arrive as indexed fragments; usage and finish reason ride trailing chunks. Errors: HTTP status with optional `Retry-After`, error payloads mid-stream, early stream end, idle expiry. Endpoints in use: OpenAI-compatible self-hosted endpoints reached through a Cloudflare tunnel (gantry), Ollama (gameservers).
- *Anthropic Messages:* `POST /v1/messages` streamed; API key header and API version header. Events for message start, content block start/delta/stop, message delta (stop reason, usage), message stop; error events mid-stream. Errors: HTTP status with `Retry-After`, overloaded, error events.
- Idempotency: a completion request is not idempotent in cost but is safe to retry; retries re-send the whole request.

**Consumers' build scripts (via `grizzly-agent-prompts` `codegen`).** Input: a prompt directory path. Output: a generated Rust module written to the build's output directory, and rebuild-on-change registration for every file in the tree. Errors: a prompt error naming the file and problem, which fails the build. The generated module's contract with the consumer: each prompt is a type with `render`; each tool is a unit type with `NAME`, `spec()`, and a `ToolDefinition` impl; params and enums are types deriving `Deserialize`. It references core types only through the configured crate path.

**Consumers' tests (via `grizzly-agent-prompts` `verify`).** Input: the prompt directory and the consumer's source root. Output: a report of orphaned prompts, stale declared call sites, and ids not found at their declared call site.

**Filesystem (skills, eval).** Skills reads skill directories at scan and activation time; unreadable or invalid skills are reported and skipped. Eval's directory helper creates a per-invocation directory under a caller-supplied root and writes the report JSON there; failure to create or write is an error returned to the caller before or after the run, never mid-suite.

**Git dependency resolution.** Consumers depend on workspace members by git URL. A consumer using more than one member (the facade plus prompts) must point every member at the same revision; otherwise Cargo builds two copies of core and generated `ToolSpec`s will not type-check against the facade's. Documented in the README as the consumption rule.

**`yaml-rust2`, `reqwest` (rustls, streaming responses), `futures-core` (the `Stream` trait in core's public API), `tokio`, `tokio-util` (`CancellationToken` in core's public API), `serde`/`serde_json`, `thiserror`, `async-trait`, `uuid`, `tracing`.** Each pinned at the version verified current and maintained at the time its phase lands. Libraries emit `tracing` events and never install a subscriber.

## Integration with existing system

**`grizzly-agent` itself.** The single crate becomes the workspace. The existing message and error types move into core, with three changes: `TurnFailure` becomes `RunFailure` (its budget and stall cases become run endings; it gains `InvalidConversation` and carries the partial trace on provider failure), `ProviderFailure` gains `InvalidRequest`, and `Content::Reasoning` gains its optional signature. `schemars` is removed. Documentation is brought to current state: the README is rewritten around the workspace, the admission bar, and the consumption rule; ADRs whose decisions this design reverses are superseded by new ADRs — the admission test and single-crate choice, no streaming (superseded by streaming as the provider primitive and a public API), tool schemas from one schemars struct, and the skills parsing/policy scope — each new ADR stating what changed and why. The primitive survey is a dated snapshot and moves to the archive. The contributor guide (`CLAUDE.md`) and `justfile` are updated for a workspace, including a recipe that builds providers with no provider features, each provider alone, and both.

**`grizzly-gameservers`.** Moves onto `grizzly-agent-prompts` for codegen and verify, and onto `grizzly-agent` for `ToolSpec` at runtime. Its in-repo `prompt-lib` crate is deleted. Its prompt tree is unchanged. The bot's existing conversion from `ToolSpec` into its own wire tool type continues to work against core's `ToolSpec` (adjusted for owned-or-borrowed strings). Its hand-written dispatch keeps compiling; adopting `ToolSet` and the typed handler adapter is available but not part of this work. Gameservers' own loop, LLM client, and session store are **not** migrated here. Its design docs and contributor guide are updated so prompt-library documentation points at the new crate; its `prompt-lib` design doc moves to its archive.

**`gantry`.** Migrates in two parts; only the first is in this work.

- **In scope:** gantry's model client is replaced by `grizzly-agent-providers`' OpenAI-compatible provider through a core `Model`, called with `complete` (gantry's endpoint is OpenAI-compatible and needs wire streaming — both covered). Gantry adopts core's conversation types and `CompletionRequest`/`Completion` throughout, including in its own loop and roles; its own chat types, client, retry, and streaming reader are deleted. This is a reshape, not a rename: gantry's flat, OpenAI-shaped messages have a distinct tool role carrying the call id, where core puts tool results as `ToolResult` blocks on a user message. Every place gantry builds or reads conversation history — its loop, its roles, and its event and transcript recording — moves to the core convention: all tool results from one round go in one user message, one `ToolResult` block per call, addressed by the call's id. Gantry's role-tier evals (watcher, policy judge, observer, reflector) are re-expressed as ResponseEval cases, keeping gantry's case files and production request builders and parsers. Gantry's harness-tier evals keep their own runner and scoring but report through the eval crate's shared core, so one `gantry eval` invocation still yields one report. Gantry's structured-output requests map onto the core response format.
- **Deferred (not in this work):** porting gantry's loop onto `Agent`, and gantry's harness tier onto AgentEval. That waits until the core loop has matured through use. Until then gantry's loop is its own, running on core's types and `Model`.

**Coexistence during transition.** Each migration lands after the grizzly-agent phase it depends on, pinned to a grizzly-agent revision that contains it. The workspace is complete and usable on its own before either sibling migrates; nothing in grizzly-agent depends on the siblings.

## Open questions

None outstanding.
