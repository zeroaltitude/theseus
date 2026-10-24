//! What the job reads of the imported archive (theseus-slvl.6): the episodes
//! that are evidence about the owner, as lines of his own words.
//!
//! - **Which episodes.** By their labels, in this order: a USER-style file
//!   (`USER.md`, `user-…`), a feedback note (`feedback` in the file's path,
//!   the place's name or a topic), an SOP (`book_hint` `sop`), and a
//!   `decision_or_preference` triage. Anything else is not read at all.
//! - **Never partner-confidential.** The label (or a partner codename) is
//!   checked from the session's record, before its nodes are touched: the
//!   episode is counted in `withheld` and its text is not loaded.
//! - **Whose words.** A conversation (`decision`) gives the owner's own
//!   messages (`Integrity::Operator`) only. A note (a file the owner kept,
//!   or his earlier assistant's USER file about him) gives operator and
//!   agent text, never outside text.
//! - **Lines.** A note is split into its lines and bullets, a message into
//!   its paragraphs; each is cut to [`LINE_CHARS`], and an episode gives at
//!   most [`PER_EPISODE`].
//! - **Not an assistant's setup** (theseus-ly75u). A note's line addressed
//!   to an assistant about running itself ([`addressed`]: "you" as its reader, or a
//!   session's machinery) is no evidence about the owner; an assistant's
//!   setup file ([`setup_file`]: its soul, identity, bootstrap, agents,
//!   memory, heartbeat or tools file, or a dated daily log) gives only the
//!   lines whose subject is the owner ([`about_owner`]). Both are counted in
//!   [`Scan::setup`].
//! - **Not one task's** (theseus-ly75u). A line that says it holds for one
//!   task, repository, branch or run ([`one_task`]) is dropped and counted
//!   in [`Scan::one_task`]: the card holds what lasts.

use std::path::Path;
use theseus_protocol::term::words::cut_marked;

use anyhow::Result;

use crate::import::{shown, write, ImportedFrom, Integrity};
use crate::node::Body;
use crate::store::Store;

/// The longest line kept, in characters.
pub const LINE_CHARS: usize = 300;
/// The most lines one episode gives.
pub const PER_EPISODE: usize = 40;
/// The fewest words a line has.
pub const MIN_WORDS: usize = 4;

/// What an episode is evidence of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    User,
    Feedback,
    Sop,
    Decision,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::User => "user",
            Kind::Feedback => "feedback",
            Kind::Sop => "sop",
            Kind::Decision => "decision",
        }
    }

    /// A note, not a conversation: its agent text counts too.
    fn note(self) -> bool {
        !matches!(self, Kind::Decision)
    }
}

/// One line of the owner's words, and the node it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub node: String,
    pub at_ms: u64,
}

/// One episode that is evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    pub id: String,
    pub session_id: String,
    pub kind: Kind,
    pub end_ms: u64,
    /// The node that cites it: its summary, else its first line's.
    pub anchor: String,
    pub lines: Vec<Line>,
}

/// What a scan found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub episodes: Vec<Episode>,
    /// Evidence episodes left unread because they are partner-confidential.
    pub withheld: u64,
    /// Lines dropped as an assistant's setup, not the owner's words.
    pub setup: u64,
    /// Lines dropped as one task's, one repository's, branch's or run's.
    pub one_task: u64,
}

/// What an assistant's instructions to itself say: a session's machinery.
const MACHINERY: &[&str] = &[
    "read this file",
    "this file is",
    "each session",
    "every session",
    "new session",
    "session start",
    "when you wake",
    "wake up",
    "bootstrap",
    "heartbeat",
    "memory file",
    "daily log",
    "daily note",
    "long-term memory",
    "your human",
    "your owner",
    "your user",
];

