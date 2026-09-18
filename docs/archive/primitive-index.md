# Primitive index

A survey of every first-party project that calls an LLM, indexed for primitives worth reconstructing here. This is the evidence base for what `grizzly-agent` should contain — and, more importantly, what it should refuse.

The admission test from `CLAUDE.md` applies throughout: **would a second project have written this the same way?** Each entry below records how many projects independently needed the thing, which is the empirical form of that test.

## Projects surveyed

| Project | Language | LLM use | Value as a source |
| --- | --- | --- | --- |
| `grizzly-gameservers` (Gary) | Rust, ~428 files | Unattended production ops agent | Highest. Best turn loop, only approval-gating and safety work anywhere. |
| `project-residuum` | Rust, ~291 files | Personal agent runtime | Highest. Only `ModelProvider` trait, memory, MCP, multi-provider. |
| `career-scanner` | Rust, ~218 files | Extraction and ranking pipeline | High. Best structured-extraction discipline. Fully implemented, not design-only. |
| `dormant/entity-extractor` | Rust workspace | High-throughput local SLM extraction | High. Only real concurrency, local-model lifecycle, and DAG work. |
| `dormant/Ursix` | Rust, ~48 files | Stateless single-call CLI | Medium. Best JSON repair and tokenizer work. |
| `residuum-code` | Rust, ~21 files | Terminal coding agent | Medium. Cleanest minimal loop; largely duplicates `project-residuum`. |
| `job-finder` | Python | LLM + browser-automation application pipeline | High as design input. Best telemetry/replay workflow anywhere; `career-scanner`'s predecessor. |
| `stan-eval-sidecar` | Python | Local Gemma behind an OpenAI-compatible surface | High. Its README *is* a provider contract spec, and it solved problems no Rust project here has hit. |
| `poe2/mcp` | Python | MCP server, ~30 tools | Medium. Valuable mostly as a worked example of the dual-registration anti-pattern. |
| `notes-reorg` | Python | Local-LLM vault classification | Medium. Only real grammar-constrained decoding and batch classification. |
| `lab-cli`, `grizzly-widget-builder`, `grizzly-invite`, `browser-ai` | — | None | No AI code. Excluded. |

## The duplication already happened

The strongest evidence for this crate is that the copying is not hypothetical — it has already occurred by hand:

- `project-residuum/src/models/retry.rs` and `residuum-code/src/core/providers/retry.rs` are **byte-for-byte identical**.
- `grizzly-gameservers/src/discord/chunking.rs` carries a doc comment stating it was **"ported from the residuum Discord adapter."**
- `Ursix::LlmClient`, `residuum::ModelProvider`, and the implicit shape in `career-scanner` and `gameservers` are four independent attempts at the same trait.
- `residuum-code` had to change `ModelError::Request` to `Arc<reqwest::Error>` purely to make the error `Clone` for retry closures — the same type, re-derived differently per project.

Every one of these passes the two-consumer test on evidence rather than argument.

## Tier 0 — the spine

Needed by every project surveyed. No product decisions embedded. These are the crate whether or not anything else is.

**Conversation types** — `Message`, `Role`, `ToolCall`, `ToolDef`, `Completion`. Four independent implementations: `residuum/src/models/mod.rs:81`, `Ursix/src/llm/mod.rs:74`, `gameservers/src/agent/llm.rs:25`, `career-scanner/src/clients/llm.rs:15`. All four converged on the OpenAI wire shape; none support Anthropic content blocks or multimodal, which is the one thing to fix rather than copy.

**`ModelProvider` trait** — `residuum/src/models/mod.rs:346` is the reference (`async fn complete(&messages, &tools, &options) -> Result<ModelResponse, ModelError>`). `Ursix/src/llm/mod.rs:151` is the same idea independently. `gameservers`, `career-scanner`, and `entity-extractor` all hardwired a single concrete client and each flagged the missing seam as their largest gap.

