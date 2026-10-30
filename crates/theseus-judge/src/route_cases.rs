//! `route.v10`'s labelled cases (theseus-h66v4): the red commit's skeleton. The types and the
//! signatures are the module's; the bodies are not built yet, so the tests written for it fail.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::builders::{Prepared, INBOUND_VERSION};
use crate::judge::{Ask, Judgment, Mode};
use crate::pack::{Dynamic, Pack};
use crate::state::{NoScrub, Scrub, StateBuilder};

/// The test set, as the pack's directory holds it.
pub const V10: &str = include_str!("../packs/route.v10.cases.toml");
pub const PINNED: u32 = 2;
pub const DESIGN_TOTAL: u32 = 60;
pub const BAR_MODEL: u32 = 54;
pub const BAR_EFFORT: u32 = 50;
pub const BAR_CONTEXT: u32 = 56;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    pub model: String,
    pub effort: String,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseState {
    pub place: String,
    pub place_kind: String,
    pub restricted_models: String,
    pub price_period: String,
    pub est_tokens: u64,
    pub last_reply_model: String,
    pub task_models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub id: String,
    pub message: String,
    pub why: String,
    pub must: bool,
    pub state: CaseState,
    pub want: Want,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseSet {
    pub pack: String,
    pub cases: Vec<Case>,
}

pub fn declared_pack(_text: &str) -> Result<String, String> {
    Err("not built".into())
}

impl CaseSet {
    pub fn parse(_text: &str, _pack: &Pack) -> Result<CaseSet, String> {
        Err("not built".into())
    }
}

pub fn state(_case: &Case, cap: u64, scrub: &dyn Scrub) -> crate::state::BuiltState {
    StateBuilder::new("inbound", INBOUND_VERSION, cap, scrub).build()
}

pub fn prepared(pack: &Pack, case: &Case) -> Prepared {
    Prepared {
        state: Arc::new(state(case, pack.state_cap_tokens, &NoScrub)),
        dynamic: Dynamic::default(),
    }
}

pub fn ask(pack: &Arc<Pack>, case: &Case, _context: &Value) -> Ask {
    Ask::new(
        pack.clone(),
        &prepared(pack, case),
        Mode::Shadow,
        json!({"case": case.id}),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub want: String,
    pub got: String,
    pub confidence: f64,
    pub agrees: bool,
    pub close: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Graded {
    pub id: String,
    pub must: bool,
    pub answered: bool,
    pub model: Check,
    pub effort: Check,
    pub context: Check,
    pub under_confirm: bool,
}

pub fn grade(_pack: &Pack, case: &Case, _j: Option<&Judgment>) -> Graded {
    let miss = |want: &str| Check {
        want: want.into(),
        got: String::new(),
        confidence: 0.0,
        agrees: false,
        close: false,
    };
    Graded {
        id: case.id.clone(),
        must: case.must,
        answered: false,
        model: miss(&case.want.model),
        effort: miss(&case.want.effort),
        context: miss(&case.want.context),
        under_confirm: false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Totals {
    pub answered: u32,
    pub model_ok: u32,
    pub effort_exact: u32,
    pub effort_close: u32,
    pub context_ok: u32,
    pub must_total: u32,
    pub must_ok: u32,
    pub must_under_confirm: u32,
    pub bar: Option<bool>,
}

pub fn totals(_graded: &[Graded]) -> Totals {
    Totals::default()
}

pub fn model_counts(_set: &CaseSet) -> std::collections::BTreeMap<String, u32> {
    Default::default()
}