/// Whether a line is addressed to an assistant about running itself: "you"
/// is its reader, or it names a session's machinery. An order the owner
/// gives ("Never commit before the gate is green") is not.
pub fn addressed(line: &str) -> bool {
    let l = line.to_lowercase();
    let first = l
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .find(|w| !w.is_empty())
        .unwrap_or("");
    matches!(
        first,
        "you" | "your" | "you're" | "youre" | "yourself" | "you'll"
    ) || MACHINERY.iter().any(|m| l.contains(m))
}

/// Whether a file, by its name, is an assistant's setup: its soul, identity,
/// bootstrap, agents, memory, heartbeat or tools file, or a dated daily log.
pub fn setup_file(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_lowercase();
    let dated = stem.len() >= 10
        && stem.chars().take(10).enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        });
    dated
        || matches!(
            stem.as_str(),
            "soul" | "identity" | "bootstrap" | "agents" | "memory" | "heartbeat" | "tools"
        )
}

/// Whether a line's subject is the owner: it starts with the owner, the
/// user, a third person, or a third-person verb ("Prefers", "Lives").
pub fn about_owner(line: &str) -> bool {
    let l = line.to_lowercase();
    let mut ws = l
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty());
    let first = ws.next().unwrap_or("");
    match first {
        "he" | "she" | "they" | "his" | "her" | "their" | "owner" | "user" => true,
        "the" | "my" => matches!(ws.next(), Some("owner" | "user" | "human")),
        "always" | "sometimes" | "perhaps" | "unless" | "thus" | "this" | "does" => false,
        w => {
            w.len() > 3
                && w.ends_with('s')
                && !w.ends_with("ss")
                && !w.ends_with("us")
                && !addressed(line)
        }
    }
}

/// What says a line holds for one task, repository, branch or run.
const ONE_TASK: &[&str] = &[
    "this task",
    "this repo",
    "this repository",
    "this branch",
    "this run",
    "this pr",
    "this pull request",
    "this ticket",
];

/// Whether a line says it holds for one task, repository, branch or run.
pub fn one_task(line: &str) -> bool {
    let l = format!(" {} ", line.to_lowercase());
    ONE_TASK.iter().any(|p| {
        l.match_indices(p).any(|(i, _)| {
            let after = l[i + p.len()..].chars().next().unwrap_or(' ');
            !after.is_alphanumeric()
        })
    })
}

/// Whether an episode's labels make it partner-confidential: the label, or a
/// partner codename.
pub fn partner(i: &ImportedFrom) -> bool {
    i.labels.sensitivity == "partner-confidential" || i.labels.partner.is_some()
}

/// What `i` is evidence of, by its labels alone.
pub fn classify(i: &ImportedFrom) -> Option<Kind> {
    let file = Path::new(&i.file)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let name = i.place.name.as_deref().unwrap_or("").to_lowercase();
    let user = |s: &str| {
        let stem = s.split('.').next().unwrap_or("");
        stem == "user" || s.starts_with("user-") || s.starts_with("user_")
    };
    if user(&file) || user(&name) {
        return Some(Kind::User);
    }
    let feedback = |s: &str| s.contains("feedback");
    // The whole path: a note in a `feedback/` directory is feedback.
    if feedback(&i.file.to_lowercase())
        || feedback(&name)
        || i.labels.topic.iter().any(|t| feedback(&t.to_lowercase()))
    {
        return Some(Kind::Feedback);
    }
    if i.labels.book_hint.as_deref() == Some("sop") {
        return Some(Kind::Sop);
    }
    if i.triage
        .as_ref()
        .is_some_and(|t| t.category == "decision_or_preference")
    {
        return Some(Kind::Decision);
    }
    None
}

