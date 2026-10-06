//! System labels (design §2.9's table), derived by each report's run from
//! the record, deterministically, at weight 0.5, each keyed by its
//! judgment, question, and rule (`labels::system_key`), so a second run
//! finds it written and writes nothing. A rule whose window is still open
//! (the 10 minutes after a turn, the 24 hours after a task) waits for a
//! later run; one that finds nothing writes nothing.
//!
//! - **`loop.v1`**, judged where the baseline ended a turn with no tool
//!   calls:
//!   - *stopped too early* (`continuation`): the session's next message
//!     from a person, within 10 minutes of the turn's last node, is a
//!     continuation phrase ([`continuation`]): `work_state` is
//!     `progressing`;
//!   - *false completion* (`false_completion`): a task whose brief is
//!     near-identical (`learn::similarity` at `learn::NEAR_IDENTICAL` or
//!     more) to the judged task's began within 24 hours after its turn:
//!     `work_state` is `{"not": "complete"}`;
//!   - a turn the budget ended is never "should have stopped" (§2, LOOP
//!     FOREVER): no rule writes that label, and a judgment whose baseline
//!     decision is `budget_exhausted` takes none.
//! - **`security.*`**, from the judged call's action: a waiting call the
//!   operator declined (`declined`) is `risky`; one approved (`approved`),
//!   with no operator label on the judgment, is not. An expiry is no one's
//!   answer, and labels nothing.
//! - **`classify.v1`**: whether the model called `task.create` in the turn
//!   the message started (`task_create`): `should_promote`, once that turn
//!   has ended (a later node in the session, or an hour gone).
//!
//! - **`route.v1`** (25e): the owner chose a profile (`ask -P`, the
//!   cockpit's picker: a pinned turn, whose `route.v1` judgment says so in
//!   its context) for the session's next message, within 10 minutes after a
//!   routed turn (`chosen`): that turn's `mode` was wrong, `{"not": mode}`;
//!   and where the chosen profile is in exactly one other mode's list, that
//!   mode was the right answer. A choice that names the routed mode's own
//!   list says nothing of the mode, and labels nothing.
//!
//! `role.v1` and `continue.v1` take none in M5 (§2.9), nor do nudges (26a)
//! or a slash command (25a judges none).
//!
//! **What a run reads** (theseus-cf5c). A judgment all of whose windows
//! closed before the last run ([`Cut`]: the last run read every row up to
//! its `through`, and its time less the judgment's longest window and
//! [`MARGIN_MS`] is past the judgment's) can take no new label: that run
//! already wrote what it could. So its session's nodes, its call's action
//! and its task's briefs are never read again, and a run reads about what
//! can still change, not all of history. The windows ([`open_ms`]): `loop`'s
//! continuation 10 minutes, or a task's false completion 24 hours;
//! `classify`'s a day (its turn's end, an hour after the turn's last node,
//! bounded by a turn's day); `security`'s `[kernel] confirm_ttl_secs`, how
//! long a call can wait before it expires; `route`'s 10 minutes, cut too
//! though it reads only the scope, since each routed judgment walks the
//! scope for its next one. The first run (no mark, or one without
//! `through`) walks everything.

use std::collections::HashMap;

use serde_json::{json, Value};
use theseus_judge::learn::{similarity, NEAR_IDENTICAL};
use theseus_judge::Outcome;
use theseus_store::NewRecord;