**`Usage` with every field optional** — `gameservers/src/agent/llm.rs:171`. A provider that reports no token count must read as *unknown*, never coerced to zero. Small, but the one project that got this right documented it as a trap: "a token count of `?` means the provider reported none, never that the call was free."

**Retry and backoff** — two incompatible designs already exist. `entity-extractor/crates/extractor-core/src/llm/retry.rs:41` is a pure decision function (`decide(attempt, outcome, retry_after) -> Retry(Duration) | Stop`) that honors `Retry-After` and classifies on structured HTTP status. `residuum/src/models/retry.rs:52` and `Ursix/src/llm/retry.rs:51` are closure combinators that classify by **substring-matching the error message** for `"rate"`, `"429"`, `"503"`. The structured decision is the correct core; the combinator is the better ergonomics. `gameservers` and `career-scanner` have **no retry at all** and both flagged it.

**Typed error taxonomy** — the public error must let a caller distinguish "rate limited, back off" from "malformed request, retrying won't help." `career-scanner/src/error.rs:19` has the best three-audience version (machine code, user sentence, developer chain via `error_chain`).

**Reasoning as a separate channel** — `stan-eval-sidecar/README.md:117` documents providers disagreeing on the field name (`thinking` / `reasoning` / `reasoning_content`) and, having hit it, insists thought text never be inlined into `content`. `entity-extractor` independently handles the same dual-field split for Gemma versus DeepSeek/Qwen. Pick one canonical field here and adapt at the edges rather than propagating the ambiguity inward.

**The wire contract itself** — `stan-eval-sidecar/README.md:49-262` is the most precise statement of the chat-completions surface in the whole survey, and `tier1/translate.py` proves it is backend-agnostic by mapping the identical contract onto a second engine with pure, I/O-free translation functions. That is the shape the provider boundary should take here: `From`/`TryFrom` between a provider's wire types and canonical types, testable without a network.

## Tier 1 — the loop

**Turn loop** — `gameservers/src/agent/session.rs:194` (`run_session`) is the cleanest: fully IO-free, provider- and transport-agnostic, driven by two injected async closures, and **proven** by two independent shells (Discord and in-game) running the same core. `residuum-code/src/core/agent/mod.rs:52` is the minimal generic version. `residuum/src/agent/turn.rs:71` is the most featureful but heavily entangled with its event bus and memory context. `career-scanner/src/interview/exchange.rs:160` independently arrived at the same bounded-iteration, trait-based shape.

**Stop conditions** — a round budget (`DEFAULT_MAX_ROUNDS = 16`) as the last-resort backstop, layered *under* a first-class escalation exit a tool can request mid-turn (`gameservers/src/agent/session.rs:53`). The two-tier design — agent-requested versus system-forced — is worth codifying once.

**Tool trait, registry, dispatch** — `residuum/src/tools/mod.rs:212` (`Tool` trait) plus `registry.rs:24`. `gameservers/src/discord/gary/tools.rs:279` contributes a better registry idea: one table is the single source of a tool's required capability tier, read by all three consumers (what's advertised to the model, the dispatch-time gate, the refusal message), so they cannot drift apart.

**Transcript trimming** — `gameservers/src/agent/store.rs:120` is the best version anywhere: caps by message count **and** byte budget walking backward from newest, and guarantees the window never starts mid tool-call/tool-result pair. The byte cap exists because one large tool result can dwarf a count-based limit. `residuum`'s equivalent is a fixed "keep last 3 exchanges" with no budget awareness.

**Tool failure is data, not a loop error** — `job-finder/src/agent/agent.py:271` never lets a failing tool raise out of the loop; the error becomes the tool-result string the model reads and reacts to. Only provider and transport failures abort. In Rust this wants to be two distinct types so the distinction cannot be conflated by accident: a recoverable `ToolError` that always becomes message content, and a loop-fatal error that propagates.

