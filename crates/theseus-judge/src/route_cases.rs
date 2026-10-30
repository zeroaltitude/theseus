//! `route.v10`'s labelled cases (theseus-h66v4, design §4): a case is a
//! message, the state the daemon puts beside it, and the model, effort and
//! context the owner (or the design) says Jev should choose. This is the pure
//! half of `theseus judge replay --cases`: read the file, build each case's
//! ask, and grade the judgment that comes back. The core makes the calls
//! ([`crate::replay`]'s way: the core reads the record, these say what it
//! means).
//!
//! - **The file** is TOML: `pack`, an optional `[defaults]` table, and a
//!   `[[case]]` for each case. A case names only the state fields that differ
//!   from the defaults. [`CaseSet::parse`] refuses an unknown key, a
//!   duplicate id, a state word outside its set, and a label that is not an
//!   option of the pack's `model`, `effort` or `context` question.
//! - **The state** is [`state`]'s: the message, `author`, `place_kind`, and
//!   the design's four new fields (`price_period`, `restricted_models`,
//!   `est_tokens`, `last_reply_model`, and `task_models` when there are
//!   any). It is built here, beside `builders::inbound`, so that a replay
//!   needs no live turn; the fields' names are the ones the daemon's own
//!   state takes when the state-and-catalog row lands, and the replay's
//!   builder follows it there.
//! - **The grade** compares what routing would decide for each of the three
//!   deciding questions to the label: Jev's top option when its band reached
//!   the question's `confirm` bar, else the question's `no_match`, both read
//!   from the pack replayed (theseus-mir9c; before it the raw top option was
//!   graded, which under-credited `context` and over-credited `model`). The
//!   raw option and its grade stay beside it. `effort` is graded to the exact
//!   rung and to within one rung (`low`, `medium`, `high`, `xhigh`, `max` in
//!   order); `default` is on no rung, so it agrees with `default` only. The
//!   daemon applies an `effort` under the bar anyway when its confidence is
//!   at least `[routing] effort_confidence` (`routing::effort::decide`); this
//!   grade does not model that second bar. A `must` case whose `model` answer
//!   sits under the confirm bar is counted apart too: it runs on the no-match
//!   option, whatever its top option.
//! - **The bar** is the design's (§4.3), counted over all sixty cases, with
//!   the two pin cases W39 and W40 (daemon tests, [`PINNED`]) counted as met:
//!   `model` at least 54, `effort` within one rung at least 50, `context` at
//!   least 56, every `must` case right and none under the confirm bar.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::band::{confidence_band, Band};
use crate::builders::Prepared;
use crate::client::Answer;
use crate::judge::{Ask, Judgment, Mode};
use crate::pack::Pack;
use crate::state::{NoScrub, Scrub};

/// The test set, as the pack's directory holds it.
pub const V10: &str = include_str!("../packs/route.v10.cases.toml");

/// The design's cases that are daemon tests, not replay cases (W39, W40).
pub const PINNED: u32 = 2;
/// Cases in the design's set; the file holds this many less [`PINNED`].
pub const DESIGN_TOTAL: u32 = 60;
/// The bar over the design's sixty: `model`, `effort` within one rung, `context`.
pub const BAR_MODEL: u32 = 54;
pub const BAR_EFFORT: u32 = 50;
pub const BAR_CONTEXT: u32 = 56;

/// The efforts on a rung, lowest first. `default` is on none.
const RUNGS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// What the owner or the design says Jev should choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    pub model: String,
    pub effort: String,
    pub context: String,
}

