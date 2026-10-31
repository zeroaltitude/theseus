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
/// theseus-a1wvq and theseus-f9qa8), `route.v10` (Jev chooses the model, the effort and the context,
/// theseus-h66v4; embedded and replayable, and not the acting pack: `ROUTE_PACK` names `route.v8`),
/// `content.v1` (the four content flags, theseus-9ymz0), and `split.v1` (whether the work divides by area, in shadow,
/// theseus-z9mze.5). Older versions
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
    ("families.v1", include_str!("../packs/families.v1.toml")),
    ("content.v1", include_str!("../packs/content.v1.toml")),
    ("content.v2", include_str!("../packs/content.v2.toml")),
    ("content.v3", include_str!("../packs/content.v3.toml")),
    ("split.v1", include_str!("../packs/split.v1.toml")),
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
    /// `families.v1`'s: the message and the tool families its request hides
    /// (theseus-xlri5).
    Families,
    /// `content.v1`'s: one item that entered a session, and what it was and
    /// where it came from (theseus-9ymz0).
    Content,
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
/// item's position), the definition it came from, and its thresholds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asked {
    pub id: String,
    pub def: String,
    /// A per-item Noul's item key.
    pub about: Option<String>,
    pub question: Question,
    pub thresholds: Thresholds,
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
        if !snake_id(&pack.id) || pack.version == 0 {
            return Err(err(
                None,
                Rule::Identity,
                "the id is lowercase snake case and the version at least 1".into(),
            ));
        }
        if !pinned_model(&pack.jev_model) {
            return Err(err(
                None,
                Rule::PinnedModel,
                format!(
                    "{:?} is not a pinned model such as jev-1.13.0 (jev-latest never)",
                    pack.jev_model
                ),
            ));
        }
        if pack.state_cap_tokens == 0 || pack.state_cap_tokens > STATE_LIMIT_TOKENS {
            return Err(err(
                None,
                Rule::StateCap,
                format!(
                    "{} is not within 1..={STATE_LIMIT_TOKENS}",
                    pack.state_cap_tokens
                ),
            ));
        }
        if !(0.0..=1.0).contains(&pack.sample) {
            return Err(err(
                None,
                Rule::Sample,
                format!("{} is not within 0..=1", pack.sample),
            ));
        }
        if file.questions.is_empty() {
            return Err(err(
                None,
                Rule::QuestionId,
                "a pack asks at least one question".into(),
            ));
        }
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
                        });
                    }
                }
                _ => out.push(Asked {
                    id: q.id.clone(),
                    def: q.id.clone(),
                    about: None,
                    question: Self::static_question(q, dynamic),
                    thresholds: q.thresholds,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::hits;

    const GOOD: &str = include_str!("../packs/probe.v1.toml");

    fn refused(text: &str) -> Rule {
        match Pack::parse(text) {
            Ok(p) => panic!("{} loaded", p.name()),
            Err(e) => e.rule,
        }
    }

    /// The good pack with one line replaced.
    fn with(from: &str, to: &str) -> String {
        assert!(GOOD.contains(from), "the test pack lacks {from:?}");
        GOOD.replacen(from, to, 1)
    }

    #[test]
    fn every_embedded_pack_loads_and_matches_its_file_name() {
        let packs = embedded().as_ref().expect("embedded packs load");
        assert_eq!(packs.len(), EMBEDDED.len());
        let p = by_name("probe.v1").unwrap();
        assert_eq!(p.jev_model, "jev-1.13.0");
        assert_eq!(p.builder, Builder::Probe);
        assert!(by_name("probe.v9").is_none());
    }

    #[test]
    fn a_choice_without_its_no_match_option_is_refused() {
        assert_eq!(refused(&with("no_match = \"other\"\n", "")), Rule::NoMatch);
        assert_eq!(
            refused(&with("no_match = \"other\"", "no_match = \"elsewhere\"")),
            Rule::NoMatch
        );
    }

    #[test]
    fn a_score_without_its_applies_noul_is_refused() {
        assert_eq!(
            refused(&with("applies = \"severity_applies\"\n", "")),
            Rule::ScoreApplies
        );
        assert_eq!(
            refused(&with(
                "applies = \"severity_applies\"",
                "applies = \"cause\""
            )),
            Rule::ScoreApplies
        );
        assert_eq!(
            refused(&with(
                "applies = \"severity_applies\"",
                "applies = \"nothing\""
            )),
            Rule::ScoreApplies
        );
    }

    #[test]
    fn questions_that_ask_jev_to_compute_are_refused() {
        for bad in [
            "How many tool calls did the assistant make?",
            "On what date was the build last green?",
            "What percentage of the tests failed in this build?",
            "Can you look up the owner of this service?",
            "Calculate whether the cost passed the limit?",
        ] {
            let text = with(
                "instructions = \"Should a person look at this failure today?\"",
                &format!("instructions = \"{bad}\""),
            );
            assert_eq!(refused(&text), Rule::NoComputation, "{bad}");
        }
        assert_eq!(computation_in("How much of the ask is done?"), None);
        assert_eq!(computation_in("Count the files."), Some("count the"));
    }

    #[test]
    fn instructions_must_carry_the_meaning() {
        let q = "instructions = \"Should a person look at this failure today?\"";
        assert_eq!(
            refused(&with(q, "instructions = \"Needs human?\"")),
            Rule::Instructions
        );
        assert_eq!(
            refused(&with(
                q,
                "instructions = \"A person should look at this today.\""
            )),
            Rule::Instructions
        );
        assert_eq!(
            refused(&with(
                q,
                "instructions = \"Is needs_human true for this failure?\""
            )),
            Rule::Instructions
        );
    }

    #[test]
    fn the_model_is_pinned_and_never_latest() {
        assert_eq!(
            refused(&with(
                "jev_model = \"jev-1.13.0\"",
                "jev_model = \"jev-latest\""
            )),
            Rule::PinnedModel
        );
        assert_eq!(
            refused(&with(
                "jev_model = \"jev-1.13.0\"",
                "jev_model = \"jev-1.13\""
            )),
            Rule::PinnedModel
        );
    }

    #[test]
    fn thresholds_caps_and_shapes_are_checked() {
        assert_eq!(refused(&with("act = 0.90", "act = 0.50")), Rule::Thresholds);
        assert_eq!(
            refused(&with("state_cap_tokens = 1000", "state_cap_tokens = 40000")),
            Rule::StateCap
        );
        assert_eq!(refused(&with("sample = 1.0", "sample = 1.5")), Rule::Sample);
        assert_eq!(
            refused(&with("id = \"probe\"", "id = \"Probe\"")),
            Rule::Identity
        );
        assert_eq!(
            refused(&with("action = \"none\"", "action = \"launch\"")),
            Rule::Syntax
        );
        assert_eq!(
            refused(&with("sample = 1.0", "sample = 1.0\nsurprise = 1")),
            Rule::Syntax
        );
        assert_eq!(
            refused(&with(
                "when_false = \"It can wait",
                "#when_false = \"It can wait"
            )),
            Rule::NoulShape
        );
    }

    #[test]
    fn dynamic_options_come_first_and_the_static_ones_follow() {
        let text = with(
            "no_match = \"other\"",
            "no_match = \"other\"\noptions_from = \"tasks\"",
        );
        let p = Pack::parse(&text).unwrap();
        let mut d = Dynamic::default();
        d.sources.insert(
            Source::Tasks,
            vec![
                Item {
                    key: "a1b2c3".into(),
                    text: "Task a1b2c3: run the tests".into(),
                },
                Item {
                    key: "other".into(),
                    text: "collides with a static option".into(),
                },
                Item {
                    key: "bad key!".into(),
                    text: "not a safe id".into(),
                },
                Item {
                    key: "a1b2c3".into(),
                    text: "a duplicate".into(),
                },
            ],
        );
        let asked = p.ask(&d);
        let cause = asked.iter().find(|a| a.id == "cause").unwrap();
        let Question::Choice { options, .. } = &cause.question else {
            panic!()
        };
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "a1b2c3",
                "dependency_change",
                "flaky_infrastructure",
                "code_bug",
                "other"
            ]
        );
    }

    #[test]
    fn a_pack_that_acts_names_rollback_rules_that_can_hold() {
        assert_eq!(
            refused(&with("action = \"none\"", "action = \"notice\"")),
            Rule::Rollback
        );
        let rule = |r: &str| format!("{GOOD}\n[[rollback]]\n{r}\n");
        let acts = |r: &str| rule(r).replacen("action = \"none\"", "action = \"notice\"", 1);
        assert!(Pack::parse(&acts("rule = \"notices_per_day\"\nmax = 30")).is_ok());
        for bad in [
            "rule = \"spend_ratio\"\nratio = 1.0\nmin_tasks = 10",
            "rule = \"spend_ratio\"\nratio = 2.0\nmin_tasks = 0",
            "rule = \"on_path_p95\"\nmax_ms = 0\nmin_samples = 20",
            "rule = \"on_path_p95\"\nmax_ms = 1000\nmin_samples = 0",
            "rule = \"labels_per_day\"\nlabel = \" \"\ncount = 3",
            "rule = \"labels_per_day\"\nlabel = \"noise\"\ncount = 0",
        ] {
            assert_eq!(refused(&rule(bad)), Rule::Rollback, "{bad}");
        }
        assert_eq!(refused(&rule("rule = \"nudge_loops\"")), Rule::Syntax);
        assert_eq!(
            refused(&rule("rule = \"notices_per_day\"\nmax = 30\nwhen = 1")),
            Rule::Syntax
        );
    }

    /// A pack's shape, one line per item: where it runs, each question's
    /// kind, whether it decides, and its options, and the rollback rules.
    fn shape(p: &Pack) -> Vec<String> {
        let mut out = vec![format!(
            "{:?} {:?} {:?} {:?}",
            p.point, p.builder, p.baseline, p.action
        )];
        for q in &p.questions {
            let mut line = format!("{} {:?}", q.id, q.kind);
            if q.decides {
                line.push_str(" decides");
                if let Some(bar) = q.decide_above {
                    line.push_str(&format!(" above {bar}"));
                }
            }
            if let Some(s) = q.options_from {
                line.push_str(&format!(" from {s:?} +"));
            }
            if !q.options.is_empty() {
                let ids: Vec<&str> = q.options.iter().map(|o| o.id.as_str()).collect();
                line.push_str(&format!(" [{}]", ids.join(" ")));
            }
            if let Some(s) = q.per {
                line.push_str(&format!(" per {s:?} max {}", q.max));
            }
            if let Some(s) = q.only_when {
                line.push_str(&format!(" only_when {s:?}"));
            }
            out.push(line);
        }
        let rules: Vec<&str> = p.rollback.iter().map(RollbackRule::name).collect();
        out.push(format!("rollback [{}]", rules.join(" ")));
        out
    }

    /// Design §2.4's table and §2.7's rollback table, pack by pack.
    #[test]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn the_six_packs_ask_what_the_design_says() {
        let want: &[(&str, &[&str])] = &[
            (
                "loop.v1",
                &[
                    "LoopEnd Loop UntilNoToolCalls NudgeTask",
                    "announced_unfinished Noul decides",
                    "cost_out_of_proportion Noul",
                    "same_action_repeating Noul",
                    "stopping_point_defined Noul",
                    "wants_human_input Noul",
                    "work_state Choice decides [complete progressing blocked_needs_human thrashing off_task other]",
                    "rollback [nudge_loop spend_ratio operator_stop_within_nudge on_path_p95]",
                ],
            ),
            (
                "security.v1",
                &[
                    "Gate Security Posture Notice",
                    "beyond_ask Noul",
                    "destructive Noul",
                    "exfiltrates Noul",
                    "kind Choice [read_only local_edit local_exec remote_write publish credentials_or_config other]",
                    "risky Noul decides",
                    "steered Noul",
                    "touches_credentials Noul",
                    "rollback [notices_per_day labels_per_day]",
                ],
            ),
            (
                "security.v3",
                &[
                    "Gate Security2 Posture None",
                    "beyond_ask Noul",
                    "destructive Noul",
                    "exfiltrates Noul",
                    "kind Choice [read_only local_edit local_exec remote_write publish credentials_or_config other]",
                    "risky Noul decides",
                    "safe Noul",
                    "steered Noul decides above 0.75",
                    "touches_credentials Noul",
                    "rollback []",
                ],
            ),
            (
                "classify.v2",
                &[
                    "Inbound Inbound Conversation ThreadRoute",
                    "addressed_task Choice from Tasks + [none] only_when Tasks",
                    "fragment Noul",
                    "kind Choice decides [new_ask follow_up correction control addressed_to_task social other]",
                    "mentions_other_conversation Noul",
                    "should_promote Noul decides",
                    "thread Choice decides from Threads + [new_thread home] only_when Threads",
                    "wants_fresh_look Noul",
                    "rollback [labels_per_day]",
                ],
            ),
            (
                "classify.v3",
                &[
                    "Inbound Inbound Conversation ThreadRoute",
                    "addressed_task Choice from Tasks + [none] only_when Tasks",
                    "fragment Noul",
                    "kind Choice decides [new_ask follow_up correction control addressed_to_task social other]",
                    "mentions_other_conversation Noul",
                    "offer_task Noul",
                    "should_promote Noul decides",
                    "thread Choice decides from Threads + [new_thread home] only_when Threads",
                    "wants_fresh_look Noul",
                    "rollback [labels_per_day]",
                ],
            ),
            (
                "classify.v4",
                &[
                    "Inbound Inbound Conversation ThreadRoute",
                    "addressed_task Choice from Tasks + [none] only_when Tasks",
                    "fragment Noul",
                    "kind Choice decides [new_ask follow_up correction control addressed_to_task social other]",
                    "mentions_other_conversation Noul",
                    "offer_task Noul",
                    "scope Choice [thread place] only_when Scope",
                    "should_promote Noul decides",
                    "thread Choice decides from Threads + [new_thread home] only_when Threads",
                    "wants_fresh_look Noul",
                    "rollback [labels_per_day]",
                ],
            ),
            (
                "role.v1",
                &[
                    "Inbound Inbound CurrentRole RoleHint",
                    "role Choice decides from Roles + [other] only_when Roles",
                    "rollback [switches_per_exchange labels_per_day]",
                ],
            ),
            (
                "directive.v1",
                &[
                    "Inbound Inbound None Capture",
                    "kind Choice decides [none order preference fact correction]",
                    "scope Choice [session place everywhere unsure]",
                    "rollback [labels_per_day]",
                ],
            ),
            (
                "continue.v1",
                &[
                    "Compile Continue Append None",
                    "decision Choice decides [append recompile_transcript recompile_ring recompile_compaction recompile_fresh other]",
                    "stronger_model_for_compaction Noul",
                    "rollback []",
                ],
            ),
            (
                "categorize.v1",
                &[
                    "ExchangeEnd Categorize NoMembership None",
                    "still_member Noul per Memberships max 5",
                    "topic Choice decides from Topics + [new_topic none]",
                    "rollback []",
                ],
            ),
            (
                "rerank.v1",
                &[
                    "Recall Rerank Fused None",
                    "helps Noul per Notes max 10",
                    "helps_more Noul per MoreNotes max 10",
                    "rollback []",
                ],
            ),
            (
                "route.v5",
                &[
                    "Inbound Inbound SessionProfile Route",
                    "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding review other]",
                    "persona Choice from Personas + [prior] only_when Personas",
                    "reply_effort Choice decides [low medium high xhigh max unclear]",
                    "seal Choice [yes no] only_when ThreadNow",
                    "rollback [labels_per_day on_path_p95]",
                ],
            ),
            (
                "route.v6",
                &[
                    "Inbound Inbound SessionProfile Route",
                    "data Choice [open private unclear]",
                    "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding review other]",
                    "persona Choice from Personas + [prior] only_when Personas",
                    "reply_effort Choice decides [low medium high xhigh max unclear]",
                    "seal Choice [yes no] only_when ThreadNow",
                    "rollback [labels_per_day on_path_p95]",
                ],
            ),
            (
                "route.v7",
                &[
                    "Inbound Inbound SessionProfile Route",
                    "data Choice [open private unclear]",
                    "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding review other]",
                    "persona Choice from Personas + [prior] only_when Personas",
                    "personal Choice [none elicit expose]",
                    "reply_effort Choice decides [low medium high xhigh max unclear]",
                    "seal Choice [yes no] only_when ThreadNow",
                    "rollback [labels_per_day on_path_p95]",
                ],
            ),
            (
                "route.v8",
                &[
                    "Inbound Inbound SessionProfile Route",
                    "data Choice [open private unclear]",
                    "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding review other]",
                    "persona Choice from Personas + [prior] only_when Personas",
                    "personal Choice [none elicit expose]",
                    "reply_effort Choice decides [low medium high xhigh max unclear]",
                    "seal Choice [yes no] only_when ThreadNow",
                    "rollback [labels_per_day on_path_p95]",
                ],
            ),
            (
                "route.v10",
                &[
                    "Inbound Inbound SessionProfile Route",
                    "context Choice decides [short full]",
                    "effort Choice decides [low medium high xhigh max default]",
                    "model Choice decides [haiku sonnet opus fable deepseek_flash]",
                    "persona Choice from Personas + [prior] only_when Personas",
                    "personal Choice [none elicit expose]",
                    "seal Choice [yes no] only_when ThreadNow",
                    "rollback [labels_per_day on_path_p95 pins_per_day]",
                ],
            ),
        ];
        for (name, lines) in want {
            let p = by_name(name).unwrap_or_else(|| panic!("{name} is embedded"));
            assert_eq!(shape(&p), *lines, "{name}");
            assert_eq!(p.jev_model, "jev-1.13.0", "{name}");
            assert_eq!(p.sample, 1.0, "{name}: Q16 samples every event");
            // Conservative, but for `directive.v1`, whose bands the owner set:
            // keep at 0.80, ask from 0.50 (2026-10-13, killer OD5).
            let bands = match *name {
                "directive.v1" => (0.80, 0.50),
                _ => (0.90, 0.60),
            };
            for q in &p.questions {
                assert_eq!(
                    (q.thresholds.act, q.thresholds.confirm),
                    bands,
                    "{name}/{}: thresholds start conservative",
                    q.id
                );
            }
        }
        // §2.7's numbers, as the rules hold them.
        let rules = |n: &str| by_name(n).unwrap().rollback.clone();
        assert_eq!(
            rules("loop.v1")[1],
            RollbackRule::SpendRatio {
                ratio: 2.0,
                min_tasks: 10
            }
        );
        assert_eq!(
            rules("loop.v1")[3],
            RollbackRule::OnPathP95 {
                max_ms: 1000,
                min_samples: 20
            }
        );
        assert_eq!(
            rules("security.v1"),
            vec![
                RollbackRule::NoticesPerDay { max: 30 },
                RollbackRule::LabelsPerDay {
                    label: "noise".into(),
                    count: 3
                }
            ]
        );
        assert_eq!(
            rules("route.v5"),
            vec![
                RollbackRule::LabelsPerDay {
                    label: "wrong model".into(),
                    count: 3
                },
                RollbackRule::OnPathP95 {
                    max_ms: 250,
                    min_samples: 20
                }
            ]
        );
        assert_eq!(
            rules("route.v6"),
            rules("route.v5"),
            "route.v6 keeps v5's rules"
        );
        assert_eq!(
            rules("route.v7"),
            rules("route.v6"),
            "route.v7 keeps v6's rules"
        );
        assert_eq!(
            rules("route.v8"),
            rules("route.v7"),
            "route.v8 keeps v7's rules"
        );
        assert_eq!(
            rules("classify.v2"),
            vec![RollbackRule::LabelsPerDay {
                label: "wrong thread".into(),
                count: 3
            }]
        );
        assert_eq!(
            rules("classify.v3"),
            rules("classify.v2"),
            "classify.v3 keeps v2's rules"
        );
        assert_eq!(
            rules("classify.v4"),
            rules("classify.v2"),
            "classify.v4 keeps v2's rules"
        );
        assert_eq!(
            rules("directive.v1"),
            vec![RollbackRule::LabelsPerDay {
                label: "wrong capture".into(),
                count: 3
            }]
        );
        assert_eq!(
            rules("role.v1"),
            vec![
                RollbackRule::SwitchesPerExchange { max: 2 },
                RollbackRule::LabelsPerDay {
                    label: "wrong role".into(),
                    count: 2
                }
            ]
        );
    }

    /// Each rule, broken in each of the six pack files, refuses that file.
    #[test]
    fn the_loaders_rules_hold_on_all_the_packs() {
        let six: Vec<&(&str, &str)> = EMBEDDED.iter().filter(|(f, _)| *f != "probe.v1").collect();
        assert_eq!(
            six.len(),
            34,
            "§2.4's six, security.v3, rerank.v1, memory.v1, attribution.v1, route.v5 to v8, route.v10, \
             citation.v1, people.v1, people_seen.v1, classify.v2 to v4, compare.v1, heard.v1, directive.v1, \
             onto.v1, same_thing.v1, two_things.v1, same_kind.v1, name_it.v1, recall_cross.v1, families.v1, \
             content.v1 to v3 and split.v1"
        );
        for (file, text) in six {
            let p = Pack::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
            let edit = |from: &str, to: &str| {
                assert!(text.contains(from), "{file} lacks {from:?}");
                text.replacen(from, to, 1)
            };
            let check = |broken: String, rule: Rule, what: &str| {
                assert_eq!(refused(&broken), rule, "{file}: {what}");
            };
            check(
                edit("jev_model = \"jev-1.13.0\"", "jev_model = \"jev-latest\""),
                Rule::PinnedModel,
                "jev-latest",
            );
            check(
                edit(
                    &format!("state_cap_tokens = {}", p.state_cap_tokens),
                    "state_cap_tokens = 40000",
                ),
                Rule::StateCap,
                "a cap past Jev's limit",
            );
            let act_line = text
                .lines()
                .find(|l| l.starts_with("act = "))
                .unwrap_or_else(|| panic!("{file} has no act line"));
            check(
                edit(act_line, "act = 0.10"),
                Rule::Thresholds,
                "act below confirm",
            );
            for q in &p.questions {
                let line = format!("instructions = \"{}\"", q.instructions);
                check(
                    edit(
                        &line,
                        &format!("instructions = \"Calculate this: {}\"", q.instructions),
                    ),
                    Rule::NoComputation,
                    &q.id,
                );
                check(
                    edit(&line, "instructions = \"Is it?\""),
                    Rule::Instructions,
                    &q.id,
                );
                if let Some(nm) = &q.no_match {
                    check(
                        edit(&format!("no_match = \"{nm}\"\n"), ""),
                        Rule::NoMatch,
                        &q.id,
                    );
                }
            }
            if p.action != Action::None {
                let cut = text
                    .find("[[rollback]]")
                    .expect("a pack that acts has rules");
                check(text[..cut].to_string(), Rule::Rollback, "no rollback rules");
            }
        }
    }

    /// `decide_above` rides on a deciding whole Noul, within confirm..=act.
    #[test]
    fn decide_above_belongs_to_a_deciding_noul_within_its_thresholds() {
        let v3 = include_str!("../packs/security.v3.toml");
        let ok = Pack::parse(v3).unwrap();
        assert_eq!(ok.question("steered").unwrap().decide_above, Some(0.75));
        assert_eq!(ok.question("risky").unwrap().decide_above, None);
        let edit = |from: &str, to: &str| {
            assert!(v3.contains(from), "{from:?}");
            v3.replacen(from, to, 1)
        };
        // Not deciding.
        let s = edit("decides = true\ndecide_above = 0.75", "decide_above = 0.75");
        assert_eq!(refused(&s), Rule::DecideAbove);
        // Below confirm, above act.
        let s = edit("decide_above = 0.75", "decide_above = 0.5");
        assert_eq!(refused(&s), Rule::DecideAbove);
        let s = edit("decide_above = 0.75", "decide_above = 0.95");
        assert_eq!(refused(&s), Rule::DecideAbove);
        // Edges are inside.
        Pack::parse(&edit("decide_above = 0.75", "decide_above = 0.60")).unwrap();
        Pack::parse(&edit("decide_above = 0.75", "decide_above = 0.90")).unwrap();
        // On a Choice.
        let s = edit(
            "no_match = \"other\"",
            "no_match = \"other\"\ndecides = true\ndecide_above = 0.75",
        );
        assert_eq!(refused(&s), Rule::DecideAbove);
    }

    /// Every pack that exists before `decide_above` loads to the same
    /// checked definition: no bar, and a serialization without the field.
    #[test]
    fn a_pack_without_decide_above_serializes_as_before() {
        for (file, text) in EMBEDDED.iter().filter(|(f, _)| *f != "security.v3") {
            let p = Pack::parse(text).unwrap();
            assert!(
                p.questions.iter().all(|q| q.decide_above.is_none()),
                "{file}"
            );
            let json = serde_json::to_string(&p).unwrap();
            assert!(!json.contains("decide_above"), "{file}");
        }
    }

    /// split.v1 (theseus-z9mze.5) is one question that decides nothing and acts on nothing, asked in the
    /// inbound batch (the `inbound` state, so it shares classify's request). It asks a property of the
    /// work, never a recommendation about the machinery: no word of helpers, tasks, cost or fanning out.
    #[test]
    fn split_v1_is_one_shadow_question_about_the_work() {
        let p = by_name("split.v1").expect("split.v1 is embedded");
        assert_eq!((p.point, p.action), (Point::Inbound, Action::None));
        assert_eq!(p.questions.len(), 1);
        let q = p.question("split").unwrap();
        assert!(!q.decides && q.kind == Kind::Choice);
        assert_eq!(q.no_match.as_deref(), Some("unclear"));
        let ids: Vec<&str> = q.options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["splits_by_area", "one_area", "unclear"]);
        let said = format!(
            "{} {}",
            q.instructions,
            q.options
                .iter()
                .filter_map(|o| o.means.as_deref())
                .collect::<String>()
        )
        .to_lowercase();
        for word in ["helper", "task", "cost", "fan out", "fan-out", "budget"] {
            assert!(!said.contains(word), "the question names {word:?}");
        }
        assert_eq!(
            p.builder,
            by_name("classify.v4").unwrap().builder,
            "it batches with classify"
        );
    }

    /// route.v5 (theseus-c1uz) asks two questions that decide nothing for the
    /// pack's promotion: `persona`, whose options are the personas and whose
    /// no-match is `prior`, and `seal`, a yes or no.
    #[test]
    fn route_v5_has_a_persona_and_a_seal_question() {
        let v5 = by_name("route.v5").unwrap();
        let persona = v5.question("persona").unwrap();
        assert!(!persona.decides && persona.kind == Kind::Choice);
        assert_eq!(persona.no_match.as_deref(), Some("prior"));
        assert_eq!(persona.options_from, Some(Source::Personas));
        let seal = v5.question("seal").unwrap();
        assert!(!seal.decides);
        assert_eq!(seal.no_match.as_deref(), Some("no"));
        assert_eq!(seal.only_when, Some(Source::ThreadNow));
    }

    /// route.v6 (theseus-a1wvq.2) is route.v5 word for word, its rules and its
    /// description's opening included, and one question that decides nothing for
    /// the pack's promotion: `data`, three options, `unclear` when nothing fits.
    #[test]
    fn route_v6_is_v5_word_for_word_and_a_data_question() {
        let (v5, v6) = (by_name("route.v5").unwrap(), by_name("route.v6").unwrap());
        for q in ["mode", "reply_effort", "persona", "seal"] {
            assert_eq!(v6.question(q), v5.question(q), "{q} word for word");
        }
        let ids: Vec<&str> = v6.questions.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids.len(), 5, "v5's four and data: {ids:?}");
        let data = v6.question("data").unwrap();
        assert!(!data.decides && data.kind == Kind::Choice);
        assert_eq!((data.thresholds.act, data.thresholds.confirm), (0.90, 0.60));
        assert_eq!(data.no_match.as_deref(), Some("unclear"));
        let options: Vec<&str> = data.options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(options, ["open", "private", "unclear"]);
        assert_eq!(v6.rollback, v5.rollback, "both rules, word for word");
        assert_eq!(v6.version, 6);
        assert_eq!(
            (v6.point, v6.builder, v6.baseline, v6.action),
            (v5.point, v5.builder, v5.baseline, v5.action)
        );
        assert_eq!(v6.state_cap_tokens, v5.state_cap_tokens);
        let opening = v5.description.trim_end_matches(['.', '"']);
        let opening = opening.replace("and whether it warrants", "whether it warrants");
        assert!(
            v6.description.starts_with(&opening),
            "v6's description keeps v5's: {}",
            v6.description
        );
        assert!(v6.description.contains("outside the owner's trust"));
    }

    /// route.v7 (theseus-a1wvq) is route.v6 word for word, but for the `data`
    /// question's wording: the same id, options, thresholds and no-match, so
    /// what reads the answer reads it unchanged; `private` asks for a sign of
    /// private matter, and work whose owner the message does not show is
    /// `unclear`, which fails closed as `private` does. And one question that
    /// decides nothing, `personal` (theseus-gasov): `none`, `elicit` or
    /// `expose`, `none` when nothing fits.
    #[test]
    fn route_v7_is_v6_word_for_word_but_the_data_wording() {
        let (v6, v7) = (by_name("route.v6").unwrap(), by_name("route.v7").unwrap());
        for q in ["mode", "reply_effort", "persona", "seal"] {
            assert_eq!(v7.question(q), v6.question(q), "{q} word for word");
        }
        let options = |q: &QuestionDef| q.options.iter().map(|o| o.id.clone()).collect::<Vec<_>>();
        let ids = |p: &Pack| p.questions.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
        let mut want = ids(&v6);
        want.push("personal".into());
        let mut got = ids(&v7);
        want.sort();
        got.sort();
        assert_eq!(got, want, "v6's five and personal");
        let personal = v7.question("personal").unwrap();
        assert!(!personal.decides && personal.kind == Kind::Choice);
        assert_eq!(personal.no_match.as_deref(), Some("none"));
        assert_eq!(options(personal), ["none", "elicit", "expose"]);
        let (d6, d7) = (v6.question("data").unwrap(), v7.question("data").unwrap());
        assert_ne!(d7.instructions, d6.instructions);
        assert_eq!(
            (&d7.decides, &d7.kind, &d7.thresholds, &d7.no_match),
            (&d6.decides, &d6.kind, &d6.thresholds, &d6.no_match)
        );
        let opt = |q: &QuestionDef, id: &str| {
            q.options
                .iter()
                .find(|o| o.id == id)
                .unwrap()
                .means
                .clone()
                .unwrap_or_default()
        };
        assert_eq!(options(d7), options(d6));
        assert!(d7.instructions.contains("not by the place"));
        assert!(opt(d7, "open").contains("self-contained exercise"));
        assert!(opt(d7, "unclear").contains("whose owner it does not show"));
        assert!(!opt(d7, "private").contains("work for a company"));
        assert_eq!(v7.rollback, v6.rollback, "both rules, word for word");
        assert_eq!(v7.version, 7);
        assert_eq!(
            (
                v7.point,
                v7.builder,
                v7.baseline,
                v7.action,
                v7.state_cap_tokens
            ),
            (
                v6.point,
                v6.builder,
                v6.baseline,
                v6.action,
                v6.state_cap_tokens
            )
        );
        let opening = v6.description.trim_end_matches('.');
        assert!(v7.description.starts_with(opening), "{}", v7.description);
    }

    /// route.v8 (theseus-f9qa8) is route.v7 word for word, but for the `data`
    /// question's `open` and `unclear` options: `open` names the public signs
    /// and says an unshown owner is no sign of private matter; `unclear` stops
    /// counting an unshown owner; `private`, the instructions, the thresholds
    /// and the no-match are v7's, so what reads the answer reads it unchanged;
    /// and `reply_effort`'s `max` (theseus-ujzuh), which adds the evidence a
    /// checkpoint shows to v7's words.
    #[test]
    fn route_v8_is_v7_word_for_word_but_the_data_options_and_effort_max() {
        let (v7, v8) = (by_name("route.v7").unwrap(), by_name("route.v8").unwrap());
        for q in ["mode", "persona", "seal", "personal"] {
            assert_eq!(v8.question(q), v7.question(q), "{q} word for word");
        }
        let ids = |p: &Pack| p.questions.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&v8), ids(&v7), "the same questions");
        let (d7, d8) = (v7.question("data").unwrap(), v8.question("data").unwrap());
        // A question's frame: all but its options.
        let frame = |q: &QuestionDef| {
            (
                q.instructions.clone(),
                q.decides,
                q.kind,
                q.thresholds,
                q.no_match.clone(),
            )
        };
        assert_eq!(frame(d8), frame(d7));
        let opts = |q: &QuestionDef| q.options.iter().map(|o| o.id.clone()).collect::<Vec<_>>();
        assert_eq!(opts(d8), ["open", "private", "unclear"]);
        assert_eq!(opts(d8), opts(d7));
        let means = |q: &QuestionDef, id: &str| {
            q.options
                .iter()
                .find(|o| o.id == id)
                .unwrap()
                .means
                .clone()
                .unwrap_or_default()
        };
        let (e7, e8) = (
            v7.question("reply_effort").unwrap(),
            v8.question("reply_effort").unwrap(),
        );
        assert_eq!(frame(e8), frame(e7));
        assert_eq!(opts(e8), opts(e7));
        for id in opts(e7).iter().filter(|id| *id != "max") {
            assert_eq!(
                means(e8, id),
                means(e7, id),
                "reply_effort {id} word for word"
            );
        }
        assert_eq!(
            means(e8, "max"),
            "The most thought there is: the hardest or most novel problems, or high-stakes work where every step must be checked, or the person asks for the most thorough reply possible, or the work has kept failing after more careful effort was already given, or a check it must pass still fails."
        );
        assert_eq!(
            means(d8, "private"),
            means(d7, "private"),
            "private verbatim"
        );
        let (open7, open8) = (means(d7, "open"), means(d8, "open"));
        assert!(
            open8.starts_with(&open7),
            "open keeps v7's words, then adds"
        );
        assert!(open8.contains("public licence") && open8.contains("toolchain"));
        assert!(open8.contains("not a sign of private matter"));
        let unclear = means(d8, "unclear");
        assert!(unclear.contains("mixes open and private matter"));
        assert!(!unclear.contains("owner it does not show"), "{unclear}");
        assert_eq!(v8.rollback, v7.rollback, "both rules, word for word");
        assert_eq!(v8.version, 8);
        assert_eq!(
            (
                v8.point,
                v8.builder,
                v8.baseline,
                v8.action,
                v8.state_cap_tokens
            ),
            (
                v7.point,
                v7.builder,
                v7.baseline,
                v7.action,
                v7.state_cap_tokens
            )
        );
        assert_eq!(v8.description, v7.description);
    }

    /// One question of a pack, or a panic naming it.
    fn q<'a>(p: &'a Pack, n: &str) -> &'a QuestionDef {
        p.question(n).unwrap_or_else(|| panic!("{n}"))
    }

    /// Everything about `content.v2` that is not its wording is `content.v1`'s
    /// (theseus-78qay): the same id, builder, point, cap, sample, baseline and
    /// action, the same four questions deciding at the same bars, and the same
    /// rollback rules. `injection` is byte-identical, since its `no` already said
    /// that discussing prompt injection is not attempting one.
    #[test]
    fn content_v2_keeps_v1s_shape_and_its_injection_question() {
        let (v1, v2) = (
            by_name("content.v1").unwrap(),
            by_name("content.v2").unwrap(),
        );
        assert_eq!(v2.version, 2);
        assert_eq!(v2.id, v1.id);
        assert_eq!(
            (
                v2.point,
                v2.builder,
                v2.baseline,
                v2.action,
                v2.state_cap_tokens,
                v2.sample
            ),
            (
                v1.point,
                v1.builder,
                v1.baseline,
                v1.action,
                v1.state_cap_tokens,
                v1.sample
            )
        );
        let ids = |p: &Pack| p.questions.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&v2), ids(&v1));
        for n in ["confidential", "injection", "personal", "secret"] {
            let (a, b) = (q(&v2, n), q(&v1, n));
            assert_eq!(
                (
                    &a.decides,
                    &a.kind,
                    &a.thresholds,
                    &a.no_match,
                    &a.only_when
                ),
                (
                    &b.decides,
                    &b.kind,
                    &b.thresholds,
                    &b.no_match,
                    &b.only_when
                ),
                "{n}"
            );
            assert!(a.decides, "{n}");
        }
        assert_eq!(q(&v2, "injection"), q(&v1, "injection"));
        assert_eq!(v2.rollback, v1.rollback, "both rules, word for word");
        assert_ne!(v2.description, v1.description, "it reveals, not shows");
    }

    /// `content.v2`'s one idea, added and nothing taken away (theseus-78qay, the
    /// owner 2026-10-27: a flag describes what an item REVEALS, not what it is
    /// ABOUT). Each of the three revealing questions carries the rule in its
    /// `instructions` and names the three near-misses in its `no`.
    #[test]
    fn content_v2_adds_the_reveal_rule_and_changes_nothing_else() {
        let (v1, v2) = (
            by_name("content.v1").unwrap(),
            by_name("content.v2").unwrap(),
        );
        let means = |d: &QuestionDef, id: &str| {
            d.options
                .iter()
                .find(|o| o.id == id)
                .unwrap()
                .means
                .clone()
                .unwrap_or_default()
        };
        // Each revealing question carries the reveal rule in the question itself,
        // where the text being judged cannot argue with it. It does NOT ask whether
        // the party is real: a first draft did, and on the live set it cost recall
        // 1.000 -> 0.623, because one item rarely shows whose anything is and Jev
        // answered "cannot tell" as "no". The rule that holds is reveal-versus-about,
        // and the near-misses are named in `no`, where they are observable.
        for n in ["secret", "confidential", "personal"] {
            let (a, b) = (q(&v2, n), q(&v1, n));
            assert_ne!(a.instructions, b.instructions, "{n}: v2 rewords it");
            assert!(
                a.instructions.contains("not what it is about"),
                "{n}: {}",
                a.instructions
            );
            assert!(
                !a.instructions.contains("real"),
                "{n}: `real` cost recall; the rule is reveal-versus-about: {}",
                a.instructions
            );
            // `yes` keeps v1's words and then adds; `no` grows, never shrinks.
            assert!(
                means(a, "yes").starts_with(&means(b, "yes")),
                "{n}: v2's yes drops words v1's had"
            );
            assert!(means(a, "no").len() > means(b, "no").len(), "{n}");
        }
        // And each `no` names the three near-misses v1 flagged on.
        for (n, words) in [
            (
                "secret",
                &[
                    "about credentials",
                    "exercise's own answer",
                    "working output",
                    // Scoped to the exercise, never to the tree: a password in an
                    // ordinary project's configuration is a credential. A review of
                    // 2026-10-27 found the unscoped version, and the set now holds
                    // `sec_14` and `sec_15` against it.
                    "whatever directory it sits in",
                ][..],
            ),
            (
                "confidential",
                &[
                    "topic of confidentiality",
                    "self-contained technical exercise",
                    "invented organisation names",
                    // A real organisation's own test suite is a test suite; its
                    // fixtures are not exempt for that reason (`conf_14`).
                    "actual customers",
                ][..],
            ),
            (
                "personal",
                &[
                    "topic of privacy",
                    "Placeholder identities",
                    "benchmark task",
                ][..],
            ),
        ] {
            let no = means(q(&v2, n), "no");
            for w in words {
                assert!(no.contains(w), "{n}'s no lacks {w:?}: {no}");
            }
        }
    }

    /// `content.v3` (theseus-gyjyo.11) keeps everything of `content.v2` but its
    /// questions' wording: the same id, builder, point, cap, bars, rollback, and
    /// `injection` byte for byte, with the reveal rule still in each revealing
    /// question's `instructions` and `yes` still starting with v2's words. What
    /// v3 adds goes in each `no`, as the explicit cues for the five new fields,
    /// and it adds them, never replaces: v2's three near-misses and `sec_14`'s
    /// ordinary-project clause stay.
    #[test]
    fn content_v3_keeps_v2s_shape_and_adds_the_field_cues_to_each_no() {
        let (v2, v3) = (
            by_name("content.v2").unwrap(),
            by_name("content.v3").unwrap(),
        );
        assert_eq!(v3.version, 3);
        assert_eq!(v3.id, v2.id);
        assert_eq!(
            (
                v3.point,
                v3.builder,
                v3.baseline,
                v3.action,
                v3.state_cap_tokens,
                v3.sample
            ),
            (
                v2.point,
                v2.builder,
                v2.baseline,
                v2.action,
                v2.state_cap_tokens,
                v2.sample
            )
        );
        assert_eq!(v3.rollback, v2.rollback);
        assert_eq!(v3.jev_model, v2.jev_model);
        assert_eq!(
            q(&v3, "injection"),
            q(&v2, "injection"),
            "injection is v2's, byte for byte"
        );
        let means = |d: &QuestionDef, id: &str| {
            d.options
                .iter()
                .find(|o| o.id == id)
                .unwrap()
                .means
                .clone()
                .unwrap_or_default()
        };
        for n in ["secret", "confidential", "personal"] {
            let (a, b) = (q(&v3, n), q(&v2, n));
            assert_eq!(
                a.instructions, b.instructions,
                "{n}: the reveal rule stays as it was"
            );
            assert!(a.instructions.contains("not what it is about"), "{n}");
            assert_eq!(
                (&a.decides, &a.thresholds, &a.no_match, &a.only_when),
                (&b.decides, &b.thresholds, &b.no_match, &b.only_when),
                "{n}"
            );
            assert!(
                means(a, "yes").starts_with(&means(b, "yes")),
                "{n}: v3's yes drops v2's words"
            );
            let (no3, no2) = (means(a, "no"), means(b, "no"));
            assert!(no3.starts_with(&no2), "{n}: v3's no drops v2's words");
            // The cue for the fields this question reads, in words the state's keys
            // appear in, so the pack and the builder cannot drift apart.
            assert!(
                no3.contains("`public`"),
                "{n}'s no does not say what `public` means: {no3}"
            );
            assert!(
                no3.contains("`context`"),
                "{n}'s no does not read `context`: {no3}"
            );
        }
        // The clause that keeps `sec_14` and `sec_15` flagging is untouched, and v3
        // says outright that public does not mean clean.
        assert!(means(q(&v3, "secret"), "no").contains("whatever directory it sits in"));
        for n in ["secret", "confidential"] {
            let no = means(q(&v3, n), "no");
            assert!(no.contains("still"), "{n}: public is not clean: {no}");
        }
        assert!(means(q(&v3, "confidential"), "no").contains("`author`"));
        assert!(means(q(&v3, "confidential"), "no").contains("`project`"));
        assert_ne!(v3.description, v2.description);
    }

    /// No pack's text names an organisation: the matcher runs over every pack
    /// against the template's `[privacy] sensitive_names` line, the owner's
    /// company and repository names (the owner, 2026-10-17: "just use the
    /// config", not a hashed name), and the mechanism is proved on a planted
    /// text. The daemon also checks every embedded pack against the operator's
    /// own list at start (`names::packs_naming`, theseus-oejh3).
    #[test]
    fn the_pack_text_names_no_organisation() {
        let template = include_str!("../../theseus-core/config/theseus.example.toml");
        let line = template
            .lines()
            .find_map(|l| l.strip_prefix("# sensitive_names = "))
            .expect("the template's [privacy] sensitive_names line");
        let names: Vec<String> = toml::from_str::<toml::Table>(&format!("n = {line}")).unwrap()
            ["n"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["examplecorp", "examplecorpinc", "example-repo"]);
        // The repository is public: the template names none of the old
        // line's four names, which are held only as digests.
        assert!(
            !crate::names::names_an_old_name(template, &mut Default::default()),
            "the template names an old name"
        );
        for (name, text) in EMBEDDED {
            assert_eq!(hits(text, &names), Vec::<String>::new(), "{name}");
        }
        let planted = format!("{}\nExampleCorp's private plans", EMBEDDED[0].1);
        assert_eq!(hits(&planted, &names), ["examplecorp"]);
    }

    /// The text of every `.toml` in the packs directory: a pack, its cases,
    /// anything put beside them (theseus-ev3ir).
    fn swept_texts(_dir: &std::path::Path) -> Vec<(String, String)> {
        EMBEDDED
            .iter()
            .map(|(name, text)| (name.to_string(), text.to_string()))
            .collect()
    }

    /// A `.cases.toml` is a case file, not a pack, so it is not in
    /// `EMBEDDED`: two sensitive names reached public refs in one, and the
    /// pack-text test never read it (theseus-ev3ir). The sweep reads the
    /// directory, and a fake name planted in a new file there is found.
    #[test]
    fn a_cases_file_beside_the_packs_is_swept() {
        let dir = std::env::temp_dir().join(format!("theseus-ev3ir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("zz.cases.toml"),
            "[[case]]\nlabel = \"ExampleCorp's private plans\"\n",
        )
        .unwrap();
        let names = vec!["examplecorp".to_string()];
        let texts = swept_texts(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let found: Vec<&str> = texts
            .iter()
            .filter(|(_, text)| !hits(text, &names).is_empty())
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(found, ["zz.cases"], "the planted name was not found");
    }

    /// The real packs directory: every `.toml` in it is swept (the count is
    /// asserted, so an empty sweep cannot pass), and none names the template's
    /// organisations or one of the old names held as digests.
    #[test]
    fn the_packs_directory_names_no_organisation() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packs");
        let on_disk = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                let path = e.as_ref().unwrap().path();
                path.extension().is_some_and(|x| x == "toml")
            })
            .count();
        let texts = swept_texts(&dir);
        assert!(on_disk > EMBEDDED.len(), "no file beside the packs to sweep");
        assert_eq!(texts.len(), on_disk, "the sweep missed a file in packs/");
        let names = ["examplecorp", "examplecorpinc", "example-repo", "example-utils"]
            .map(String::from);
        let mut clean = Default::default();
        for (name, text) in &texts {
            assert_eq!(hits(text, &names), Vec::<String>::new(), "{name}");
            assert!(
                !crate::names::names_an_old_name(text, &mut clean),
                "{name} names an old name"
            );
        }
    }

    /// A judgment's record of its dynamic sources (theseus-gyjyo.1): every
    /// source the pack's questions draw on, an empty one included, and only
    /// those; read back for a pack that needs the same, and refused, naming
    /// them, for one that needs another.
    #[test]
    fn recorded_sources_round_trip_and_name_what_is_missing() {
        let pack = by_name("content.v2").unwrap();
        assert_eq!(
            Dynamic::needed(&pack).into_iter().collect::<Vec<_>>(),
            [Source::Untrusted]
        );
        let held = Dynamic {
            sources: BTreeMap::from([
                (
                    Source::Untrusted,
                    vec![Item {
                        key: "untrusted".into(),
                        text: "not the operator's own words".into(),
                    }],
                ),
                (Source::Tasks, vec![]),
            ]),
            assets: vec![],
        };
        let rec = held.recorded(&pack);
        assert_eq!(
            rec,
            serde_json::json!({"untrusted": [{"key": "untrusted",
                "text": "not the operator's own words"}]})
        );
        let back = Dynamic::from_record(&rec, &pack).unwrap();
        assert_eq!(back.items(Source::Untrusted), held.items(Source::Untrusted));
        // Nothing found is recorded as an empty list, which is a source held.
        let none = Dynamic::default().recorded(&pack);
        assert_eq!(none, serde_json::json!({"untrusted": []}));
        assert!(Dynamic::from_record(&none, &pack).is_ok());
        // A record of nothing, or of other sources, holds none of what it needs.
        for rec in [
            serde_json::json!({}),
            serde_json::json!(null),
            serde_json::json!({"tasks": []}),
        ] {
            assert_eq!(
                Dynamic::from_record(&rec, &pack).unwrap_err(),
                [Source::Untrusted]
            );
        }
    }
}
