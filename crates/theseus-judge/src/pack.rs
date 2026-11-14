//! Packs are versioned data (design §2.3, §2.4; spec §3.7). A pack version
//! is one TOML file in `packs/`, named `<id>.v<version>.toml` and embedded
//! with `include_str!`. Any change (wording, criteria, thresholds, builder,
//! model) is a new version, which runs in shadow beside the incumbent.
//!
//! The loader's rules, each held by a test:
//! - every Choice has a no-match option, and every option says what it means;
//! - every Score has a companion `applies` Noul;
//! - instructions carry the whole meaning (Jev never sees an id): a question
//!   of at least four words, ending in `?`, that does not lean on its id;
//! - no question asks for math, counting, dates, or exact lookup: the
//!   builder computes those and states them as fields (`loops: 7`);
//! - thresholds are well formed (0 < confirm ≤ act ≤ 1; a Noul's confirm
//!   above 0.5, or its middle band would be empty);
//! - the model is pinned (`jev-1.13.0`), never `jev-latest`;
//! - the state cap is inside Jev's state limit;
//! - a pack with a live action names its rollback rules (§2.7), and each
//!   rule's numbers can hold.
//!
//! A question's options or its per-item Nouls may come from the builder's
//! inputs (`options_from`, `per`): live task ids, the roles table, candidate
//! topics, current memberships. Its static options (the no-match option
//! among them) always follow.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::band::Thresholds;
use crate::client::{
    ChoiceOption, Kind, Question, MAX_CHOICE_OPTIONS, MAX_SCORE_LEVELS, MIN_SCORE_LEVELS,
    STATE_LIMIT_TOKENS,
};
use crate::learn::RollbackRule;

/// Every pack version this build knows, by file name: the test pack, §2.4's six, `security.v3` beside
/// `security.v1`, `rerank.v1`, recall's `+rerank` arm (M6 step 32c), `classify.v2` to `classify.v4`
/// (the `thread`, `offer_task` and `scope` questions, theseus-rha2, theseus-bh3t.5 and theseus-98bz.9), and `route.v5` to `route.v8`
/// (the `persona` and `seal` questions, theseus-c1uz; the `data` question, theseus-a1wvq.2; its wording,
/// theseus-a1wvq and theseus-f9qa8), `route.v10` (Jev chooses the model and the effort,
/// theseus-h66v4, revised in place by theseus-h66v4.1; the acting pack: `ROUTE_PACK` names it),
/// `content.v1` (the four content flags, theseus-9ymz0), and `split.v1` (whether the work divides by area, in shadow,
/// theseus-z9mze.5), and `undo.v1` (whether a tool call is reversible, the replacement for the approve
/// lists 1.0.4 deleted; shadow, dispatched from nowhere, theseus-p5kln). Older versions
/// (`classify.v1`, `route.v1`-`route.v4`, `security.v2`) are in git, and replay by `--pack-file`.
pub const EMBEDDED: &[(&str, &str)] = &[
    ("probe.v1", include_str!("../packs/probe.v1.toml")),
    ("loop.v1", include_str!("../packs/loop.v1.toml")),
    ("security.v1", include_str!("../packs/security.v1.toml")),
    ("security.v3", include_str!("../packs/security.v3.toml")),
    ("classify.v2", include_str!("../packs/classify.v2.toml")),
    ("classify.v3", include_str!("../packs/classify.v3.toml")),
    ("classify.v4", include_str!("../packs/classify.v4.toml")),
    ("role.v1", include_str!("../packs/role.v1.toml")),
    ("directive.v1", include_str!("../packs/directive.v1.toml")),
    ("continue.v1", include_str!("../packs/continue.v1.toml")),
    ("categorize.v1", include_str!("../packs/categorize.v1.toml")),
    ("rerank.v1", include_str!("../packs/rerank.v1.toml")),
    ("memory.v1", include_str!("../packs/memory.v1.toml")),
    (
        "attribution.v1",
        include_str!("../packs/attribution.v1.toml"),
    ),
    ("route.v5", include_str!("../packs/route.v5.toml")),
    ("route.v6", include_str!("../packs/route.v6.toml")),
    ("route.v7", include_str!("../packs/route.v7.toml")),
    ("route.v8", include_str!("../packs/route.v8.toml")),
    ("route.v10", include_str!("../packs/route.v10.toml")),
    ("effort.v1", include_str!("../packs/effort.v1.toml")),
    ("work.v1", include_str!("../packs/work.v1.toml")),
    ("citation.v1", include_str!("../packs/citation.v1.toml")),
    ("compare.v1", include_str!("../packs/compare.v1.toml")),
    ("heard.v1", include_str!("../packs/heard.v1.toml")),
    ("people.v1", include_str!("../packs/people.v1.toml")),
    (
        "people_seen.v1",
        include_str!("../packs/people_seen.v1.toml"),
    ),
    ("onto.v1", include_str!("../packs/onto.v1.toml")),
    ("same_thing.v1", include_str!("../packs/same_thing.v1.toml")),
    ("two_things.v1", include_str!("../packs/two_things.v1.toml")),
    ("same_kind.v1", include_str!("../packs/same_kind.v1.toml")),
    ("name_it.v1", include_str!("../packs/name_it.v1.toml")),
    (
        "recall_cross.v1",
        include_str!("../packs/recall_cross.v1.toml"),
    ),
    ("precedence.v1", include_str!("../packs/precedence.v1.toml")),
    ("families.v1", include_str!("../packs/families.v1.toml")),
    ("content.v1", include_str!("../packs/content.v1.toml")),
    ("content.v2", include_str!("../packs/content.v2.toml")),
    ("content.v3", include_str!("../packs/content.v3.toml")),
    ("content.v4", include_str!("../packs/content.v4.toml")),
    ("split.v1", include_str!("../packs/split.v1.toml")),
    ("delay.v1", include_str!("../packs/delay.v1.toml")),
    ("undo.v1", include_str!("../packs/undo.v1.toml")),
    ("allow_said.v1", include_str!("../packs/allow_said.v1.toml")),
];