/// The lines of one text: its bullets and paragraphs, markdown marks off.
pub fn lines_of(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut open = false;
    for raw in text.lines() {
        let t = raw.trim();
        if t.is_empty() || t.starts_with("```") || t.starts_with("---") {
            open = false;
            continue;
        }
        let bullet = t.starts_with(['-', '*', '•', '>'])
            || t.starts_with('#')
            || t.chars().take_while(char::is_ascii_digit).count() > 0
                && t.trim_start_matches(|c: char| c.is_ascii_digit())
                    .starts_with(['.', ')']);
        let clean = t
            .trim_start_matches(['-', '*', '•', '>', '#', ' '])
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ')', ' '])
            .replace("**", "");
        if clean.is_empty() {
            continue;
        }
        if open && !bullet {
            if let Some(last) = out.last_mut() {
                last.push(' ');
                last.push_str(&clean);
            }
        } else {
            out.push(clean);
            open = true;
        }
    }
    out.into_iter()
        .map(|l| cut(&l, LINE_CHARS))
        .filter(|l| l.split_whitespace().count() >= MIN_WORDS)
        .collect()
}

fn cut(s: &str, max: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    cut_marked(&one, max)
}

/// Read the archive's evidence. A read for a job, off every turn: it walks
/// the tags' session records, and the nodes of the evidence episodes alone.
pub fn scan(store: &Store) -> Result<Scan> {
    let mut out = Scan::default();
    for tag in write::list(store)?.tags {
        let sessions = write::sessions_of_tag(store, &tag.tag, &|| false)?.unwrap_or_default();
        for (sid, rec) in sessions {
            let Some(i) = rec.imported else { continue };
            if i.erased.is_some() {
                continue;
            }
            let Some(kind) = classify(&i) else { continue };
            // The label decides before a node is read.
            if partner(&i) {
                out.withheld += 1;
                continue;
            }
            if let Some(e) = episode(store, &sid, &i, kind, &mut out)? {
                out.episodes.push(e);
            }
        }
    }
    out.episodes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Whether a line of an episode is evidence about the owner: not an
/// assistant's setup, and not one task's. `note`: the episode is a file, not
/// a conversation (in a conversation "you" is the owner speaking to his
/// assistant); `setup`: its file is an assistant's setup file. What is
/// dropped is counted in `scan`.
pub fn kept(line: &str, note: bool, setup: bool, scan: &mut Scan) -> bool {
    if (note || setup) && (addressed(line) || (setup && !about_owner(line))) {
        scan.setup += 1;
        return false;
    }
    if one_task(line) {
        scan.one_task += 1;
        return false;
    }
    true
}

/// Whether an episode's file (its note's name, else its path) is an
/// assistant's setup file.
fn setup_of(i: &ImportedFrom) -> bool {
    let file = Path::new(&i.file)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    i.place.name.as_deref().is_some_and(setup_file) || setup_file(&file)
}

fn episode(
    store: &Store,
    sid: &str,
    i: &ImportedFrom,
    kind: Kind,
    scan: &mut Scan,
) -> Result<Option<Episode>> {
    let setup = setup_of(i);
    let note = kind.note() || i.place.kind == "file";
    let mut lines = Vec::new();
    let mut summary = None;
    for (_, n) in shown(store, store.session_nodes(sid)?)? {
        match n.body {
            Body::Imported {
                text, integrity, ..
            } => {
                let ok = match integrity {
                    Integrity::Operator => true,
                    Integrity::Agent => kind.note(),
                    Integrity::Outside => false,
                };
                if !ok {
                    continue;
                }
                for l in lines_of(&text) {
                    if !kept(&l, note, setup, scan) {
                        continue;
                    }
                    lines.push(Line {
                        text: l,
                        node: n.id.clone(),
                        at_ms: n.created_at_ms,
                    });
                }
            }
            Body::ImportedSummary { .. } => summary = Some(n.id),
            _ => {}
        }
    }
    lines.truncate(PER_EPISODE);
    let Some(first) = lines.first() else {
        return Ok(None);
    };
    Ok(Some(Episode {
        id: i.episode_id.clone(),
        session_id: sid.to_string(),
        kind,
        end_ms: i.as_of.end_ms,
        anchor: summary.unwrap_or_else(|| first.node.clone()),
        lines,
    }))
}
