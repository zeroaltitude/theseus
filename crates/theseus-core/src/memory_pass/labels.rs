//! The deterministic labeler (M6 §2.6, step 31a): each eligible node's
//! kind, durability, what it is about, whether it holds volatile values,
//! its trust, and whether the operator wrote it as a correction. Pure, and
//! table-driven: every rule is a row of a table below, and the tests walk a
//! table of their own. `about` is the index's entity field, asked of the
//! tender (`index.entities`): there is one extractor, the tender's, and no
//! copy of its rules here.

use serde::Serialize;

use crate::node::Origin;

/// What a node is, as memory keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Preference,
    Decision,
    Procedure,
    Episode,
    Transient,
    Fact,
    Other,
}

/// How long it should matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Durability {
    High,
    Medium,
    Low,
    Floor,
}

/// Where a node's text came from, as the labeler reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A human's message (or a task's report, a wake: a `UserMessage`).
    Message,
    /// The model's reply.
    Reply,
    /// A tool's result.
    Result,
    /// A compaction's summary (30c): the model's account of a run of the
    /// session's messages, labeled as a reply is (decision 10).
    Summary,
}

/// Who a rule hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Operator,
    Anyone,
}

/// A phrase rule: a node whose words hold any of `phrases` (whole words, in
/// order) is `kind`, when `who` wrote it.
pub struct PhraseRule {
    pub kind: Kind,
    pub who: Who,
    pub phrases: &'static [&'static str],
}

/// §2.6's kind rules, in the order they are tried after a tool result
/// (always `episode`) and a short acknowledgement (`transient`).
pub const KIND_RULES: &[PhraseRule] = &[
    PhraseRule {
        kind: Kind::Decision,
        who: Who::Operator,
        phrases: &[
            "we decided",
            "we've decided",
            "i decided",
            "let's go with",
            "lets go with",
            "we'll go with",
            "go with",
            "the decision is",
        ],
    },
    PhraseRule {
        kind: Kind::Preference,
        who: Who::Operator,
        phrases: &[
            "always",
            "never",
            "prefer",
            "i prefer",
            "don't",
            "do not",
            "please don't",
        ],
    },
];

/// Words that open a short acknowledgement.
pub const ACKS: &[&str] = &[
    "ok", "okay", "k", "thanks", "thank", "thx", "ty", "yes", "yep", "yeah", "sure", "great",
    "nice", "cool", "perfect", "lgtm", "done", "got", "sounds", "good", "fine", "right", "no",
    "nope", "agreed", "ack", "👍", "🙏",
];

/// A short acknowledgement has at most this many words.
pub const ACK_WORDS: usize = 4;

/// The verbs that make a code block an instruction: a procedure.
pub const INSTRUCTIONS: &[&str] = &[
    "run", "use", "type", "execute", "call", "install", "build", "start", "restart", "deploy",
    "invoke", "to",
];

/// A message of at least this many words that no rule takes is a fact; a
/// shorter one is other.
pub const FACT_WORDS: usize = 4;

/// The operator's corrections (§2.3's "a human correction"): the gate's
/// `supersedes` reads it.
pub const CORRECTIONS: &[&str] = &[
    "correction",
    "actually",
    "i was wrong",
    "that was wrong",
    "that's wrong",
    "is wrong",
    "i misspoke",
    "scratch that",
    "to correct",
    "not quite",
    "instead of",
    "rather than",
];

/// What makes a value volatile (§2.6): words of time, and the shapes of
/// versions, times, and counts. Commit hashes come from the entities.
pub const VOLATILE_WORDS: &[&str] = &[
    "currently",
    "right now",
    "at the moment",
    "for now",
    "as of",
    "today",
    "yesterday",
    "this morning",
    "this week",
    "branch",
    "latest",
];

/// The nouns a number counts, which make a count volatile.
pub const COUNTED: &[&str] = &[
    "files", "tests", "items", "errors", "warnings", "commits", "nodes", "rows", "turns",
    "sessions", "tasks", "issues", "lines", "bytes", "users", "failures", "passed", "failed",
];

/// Durability by kind, and whether the operator wrote it (§2.6's table).
pub fn durability(kind: Kind, operator: bool) -> Durability {
    match (kind, operator) {
        (Kind::Preference | Kind::Decision, true) => Durability::High,
        (Kind::Preference | Kind::Decision, false) => Durability::Medium,
        (Kind::Fact | Kind::Procedure, _) => Durability::Medium,
        (Kind::Episode | Kind::Other, _) => Durability::Low,
        (Kind::Transient, _) => Durability::Floor,
    }
}