/// Where a pack runs (§2.4). `probe` is the test pack's: the core never
/// dispatches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Point {
    Inbound,
    Compile,
    Gate,
    LoopEnd,
    ExchangeEnd,
    /// After a recall's pipeline (M6 step 32c), off the turn's path.
    Recall,
    Probe,
    /// After a turn ends, off its path: the memory pass (M6 31a).
    MemoryPass,
    /// Off every turn: consolidation's citation check (M6 31b).
    Consolidation,
    /// Off every turn: a replayed turn's comparison (own-benchmark).
    Replay,
    /// A voice call's engine waits for it, live (theseus-s3dwj).
    Voice,
    /// One item that entered a session, asked off the turn's path and landed
    /// between turns (`content.v1`, theseus-9ymz0).
    Item,
    /// A delay in a turn, asked at the one reconsideration point after the
    /// free pre-check could not answer it (`delay.v1`, theseus-5x8ry).
    Delay,
}

/// The state builder a pack names (a closed set, in code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builder {
    Probe,
    Loop,
    Security,
    /// `security.v3`'s: `security.v1`'s state, and facts computed for Jev.
    Security2,
    /// `classify.v2`, `role.v1` and `directive.v1` share it, so they batch.
    Inbound,
    /// `route.v10`'s own slice of the inbound input (theseus-h66v4.6): the message and
    /// the facts the `model` question reads, with no live tasks and no onto fields.
    Route,
    /// `effort.v1`'s: the message, the previous human message and the last reply, and
    /// nothing else (theseus-h66v4.6).
    Effort,
    /// `work.v1`'s: the message, the live tasks and where it came in (theseus-h66v4.6).
    Work,
    Continue,
    Categorize,
    /// `rerank.v1`'s: the message and recall's top notes (M6 step 32c).
    Rerank,
    /// `memory.v1`'s: one node the memory pass labels (M6 31a).
    Memory,
    /// `attribution.v1`'s: a reply and the notes recall admitted (M6 31a).
    Attribution,
    /// `citation.v1`'s: a synthesis's sentences and sources (M6 31b).
    Citation,
    /// `people.v1`'s: one candidate person and the held people it may be
    /// (theseus-wy7y).
    People,
    /// `people_seen.v1`'s: a private exchange's lines and the held people it
    /// may involve (theseus-u5n8, the gate on people.v1's live point).
    PeopleSeen,
    /// `compare.v1`'s: a request and two replies (own-benchmark).
    Compare,
    /// `heard.v1`'s: a short or unclear transcript in a voice call (theseus-s3dwj).
    Heard,
    /// `same_thing.v1`'s: two categories of one kind (theseus-nzzz4).
    SameThing,
    /// `two_things.v1`'s: one category and a sample of its sessions (theseus-nzzz4).
    TwoThings,
    /// `same_kind.v1`'s: two groups of `other` answers (theseus-63tfk.1).
    SameKind,
    /// `name_it.v1`'s: a group of sessions and the names to choose among (theseus-63tfk.1).
    NameIt,
    /// `recall_cross.v1`'s: the message and the notes the thread lens dropped
    /// near the bar (theseus-98bz.10).
    RecallCross,
    /// `precedence.v1`'s (theseus-3nk): pairs of testimony that may disagree.
    Precedence,
    /// `families.v1`'s: the message and the tool families its request hides
    /// (theseus-xlri5).
    Families,
    /// `content.v1`'s: one item that entered a session, and what it was and
    /// where it came from (theseus-9ymz0).
    Content,
    /// `delay.v1`'s: one delayed piece of work, its elapsed and cost, and
    /// each progress signal with whether it moved (theseus-5x8ry).
    Delay,
    /// `undo.v1`'s: one tool call, and what code found about putting back what
    /// it would change (theseus-p5kln).
    Undo,
    /// `allow_said.v1`'s: the operator's latest message and the held call the
    /// assistant decided it means (chat-acts-are-tools, S43-1).
    AllowSaid,
}