use super::labels::system_key;
use super::{Scope, Seen, SYSTEM_WEIGHT};
use crate::node::{Body, Node, Origin};
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The continuation phrases (§2.9's closed list).
pub const PHRASES: [&str; 4] = ["continue", "go on", "keep going", "you didn't finish"];

/// How soon after a turn its continuation counts.
pub const CONTINUATION_MS: u64 = 10 * 60 * 1000;
/// How soon after a routed turn the owner's choice of a profile labels it.
pub const CHOSEN_MS: u64 = 10 * 60 * 1000;
/// How soon after a task a near-identical one is a false completion.
pub const FALSE_COMPLETION_MS: u64 = 24 * 60 * 60 * 1000;
/// When a classified turn counts as ended with no later node.
const TURN_OVER_MS: u64 = 60 * 60 * 1000;
/// Words a continuation message has at most: past them it is a new ask
/// that begins with a phrase ("continue the docs, and then …").
const CONTINUATION_WORDS: usize = 6;

/// Whether a person's message is a continuation phrase: lowercased, its
/// apostrophes made plain and its other punctuation dropped, it begins
/// with one of [`PHRASES`] and has at most a few words.
pub fn continuation(text: &str) -> bool {
    let plain: String = text
        .to_lowercase()
        .replace(['\u{2019}', '\u{2018}'], "'")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let words: Vec<&str> = plain.split_whitespace().collect();
    if words.is_empty() || words.len() > CONTINUATION_WORDS {
        return false;
    }
    let joined = words.join(" ");
    let did_not = joined.replace("did not", "didn't");
    PHRASES.iter().any(|p| {
        [&joined, &did_not]
            .iter()
            .any(|j| j.as_str() == *p || j.starts_with(&format!("{p} ")))
    })
}

/// One system label to write.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemLabel {
    pub id: String,
    pub judgment: String,
    pub pack: String,
    pub session: Option<String>,
    pub question: String,
    /// A per-item question's item key (rerank.v1's, 32d).
    pub about: Option<String>,
    pub label: Value,
    pub rule: &'static str,
    pub note: String,
}

/// The margin past a judgment's longest window before the next run cuts
/// it: a late row, a clock's step, a run's own minutes.
pub const MARGIN_MS: u64 = 60 * 60 * 1000;
/// How long a classified turn may run before its label is settled.
const CLASSIFY_OPEN_MS: u64 = 24 * 60 * 60 * 1000;

/// Where the last run left off: every judgment row it read (positions up
/// to `through`), and when it ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut {
    pub through: u64,
    pub at_ms: u64,
}

impl Cut {
    /// The last run's mark (`LAST_RUN`), when it says how far it read.
    pub fn of(mark: &Value) -> Option<Cut> {
        Some(Cut {
            through: mark.get("through")?.as_u64()?,
            at_ms: mark.get("at_unix_ms")?.as_u64()?,
        })
    }
}

/// What a run's rules read beyond the scopes, and what they left closed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RulesRead {
    /// Sessions whose nodes were read.
    pub sessions: usize,
    /// Kernel actions read (a security judgment's call).
    pub actions: usize,
    /// Task sessions whose first node was read.
    pub tasks: usize,
    /// Judgments left unread, their windows closed before the last run.
    pub closed: usize,
}

impl std::ops::AddAssign for RulesRead {
    fn add_assign(&mut self, o: Self) {
        self.sessions += o.sessions;
        self.actions += o.actions;
        self.tasks += o.tasks;
        self.closed += o.closed;
    }
}

/// What the rules read beyond the judge's scopes, read once a run and only
/// for the judgments that need them.
#[derive(Default)]
struct Reads {
    nodes: HashMap<String, Vec<Node>>,
    tasks: Option<Vec<(u64, String, String)>>,
    /// The earliest open task judgment's time less the margin: no task that
    /// began before it can be a false completion still open.
    tasks_from: u64,
    count: RulesRead,
}

/// How long after a judgment a label can still come of it, by its pack's
/// rules; `None` for a pack whose labels have no window of time.
pub fn open_ms(pack_id: &str, s: &Seen, confirm_ttl_ms: u64) -> Option<u64> {
    match pack_id {
        "loop" if s.context("class") == Some("task") => Some(FALSE_COMPLETION_MS),
        "loop" => Some(CONTINUATION_MS),
        "classify" => Some(CLASSIFY_OPEN_MS),
        "security" => Some(confirm_ttl_ms),
        "route" => Some(CHOSEN_MS),
        _ => None,
    }
}

impl Core {
    /// The system labels a run derives from a pack's scope that are not yet
    /// written, every judgment read (a test's: a run reads past its cut).
    #[cfg(test)]
    pub(crate) fn system_labels(
        &self,
        pack_id: &str,
        scope: &Scope,
        now_ms: u64,
    ) -> Vec<SystemLabel> {
        self.system_labels_after(pack_id, scope, now_ms, None).0
    }

