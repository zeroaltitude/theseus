//! `route.v10`'s labelled cases (theseus-h66v4, design §4): a case is a
//! message, the state the daemon puts beside it, and the model and effort the
//! owner (or the design) says Jev should choose. This is the pure
//! half of `theseus judge replay --cases`: read the file, build each case's
//! ask, and grade the judgment that comes back. The core makes the calls
//! ([`crate::replay`]'s way: the core reads the record, these say what it
//! means).
//!
//! - **The file** is TOML: `pack`, an optional `[defaults]` table, and a
//!   `[[case]]` for each case. A case names only the state fields that differ
//!   from the defaults. [`CaseSet::parse`] refuses an unknown key, a
//!   duplicate id, a state word outside its set, and a label that is not an
//!   option of the pack's `model` or `effort` question.
//! - **The state** is [`state`]'s: the message, `author`, `place_kind`, and
//!   the daemon's route facts (`price_period`, `restricted_models`,
//!   `est_tokens`, `last_reply_model`, `current_model`, `switch_cost`, and
//!   `task_models` when there are any). It is built here, beside `builders::inbound`, so that a replay
//!   needs no live turn; the fields' names are the ones the daemon's own
//!   state takes when the state-and-catalog row lands, and the replay's
//!   builder follows it there.
//! - **The grade** compares what routing would decide for each of the two
//!   deciding questions to the label: Jev's top option when its band reached
//!   the question's `confirm` bar, else the question's `no_match`, both read
//!   from the pack replayed (theseus-mir9c; before it the raw top option was
//!   graded, which over-credited `model`). The
//!   raw option and its grade stay beside it. `effort` is graded to the exact
//!   rung and to within one rung (`low`, `medium`, `high`, `xhigh`, `max` in
//!   order). The
//!   daemon applies an `effort` under the bar anyway when its confidence is
//!   at least `[routing] effort_confidence` (`routing::effort::decide`); this
//!   grade does not model that second bar. A `must` case whose `model` answer
//!   sits under the confirm bar is counted apart too: it runs on the no-match
//!   option, whatever its top option.
//! - **The four sets** (theseus-h66v4.6): `route.v10.cases.toml` labels `model` and `effort` on the
//!   same cases (it is graded by `route.v10` for the model and, through its `also` key, by
//!   `effort.v1` for the effort); `work.v1.cases.toml` and `split.v1.cases.toml` are each pack's own.
//!   One run replays one pack over one file, as before; a pack is graded on the deciding questions
//!   it asks and the file labels.
//! - **The bars** are the owner's of 2026-10-30 16:01 ("the bars should all be set to about 80% of
//!   the total cases"): each is a RATIO of its own set with an explicit rounding rule, never an
//!   integer written down: [`bar_need`] is `ceil(cases * 4 / 5)`. A fixed integer is how
//!   `bench/smoke/smoke.py` came to gate on a retired 54 (theseus-h66v4.4). A pack's bars are the
//!   questions it asks, plus `must` for the pack that asks `model`. `effort` is judged within one
//!   rung, as the design's bar was; the exact count is printed beside it.
//! - **The must mapping.** The file carries 13 `must` cases; the design's fourteenth, W40 (the
//!   operator pin), is a daemon test (`crates/theseusd/tests/route_owner.rs`) and is not in the
//!   replay. The replay grades the 13 and its bar is `bar_need(13)` = 11 of 13; W40 is asserted by
//!   the daemon test, outside this denominator. W39 (the other pin) is held the same way, and the
//!   `model` and `effort` bars are over the replay's own cases, with no credit for the two pins.
//! - **What the old must rule had that this one does not** (theseus-h66v4.6 fix pass, F8). Before,
//!   a pack passed `must` only when every must was ok AND no must sat under the confirm bar
//!   (`must_under_confirm == 0`). The owner replaced "every must" with a ratio, and the second
//!   clause went with it: a must graded ok by the `no_match` (an answer under the bar, run as the
//!   no-match option, which is the must's label) now counts, as the same answer counts for every
//!   other case, because the grade models what the daemon would run. The count of musts under the
//!   bar is still counted (`must_under_confirm`) and printed beside the bar, so a "met" that leans
//!   on under-bar musts shows it; putting the clause back is one subtraction in `Bar::new("must", ..)`,
//!   and the owner's to ask for.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::band::{confidence_band, Band};
use crate::builders::{Input, Prepared};
use crate::client::Answer;
use crate::judge::{Ask, Judgment, Mode};
use crate::pack::Pack;
use crate::state::{NoScrub, Scrub};