/// What decides when the pack does not (§2.4's baseline column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Baseline {
    None,
    UntilNoToolCalls,
    Posture,
    Conversation,
    CurrentRole,
    Append,
    NoMembership,
    /// Recall's fused order, which `+rerank` re-sorts.
    Fused,
    /// The memory pass's deterministic labels and attribution (M6 31a).
    Rules,
    /// The turn runs on the session's own profile (`route.v5`).
    SessionProfile,
}

/// The live action in code (closed set), or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    None,
    NudgeTask,
    Notice,
    RoleHint,
    /// Runs the turn on the mode's profile (`route.v5`).
    Route,
    /// Routes an unaddressed message to the thread the `thread` question
    /// names (`classify.v2`, theseus-rha2).
    ThreadRoute,
    /// Keeps, asks about, or notes a standing record from the owner's words
    /// (`directive.v1`, theseus-slvl.5).
    Capture,
    /// Holds a tool call that would lose something a reshaped call would
    /// keep, and raises one whose loss no reshaping could touch
    /// (`undo.v1`, theseus-p5kln). Its reader is theseus-core's
    /// `toolrun/reversible.rs`.
    Hold,
}

/// Where a question's dynamic options or per-item Nouls come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Tasks,
    Roles,
    Topics,
    Memberships,
    /// Recall's notes: the first ten, and the next ten (a per-item Noul asks
    /// at most ten); `attribution.v1`'s are the notes a recall admitted.
    Notes,
    MoreNotes,
    /// A synthesis's (sentence, cited source) pairs (M6 31b): the first ten,
    /// and the next ten.
    Pairs,
    MorePairs,
    /// The held people a candidate may be (`people.v1`), nearest first.
    People,
    /// A candidate's role line (`people.v1`): one item, or none.
    RoleLine,
    /// The held people past the first ten (`people_seen.v1`, whose first
    /// ten are `People`): a per-item Noul asks at most ten.
    MorePeople,
    /// The live threads a message may be about (`classify.v2`, theseus-rha2):
    /// one option each, its title, goal and top working-set entities.
    Threads,
    /// The personas a message may be answered as (`route.v5`, theseus-c1uz):
    /// one option each, its specialization card.
    Personas,
    /// The thread a message arrives in: one item, or none (`route.v5`'s
    /// `seal` is asked only in a thread).
    ThreadNow,
    /// One item when the place has live threads: the router listed them, or
    /// the message arrives in a thread (`classify.v4`'s `scope`,
    /// theseus-98bz.9).
    Scope,
    /// `onto.v1`'s candidate categories of a kind, picked by code (at most
    /// twenty, the kind's `new` option last): a kind is asked when its list
    /// is there. A topic's are [`Source::Topics`].
    Projects,
    Events,
    Resources,
    Capabilities,
    Expertise,
    /// The categories a session holds now, per kind (`onto.v1`'s `still_<kind>`
    /// Nouls), at most five.
    TopicMembers,
    ProjectMembers,
    EventMembers,
    ResourceMembers,
    CapabilityMembers,
    ExpertiseMembers,
    /// The names code mined from the message and its session (`onto.v1`'s
    /// `<kind>_name` options), at most ten.
    Names,
    /// The kinds of the table, each with what it is for (`onto.v1`'s
    /// `kind_of_thing`).
    Kinds,
    /// The pictures the state carries, one item each (`onto.v1`'s image
    /// questions, theseus-und4g): asked only when there are some.
    Images,
    /// The sessions `two_things.v1` samples from a category, numbered: the
    /// first ten, and the rest (a per-item Noul asks at most ten).
    Sessions,
    MoreSessions,
    /// The tool families a request hides (`families.v1`, theseus-xlri5): the
    /// first ten, and the next ten (a per-item Noul asks at most ten).
    Families,
    MoreFamilies,
    /// One item when the item is not the operator's own words (`content.v1`'s
    /// `injection` is asked only then, theseus-9ymz0): the builder sets it
    /// from the item's kind, so the rule is the code's, not the wording's.
    Untrusted,
    /// `precedence.v1`'s pairs of testimony that may disagree (theseus-3nk),
    /// numbered: at most ten, as a per-item Noul asks.
    SourcePairs,
}

