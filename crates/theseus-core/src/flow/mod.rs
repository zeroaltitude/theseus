//! Flows (theseus-pw1q.21.4; RSI loop 1 §1.3 and §5.2): the cheapest kind of
//! minted tool. A flow is a frozen JSON spec of steps over tools that already
//! exist, and `flow:<name>` runs it. **Every step goes through
//! `ToolRuntime` exactly as the model's own call would**: planned, gated at
//! the model's posture, under the place rule, L0 or L1, with its ordinary
//! `ToolCall`, result and ledger rows. So a flow can do nothing the model
//! could not have done with the same calls in that place, adds no capability,
//! and can load live.
//!
//! - [`spec`]: the v1 spec, its validation (the allow-list, the bounds), its
//!   digest and its bindings. R7 builds a spec only through these types.
//! - [`class`]: a result's classes, the one reading the miner (R5) and the
//!   mint (R7) share.
//! - [`load`]: freeze by digest at `<state>/self/mint/<name>/<digest>/`,
//!   load, retire, list, and the board the catalog reads. R8 loads a flow
//!   only through [`Core::flow_load`](crate::rpc::Core).
//! - [`tool`]: the loaded flow as a tool and a deferred family, `flow:<name>`
//!   (wire `flow__<name>`), offered only in its scope: the repositories it
//!   names, and the kinds of place. With no flow loaded (the default) nothing
//!   is added to any request.
//! - [`run`]: the run. A step that would ask for approval, or that policy
//!   refuses, stops the flow: refusal is the flow's result, and an approval
//!   is handed back as the exact call, which the model then makes itself and
//!   so waits as any call waits (the flow holds no state across a turn's
//!   park). A result that needs judgment hands the model structured state.
//!
//! The caps: [`spec::MAX_STEPS`] steps in a spec and in a run,
//! [`MAX_SECS`] seconds, and [`run::TAIL`] characters of any output tail.

pub mod class;
pub mod load;
pub mod run;
pub mod spec;
pub mod tool;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_catalog;
#[cfg(test)]
mod tests_load;
#[cfg(test)]
mod tests_run;

pub use load::Board;
pub use tool::is_flow;

/// The longest a run goes on before its next step: ten minutes.
pub const MAX_SECS: u64 = 600;

/// A flow's own call at the gate: it only starts its steps, each of which is
/// gated as the model's own call, so it asks nothing itself. The floor still
/// holds it, as it holds any call.
pub(crate) fn quiet(d: crate::policy::Decision) -> crate::policy::Decision {
    use crate::policy::{Decision, Posture};
    if d.floor || d.posture == Posture::Open {
        return d;
    }
    Decision {
        posture: Posture::Open,
        reason: "a flow — open (each step is gated as your own call)".into(),
        notify: None,
        ..d
    }
}