/// A node's labels: a `memory.labeled` row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Labels {
    pub kind: Kind,
    pub durability: Durability,
    /// The entity field's terms (`path:…`, `commit:…`), from the tender.
    pub about: Vec<String>,
    pub volatile: bool,
    /// What made it volatile.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub volatile_why: Vec<String>,
    /// DD5's `external` flag: `external`, or `own`.
    pub trust: &'static str,
    /// The operator wrote it as a correction of something said before.
    pub correction: bool,
}

/// The node's words, lowercased, as the rules read them: letters, digits,
/// and apostrophes (curly ones read straight), split on everything else.
pub fn words(text: &str) -> Vec<String> {
    let lower = text.to_lowercase().replace(['\u{2019}', '\u{2018}'], "'");
    lower
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || is_emoji(c)))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

fn is_emoji(c: char) -> bool {
    matches!(c, '👍' | '🙏')
}

/// Whether `words` hold `phrase` as whole words, in order.
pub fn has_phrase(words: &[String], phrase: &str) -> bool {
    let p: Vec<&str> = phrase.split(' ').collect();
    !p.is_empty()
        && words
            .windows(p.len())
            .any(|w| w.iter().zip(&p).all(|(a, b)| a == b))
}

fn any_phrase(words: &[String], phrases: &[&str]) -> bool {
    phrases.iter().any(|p| has_phrase(words, p))
}

/// Whether the operator's `text` corrects something said before.
pub fn is_correction(text: &str) -> bool {
    let w = words(text);
    if any_phrase(&w, CORRECTIONS) {
        return true;
    }
    // "No, …" opening a longer message; and "8082, not 8081".
    let lower = text.trim_start().to_lowercase();
    (lower.starts_with("no,") && w.len() > ACK_WORDS) || lower.contains(", not ")
}

/// The kind, by the table: a result is an episode; a short
/// acknowledgement transient; then the phrase rules; then a code block with
/// an instruction is a procedure; then a fact, or other.
pub fn kind(shape: Shape, origin: Origin, text: &str) -> Kind {
    if shape == Shape::Result {
        return Kind::Episode;
    }
    let w = words(text);
    if !w.is_empty() && w.len() <= ACK_WORDS && ACKS.contains(&w[0].as_str()) {
        return Kind::Transient;
    }
    let operator = origin == Origin::Operator && shape == Shape::Message;
    for rule in KIND_RULES {
        if (rule.who == Who::Anyone || operator) && any_phrase(&w, rule.phrases) {
            return rule.kind;
        }
    }
    if text.contains("```") && w.iter().any(|x| INSTRUCTIONS.contains(&x.as_str())) {
        return Kind::Procedure;
    }
    if w.len() >= FACT_WORDS {
        Kind::Fact
    } else {
        Kind::Other
    }
}

/// What makes `text` volatile, given its entities.
pub fn volatile(text: &str, about: &[String]) -> Vec<String> {
    let mut why = Vec::new();
    if about.iter().any(|e| e.starts_with("commit:")) {
        why.push("commit".to_string());
    }
    let w = words(text);
    for p in VOLATILE_WORDS {
        if has_phrase(&w, p) {
            why.push(format!("word:{p}"));
        }
    }
    let raw: Vec<&str> = text.split_whitespace().collect();
    if raw.iter().any(|t| is_version(t)) {
        why.push("version".into());
    }
    if raw.iter().any(|t| is_time(t)) {
        why.push("time".into());
    }
    if w.windows(2)
        .any(|p| p[0].chars().all(|c| c.is_ascii_digit()) && COUNTED.contains(&p[1].as_str()))
    {
        why.push("count".into());
    }
    why
}

/// `v1.2`, `1.2.3`, `0.26`: digits and dots with at least one dot, an
/// optional `v` before.
fn is_version(token: &str) -> bool {
    let t = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let (v, t) = match t.strip_prefix('v') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let parts: Vec<&str> = t.split('.').collect();
    let numeric = parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    // A bare decimal (`0.5`) is a number, not a version; three parts, or a
    // `v`, are.
    numeric && (v || parts.len() >= 3)
}