**Stuck-loop detection** — `notes-reorg/src/notes_reorg/agent.py:173` aborts when the same `(action, args)` signature repeats four times running, rather than burning the whole iteration budget. `job-finder` instead uses a ten-minute wall-clock idle timer that only resets on non-error results — which misses the common failure entirely, since a tight unproductive loop returns *fast*. Both are needed: a per-step timeout and a repeat-signature breaker.

## Tier 2 — structured output

The deepest vein across the survey, and the place where four projects each solved half the problem.

**JSON repair** — `Ursix/src/json_repair.rs:106` is the most complete: strips fences, drops preamble prose, fixes Python `True`/`None`, single quotes, unquoted keys, trailing commas, unescaped control characters, and closes truncated structures with a string-aware brace stack. Reports which repairs it applied and distinguishes truncated-unrecoverable from malformed-unrecoverable. Pure string→string, zero dependencies — the single most portable item found.

**Defensive extraction** — `career-scanner/src/domain/json_extract.rs:27`. Simpler than the above: strip fences, take first `{` through last `}`, deserialize.

**Schema-constrained decoding** — `residuum/src/models/mod.rs:283` (`ResponseFormat::JsonSchema`) with per-provider mapping for Anthropic, OpenAI, Gemini, and Ollama. `entity-extractor` uses the same against vLLM.

**Validate-then-accept** — `career-scanner/src/pipeline/evaluate.rs:85`. Define a lenient all-`Option` reply struct, then a strict target with `#[serde(try_from = "...")]` whose `TryFrom` validates ranges and required fields. The resulting `Err(String)` is fed back to the model **as the retry prompt**. This is the idiom that makes retry actually converge.

**Parse-retry gateway** — `career-scanner/src/llm/gateway.rs:43`. One choke point: send, parse, and on parse failure retry exactly once with a corrective turn appended. Transport failures are deliberately not retried here. Preserves every attempt for telemetry.

**Schema and prompt from one source** — `entity-extractor/crates/extractor-pipelines/src/targets/schema_builder.rs:7` and `prompt_builder.rs:7`. A single target definition generates both the JSON Schema *and* the natural-language instruction, so the two cannot drift. Stronger than hand-writing both, which is what every other project does.

**Generate → validate → retry-with-the-error** — `job-finder` implements this shape **three separate times** with different signatures and attempt counts (`tailoring/resume_generator.py:21`, `search/strategy.py:58`, and `tailoring/renderer.py:78`). The third is the interesting one: it validates *after* rendering to PDF, checking page-fill ratio, and feeds an actionable hint ("add more bullet points") back into the next generation cycle. That argues the validator should be a pluggable, composable trait — JSON shape, then domain rules, then an external-process check — rather than a fixed pipeline.

The composition nobody built: **constrain first, repair as fallback.** `Ursix` has only repair, `entity-extractor` and `notes-reorg` have only constraint, `job-finder` has only repair *and* relies on it as the primary path because its hosted model ignores JSON mode. Structured output should be a provider *capability query* first, with text repair as the documented fallback — not the default path it became by accident.

## Tier 3 — the judgment calls

Each of these is genuinely useful and each carries a real cost — heavy dependencies, an embedded product opinion, or both. These are the scope decisions, not defaults.

**Approval gating for destructive actions** — `gameservers/src/discord/gary/tools.rs:2666`. A click-to-approve gate with an audited `Preapproved` escape hatch for headless driving, and a type-level guard (no reachable `Default`) so a new surface cannot silently inherit unattended-destructive. The only safety work of its kind in the survey.

**Snapshot → apply → verify → auto-rollback** — `gameservers/src/discord/gary/recovery.rs`. ~15 lines of pure state machine deciding healthy / roll back / escalate / inconclusive. Deliberately owned by the loop, not the model, because recovery "must not depend on the model still having round budget."

**Untrusted-text fencing** — `gameservers/src/untrusted.rs`. Wraps non-platform-authored text in a fence and *removes* lines attempting to forge a fence marker. Its own docs flag that it does not address tool-result-side injection.

