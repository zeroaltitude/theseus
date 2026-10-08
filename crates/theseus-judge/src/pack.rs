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

/// Every pack version this build knows, by file name: the test pack,
/// §2.4's six, `security.v2` and `security.v3`, candidates beside `security.v1`,
/// `rerank.v1`, recall's `+rerank` arm (M6 step 32c), and `route.v1`, the
/// model per interaction mode (M5 step 25e), with `route.v2`, its successor
/// with a `quick` mode (theseus-3okf), and `route.v3`, which asks the reply's
/// effort beside v2's mode (theseus-qe3v).
pub const EMBEDDED: &[(&str, &str)] = &[
    ("probe.v1", include_str!("../packs/probe.v1.toml")),
    ("loop.v1", include_str!("../packs/loop.v1.toml")),
    ("security.v1", include_str!("../packs/security.v1.toml")),
    ("security.v2", include_str!("../packs/security.v2.toml")),
    ("security.v3", include_str!("../packs/security.v3.toml")),
    ("classify.v1", include_str!("../packs/classify.v1.toml")),
    ("role.v1", include_str!("../packs/role.v1.toml")),
    ("continue.v1", include_str!("../packs/continue.v1.toml")),
    ("categorize.v1", include_str!("../packs/categorize.v1.toml")),
    ("rerank.v1", include_str!("../packs/rerank.v1.toml")),
    ("memory.v1", include_str!("../packs/memory.v1.toml")),
    (
        "attribution.v1",
        include_str!("../packs/attribution.v1.toml"),
    ),
    ("route.v1", include_str!("../packs/route.v1.toml")),
    ("route.v2", include_str!("../packs/route.v2.toml")),
    ("route.v3", include_str!("../packs/route.v3.toml")),
    ("citation.v1", include_str!("../packs/citation.v1.toml")),
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
}

/// The state builder a pack names (a closed set, in code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builder {
    Probe,
    Loop,
    Security,
    /// `security.v2`'s: `security.v1`'s state, and facts computed for Jev.
    Security2,
    /// `classify.v1` and `role.v1` share it, so they batch.
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
    /// The turn runs on the session's own profile (`route.v1`, 25e).
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
    /// Runs the turn on the mode's profile (`route.v1`, 25e).
    Route,
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
}

impl Dynamic {
    pub fn items(&self, s: Source) -> &[Item] {
        self.sources.get(&s).map_or(&[], Vec::as_slice)
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
                "security.v2",
                &[
                    "Gate Security2 Posture None",
                    "beyond_ask Noul",
                    "destructive Noul",
                    "exfiltrates Noul",
                    "kind Choice [read_only local_edit local_exec remote_write publish credentials_or_config other]",
                    "risky Noul decides",
                    "safe Noul",
                    "steered Noul",
                    "touches_credentials Noul",
                    "rollback []",
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
                "classify.v1",
                &[
                    "Inbound Inbound Conversation None",
                    "addressed_task Choice from Tasks + [none] only_when Tasks",
                    "fragment Noul",
                    "kind Choice decides [new_ask follow_up correction control addressed_to_task social other]",
                    "mentions_other_conversation Noul",
                    "should_promote Noul decides",
                    "wants_fresh_look Noul",
                    "rollback []",
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
                "route.v1",
                &[
                    "Inbound Inbound SessionProfile Route",
                    "mode Choice decides [trivial chat sophisticated deep_coding routine_coding other]",
                    "rollback [labels_per_day on_path_p95]",
                ],
            ),
            (
                "route.v2",
                &[
                    "Inbound Inbound SessionProfile Route",
                    "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding other]",
                    "rollback [labels_per_day on_path_p95]",
                ],
            ),
            (
                "route.v3",
                &[
                    "Inbound Inbound SessionProfile Route",
                    "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding other]",
                    "reply_effort Choice decides [low medium high xhigh max unclear]",
                    "rollback [labels_per_day on_path_p95]",
                ],
            ),
        ];
        for (name, lines) in want {
            let p = by_name(name).unwrap_or_else(|| panic!("{name} is embedded"));
            assert_eq!(shape(&p), *lines, "{name}");
            assert_eq!(p.jev_model, "jev-1.13.0", "{name}");
            assert_eq!(p.sample, 1.0, "{name}: Q16 samples every event");
            for q in &p.questions {
                assert_eq!(
                    (q.thresholds.act, q.thresholds.confirm),
                    (0.90, 0.60),
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
            rules("route.v1"),
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
            rules("route.v2"),
            rules("route.v1"),
            "route.v2 keeps v1's rules"
        );
        assert_eq!(
            rules("route.v3"),
            rules("route.v1"),
            "route.v3 keeps v1's rules"
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
            15,
            "§2.4's six, security.v2 and v3, rerank.v1, memory.v1, attribution.v1, route.v1, v2 \
             and v3, and citation.v1"
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
            check(
                edit("act = 0.90", "act = 0.50"),
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

    /// route.v3 (theseus-qe3v) asks route.v2's mode word for word, and the
    /// reply's effort on the five levels Anthropic's models take, `unclear`
    /// its no-match option; the effort sorts after the mode, so the mode
    /// stays the judgment's first deciding answer (its headline).
    #[test]
    fn route_v3_asks_v2s_mode_and_the_replys_effort() {
        let (v2, v3) = (by_name("route.v2").unwrap(), by_name("route.v3").unwrap());
        assert_eq!(v3.question("mode"), v2.question("mode"), "word for word");
        let effort = v3.question("reply_effort").unwrap();
        let ids: Vec<&str> = effort.options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["low", "medium", "high", "xhigh", "max", "unclear"]);
        assert_eq!(effort.no_match.as_deref(), Some("unclear"));
        assert!(effort.decides && effort.kind == Kind::Choice);
        let order: Vec<&str> = v3.questions.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(order, ["mode", "reply_effort"]);
    }
}
