# ADR-0005 — The provider trait is object-safe

**Status:** Accepted (2026-08-09)

## Context

The surveyed projects split three ways on how to hold a provider. `project-residuum` uses `Box<dyn ModelProvider>` and gets a failover chain out of it (`src/models/failover.rs:11` tries providers in order). `residuum-code` uses a generic `Agent<P: ModelProvider>` with static dispatch. `grizzly-gameservers` skips the trait entirely and passes `CompleteFn`/`DispatchFn` closures into `run_session`.

Rust 1.75 made `async fn` in traits work, but not dyn-compatibly — a trait with `async fn` cannot be made into a trait object without boxing the returned future.

## Decision

**The provider trait is object-safe, using `async-trait` to box returned futures.** Consumers hold `Box<dyn ModelProvider>` or `Arc<dyn ModelProvider>`.

The deciding case is that provider selection is usually a *runtime* decision. `residuum` picks providers from config and chains them for failover; a generic parameter makes that a compile-time choice, which is the wrong shape for something read out of a config file. Static dispatch would push every consumer wanting runtime selection into writing their own enum wrapper — which is the boilerplate this crate exists to delete.

The cost is one heap allocation per call. A model call is a network round trip measured in hundreds of milliseconds; the allocation is not measurable against it.

### Rejected: generic `P: ModelProvider` with native `async fn`

No dependency, no allocation, cleanest signatures. Rejected because failover — a feature one consumer already has and ships — becomes hand-written enum dispatch in every consumer that wants it, and provider choice stops being config-driven.

### Rejected: generic core plus a boxed adapter

Serves both audiences and costs nothing when unused. Rejected because it means two ways to hold a provider, and every downstream helper — the turn loop, retry wrappers, failover — has to pick which one it accepts. That choice then leaks into consumer code as an awkward conversion at each boundary. One shape that is slightly suboptimal everywhere beats two shapes that are each optimal somewhere.

### Rejected: closures instead of a trait, as `gameservers` does

`run_session` taking `CompleteFn` and `DispatchFn` is genuinely flexible and made that codebase's core testable. But closures cannot carry associated metadata — the model name, the provider's identity for error messages — so every caller ends up threading that alongside, which is a struct with extra steps.

## Consequences

- `async-trait` is a non-optional dependency. It is a proc macro on a stable, widely-depended-on crate, and the alternative is hand-writing `Pin<Box<dyn Future + Send>>` in every signature.
- Provider implementations are `Send + Sync` so they can sit behind an `Arc` and be shared across concurrent turns.
- If native dyn-compatible `async fn` in traits lands, this can migrate without changing consumer-facing signatures — `async-trait` and the hand-written form produce the same call syntax.