**Admission throttle** — `gameservers/src/throttle.rs`. Per-subject token bucket that **refuses rather than queues** ("a queued flood is still a billed flood") plus a global concurrency semaphore, both live-retunable.

**Concurrency-bounded client** — `entity-extractor/crates/extractor-core/src/llm/client.rs:116`. Semaphore backpressure plus in-flight/peak/latency stats. The best throughput primitive found; `career-scanner` has none and processes every batch strictly sequentially.

**Local model lifecycle** — `entity-extractor/.../server/process.rs:34` (`VllmServer`: process-group spawn, readiness polling, SIGTERM→SIGKILL shutdown) and `swap.rs:21` (`ProcessSwap`: single-active-model hot-swap for GPU-constrained hardware). Correct process-group signal handling is rare. Hardcoded to `uv run vllm serve`. `stan-eval-sidecar/sidecar/app.py` adds the in-process variant: a single dedicated worker thread because the native GPU context must stay on one OS thread, plus a readiness gate that returns a legible 503 instead of hanging and sheds load past a queue-depth cap. Rust can make the thread-affinity requirement structural — confine the engine to one thread by construction with a channel actor as the only way in — rather than relying on convention.

**Prefix-keyed conversation cache** — `stan-eval-sidecar/sidecar/conversation_cache.py:37`. Wraps a stateful engine (a KV cache that wants one message at a time) behind a stateless API where the client resends the whole array every request: keep live conversations keyed by the prefix they represent, find the longest match, feed only the new tail, rebuild on miss rather than failing. Paired with `messages.py:81`, which compares messages by **parsed value rather than wire bytes** — a client re-serializing tool-call JSON with different key order was silently missing the cache on every follow-up round, and the eval scored that as the model declining to answer. A correctness bug wearing a quality-signal costume, and worth encoding as a type invariant here rather than a runtime convention.

**Rate limiting** — `poe2/mcp/src/api/rate_limiter.py:14`. Token bucket fused with adaptive exponential backoff (`record_success` / `record_failure` scaling a multiplier up to 32×), keyed per endpoint. Complements `gameservers`' throttle, which governs admission rather than outbound pacing.

**MCP client** — `residuum/src/mcp/client.rs:30` (stdio and Streamable HTTP over `rmcp`) plus `registry.rs:99`, which does pure desired-vs-running diff reconciliation with reference-counted lifecycle.

**Memory and retrieval** — `residuum/src/memory/search.rs:752`. Full hybrid BM25 (tantivy) + vector (sqlite-vec) retrieval with score normalization, weighted merge, and exponential temporal decay. Heavy, but self-contained — no hosted vector DB required.

**Prompt assets as files** — `residuum/src/skills/parser.rs:14` parses Markdown + YAML frontmatter (the Claude Code skill format). `gameservers/crates/prompt-lib` goes further and *compiles* prompt files into typed Rust at build time, generating zero-cost `render()` and tool `spec()`. `prompt-lib/src/verify.rs:34` cross-references each prompt's declared call site against the source tree to catch orphaned or stale prompts — a problem every prompt-heavy codebase has and almost none tests for.

**Telemetry** — three independent designs. `gameservers/src/agent/recorder.rs` makes instrumentation *structural* (the loop feeds the recorder, so a shell cannot forget to wrap) and mints trace ids before spans exist so a turn cut short still exports. `residuum/src/util/telemetry/buffer.rs:26` is a bounded span ring buffer with field-name-based redaction before export. `career-scanner/src/pipeline/telemetry.rs:36` puts transcripts in S3 with a DB row as pointer.

**DAG pipeline** — `entity-extractor/crates/extractor-pipelines/src/dag/`. `Handle<T>` gives compile-time-checked edge wiring over type-erased runtime storage; the executor clusters consecutive same-model nodes to minimize local-model swaps and runs two-level concurrency. Elegant, and arguably a separate crate rather than part of an agent toolkit.

## Gaps — nobody has these

Where all six Rust projects came up empty. These cannot be extracted; they must be built.