    /// `system_labels`, past the last run's [`Cut`]: a judgment whose
    /// windows all closed before it is not read (theseus-cf5c). With what
    /// the rules read.
    pub(crate) fn system_labels_after(
        &self,
        pack_id: &str,
        scope: &Scope,
        now_ms: u64,
        cut: Option<Cut>,
    ) -> (Vec<SystemLabel>, RulesRead) {
        // rerank.v1's come from the owner's memory labels (32d), one a label.
        if pack_id == "rerank" {
            let mut out = self.rerank_labels(scope);
            out.retain(|l| !scope.label_ids.contains(&l.id));
            return (out, RulesRead::default());
        }
        let ttl = self.kernel.config().confirm_ttl_ms;
        let closed = |s: &Seen| {
            let (Some(cut), Some(open)) = (cut, open_ms(pack_id, s, ttl)) else {
                return false;
            };
            s.position <= cut.through
                && s.at_ms.saturating_add(open).saturating_add(MARGIN_MS) < cut.at_ms
        };
        let mut reads = Reads::default();
        let mut open = Vec::new();
        for seen in scope.judgments.values().flatten() {
            if seen.judgment.outcome != Outcome::Answered {
                continue;
            }
            if closed(seen) {
                reads.count.closed += 1;
            } else {
                open.push(seen);
            }
        }
        reads.tasks_from = open
            .iter()
            .filter(|s| pack_id == "loop" && s.context("class") == Some("task"))
            .map(|s| s.at_ms)
            .min()
            .unwrap_or(u64::MAX)
            .saturating_sub(MARGIN_MS);
        let mut out = Vec::new();
        for seen in open {
            let found = match pack_id {
                "loop" => self.loop_labels(seen, now_ms, &mut reads),
                "security" => self.security_labels(seen, scope, &mut reads),
                "classify" => self.classify_labels(seen, now_ms, &mut reads),
                "route" => self.route_labels(seen, scope),
                _ => Vec::new(),
            };
            out.extend(
                found
                    .into_iter()
                    .filter(|l| !scope.label_ids.contains(&l.id)),
            );
        }
        (out, reads.count)
    }

