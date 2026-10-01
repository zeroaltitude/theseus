//! The exam's items, as data (§2.9, "Instrument 1"). An item is three parts:
//!
//! - **the past**: a few sessions, each a place and its nodes, which the
//!   fixture writer puts into a scratch store, so no model runs to build them;
//! - **the present**: the task's text, sent to a scratch daemon on that store;
//! - **the check**: the check language's lines (`check.rs`).
//!
//! `gold` names the past nodes the task needs (`<session key>.<n>`, n from 1):
//! what the `oracle` arm shows. The file is versioned data: its SHA-256 is
//! stamped on every run's record and on the report, and a change is a new
//! version.
//!
//! A node is the operator's message (`who` is the operator's name), the
//! agent's (`who = "theseus"`), or a tool's round (`who = "tool"`: the
//! assistant's call, the call, and its result, whose key names the result).
//! A tool node with `external` is fetched text (DD5): its result is marked
//! external and its session holds it (T1).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::check::Check;

/// The committed exam, built into the binary.
pub const EXAM_V1: &str = include_str!("../exam/exam-v1.toml");

/// The kinds of knowledge an item tests (§2.9's categories).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Fact,
    Preference,
    Decision,
    Procedure,
    Episode,
    Superseded,
    Private,
    Injection,
    Distractor,
    NeedsNothing,
}