**Streaming.** Zero streaming anywhere. Every provider client in every project sets `stream: false` and blocks for the full response. `residuum` fakes the UX by publishing whole text blocks between tool calls. For an interactive agent toolkit this is table stakes and it is entirely absent.

**Real tokenization.** Every project uses `chars / 4`. Only `Ursix/src/tokens.rs:128` loads an actual tokenizer, and it hardcodes GPT-2 as a cross-provider approximation — wrong for Claude, Gemini, and Llama families. `career-scanner` truncates by character count with a comment estimating the token equivalent.

**Cost and budget tracking.** `Usage` is parsed from responses in every project and then discarded. `entity-extractor` tracks latency and request counts but never tokens-to-dollars. `gameservers`' throttle counts *turns*, not tokens — for a system whose premise is unattended production spend, there is no token-aware admission control anywhere.

**Prompt caching strategy.** `residuum` applies Anthropic's `cache_control` uniformly with no breakpoint placement; nothing separates the stable system prompt from fast-changing tool definitions or history. No other project addresses it.

**Evaluation harness.** No golden-transcript regression suite, no scoring rubric persistence, no batch grading in any project. The closest analogues are `gameservers`' operator HTTP harness (drive a live turn, get a full tool trace back — genuinely good, and what makes agent-driven verification possible), `residuum`'s mid-turn LLM-judge "subconscious" (a runtime supervisor, not an offline eval), and `stan-eval-sidecar/scripts/verify_api_contract.py`, which is an acceptance gate for a *serving contract* rather than agent judgment. Since `job-finder` is `career-scanner`'s predecessor and neither has one, this is the most clearly actionable gap.

**Prompt versioning.** Prompts are edited in place everywhere. `job-finder` stores the fully-rendered prompt text per telemetry event but no template identity or hash, so "did this bad evaluation come from the old or new prompt?" can only be answered by diffing file history against timestamps. Telemetry should record `(template_id, template_hash, rendered_vars)`, not just the rendered string. `gameservers`' `prompt-lib` is the only project close to solving this, and it solves staleness rather than versioning.

## Friction worth designing away

Recurring pain that indicates a primitive should exist:

- **Tool dispatch has no single source of truth — in three projects.** `gameservers`' `prompt-lib` generates tool *schemas* and *params structs* but stops short of dispatch, leaving ~30 hand-written match arms. `poe2/mcp` declares each tool's schema in `list_tools()` and its handler in a parallel 30-branch `if/elif` roughly 700 lines away, with nothing keeping them in sync. `job-finder` splits dispatch three ways (an MCP name-set check, one hardcoded special case, and a handler dict), so adding a tool means remembering three places. One structure defining name, schema, and handler together is the single most repeated unmet need in the survey.
- **Config loading is repeated per subsystem.** `gameservers/src/config.rs` repeats the closure-based env-lookup pattern six times by hand. The pattern is right; the repetition wants a derive.
- **Cross-cutting tool signals have no home.** `gameservers`' `ToolCtx` accumulates ad-hoc `Mutex<Option<T>>` scratch slots (pending change, guardrail verdict, escalation request), each with its own note/take/lock trio, because the loop offers no typed side-channel.
- **Optional subsystems that never branch on "is it configured."** `gameservers` uses a consistently good shape — the telemetry, memory, and defer subsystems all construct successfully with missing credentials and degrade to a silent no-op sink, so no caller writes `if telemetry.is_some()`. `poe2/mcp`'s tiered cache does the same by simply omitting an unavailable tier from the list rather than flag-checking per call. Worth adopting as a convention here.
- **Partially-specified config silently changes behavior.** `stan-eval-sidecar`'s README documents a sampler config that, half-filled, quietly reintroduced nondeterminism by defaulting the unset fields. The fix was a runtime discipline ("pass nothing or fill it completely"). Rust can close this class structurally: make all-or-nothing groups an enum rather than a struct of `Option` fields, so "partially specified" is not representable.