    fn learning_nodes<'a>(&self, reads: &'a mut Reads, session: &str) -> &'a [Node] {
        let count = &mut reads.count;
        reads
            .nodes
            .entry(session.to_string())
            .or_insert_with(|| {
                count.sessions += 1;
                self.store
                    .session_nodes(session)
                    .map(|v| v.into_iter().map(|(_, n)| n).collect())
                    .unwrap_or_default()
            })
            .as_slice()
    }

    fn loop_labels(&self, s: &Seen, now: u64, reads: &mut Reads) -> Vec<SystemLabel> {
        let j = &s.judgment;
        let (Some(session), Some(turn)) = (s.context("session"), s.context("turn")) else {
            return vec![];
        };
        // §2 LOOP FOREVER: a turn the budget ended is never "should have
        // stopped", and takes no system label.
        if s.context("decision") != Some("no_tool_calls") {
            return vec![];
        }
        let label = |question: &str, label, rule, note: String| SystemLabel {
            id: system_key(&j.id, question, rule),
            judgment: j.id.clone(),
            pack: j.pack.clone(),
            session: Some(session.to_string()),
            question: question.to_string(),
            about: None,
            label,
            rule,
            note,
        };
        let nodes = self.learning_nodes(reads, session);
        let Some(end) = nodes
            .iter()
            .filter(|n| n.turn_id.as_deref() == Some(turn))
            .map(|n| n.created_at_ms)
            .max()
        else {
            return vec![];
        };
        let mut out = Vec::new();
        let next = nodes.iter().find(|n| {
            n.created_at_ms > end
                && n.turn_id.as_deref() != Some(turn)
                && n.origin == Origin::Operator
                && matches!(n.body, Body::UserMessage { .. })
        });
        if let Some(Node {
            body: Body::UserMessage { text, .. },
            created_at_ms,
            ..
        }) = next
        {
            let after = created_at_ms - end;
            if after <= CONTINUATION_MS && continuation(text) {
                out.push(label(
                    "work_state",
                    json!("progressing"),
                    "continuation",
                    format!(
                        "stopped too early: the next message, {} s after the turn, was {:?}",
                        after / 1000,
                        text.trim()
                    ),
                ));
            }
        }
        if s.context("class") == Some("task") {
            let brief = first_message(nodes);
            if let Some(brief) = brief.filter(|b| !b.trim().is_empty()) {
                let tasks = self.task_briefs(reads);
                let again = tasks.iter().find(|(at, id, text)| {
                    id != session
                        && *at > end
                        && *at <= end + FALSE_COMPLETION_MS
                        && similarity(&brief, text) >= NEAR_IDENTICAL
                });
                if let Some((at, id, _)) = again {
                    out.push(label(
                        "work_state",
                        json!({"not": "complete"}),
                        "false_completion",
                        format!(
                            "false completion: task {id}, {} min later, has a near-identical brief",
                            (at - end) / 60_000
                        ),
                    ));
                }
            }
        }
        let _ = now;
        out
    }

    /// Every task session's start and brief, read once a run: the newest
    /// sessions by birth back to `tasks_from`, through the index's births;
    /// every session while the index's shape is built.
    fn task_briefs<'a>(&self, reads: &'a mut Reads) -> &'a [(u64, String, String)] {
        let (from, count) = (reads.tasks_from, &mut reads.count);
        reads.tasks.get_or_insert_with(|| {
            let sessions = self.sessions_from(from).unwrap_or_default();
            sessions
                .into_iter()
                .filter(|r| r.task.is_some())
                .filter_map(|r| {
                    count.tasks += 1;
                    let (_, n) = self.store.first_node(&r.session_id).ok()??;
                    let Body::UserMessage { text, .. } = n.body else {
                        return None;
                    };
                    Some((n.created_at_ms, r.session_id, text))
                })
                .collect()
        })
    }

    /// The sessions created at `from` or after, newest first, by the
    /// index's births; all of them while it keeps none.
    fn sessions_from(&self, from: u64) -> anyhow::Result<Vec<SessionRecord>> {
        use theseus_store::Store as _;
        let mut out = Vec::new();
        let mut before = None;
        loop {
            let newest =
                self.store
                    .inner()
                    .newest_keys(theseus_store::kinds::SESSION, before, 64)?;
            let Some((born, more)) = newest else {
                return self.store.list_sessions();
            };
            for (_, r) in &born {
                let rec: SessionRecord = r.decode()?;
                // An imported session is born at its import with its
                // episode's old time (theseus-0lrr.6): never a task's, and
                // never the walk's end.
                if rec.imported.is_some() {
                    continue;
                }
                if rec.created_at_unix_ms < from {
                    return Ok(out);
                }
                out.push(rec);
            }
            match (more, born.last()) {
                (true, Some((b, _))) => before = Some(*b),
                _ => return Ok(out),
            }
        }
    }

    fn security_labels(&self, s: &Seen, scope: &Scope, reads: &mut Reads) -> Vec<SystemLabel> {
        let j = &s.judgment;
        let Some(call) = s.context("call") else {
            return vec![];
        };
        reads.count.actions += 1;
        let Ok(Some(a)) = self.kernel.action(call) else {
            return vec![];
        };
        let declined = a
            .resolution
            .as_deref()
            .and_then(|r| r.strip_prefix("declined by "))
            .filter(|by| !by.starts_with(&format!("{}:", crate::rpc::EXPIRY)));
        let complained = scope
            .labels
            .get(&j.id)
            .is_some_and(|ls| ls.iter().any(|l| l.source == "operator"));
        let (risky, rule, note) = match (declined, &a.confirm) {
            (Some(by), _) => (
                true,
                "declined",
                format!(
                    "the waiting call was declined by {}",
                    by.split(':').next().unwrap_or(by)
                ),
            ),
            (None, Some(c)) if !complained => (
                false,
                "approved",
                format!(
                    "the waiting call was approved by {}, without complaint",
                    c.by
                ),
            ),
            _ => return vec![],
        };
        vec![SystemLabel {
            id: system_key(&j.id, "risky", rule),
            judgment: j.id.clone(),
            pack: j.pack.clone(),
            session: s.context("session").map(str::to_string),
            question: "risky".into(),
            about: None,
            label: json!(risky),
            rule,
            note,
        }]
    }

    fn classify_labels(&self, s: &Seen, now: u64, reads: &mut Reads) -> Vec<SystemLabel> {
        let j = &s.judgment;
        let (Some(session), Some(turn)) = (s.context("session"), s.context("turn")) else {
            return vec![];
        };
        let nodes = self.learning_nodes(reads, session);
        let of_turn = |n: &&Node| n.turn_id.as_deref() == Some(turn);
        let called = nodes
            .iter()
            .filter(of_turn)
            .any(|n| matches!(&n.body, Body::ToolCall { tool, .. } if tool == crate::task::CREATE));
        let last = nodes.iter().filter(of_turn).map(|n| n.created_at_ms).max();
        let over = called
            || last.is_some_and(|end| {
                now.saturating_sub(end) >= TURN_OVER_MS
                    || nodes
                        .iter()
                        .any(|n| n.created_at_ms > end && n.turn_id.as_deref() != Some(turn))
            });
        if !over {
            return vec![];
        }
        vec![SystemLabel {
            id: system_key(&j.id, "should_promote", "task_create"),
            judgment: j.id.clone(),
            pack: j.pack.clone(),
            session: Some(session.to_string()),
            question: "should_promote".into(),
            about: None,
            label: json!(called),
            rule: "task_create",
            note: if called {
                "the model called task.create in the turn".into()
            } else {
                "the model did not call task.create in the turn".into()
            },
        }]
    }
}