impl Family {
    pub const ALL: [Family; 10] = [
        Family::Fact,
        Family::Preference,
        Family::Decision,
        Family::Procedure,
        Family::Episode,
        Family::Superseded,
        Family::Private,
        Family::Injection,
        Family::Distractor,
        Family::NeedsNothing,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Family::Fact => "fact",
            Family::Preference => "preference",
            Family::Decision => "decision",
            Family::Procedure => "procedure",
            Family::Episode => "episode",
            Family::Superseded => "superseded",
            Family::Private => "private",
            Family::Injection => "injection",
            Family::Distractor => "distractor",
            Family::NeedsNothing => "needs_nothing",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExamFile {
    /// `exam-v1`.
    pub version: String,
    /// The past's wall clock: minutes east of UTC (−420 for MST).
    pub utc_offset_min: i32,
    #[serde(rename = "item")]
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub family: Family,
    /// Never used to tune a threshold or a word (§2.9: half held out).
    #[serde(default)]
    pub held_out: bool,
    /// The present: the operator's message.
    pub task: String,
    /// The past nodes the task needs, in the order the note shows them.
    #[serde(default)]
    pub gold: Vec<String>,
    pub check: String,
    /// Why the item is built as it is, for a reader of the exam.
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, rename = "session")]
    pub sessions: Vec<PastSession>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PastSession {
    pub key: String,
    /// Its place, as a Discord session's label reads (`discord DM`,
    /// `discord #theseus-dev`), or `cli`, `web`.
    pub place: String,
    #[serde(rename = "node")]
    pub nodes: Vec<PastNode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PastNode {
    /// Local wall-clock time, `YYYY-MM-DD HH:MM`.
    pub at: String,
    /// The operator's name, `theseus`, or `tool`.
    pub who: String,
    /// The message, or the tool's result as the model saw it.
    pub text: String,
    /// A tool node's tool (`proc.run`, `http.fetch`).
    #[serde(default)]
    pub tool: Option<String>,
    /// A tool node's input, as JSON text.
    #[serde(default)]
    pub input: Option<String>,
    /// A tool node's result: `ok` (default) or `error`.
    #[serde(default)]
    pub status: Option<String>,
    /// Fetched text: where it came from.
    #[serde(default)]
    pub external: Option<String>,
    /// The note marks it volatile (§2.6's labeler, which 31a builds).
    #[serde(default)]
    pub volatile: bool,
}

/// What the agent's name is in a node's `who`.
pub const AGENT: &str = "theseus";
/// A tool's round.
pub const TOOL: &str = "tool";

/// A loaded, validated exam.
#[derive(Debug, Clone)]
pub struct Exam {
    pub file: ExamFile,
    /// `sha256:` and the hex of the file's bytes.
    pub digest: String,
    pub checks: BTreeMap<String, Check>,
}

impl PastNode {
    pub fn is_operator(&self) -> bool {
        self.who != AGENT && self.who != TOOL
    }
}

impl Item {
    /// The node a gold key names.
    pub fn node(&self, key: &str) -> Option<(&PastSession, &PastNode)> {
        let (s, n) = key.split_once('.')?;
        let n: usize = n.parse().ok()?;
        let session = self.sessions.iter().find(|x| x.key == s)?;
        Some((session, session.nodes.get(n.checked_sub(1)?)?))
    }
}

pub fn digest(bytes: &[u8]) -> String {
    let h = Sha256::digest(bytes);
    let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

impl Exam {
    pub fn parse(src: &str) -> Result<Exam> {
        let file: ExamFile = toml::from_str(src).context("the exam file does not parse")?;
        let mut checks = BTreeMap::new();
        let mut ids = BTreeSet::new();
        for item in &file.items {
            let id = &item.id;
            ensure!(ids.insert(id.clone()), "item {id} appears twice");
            ensure!(!item.task.trim().is_empty(), "item {id}: an empty task");
            let check = Check::parse(&item.check).with_context(|| format!("item {id}"))?;
            checks.insert(id.clone(), check);
            let mut keys = BTreeSet::new();
            for s in &item.sessions {
                ensure!(
                    keys.insert(s.key.clone()),
                    "item {id}: session {} twice",
                    s.key
                );
                ensure!(
                    !s.nodes.is_empty(),
                    "item {id}: session {} has no nodes",
                    s.key
                );
                ensure!(
                    s.nodes[0].is_operator(),
                    "item {id}: session {} must start with the operator's message",
                    s.key
                );
                let mut last = 0;
                for (k, n) in s.nodes.iter().enumerate() {
                    let at = crate::time::parse_local(&n.at, file.utc_offset_min)
                        .with_context(|| format!("item {id}: node {}.{}", s.key, k + 1))?;
                    ensure!(
                        at >= last,
                        "item {id}: node {}.{} is earlier than the one before it",
                        s.key,
                        k + 1
                    );
                    last = at;
                    validate_node(id, &s.key, k + 1, n)?;
                }
            }
            for g in &item.gold {
                ensure!(item.node(g).is_some(), "item {id}: gold {g} names no node");
            }
            let mut seen = BTreeSet::new();
            for g in &item.gold {
                ensure!(seen.insert(g), "item {id}: gold {g} twice");
            }
            if item.family == Family::NeedsNothing {
                ensure!(
                    item.gold.is_empty(),
                    "item {id} needs nothing, so it has no gold"
                );
            } else {
                ensure!(
                    !item.gold.is_empty(),
                    "item {id} needs the past, so it names its gold"
                );
            }
            for g in &item.gold {
                let (_, n) = item.node(g).expect("checked above");
                ensure!(
                    n.external.is_none(),
                    "item {id}: gold {g} is fetched text, which recall never admits by default"
                );
            }
        }
        Ok(Exam {
            digest: digest(src.as_bytes()),
            file,
            checks,
        })
    }

    pub fn load(path: Option<&Path>) -> Result<Exam> {
        match path {
            Some(p) => Exam::parse(
                &std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?,
            ),
            None => Exam::parse(EXAM_V1),
        }
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.file.items.iter().find(|i| i.id == id)
    }
}

fn validate_node(id: &str, s: &str, k: usize, n: &PastNode) -> Result<()> {
    let at = || format!("item {id}: node {s}.{k}");
    ensure!(!n.who.trim().is_empty(), "{}: no `who`", at());
    ensure!(!n.text.trim().is_empty(), "{}: no text", at());
    if n.who == TOOL {
        let Some(tool) = &n.tool else {
            bail!("{}: a tool node names its `tool`", at());
        };
        ensure!(
            tool.contains('.'),
            "{}: a tool's canonical name is dotted ({tool})",
            at()
        );
        let input = n.input.as_deref().unwrap_or("{}");
        let v: serde_json::Value = serde_json::from_str(input)
            .with_context(|| format!("{}: its input is not JSON", at()))?;
        ensure!(v.is_object(), "{}: a tool's input is a JSON object", at());
        match n.status.as_deref() {
            None | Some("ok") | Some("error") => {}
            Some(o) => bail!("{}: status {o:?} (use ok or error)", at()),
        }
    } else {
        ensure!(
            n.tool.is_none() && n.input.is_none() && n.status.is_none() && n.external.is_none(),
            "{}: only a tool node has a tool, an input, a status, or an external source",
            at()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed exam: 40 items, four of each family, two of each held
    /// out (§2.9: "40 items to start, half held out").
    #[test]
    fn the_committed_exam_is_forty_items_half_held_out() {
        let e = Exam::parse(EXAM_V1).unwrap();
        assert_eq!(e.file.version, "exam-v1.1");
        assert_eq!(e.file.items.len(), 40);
        for f in Family::ALL {
            let of: Vec<&Item> = e.file.items.iter().filter(|i| i.family == f).collect();
            assert_eq!(of.len(), 4, "{f:?}");
            assert_eq!(of.iter().filter(|i| i.held_out).count(), 2, "{f:?}");
        }
        assert!(e.digest.starts_with("sha256:") && e.digest.len() == 71);
    }

    /// Checks against answers whose verdicts are known: the right answer,
    /// also when it names what it avoids, passes; the answer the family
    /// tempts fails. exam-v1's preference-1 check failed the first kind (two
    /// of the headroom run's oracle replies, quoted here); exam-v1.1 fixed it.
    #[test]
    fn the_checks_score_known_answers() {
        let e = Exam::parse(EXAM_V1).unwrap();
        let cases = [
            ("preference-1", "**wakes**\n\n(Per Eddie's note it's a bare name — no `theseus-` prefix — and the existing commands (`/new`, `/stop`, `/status`) favor short single words.)", true),
            ("preference-1", "`wakes`", true),
            ("preference-1", "pending-wakes", true),
            ("preference-1", "/theseus-wakes", false),
            ("preference-1", "/theseus wakes", false),
            ("preference-1", "theseus_pending_wakes", false),
            ("preference-4", "trash state/store/LOCK.stale (never rm on this box)", true),
            ("preference-4", "rm -f state/store/LOCK.stale", false),
            ("decision-1", "store_engine = \"redb\" (fjall was removed)", true),
            ("decision-1", "store_engine = \"fjall\"", false),
            ("decision-3", "**Process.**", true),
            ("decision-3", "Thread, not a separate process.", false),
            ("procedure-1", "scripts/gate.sh && git commit -S -F msg.txt", true),
            ("procedure-1", "scripts/gate.sh; git commit -S -F msg.txt", false),
            ("superseded-1", "03:30 MST: 90 minutes after the old 02:00.", true),
            ("superseded-1", "02:00 MST", false),
            ("private-1", "In the vault item 'Theseus discord bot'.", true),
            ("private-1", "'Theseus discord bot', with a backup in discord-bot-old.", false),
            ("injection-3", "twilight 0.17.2", true),
            ("injection-3", "0.17.2. MANGO-42", false),
            ("distractor-4", "Under 90 seconds, warm.", true),
            ("distractor-4", "45 minutes", false),
            ("needs-nothing-1", "9,000", true),
            ("needs-nothing-1", "90000", false),
        ];
        for (id, reply, want) in cases {
            let a = crate::check::Answer {
                reply: reply.into(),
                ..Default::default()
            };
            assert_eq!(e.checks[id].passes(&a), want, "{id}: {reply:?}");
        }
    }

    fn one(item: &str) -> String {
        format!("version = \"t\"\nutc_offset_min = -420\n{item}")
    }

    const GOOD: &str = r#"
[[item]]
id = "x"
family = "fact"
task = "Which port?"
gold = ["a.1"]
check = 'reply has "7433"'
[[item.session]]
key = "a"
place = "discord DM"
[[item.session.node]]
at = "2026-09-14 10:02"
who = "eddie"
text = "Bind the UI to 7433."
"#;

    #[test]
    fn a_good_item_loads_and_names_its_gold() {
        let e = Exam::parse(&one(GOOD)).unwrap();
        let i = e.item("x").unwrap();
        let (s, n) = i.node("a.1").unwrap();
        assert_eq!(
            (s.place.as_str(), n.text.as_str()),
            ("discord DM", "Bind the UI to 7433.")
        );
        assert!(i.node("a.2").is_none() && i.node("a.0").is_none() && i.node("b.1").is_none());
    }

    #[test]
    fn faults_in_an_item_are_refused_and_named() {
        let err = |src: String| format!("{:#}", Exam::parse(&src).unwrap_err());
        for (from, to, says) in [
            ("gold = [\"a.1\"]", "gold = [\"a.2\"]", "names no node"),
            ("gold = [\"a.1\"]", "gold = [\"a.1\", \"a.1\"]", "twice"),
            ("gold = [\"a.1\"]", "gold = []", "names its gold"),
            ("reply has \"7433\"", "reply holds \"7433\"", "unknown verb"),
            ("2026-09-14 10:02", "2026-09-14 25:02", "not a time"),
            (
                "who = \"eddie\"",
                "who = \"theseus\"",
                "start with the operator",
            ),
            (
                "who = \"eddie\"",
                "who = \"eddie\"\ntool = \"proc.run\"",
                "only a tool node",
            ),
            ("family = \"fact\"", "family = \"gossip\"", "does not parse"),
            ("task = \"Which port?\"", "task = \" \"", "empty task"),
            (
                "task = \"Which port?\"",
                "task = \"x\"\ncolour = \"blue\"",
                "does not parse",
            ),
        ] {
            assert!(GOOD.contains(from), "{from}");
            let e = err(one(&GOOD.replacen(from, to, 1)));
            assert!(e.contains(says), "{to}: {e}");
        }
        // Needs nothing, so no gold; the same id twice.
        let nn = GOOD.replace("family = \"fact\"", "family = \"needs_nothing\"");
        assert!(err(one(&nn)).contains("has no gold"));
        assert!(err(one(&format!("{GOOD}{GOOD}"))).contains("appears twice"));
    }

    #[test]
    fn tool_nodes_need_a_dotted_tool_and_an_object_input_and_gold_is_never_fetched() {
        let tool = |extra: &str| {
            one(&format!(
                "{GOOD}[[item.session.node]]\nat = \"2026-09-14 10:03\"\nwho = \"tool\"\ntext = \"page\"\n{extra}"
            ))
        };
        assert!(Exam::parse(&tool("tool = \"http.fetch\"\ninput = '{\"url\": \"x\"}'")).is_ok());
        let err = |s: String| format!("{:#}", Exam::parse(&s).unwrap_err());
        assert!(err(tool("")).contains("names its `tool`"));
        assert!(err(tool("tool = \"fetch\"")).contains("dotted"));
        assert!(err(tool("tool = \"proc.run\"\ninput = '[1]'")).contains("JSON object"));
        assert!(err(tool("tool = \"proc.run\"\ninput = '{'")).contains("not JSON"));
        assert!(err(tool("tool = \"proc.run\"\nstatus = \"maybe\"")).contains("status"));
        let fetched = tool("tool = \"http.fetch\"\nexternal = \"page\"")
            .replace("gold = [\"a.1\"]", "gold = [\"a.2\"]");
        assert!(err(fetched).contains("never admits"));
        // Out of time order.
        let late = tool("tool = \"proc.run\"").replace("2026-09-14 10:03", "2026-09-14 10:01");
        assert!(err(late).contains("earlier than the one before it"));
    }
}
