//! Phase 4.3 — governed autonomous agents.
//!
//! Unattended writes are allowed only inside a declared org policy.
//! Effective permissions = policy allow-list ∩ on_behalf_of ∩ org roles.
//! Kill switch + budget are checked before every tool. No AI bypass of authz.

#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

pub mod action;
pub mod kill_switch;
pub mod nl_workflow;
pub mod policy;
pub mod principal;
pub mod prompt_pack;
pub mod receivables;
pub mod review;
pub mod runtime;

pub use policy::{AgentPolicyDoc, PolicySnapshot};
pub use runtime::{AgentRunOutcome, StartRunRequest};