/// `14:32`, `9:05`, `14:32:10`.
fn is_time(token: &str) -> bool {
    let t = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let parts: Vec<&str> = t.split(':').collect();
    (2..=3).contains(&parts.len())
        && parts[0].len() <= 2
        && parts[1..].iter().all(|p| p.len() == 2)
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// The labels of a node, from its shape, who wrote it, its text, its
/// entities, and DD5's flag.
pub fn label(
    shape: Shape,
    origin: Origin,
    text: &str,
    about: Vec<String>,
    external: bool,
) -> Labels {
    let kind = kind(shape, origin, text);
    let operator = origin == Origin::Operator && shape == Shape::Message;
    let volatile_why = volatile(text, &about);
    Labels {
        kind,
        durability: durability(kind, operator),
        volatile: !volatile_why.is_empty(),
        volatile_why,
        about,
        trust: if external { "external" } else { "own" },
        correction: operator && is_correction(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §2.6's rules, a row each: the shape, who wrote it, the text, and the
    /// kind, durability, volatility, and correction it gets.
    #[test]
    fn the_labelers_rules_follow_the_table() {
        use Durability as D;
        use Kind as K;
        use Origin::{Agent, Harness, Operator, Tool};
        use Shape::{Message, Reply, Result, Summary};
        #[rustfmt::skip]
        let table: &[(Shape, Origin, &str, K, D, bool, bool)] = &[
            (Message, Operator, "Always run the tests before you commit.", K::Preference, D::High, false, false),
            (Message, Operator, "I prefer short commit subjects", K::Preference, D::High, false, false),
            (Message, Operator, "Please don’t push to main", K::Preference, D::High, false, false),
            (Message, Operator, "We decided to keep the WAL format", K::Decision, D::High, false, false),
            (Message, Operator, "let's go with the flat scan for now", K::Decision, D::High, true, false),
            (Reply, Agent, "I will always check first, never guess.", K::Fact, D::Medium, false, false),
            (Message, Operator, "To build it, run this:\n```\ncargo build -p theseus-heron\n```", K::Procedure, D::Medium, false, false),
            (Result, Tool, "ok: 12 tests passed", K::Episode, D::Low, true, false),
            (Message, Operator, "ok thanks", K::Transient, D::Floor, false, false),
            (Message, Operator, "👍", K::Transient, D::Floor, false, false),
            (Message, Operator, "The staging port of the Larkspur service is 8081.", K::Fact, D::Medium, false, false),
            (Message, Operator, "Correction: Larkspur's staging port is 8082, not 8081.", K::Fact, D::Medium, false, true),
            (Message, Operator, "Actually the heron job runs nightly at 02:30", K::Fact, D::Medium, true, true),
            (Message, Operator, "No, the bucket is in the second account, not the first.", K::Fact, D::Medium, false, true),
            (Message, Operator, "what next", K::Other, D::Low, false, false),
            (Message, Operator, "The release is v2.4 currently", K::Fact, D::Medium, true, false),
            (Reply, Agent, "Correction: I misread it, the port is 8082.", K::Fact, D::Medium, false, false),
            (Summary, Harness, "The operator chose the flat scan; we agreed to ship v2.4 on Friday.", K::Fact, D::Medium, true, false),
            (Summary, Harness, "We decided nothing yet.", K::Fact, D::Medium, false, false),
        ];
        for (shape, origin, text, k, d, vol, corr) in table {
            let l = label(*shape, *origin, text, vec![], false);
            assert_eq!(
                (l.kind, l.durability, l.volatile, l.correction),
                (*k, *d, *vol, *corr),
                "{text:?}: {l:?}"
            );
        }
    }

    /// Volatile values: a commit hash (from the entities), versions, times,
    /// counts, and words of time; a plain number is not one.
    #[test]
    fn volatile_values_are_named() {
        let v = |t: &str, about: &[&str]| {
            volatile(t, &about.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        };
        assert_eq!(v("merged it", &["commit:d069c4c"]), ["commit"]);
        assert_eq!(v("bumped to 1.98.1", &[]), ["version"]);
        assert_eq!(v("it ran at 14:32", &[]), ["time"]);
        assert_eq!(v("3 tests failed", &[]), ["count"]);
        assert_eq!(v("it is currently down", &[]), ["word:currently"]);
        assert!(v("the port is 8081 and the ratio 0.5", &[]).is_empty());
    }

    /// Trust is DD5's flag; `about` is what the tender named.
    #[test]
    fn trust_and_about_come_from_the_node() {
        let l = label(
            Shape::Result,
            Origin::Tool,
            "<html>",
            vec!["host:example.org".into()],
            true,
        );
        assert_eq!((l.trust, l.about.len()), ("external", 1));
        let l = label(
            Shape::Message,
            Origin::Operator,
            "hello there friend now",
            vec![],
            false,
        );
        assert_eq!(l.trust, "own");
    }
}