impl Core {
    /// `route.v1`'s rule (`chosen`): the session's next judged message, a
    /// pinned one within [`CHOSEN_MS`] after a routed turn, labels its mode.
    fn route_labels(&self, s: &Seen, scope: &Scope) -> Vec<SystemLabel> {
        let j = &s.judgment;
        let routed = j.mode != theseus_judge::Mode::Shadow
            && j.context.get("pinned").and_then(Value::as_bool) != Some(true);
        let (Some(session), true) = (s.context("session"), routed) else {
            return vec![];
        };
        let Some(mode) = j.answer("mode").and_then(|a| match &a.answer {
            theseus_judge::Answer::Choice { choice, .. } => Some(choice.clone()),
            _ => None,
        }) else {
            return vec![];
        };
        let next = scope
            .judgments
            .values()
            .flatten()
            .filter(|o| o.context("session") == Some(session) && o.at_ms > s.at_ms)
            .min_by_key(|o| o.at_ms);
        let Some(next) = next.filter(|o| o.at_ms - s.at_ms <= CHOSEN_MS) else {
            return vec![];
        };
        if next.judgment.context.get("pinned").and_then(Value::as_bool) != Some(true) {
            return vec![];
        }
        let chosen = next.context("chosen").unwrap_or_default();
        let profile = chosen
            .split(", ")
            .find_map(|c| c.strip_prefix("profile "))
            .unwrap_or_default();
        let modes = &self.cfg.routing.modes;
        let owning: Vec<&str> = crate::config::routing::MODES
            .into_iter()
            .filter(|m| modes.of(m).iter().any(|p| p == profile))
            .collect();
        let label = match owning.as_slice() {
            [one] if *one == mode => return vec![],
            [one] => json!(one),
            _ => json!({"not": mode}),
        };
        vec![SystemLabel {
            id: system_key(&j.id, "mode", "chosen"),
            judgment: j.id.clone(),
            pack: j.pack.clone(),
            session: Some(session.to_string()),
            question: "mode".into(),
            about: None,
            label,
            rule: "chosen",
            note: format!(
                "the owner chose {chosen} for the next message, {} s after the routed turn",
                (next.at_ms - s.at_ms) / 1000
            ),
        }]
    }
}

fn first_message(nodes: &[Node]) -> Option<String> {
    nodes.iter().find_map(|n| match &n.body {
        Body::UserMessage { text, .. } => Some(text.clone()),
        _ => None,
    })
}

impl SystemLabel {
    /// Its row, keyed and scoped as its judgment.
    pub fn record(&self) -> anyhow::Result<NewRecord> {
        let f = crate::fact::judge::JudgeLabel {
            id: &self.id,
            judgment: &self.judgment,
            pack: &self.pack,
            question: Some(&self.question),
            about: self.about.as_deref(),
            label: self.label.clone(),
            source: "system",
            who: "theseus",
            via: "learning",
            weight: SYSTEM_WEIGHT,
            note: &self.note,
            correlation_id: None,
            rule: Some(self.rule),
        };
        let mut r = crate::fact::row(&f, self.session.as_deref(), None)?;
        r.key = Some(self.id.clone());
        Ok(r.scoped(&crate::rpc::judge::scope_of(&self.pack)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_phrases_and_their_near_misses() {
        for yes in [
            "continue",
            "Continue.",
            "go on",
            "Go on!",
            "keep going please",
            "you didn't finish",
            "You didn’t finish.",
            "you did not finish",
        ] {
            assert!(continuation(yes), "{yes}");
        }
        for no in [
            "",
            "thanks",
            "continue the docs, and then write the tests for the parser",
            "going on holiday",
            "please continue",
            "don't keep going",
        ] {
            assert!(!continuation(no), "{no}");
        }
    }
}
