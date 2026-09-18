//! The turn loop and [`Agent`]: a configured, reusable harness combining a
//! [`crate::Model`], a [`crate::ToolSet`], a system prompt built from
//! sections, and limits on how long a run may go.

mod observer;
mod run;
mod section;
mod trace;

pub use observer::RunObserver;
pub use run::{Agent, AgentBuilder, Limits};
pub use section::{DynamicSection, SystemSection};
pub use trace::{RoundRecord, RunEnding, RunRecord, RunTrace, ToolCallRecord};