/// The test set, as the pack's directory holds it.
pub const V10: &str = include_str!("../packs/route.v10.cases.toml");

/// The work set and the split set, as the pack's directory holds them.
pub const WORK: &str = include_str!("../packs/work.v1.cases.toml");
pub const SPLIT: &str = include_str!("../packs/split.v1.cases.toml");

/// The design's cases that are daemon tests, not replay cases (W39, W40): the operator's pin is
/// never asked of Jev. They are outside every denominator here.
pub const PINNED: u32 = 2;

/// Every bar is this fraction of its own set, rounded up: 4 over 5, the owner's "about 80%".
pub const BAR_NUM: u32 = 4;
pub const BAR_DEN: u32 = 5;

/// The least a bar over `of` cases needs: `of * 4 / 5` rounded UP, in integers (`div_ceil`), so
/// 13 musts need 11 and 61 cases need 49. Written as a rule, not a number.
pub fn bar_need(of: u32) -> u32 {
    (of * BAR_NUM).div_ceil(BAR_DEN)
}

/// The deciding questions a cases file can label, in the order they are graded.
pub const QUESTIONS: [&str; 4] = ["model", "effort", "work", "split"];

/// The efforts on a rung, lowest first.
const RUNGS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// What the owner or the design says Jev should choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    pub model: String,
    pub effort: String,
    pub work: String,
    pub split: String,
}

impl Want {
    /// The label for a deciding question; empty when the file does not label it.
    pub fn of(&self, q: &str) -> &str {
        match q {
            "model" => &self.model,
            "effort" => &self.effort,
            "work" => &self.work,
            "split" => &self.split,
            _ => "",
        }
    }
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
    /// The session's warm model: the case's own, else the last reply's; empty
    /// for none.
    pub current_model: String,
    /// What the daemon says a switch would cost this message
    /// (`builders::SwitchCost`'s words).
    pub switch_cost: String,
    /// Every model that has written in the task.
    pub task_models: Vec<String>,
    /// The live tasks beside the message (`work.v1`'s state); none for most cases.
    pub live_tasks: Vec<crate::builders::TaskInput>,
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
    switch_cost: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTask {
    id: String,
    brief: String,
    state: String,
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
    current_model: Option<String>,
    switch_cost: Option<String>,
    #[serde(default)]
    task_models: Vec<String>,
    #[serde(default)]
    live_tasks: Vec<FileTask>,
    model: Option<String>,
    effort: Option<String>,
    work: Option<String>,
    split: Option<String>,
    #[serde(default)]
    must: bool,
    #[serde(default)]
    why: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    pack: String,
    /// Other packs the file's labels also grade (`effort.v1` over the model file's `effort`).
    #[serde(default)]
    also: Vec<String>,
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

/// A case's label for `q`, from its own column.
fn label_of<'a>(c: &'a FileCase, q: &str) -> Option<&'a String> {
    match q {
        "model" => c.model.as_ref(),
        "effort" => c.effort.as_ref(),
        "work" => c.work.as_ref(),
        "split" => c.split.as_ref(),
        _ => None,
    }
}

/// The deciding questions of `pack` this module grades, in [`QUESTIONS`]' order.
pub fn graded_questions(pack: &Pack) -> Vec<&'static str> {
    QUESTIONS
        .iter()
        .copied()
        .filter(|q| pack.question(q).is_some_and(|d| d.decides))
        .collect()
}

