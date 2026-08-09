# ADR-0004 — Token counts come from the provider

**Status:** Accepted (2026-08-09)

## Context

Every surveyed project estimates tokens by dividing character count by four. `project-residuum/src/memory/tokens.rs:12` says so in its own doc comment — "sufficient for threshold comparisons, not billing." `career-scanner` truncates context by character count with a comment converting the cap to an approximate token figure. Only `Ursix/src/tokens.rs:128` loads a real tokenizer, and it hardcodes GPT-2 as a cross-provider approximation, which is wrong for the Claude, Gemini, and Llama families it is standing in for.

So the survey shows a genuine gap. It does not show that this crate should fill it, because the same survey shows every provider already returns exact counts: `entity-extractor` parses `Usage { prompt_tokens, completion_tokens, total_tokens }` from every response, `career-scanner` reads Ollama's `prompt_eval_count`/`eval_count`, and `gameservers` captures the same and explicitly notes that a count the provider did not report must read as *unknown* rather than zero. All three then discard the number.

The gap is not that counts are unavailable. It is that nobody kept them.

## Decision

**This crate reports the provider's token counts and does not compute its own.** No tokenizer dependency, no `chars / 4` helper.

`Usage` carries every field as `Option`, following `gameservers/src/agent/llm.rs:171`: a provider that reported no count must be distinguishable from one that reported zero. Coercing an absent count to zero turns "we don't know what this cost" into "this was free," which is the one wrong answer.

A consumer that needs a count *before* sending — to budget a prompt against a context window — brings its own tokenizer. That is a legitimate need, and it is also exactly where model-family-specific accuracy matters most, which is the part this crate cannot do well for someone else.

### Rejected: bundle `tokenizers` with a pluggable model-family selection

The honest version of this is a trait plus a dependency that pulls in Hugging Face tokenizer loading, and a per-model-family mapping table that goes stale every time a provider ships a new model. Consumers who never count tokens ahead of time — which is all ten surveyed projects — would carry that for nothing.

### Rejected: ship the `chars / 4` heuristic as a documented approximation

It is four lines, so it looks free. But shipping it blesses it: it becomes the obvious thing to reach for, and the projects that reached for it are precisely the ones that ended up truncating context by character count and calling it a token budget. A crate that offers a wrong-but-convenient answer will have that answer used.

## Consequences

- Context-window budgeting is the consumer's responsibility. What this crate provides instead is transcript trimming by **message count and byte budget**, which is what `gameservers/src/agent/store.rs:120` actually does and which needs no tokenizer to be correct.
- Cost tracking is possible for consumers that want it, since exact counts now survive instead of being discarded — but this crate does not enforce a budget. A cost ceiling that can abort a run mid-flight is a real gap in the survey, and it is a consumer-side policy decision.
- If a provider stops reporting usage, that surfaces as `None` rather than silently as zero.
