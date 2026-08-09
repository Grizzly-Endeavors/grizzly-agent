//! Foundational primitives for building LLM agents in Rust.
//!
//! This crate is a dependency, not an application. It provides the pieces an
//! agent is assembled from — provider connectors, conversation types, tool
//! definition and dispatch, the turn loop — and leaves policy to the consumer.
//!
//! # Scope
//!
//! The boundary is deliberate: a primitive belongs here when at least two
//! independent projects need it and neither can define it better locally.
//! Anything that encodes a product decision — which model to use, what a tool
//! is allowed to do, how a conversation should be persisted — belongs in the
//! consumer, behind a trait this crate defines but does not implement.
//!
//! `docs/decisions/` records the calls that were close.