/// The state a case carries beside its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseState {
    /// `dm` or `channel`.
    pub place: String,
    /// The state's `place_kind`: `discord_dm`, `discord_channel`, or a case's
    /// own (`job`, `task_brief`, `task_checkpoint`).
    pub place_kind: String,
    /// `allowed` or `not_allowed`: China-origin models.
    pub restricted_models: String,
    /// `peak` or `off_peak`.
    pub price_period: String,
    pub est_tokens: u64,
    pub last_reply_model: String,
    /// Every model that has written in the task.
    pub task_models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub id: String,
    pub message: String,
    /// The rule the case tests, or where its label came from.
    pub why: String,
    /// The pack must pass it on every run.
    pub must: bool,
    pub state: CaseState,
    pub want: Want,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseSet {
    /// The pack the file is written for (`route.v10`).
    pub pack: String,
    pub cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDefaults {
    place: Option<String>,
    restricted_models: Option<String>,
    price_period: Option<String>,
    est_tokens: Option<u64>,
    last_reply_model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCase {
    id: String,
    message: String,
    place: Option<String>,
    place_kind: Option<String>,
    restricted_models: Option<String>,
    price_period: Option<String>,
    est_tokens: Option<u64>,
    last_reply_model: Option<String>,
    #[serde(default)]
    task_models: Vec<String>,
    model: String,
    effort: String,
    context: String,
    #[serde(default)]
    must: bool,
    #[serde(default)]
    why: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    pack: String,
    /// Where the labels came from; the file's own words, never read by code.
    #[serde(default, rename = "derivation")]
    _derivation: String,
    defaults: Option<FileDefaults>,
    #[serde(default, rename = "case")]
    cases: Vec<FileCase>,
}

/// A state word outside its set is refused: `price_period = "off-peak"` would
/// otherwise reach Jev as a word the pack never names.
fn one_of(case: &str, field: &str, value: &str, set: &[&str]) -> Result<(), String> {
    match set.contains(&value) {
        true => Ok(()),
        false => Err(format!("{case}: {field} is {value:?}, not one of {set:?}")),
    }
}

/// A case's own word for a state field, else the file's default, else a refusal.
fn field(
    case: &str,
    name: &str,
    own: &Option<String>,
    default: Option<&String>,
) -> Result<String, String> {
    own.clone()
        .or_else(|| default.cloned())
        .ok_or_else(|| format!("{case}: no {name}, and the file's [defaults] name none"))
}

/// A label must be an option of the pack's question of that name.
fn check_labels(c: &FileCase, pack: &Pack) -> Result<(), String> {
    for (q, label) in [
        ("model", &c.model),
        ("effort", &c.effort),
        ("context", &c.context),
    ] {
        let def = pack
            .question(q)
            .ok_or_else(|| format!("{} asks no `{q}` question", pack.name()))?;
        if !def.options.iter().any(|o| &o.id == label) {
            return Err(format!(
                "{}: {q} is {label:?}, which {} does not offer",
                c.id,
                pack.name()
            ));
        }
    }
    Ok(())
}

/// A case's state: its own words over the file's defaults, each checked.
fn state_of(c: &FileCase, d: &FileDefaults) -> Result<CaseState, String> {
    let id = c.id.as_str();
    let place = field(id, "place", &c.place, d.place.as_ref())?;
    one_of(id, "place", &place, &["dm", "channel"])?;
    let restricted_models = field(
        id,
        "restricted_models",
        &c.restricted_models,
        d.restricted_models.as_ref(),
    )?;
    one_of(
        id,
        "restricted_models",
        &restricted_models,
        &["allowed", "not_allowed"],
    )?;
    let price_period = field(id, "price_period", &c.price_period, d.price_period.as_ref())?;
    one_of(id, "price_period", &price_period, &["peak", "off_peak"])?;
    let est_tokens = c
        .est_tokens
        .or(d.est_tokens)
        .ok_or_else(|| format!("{id}: no est_tokens, and the file's [defaults] name none"))?;
    let place_kind = c
        .place_kind
        .clone()
        .unwrap_or_else(|| match place.as_str() {
            "dm" => "discord_dm".into(),
            _ => "discord_channel".into(),
        });
    Ok(CaseState {
        place,
        place_kind,
        restricted_models,
        price_period,
        est_tokens,
        last_reply_model: field(
            id,
            "last_reply_model",
            &c.last_reply_model,
            d.last_reply_model.as_ref(),
        )?,
        task_models: c.task_models.clone(),
    })
}

/// The pack a cases file says it is written for, before any pack is loaded:
/// the default candidate of a run that names none.
pub fn declared_pack(text: &str) -> Result<String, String> {
    let t: toml::Table = toml::from_str(text).map_err(|e| format!("the cases file: {e}"))?;
    t.get("pack")
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "the cases file names no `pack`".to_string())
}

impl CaseSet {
    /// The cases in `text`, each label checked against the pack's own
    /// `model`, `effort` and `context` options.
    pub fn parse(text: &str, pack: &Pack) -> Result<CaseSet, String> {
        let file: File = toml::from_str(text).map_err(|e| format!("the cases file: {e}"))?;
        if file.pack != pack.name() {
            return Err(format!(
                "the cases are written for {}, not {}",
                file.pack,
                pack.name()
            ));
        }
        let defaults = file.defaults.unwrap_or(FileDefaults {
            place: None,
            restricted_models: None,
            price_period: None,
            est_tokens: None,
            last_reply_model: None,
        });
        let mut seen = BTreeSet::new();
        let mut cases = Vec::new();
        for c in file.cases {
            if !seen.insert(c.id.clone()) {
                return Err(format!("the case {} is in the file twice", c.id));
            }
            check_labels(&c, pack)?;
            let state = state_of(&c, &defaults)?;
            cases.push(Case {
                id: c.id,
                message: c.message,
                why: c.why,
                must: c.must,
                state,
                want: Want {
                    model: c.model,
                    effort: c.effort,
                    context: c.context,
                },
            });
        }
        Ok(CaseSet {
            pack: file.pack,
            cases,
        })
    }
}

/// A case's input as the daemon builds one: the message and the facts, through
/// `builders::route_input`, so a replay's state and the daemon's are one
/// definition (`builders::ROUTE_STATE_KEYS`, held by `tests_route_state`).
pub fn input(case: &Case) -> crate::builders::InboundInput {
    use crate::builders::{PricePeriod, RestrictedModels, RouteFacts};
    let s = &case.state;
    crate::builders::route_input(
        &case.message,
        &s.place_kind,
        RouteFacts {
            price_period: match s.price_period.as_str() {
                "peak" => PricePeriod::Peak,
                _ => PricePeriod::OffPeak,
            },
            restricted_models: RestrictedModels::allowed_if(s.restricted_models == "allowed"),
            last_reply_model: Some(s.last_reply_model.clone()).filter(|m| !m.is_empty()),
            task_models: s.task_models.clone(),
            est_tokens: Some(s.est_tokens),
        },
    )
}

/// The state Jev reads for a case: `builders::inbound` over [`input`], never a
/// state built beside it.
pub fn state(case: &Case, cap: u64, scrub: &dyn Scrub) -> crate::state::BuiltState {
    let built = crate::builders::inbound(&input(case), cap, scrub).state;
    Arc::try_unwrap(built).unwrap_or_else(|a| (*a).clone())
}

/// A case's state and its dynamic items, from the daemon's own builder:
/// `persona` and `seal` are asked of nothing here, as of a message with one
/// persona and no thread.
pub fn prepared(pack: &Pack, case: &Case) -> Prepared {
    crate::builders::inbound(&input(case), pack.state_cap_tokens, &NoScrub)
}

/// A case's ask: the pack's questions over its state, in shadow, with
/// `context`'s fields beside the case's id.
pub fn ask(pack: &Arc<Pack>, case: &Case, context: &Value) -> Ask {
    let mut ctx = json!({"case": case.id});
    if let (Some(m), Some(c)) = (ctx.as_object_mut(), context.as_object()) {
        m.extend(c.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    Ask::new(pack.clone(), &prepared(pack, case), Mode::Shadow, ctx)
}

/// One question of one case, graded.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub want: String,
    /// The answer graded: Jev's top option when its band reached the
    /// question's `confirm` bar, else the question's `no_match`, as routing
    /// reads it (theseus-mir9c). Empty when Jev did not answer.
    pub got: String,
    /// The top option's confidence.
    pub confidence: f64,
    /// `got` is the label.
    pub agrees: bool,
    /// Within one rung (`effort`); as `agrees` for the others.
    pub close: bool,
    /// Jev's own top option, before the bar; empty when it did not answer.
    pub raw: String,
    /// The question's `confirm` bar, read from the pack.
    pub confirm: f64,
    /// The band did not reach `confirm`, so `got` is the question's
    /// `no_match` and `raw` is what Jev said.
    pub sub_confirm: bool,
    /// `raw` is the label: the grade before the bar was applied.
    pub raw_agrees: bool,
    /// `raw` is within one rung of the label (`effort`); as `raw_agrees`
    /// for the others.
    pub raw_close: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Graded {
    pub id: String,
    pub must: bool,
    pub answered: bool,
    pub model: Check,
    pub effort: Check,
    pub context: Check,
    /// The `model` answer is under the pack's confirm bar.
    pub under_confirm: bool,
}

fn rung(effort: &str) -> Option<usize> {
    RUNGS.iter().position(|r| *r == effort)
}

/// `got` is the label, and `got` is within one rung of it (`effort` only).
fn agrees_close(q: &str, want: &str, got: &str) -> (bool, bool) {
    let agrees = got == want;
    let close = agrees
        || match (rung(want), rung(got)) {
            (Some(a), Some(b)) if q == "effort" => a.abs_diff(b) <= 1,
            _ => false,
        };
    (agrees, close)
}

/// One deciding question graded the way routing decides it
/// (`inbound::verdict`'s `sure`, `Verdict::short`; theseus-mir9c): Jev's top
/// option stands when its band reached the question's `confirm` bar, and
/// otherwise the answer is the question's `no_match`. Both come from the
/// pack replayed, through the band function the daemon's own judgments use.
fn check(want: &str, j: Option<&Judgment>, pack: &Pack, q: &str) -> Check {
    let top = j.and_then(|j| j.answer(q)).and_then(|a| match &a.answer {
        Answer::Choice {
            choice, confidence, ..
        } => Some((choice.clone(), *confidence)),
        _ => None,
    });
    let answered = top.is_some();
    let (raw, confidence) = top.unwrap_or_default();
    let def = pack.question(q);
    let no_match = def.and_then(|d| d.no_match.clone());
    let sub_confirm = answered
        && no_match.is_some()
        && def.is_some_and(|d| confidence_band(confidence, d.thresholds) == Band::Escalate);
    let got = match (&no_match, sub_confirm) {
        (Some(no_match), true) => no_match.clone(),
        _ => raw.clone(),
    };
    let (agrees, close) = agrees_close(q, want, &got);
    let (raw_agrees, raw_close) = agrees_close(q, want, &raw);
    Check {
        want: want.to_string(),
        got,
        confidence,
        agrees,
        close,
        raw,
        confirm: def.map_or(0.0, |d| d.thresholds.confirm),
        sub_confirm,
        raw_agrees,
        raw_close,
    }
}

/// A case graded by the judgment that answered it (none, or one that did not
/// answer, grades every question a miss).
pub fn grade(pack: &Pack, case: &Case, j: Option<&Judgment>) -> Graded {
    let j = j.filter(|j| j.actionable());
    let model = check(&case.want.model, j, pack, "model");
    Graded {
        id: case.id.clone(),
        must: case.must,
        answered: j.is_some(),
        under_confirm: model.sub_confirm,
        model,
        effort: check(&case.want.effort, j, pack, "effort"),
        context: check(&case.want.context, j, pack, "context"),
    }
}

/// The numbers over a run, and the bar.
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
    /// The same counts for Jev's raw top pick, before the confirm bar and
    /// `no_match` (how `--cases` graded until theseus-mir9c).
    pub raw_model_ok: u32,
    pub raw_effort_exact: u32,
    pub raw_effort_close: u32,
    pub raw_context_ok: u32,
    /// The design's bar met, counting [`PINNED`] as met; none unless the run
    /// is the file's whole set (`DESIGN_TOTAL - PINNED` cases).
    pub bar: Option<bool>,
}

pub fn totals(graded: &[Graded]) -> Totals {
    let count = |f: &dyn Fn(&Graded) -> bool| graded.iter().filter(|g| f(g)).count() as u32;
    let mut t = Totals {
        answered: count(&|g| g.answered),
        model_ok: count(&|g| g.model.agrees),
        effort_exact: count(&|g| g.effort.agrees),
        effort_close: count(&|g| g.effort.close),
        context_ok: count(&|g| g.context.agrees),
        must_total: count(&|g| g.must),
        must_ok: count(&|g| g.must && g.model.agrees),
        must_under_confirm: count(&|g| g.must && g.under_confirm),
        raw_model_ok: count(&|g| g.model.raw_agrees),
        raw_effort_exact: count(&|g| g.effort.raw_agrees),
        raw_effort_close: count(&|g| g.effort.raw_close),
        raw_context_ok: count(&|g| g.context.raw_agrees),
        bar: None,
    };
    if graded.len() as u32 == DESIGN_TOTAL - PINNED {
        t.bar = Some(
            t.model_ok + PINNED >= BAR_MODEL
                && t.effort_close + PINNED >= BAR_EFFORT
                && t.context_ok + PINNED >= BAR_CONTEXT
                && t.must_ok == t.must_total
                && t.must_under_confirm == 0,
        );
    }
    t
}

/// How many cases name each model: the design's table of counts.
pub fn model_counts(set: &CaseSet) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for c in &set.cases {
        *out.entry(c.want.model.clone()).or_default() += 1;
    }
    out
}
