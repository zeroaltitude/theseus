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
//! - the state cap is inside Jev's state limit.
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

/// Every pack version this build knows, by file name.
pub const EMBEDDED: &[(&str, &str)] = &[("probe.v1", include_str!("../packs/probe.v1.toml"))];

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
    Probe,
}

/// The state builder a pack names (a closed set, in code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builder {
    Probe,
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
}

/// The live action in code (closed set), or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    None,
    NudgeTask,
    Notice,
    RoleHint,
}

/// Where a question's dynamic options or per-item Nouls come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Tasks,
    Roles,
    Topics,
    Memberships,
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
}