/// A label must be an option of the pack's question of that name, for every question the pack
/// grades, and a case labels nothing the pack does not ask.
fn check_labels(c: &FileCase, pack: &Pack) -> Result<(), String> {
    let asked = graded_questions(pack);
    if asked.is_empty() {
        return Err(format!("{} decides none of {QUESTIONS:?}", pack.name()));
    }
    for q in asked {
        let label = label_of(c, q)
            .ok_or_else(|| format!("{}: no `{q}` label, which {} grades", c.id, pack.name()))?;
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
    let switch_cost = field(id, "switch_cost", &c.switch_cost, d.switch_cost.as_ref())?;
    one_of(
        id,
        "switch_cost",
        &switch_cost,
        &["cheaper_to_stay", "cheaper_to_switch", "about_the_same"],
    )?;
    let last_reply_model = field(
        id,
        "last_reply_model",
        &c.last_reply_model,
        d.last_reply_model.as_ref(),
    )?;
    Ok(CaseState {
        place,
        place_kind,
        restricted_models,
        price_period,
        est_tokens,
        current_model: c
            .current_model
            .clone()
            .unwrap_or_else(|| last_reply_model.clone()),
        last_reply_model,
        switch_cost,
        task_models: c.task_models.clone(),
        live_tasks: c
            .live_tasks
            .iter()
            .map(|t| crate::builders::TaskInput {
                id: t.id.clone(),
                brief: t.brief.clone(),
                state: t.state.clone(),
            })
            .collect(),
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
    /// `model` and `effort` options.
    pub fn parse(text: &str, pack: &Pack) -> Result<CaseSet, String> {
        let file: File = toml::from_str(text).map_err(|e| format!("the cases file: {e}"))?;
        if file.pack != pack.name() && !file.also.contains(&pack.name()) {
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
            switch_cost: None,
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
                    model: c.model.unwrap_or_default(),
                    effort: c.effort.unwrap_or_default(),
                    work: c.work.unwrap_or_default(),
                    split: c.split.unwrap_or_default(),
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
    use crate::builders::{PricePeriod, RestrictedModels, RouteFacts, SwitchCost};
    let s = &case.state;
    let mut input = crate::builders::route_input(
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
            current_model: Some(s.current_model.clone()).filter(|m| !m.is_empty()),
            switch_cost: SwitchCost::parse(&s.switch_cost),
        },
    );
    input.live_tasks = s.live_tasks.clone();
    input
}

/// The state Jev reads for a case under `route.v10` (the model slice): the daemon's own builder
/// over [`input`], never a state built beside it. The other packs' slices are [`prepared`]'s.
pub fn state(case: &Case, cap: u64, scrub: &dyn Scrub) -> crate::state::BuiltState {
    let built = crate::builders::route_state(&input(case), cap, scrub).state;
    Arc::try_unwrap(built).unwrap_or_else(|a| (*a).clone())
}

/// A case's state and its dynamic items, from the daemon's own builder for `pack`: its slice of
/// [`input`]. `persona` and `seal` are asked of nothing here, as of a message with one persona and
/// no thread.
pub fn prepared(pack: &Pack, case: &Case) -> Prepared {
    crate::builders::prepare(pack, &Input::Inbound(input(case)), &NoScrub)
        .expect("a route family pack's builder reads the inbound input")
}

/// A case's ask: the pack's questions over its state, in shadow, with
/// `context`'s fields beside the case's id (the judgment's own context).
pub fn ask(pack: &Arc<Pack>, case: &Case, context: &Value) -> Ask {
    let mut ctx = json!({"case": case.id});
    if let (Some(m), Some(c)) = (ctx.as_object_mut(), context.as_object()) {
        m.extend(c.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    Ask::new(pack.clone(), &prepared(pack, case), Mode::Shadow, ctx)
}

/// One question of one case, graded.
#[derive(Debug, Clone, Default, PartialEq)]
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
    /// Within one rung (`effort`); as `agrees` for `model`.
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
    /// One per deciding question; the default (nothing wanted, nothing graded) for a question
    /// the replayed pack does not ask ([`graded_questions`]).
    pub model: Check,
    pub effort: Check,
    pub work: Check,
    pub split: Check,
    /// The `model` answer is under the pack's confirm bar.
    pub under_confirm: bool,
}

impl Graded {
    /// The check of one deciding question.
    pub fn check(&self, q: &str) -> &Check {
        match q {
            "model" => &self.model,
            "effort" => &self.effort,
            "work" => &self.work,
            _ => &self.split,
        }
    }
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
/// answer, grades every question a miss). Only the questions `pack` decides are graded.
pub fn grade(pack: &Pack, case: &Case, j: Option<&Judgment>) -> Graded {
    let j = j.filter(|j| j.actionable());
    let asked = graded_questions(pack);
    let one = |q: &str| match asked.contains(&q) {
        true => check(case.want.of(q), j, pack, q),
        false => Check::default(),
    };
    let model = one("model");
    Graded {
        id: case.id.clone(),
        must: case.must,
        answered: j.is_some(),
        under_confirm: model.sub_confirm,
        model,
        effort: one("effort"),
        work: one("work"),
        split: one("split"),
    }
}

/// One bar: `ok` of `of` cases, against what the rule needs ([`bar_need`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    /// `model`, `effort`, `work`, `split` or `must`.
    pub question: String,
    pub ok: u32,
    pub of: u32,
    /// `bar_need(of)`, computed, not written down.
    pub need: u32,
    pub met: bool,
}

impl Bar {
    fn new(question: &str, ok: u32, of: u32) -> Self {
        let need = bar_need(of);
        Self {
            question: question.into(),
            ok,
            of,
            need,
            // A bar over no cases is not met: an empty set proves nothing.
            met: of > 0 && ok >= need,
        }
    }
}

/// The numbers over a run, and the bars.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Totals {
    /// The deciding questions the run graded.
    pub asked: Vec<String>,
    pub answered: u32,
    pub model_ok: u32,
    pub effort_exact: u32,
    pub effort_close: u32,
    pub work_ok: u32,
    pub split_ok: u32,
    pub must_total: u32,
    pub must_ok: u32,
    pub must_under_confirm: u32,
    /// The same counts for Jev's raw top pick, before the confirm bar and
    /// `no_match` (how `--cases` graded until theseus-mir9c).
    pub raw_model_ok: u32,
    pub raw_effort_exact: u32,
    pub raw_effort_close: u32,
    pub raw_work_ok: u32,
    pub raw_split_ok: u32,
    /// Answers under the question's confirm bar, per question: those graded as its `no_match`.
    pub under_model: u32,
    pub under_effort: u32,
    pub under_work: u32,
    pub under_split: u32,
    /// One bar per asked question, and `must` beside `model`; empty for no cases.
    pub bars: Vec<Bar>,
    /// Every bar met; none when there is no bar to judge.
    pub bar: Option<bool>,
}

pub fn totals(graded: &[Graded], asked: &[&str]) -> Totals {
    let count = |f: &dyn Fn(&Graded) -> bool| graded.iter().filter(|g| f(g)).count() as u32;
    let n = graded.len() as u32;
    let mut t = Totals {
        asked: asked.iter().map(|q| (*q).to_string()).collect(),
        answered: count(&|g| g.answered),
        model_ok: count(&|g| g.model.agrees),
        effort_exact: count(&|g| g.effort.agrees),
        effort_close: count(&|g| g.effort.close),
        work_ok: count(&|g| g.work.agrees),
        split_ok: count(&|g| g.split.agrees),
        must_total: count(&|g| g.must),
        must_ok: count(&|g| g.must && g.model.agrees),
        must_under_confirm: count(&|g| g.must && g.under_confirm),
        raw_model_ok: count(&|g| g.model.raw_agrees),
        raw_effort_exact: count(&|g| g.effort.raw_agrees),
        raw_effort_close: count(&|g| g.effort.raw_close),
        raw_work_ok: count(&|g| g.work.raw_agrees),
        raw_split_ok: count(&|g| g.split.raw_agrees),
        under_model: count(&|g| g.model.sub_confirm),
        under_effort: count(&|g| g.effort.sub_confirm),
        under_work: count(&|g| g.work.sub_confirm),
        under_split: count(&|g| g.split.sub_confirm),
        bars: Vec::new(),
        bar: None,
    };
    for q in asked {
        let ok = match *q {
            "model" => t.model_ok,
            // Within one rung, as the design's bar was; the exact count is `effort_exact`.
            "effort" => t.effort_close,
            "work" => t.work_ok,
            _ => t.split_ok,
        };
        t.bars.push(Bar::new(q, ok, n));
        if *q == "model" {
            // The 13 replayed musts, not the design's 14: W40 is the daemon's test (see the module's doc).
            t.bars.push(Bar::new("must", t.must_ok, t.must_total));
        }
    }
    if !t.bars.is_empty() {
        t.bar = Some(t.bars.iter().all(|b| b.met));
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
