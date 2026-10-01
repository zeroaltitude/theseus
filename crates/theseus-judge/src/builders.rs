//! The state builders (design §2.4): each takes a plain input struct this
//! crate defines, and returns a capped state and the dynamic items its pack's
//! questions draw on. The core maps its own types onto these inputs at the
//! wire-in (`theseus-core/src/judge/inputs.rs`, 23a), scrubbing as it goes;
//! every string is scrubbed here again before it is cut.
//!
//! A builder computes what Jev is weak at (counts, durations, sums) and
//! states it as a field, so no question has to ask for it.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::pack::{Builder, Dynamic, Pack};
use crate::state::{BuiltState, Keep, Scrub, StateBuilder};

/// Each builder's version: any change to what a builder emits is a new
/// version, recorded with every judgment, so a replay knows to rebuild.
pub const PROBE_VERSION: u32 = 1;

/// A state, and its dynamic items, ready to ask.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub state: Arc<BuiltState>,
    pub dynamic: Dynamic,
}

/// The probe's synthetic event (the test pack's state).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeInput {
    pub service: String,
    pub event: String,
    #[serde(default)]
    pub recent_events: Vec<String>,
    pub operator_on_call: bool,
}

/// Any builder's input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "builder", content = "input", rename_all = "snake_case")]
pub enum Input {
    Probe(ProbeInput),
}

impl Input {
    pub fn builder(&self) -> Builder {
        match self {
            Input::Probe(_) => Builder::Probe,
        }
    }

    /// A builder's input from its JSON (a fixture, or the probe's file).
    pub fn parse(builder: Builder, json: &str) -> anyhow::Result<Input> {
        Ok(match builder {
            Builder::Probe => Input::Probe(serde_json::from_str(json)?),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("pack {pack} builds its state with {wants:?}, but the input is for {got:?}")]
pub struct WrongInput {
    pub pack: String,
    pub wants: Builder,
    pub got: Builder,
}

/// The pack's state from its builder's input, under the pack's cap.
pub fn prepare(pack: &Pack, input: &Input, scrub: &dyn Scrub) -> Result<Prepared, WrongInput> {
    if input.builder() != pack.builder {
        return Err(WrongInput {
            pack: pack.name(),
            wants: pack.builder,
            got: input.builder(),
        });
    }
    let cap = pack.state_cap_tokens;
    Ok(match input {
        Input::Probe(i) => probe(i, cap, scrub),
    })
}

/// The test pack's state: the service, the event, what happened just before
/// (newest last), and whether someone is on call.
pub fn probe(i: &ProbeInput, cap_tokens: u64, scrub: &dyn Scrub) -> Prepared {
    let mut b = StateBuilder::new("probe", PROBE_VERSION, cap_tokens, scrub);
    let recent: Vec<serde_json::Value> = i
        .recent_events
        .iter()
        .map(|e| serde_json::Value::String(b.clip(e, 300)))
        .collect();
    b.scalar("service", b.clip(&i.service, 80))
        .text("event", 9, cap_tokens / 2, Keep::Both, &i.event)
        .list("recent_events", 5, cap_tokens / 3, recent)
        .scalar("operator_on_call", i.operator_on_call);
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::by_name;
    use crate::state::NoScrub;

    pub(crate) fn probe_input() -> ProbeInput {
        serde_json::from_str(include_str!("../fixtures/inputs/probe.json")).unwrap()
    }

    #[test]
    fn the_probe_state_is_its_fields_in_order() {
        let p = prepare(
            &by_name("probe.v1").unwrap(),
            &Input::Probe(probe_input()),
            &NoScrub,
        )
        .unwrap();
        assert_eq!(
            p.state.fields,
            vec!["service", "event", "recent_events", "operator_on_call"]
        );
        assert!(p.state.truncated.is_empty());
        assert_eq!(p.state.builder, "probe");
    }
}