/// One dynamic item: its key (an option id, or what a per-item Noul is
/// about) and the text Jev reads for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub key: String,
    pub text: String,
}

/// What a builder found for a pack's dynamic questions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dynamic {
    pub sources: BTreeMap<Source, Vec<Item>>,
    /// The images the state refers to (`onto.v1`, theseus-und4g): sent beside
    /// it, never part of it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<crate::client::Asset>,
}

impl Dynamic {
    pub fn items(&self, s: Source) -> &[Item] {
        self.sources.get(&s).map_or(&[], Vec::as_slice)
    }

    /// The sources a pack's questions draw on: what its `only_when`,
    /// `options_from` and `per` read.
    pub fn needed(pack: &Pack) -> BTreeSet<Source> {
        let mut out = BTreeSet::new();
        for q in &pack.questions {
            out.extend([q.only_when, q.options_from, q.per].into_iter().flatten());
        }
        out
    }

    /// What a judgment's record keeps of these sources (theseus-gyjyo.1): the
    /// items of every source `pack`'s questions draw on, an empty list for one
    /// the builder found nothing for, so a source missing from the record
    /// means it was never recorded, not that it was empty.
    pub fn recorded(&self, pack: &Pack) -> serde_json::Value {
        let map: BTreeMap<Source, &[Item]> = Self::needed(pack)
            .into_iter()
            .map(|s| (s, self.items(s)))
            .collect();
        serde_json::to_value(map).unwrap_or_default()
    }

    /// The sources back from a judgment's record, for a candidate whose
    /// questions draw on them. `Err`: the ones the record does not hold (all
    /// of them when it holds none).
    pub fn from_record(record: &serde_json::Value, pack: &Pack) -> Result<Dynamic, Vec<Source>> {
        let held: BTreeMap<Source, Vec<Item>> =
            serde_json::from_value(record.clone()).unwrap_or_default();
        let missing: Vec<Source> = Self::needed(pack)
            .into_iter()
            .filter(|s| !held.contains_key(s))
            .collect();
        if !missing.is_empty() {
            return Err(missing);
        }
        Ok(Dynamic {
            sources: held,
            assets: Vec::new(),
        })
    }
}

/// A question as one state asks it: its id (a per-item Noul's carries the
/// item's position), the definition it came from, its thresholds, and the
/// default it declared for an answer that does not reach them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asked {
    pub id: String,
    pub def: String,
    /// A per-item Noul's item key.
    pub about: Option<String>,
    pub question: Question,
    pub thresholds: Thresholds,
    /// The definition's declared `no_match`: what runs when the answer's band
    /// does not reach `confirm`. Carried here so that the answer record can
    /// carry it too, and the live path never has to find the pack again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_match: Option<String>,
}

/// One question's definition, checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionDef {
    pub id: String,
    pub kind: Kind,
    pub instructions: String,
    pub thresholds: Thresholds,
    /// Whether the pack's live action reads it (its sample minimum is 200).
    pub decides: bool,
    /// A deciding Noul's own bar: it decides (asks) when p reaches this,
    /// instead of at its `confirm`. None keeps the `confirm` line, which is
    /// every pack that existed before the field. Within `confirm..=act`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decide_above: Option<f64>,
    /// A Choice's static options, the no-match option among them.
    pub options: Vec<ChoiceOption>,
    pub no_match: Option<String>,
    pub options_from: Option<Source>,
    pub levels: Vec<String>,
    pub applies: Option<String>,
    pub when_true: Option<String>,
    pub when_false: Option<String>,
    pub per: Option<Source>,
    /// Per-item Nouls at most.
    pub max: usize,
    /// Asked only when this source has items.
    pub only_when: Option<Source>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pack {
    pub id: String,
    pub version: u32,
    pub jev_model: String,
    pub point: Point,
    pub builder: Builder,
    pub state_cap_tokens: u64,
    pub sample: f64,
    pub baseline: Baseline,
    pub action: Action,
    pub description: String,
    /// By id.
    pub questions: Vec<QuestionDef>,
    /// What rolls a canary of this pack back (`learn::check_all`).
    pub rollback: Vec<RollbackRule>,
    /// The file's sha256, for the record.
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    id: String,
    version: u32,
    jev_model: String,
    point: Point,
    state: Builder,
    state_cap_tokens: u64,
    sample: f64,
    baseline: Baseline,
    action: Action,
    #[serde(default)]
    description: String,
    questions: BTreeMap<String, QuestionFile>,
    #[serde(default)]
    rollback: Vec<RollbackRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionFile {
    id: String,
    means: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionFile {
    #[serde(rename = "type")]
    kind: Kind,
    instructions: String,
    act: f64,
    confirm: f64,
    #[serde(default)]
    decides: bool,
    decide_above: Option<f64>,
    #[serde(default)]
    options: Vec<OptionFile>,
    no_match: Option<String>,
    options_from: Option<Source>,
    #[serde(default)]
    levels: Vec<String>,
    applies: Option<String>,
    when_true: Option<String>,
    when_false: Option<String>,
    per: Option<Source>,
    max: Option<usize>,
    only_when: Option<Source>,
}

/// The loader's rules, so a test can say which one refused a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// TOML, an unknown key, or a value outside its closed set.
    Syntax,
    Identity,
    PinnedModel,
    StateCap,
    Sample,
    QuestionId,
    Instructions,
    NoComputation,
    Thresholds,
    NoMatch,
    ChoiceShape,
    ScoreApplies,
    ScoreShape,
    NoulShape,
    Dynamic,
    Rollback,
    /// A `decide_above` on a question that cannot carry one, or outside its
    /// thresholds.
    DecideAbove,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[error("pack {pack}{}: {rule:?}: {detail}", question.as_ref().map(|q| format!(", question {q}")).unwrap_or_default())]
pub struct PackError {
    pub pack: String,
    pub question: Option<String>,
    pub rule: Rule,
    pub detail: String,
}

/// Phrases that ask Jev to compute, count, date, or look up. Matched on
/// whole words, case-insensitive, in every text Jev reads.
const COMPUTATION: &[&str] = &[
    "how many",
    "how much time",
    "how long ago",
    "how long has",
    "how long did",
    "count the",
    "count how",
    "counting",
    "calculate",
    "compute",
    "add up",
    "sum of",
    "average",
    "percentage",
    "what percent",
    "multiply",
    "divide",
    "what date",
    "which date",
    "what day",
    "which day",
    "what time",
    "what year",
    "look up",
    "lookup",
    "exact number",
    "exact value",
    "exact amount",
];

fn words(s: &str) -> String {
    let w: Vec<String> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    format!(" {} ", w.join(" "))
}

/// The first computation phrase in `text`, if any.
pub fn computation_in(text: &str) -> Option<&'static str> {
    let w = words(text);
    COMPUTATION
        .iter()
        .find(|p| w.contains(&format!(" {p} ")))
        .copied()
}

/// An id the packs and dynamic options may use.
pub fn safe_id(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

fn snake_id(s: &str) -> bool {
    s.len() <= 48
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn pinned_model(m: &str) -> bool {
    let Some(v) = m.strip_prefix("jev-") else {
        return false;
    };
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

impl Pack {
    /// `loop.v1`.
    pub fn name(&self) -> String {
        format!("{}.v{}", self.id, self.version)
    }

    pub fn question(&self, id: &str) -> Option<&QuestionDef> {
        self.questions.iter().find(|q| q.id == id)
    }

    /// Parses and checks one pack file.
    pub fn parse(text: &str) -> Result<Pack, PackError> {
        let file: PackFile = toml::from_str(text).map_err(|e| PackError {
            pack: "?".into(),
            question: None,
            rule: Rule::Syntax,
            detail: e.message().to_string(),
        })?;
        let mut pack = Pack {
            id: file.id,
            version: file.version,
            jev_model: file.jev_model,
            point: file.point,
            builder: file.state,
            state_cap_tokens: file.state_cap_tokens,
            sample: file.sample,
            baseline: file.baseline,
            action: file.action,
            description: file.description.trim().to_string(),
            questions: Vec::new(),
            rollback: file.rollback,
            sha256: hex::encode(Sha256::digest(text.as_bytes())),
        };
        let name = pack.name();
        let err = |q: Option<&str>, rule: Rule, detail: String| PackError {
            pack: name.clone(),
            question: q.map(str::to_string),
            rule,
            detail,
        };
        check_header(&pack, file.questions.len())
            .map_err(|(rule, detail)| err(None, rule, detail))?;
        for (id, q) in file.questions {
            let qd =
                check_question(&id, q).map_err(|(rule, detail)| err(Some(&id), rule, detail))?;
            pack.questions.push(qd);
        }
        // Every Score names its companion `applies` Noul.
        for q in pack.questions.iter().filter(|q| q.kind == Kind::Score) {
            let ok = q
                .applies
                .as_deref()
                .and_then(|a| pack.question(a))
                .is_some_and(|a| a.kind == Kind::Noul && a.per.is_none() && a.id != q.id);
            if !ok {
                return Err(err(
                    Some(&q.id),
                    Rule::ScoreApplies,
                    "a Score names its companion Noul with `applies`, and that Noul exists".into(),
                ));
            }
        }
        // Moving down needs nobody: a pack that can act says what rolls it
        // back (§2.7).
        if pack.action != Action::None && pack.rollback.is_empty() {
            return Err(err(
                None,
                Rule::Rollback,
                "a pack with a live action names its rollback rules".into(),
            ));
        }
        for r in &pack.rollback {
            r.check()
                .map_err(|detail| err(None, Rule::Rollback, detail))?;
        }
        Ok(pack)
    }

    /// The questions one state asks, with dynamic options and per-item
    /// Nouls expanded from the builder's items.
    pub fn ask(&self, dynamic: &Dynamic) -> Vec<Asked> {
        let mut out = Vec::new();
        for q in &self.questions {
            if q.only_when.is_some_and(|s| dynamic.items(s).is_empty()) {
                continue;
            }
            match (q.kind, q.per) {
                (Kind::Noul, Some(source)) => {
                    for (i, item) in dynamic.items(source).iter().take(q.max).enumerate() {
                        out.push(Asked {
                            id: format!("{}.{}", q.id, i + 1),
                            def: q.id.clone(),
                            about: Some(item.key.clone()),
                            question: Question::Noul {
                                instructions: q.instructions.replace("{item}", &item.text),
                                when_true: q.when_true.clone(),
                                when_false: q.when_false.clone(),
                            },
                            thresholds: q.thresholds,
                            no_match: q.no_match.clone(),
                        });
                    }
                }
                _ => out.push(Asked {
                    id: q.id.clone(),
                    def: q.id.clone(),
                    about: None,
                    question: Self::static_question(q, dynamic),
                    thresholds: q.thresholds,
                    no_match: q.no_match.clone(),
                }),
            }
        }
        out
    }

    fn static_question(q: &QuestionDef, dynamic: &Dynamic) -> Question {
        match q.kind {
            Kind::Choice => {
                let fixed: BTreeSet<&str> = q.options.iter().map(|o| o.id.as_str()).collect();
                let mut seen = BTreeSet::new();
                let room = MAX_CHOICE_OPTIONS.saturating_sub(q.options.len());
                let mut options: Vec<ChoiceOption> = q
                    .options_from
                    .map(|s| dynamic.items(s))
                    .unwrap_or_default()
                    .iter()
                    .filter(|i| safe_id(&i.key, 64) && !fixed.contains(i.key.as_str()))
                    .filter(|i| seen.insert(i.key.clone()))
                    .take(room)
                    .map(|i| ChoiceOption {
                        id: i.key.clone(),
                        means: Some(i.text.clone()),
                    })
                    .collect();
                options.extend(q.options.iter().cloned());
                Question::Choice {
                    instructions: q.instructions.clone(),
                    options,
                }
            }
            Kind::Score => Question::Score {
                instructions: q.instructions.clone(),
                levels: q.levels.clone(),
            },
            Kind::Noul => Question::Noul {
                instructions: q.instructions.clone(),
                when_true: q.when_true.clone(),
                when_false: q.when_false.clone(),
            },
        }
    }
}

#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn check_question(id: &str, q: QuestionFile) -> Result<QuestionDef, (Rule, String)> {
    if !snake_id(id) {
        return Err((
            Rule::QuestionId,
            format!("{id:?} is not a lowercase snake-case id"),
        ));
    }
    // Instructions carry the whole meaning: a real question, not an id.
    let ins = q.instructions.trim().to_string();
    let n_words = ins.split_whitespace().count();
    if n_words < 4 || !ins.ends_with('?') {
        return Err((
            Rule::Instructions,
            "a question of at least four words, ending in '?'".into(),
        ));
    }
    if id.contains('_') && ins.contains(id) {
        return Err((
            Rule::Instructions,
            format!("the instructions lean on the id {id:?}, which Jev never sees"),
        ));
    }
    let per_item = q.per.is_some();
    let placeholders = ins.matches("{item}").count();
    if per_item && placeholders != 1 {
        return Err((
            Rule::Dynamic,
            "a per-item Noul names its item once, as {item}".into(),
        ));
    }
    if ins.replace("{item}", "").contains(['{', '}']) {
        return Err((
            Rule::Instructions,
            "braces only as a per-item Noul's {item}".into(),
        ));
    }
    // No math, counting, dates, or exact lookup anywhere Jev reads.
    let mut texts: Vec<&str> = vec![&ins];
    texts.extend(q.options.iter().map(|o| o.means.as_str()));
    texts.extend(q.levels.iter().map(String::as_str));
    texts.extend(q.when_true.as_deref());
    texts.extend(q.when_false.as_deref());
    for t in &texts {
        if let Some(p) = computation_in(t) {
            return Err((
                Rule::NoComputation,
                format!("{p:?} asks Jev to compute; state it as a field instead: {t:?}"),
            ));
        }
    }
    let t = Thresholds {
        act: q.act,
        confirm: q.confirm,
    };
    if !(t.confirm > 0.0 && t.confirm <= t.act && t.act <= 1.0) {
        return Err((
            Rule::Thresholds,
            format!("0 < confirm ({}) <= act ({}) <= 1", t.confirm, t.act),
        ));
    }
    if q.kind == Kind::Noul && t.confirm <= 0.5 {
        return Err((
            Rule::Thresholds,
            "a Noul's confirm is above 0.5, or its middle band is empty".into(),
        ));
    }
    if let Some(bar) = q.decide_above {
        if !q.decides || q.kind != Kind::Noul || q.per.is_some() {
            return Err((
                Rule::DecideAbove,
                "`decide_above` belongs to a deciding, whole (not per-item) Noul".into(),
            ));
        }
        if !(t.confirm <= bar && bar <= t.act) {
            return Err((
                Rule::DecideAbove,
                format!(
                    "decide_above ({bar}) lies within confirm ({}) to act ({})",
                    t.confirm, t.act
                ),
            ));
        }
    }
    let choice_only = !q.options.is_empty() || q.no_match.is_some() || q.options_from.is_some();
    let score_only = !q.levels.is_empty() || q.applies.is_some();
    let noul_only =
        q.when_true.is_some() || q.when_false.is_some() || q.per.is_some() || q.max.is_some();
    match q.kind {
        Kind::Choice => {
            if score_only || noul_only {
                return Err((
                    Rule::ChoiceShape,
                    "a Choice has options, not levels or Noul criteria".into(),
                ));
            }
            let mut ids = BTreeSet::new();
            for o in &q.options {
                if !safe_id(&o.id, 64) || !ids.insert(o.id.as_str()) {
                    return Err((
                        Rule::ChoiceShape,
                        format!("option {:?} is not a unique safe id", o.id),
                    ));
                }
                if o.means.trim().is_empty() {
                    return Err((
                        Rule::ChoiceShape,
                        format!("option {:?} does not say what it means", o.id),
                    ));
                }
            }
            let min = if q.options_from.is_some() { 1 } else { 2 };
            if q.options.len() < min || q.options.len() > MAX_CHOICE_OPTIONS {
                return Err((
                    Rule::ChoiceShape,
                    format!(
                        "{} options; a Choice has {min} to {MAX_CHOICE_OPTIONS}",
                        q.options.len()
                    ),
                ));
            }
            match q.no_match.as_deref() {
                Some(nm) if ids.contains(nm) => {}
                Some(nm) => {
                    return Err((
                        Rule::NoMatch,
                        format!("the no-match option {nm:?} is not among the options"),
                    ))
                }
                None => {
                    return Err((
                        Rule::NoMatch,
                        "every Choice names its no-match option".into(),
                    ))
                }
            }
        }
        Kind::Score => {
            if choice_only || noul_only {
                return Err((
                    Rule::ScoreShape,
                    "a Score has levels and `applies`, nothing else".into(),
                ));
            }
            let unique: BTreeSet<&String> = q.levels.iter().collect();
            if q.levels.len() < MIN_SCORE_LEVELS
                || q.levels.len() > MAX_SCORE_LEVELS
                || unique.len() != q.levels.len()
                || q.levels.iter().any(|l| l.trim().is_empty())
            {
                return Err((
                    Rule::ScoreShape,
                    format!(
                        "{MIN_SCORE_LEVELS} to {MAX_SCORE_LEVELS} distinct levels, lowest first"
                    ),
                ));
            }
        }
        Kind::Noul => {
            if choice_only || score_only {
                return Err((Rule::NoulShape, "a Noul has no options or levels".into()));
            }
            if q.when_true.is_some() != q.when_false.is_some() {
                return Err((
                    Rule::NoulShape,
                    "a Noul's criteria give both sides or neither".into(),
                ));
            }
            if q.max.is_some() && q.per.is_none() {
                return Err((Rule::Dynamic, "`max` belongs to a per-item Noul".into()));
            }
            if q.max.is_some_and(|m| !(1..=10).contains(&m)) {
                return Err((Rule::Dynamic, "a per-item Noul asks 1 to 10 items".into()));
            }
        }
    }
    Ok(QuestionDef {
        id: id.to_string(),
        kind: q.kind,
        instructions: ins,
        thresholds: t,
        decides: q.decides,
        decide_above: q.decide_above,
        options: q
            .options
            .into_iter()
            .map(|o| ChoiceOption {
                id: o.id,
                means: Some(o.means.trim().to_string()),
            })
            .collect(),
        no_match: q.no_match,
        options_from: q.options_from,
        levels: q.levels,
        applies: q.applies,
        when_true: q.when_true,
        when_false: q.when_false,
        per: q.per,
        max: q.max.unwrap_or(5),
        only_when: q.only_when,
    })
}

/// A pack's own fields, before any question is read: its identity, its pinned
/// model, its state cap, its sample, and that it asks anything at all. The
/// order is the order the rules are reported in.
fn check_header(pack: &Pack, questions: usize) -> Result<(), (Rule, String)> {
    if !snake_id(&pack.id) || pack.version == 0 {
        return Err((
            Rule::Identity,
            "the id is lowercase snake case and the version at least 1".into(),
        ));
    }
    if !pinned_model(&pack.jev_model) {
        return Err((
            Rule::PinnedModel,
            format!(
                "{:?} is not a pinned model such as jev-1.13.0 (jev-latest never)",
                pack.jev_model
            ),
        ));
    }
    if pack.state_cap_tokens == 0 || pack.state_cap_tokens > STATE_LIMIT_TOKENS {
        return Err((
            Rule::StateCap,
            format!(
                "{} is not within 1..={STATE_LIMIT_TOKENS}",
                pack.state_cap_tokens
            ),
        ));
    }
    if !(0.0..=1.0).contains(&pack.sample) {
        return Err((Rule::Sample, format!("{} is not within 0..=1", pack.sample)));
    }
    if questions == 0 {
        return Err((Rule::QuestionId, "a pack asks at least one question".into()));
    }
    Ok(())
}

/// Every embedded pack, parsed and checked once, each file's name matching
/// its pack's.
pub fn embedded() -> &'static Result<Vec<Arc<Pack>>, PackError> {
    static PACKS: OnceLock<Result<Vec<Arc<Pack>>, PackError>> = OnceLock::new();
    PACKS.get_or_init(|| {
        EMBEDDED
            .iter()
            .map(|(file, text)| {
                let p = Pack::parse(text)?;
                if p.name() != *file {
                    return Err(PackError {
                        pack: p.name(),
                        question: None,
                        rule: Rule::Identity,
                        detail: format!("its file is named {file}"),
                    });
                }
                Ok(Arc::new(p))
            })
            .collect()
    })
}

/// An embedded pack by name (`loop.v1`).
pub fn by_name(name: &str) -> Option<Arc<Pack>> {
    embedded()
        .as_ref()
        .ok()?
        .iter()
        .find(|p| p.name() == name)
        .cloned()
}
