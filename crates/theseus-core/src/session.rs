//! Sessions (spec §3.2a, §4.4b). A session is a compiler scope with exactly one
//! execution, and that execution has the turn lock (the kernel holds it, M2).
//! Its content is nodes (§4.1, `node.rs`); its context is a compilation plus an
//! append tail (§4.4a, `compiler.rs`). The record itself is small and durable
//! by reference: a thousand waiting sessions cost a record each.

use serde::{Deserialize, Serialize};
use theseus_protocol::{SessionInfo, SessionKind, Usage};

/// What the session's last turn ran against, recorded from that turn's start
/// (theseus-kol); a continuation reuses it, so a conversation does not change
/// model under the model's own thinking blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetRef {
    pub profile: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub kind: SessionKind,
    pub label: Option<String>,
    pub created_at_unix_ms: u64,
    pub turns: u64,
    pub last_turn_id: Option<String>,
    #[serde(default)]
    pub usage: Usage,
    /// The session's one kernel execution (§3.2a). Sessions written before M2
    /// have none; the turn runner opens one on their next turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    /// The current compilation (§4.4a); none until the first turn compiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compilation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_target: Option<TargetRef>,
    #[serde(default)]
    pub last_active_ms: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub tool_calls: u64,
    /// The first words of the first prompt, for pickers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// An operator asked for a recompile; the next turn applies it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_recompile: Option<crate::compiler::Recompile>,
    /// A task session's origin (DD7): the session and the call that started
    /// it, and where it reports. Written once, when the task opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskOf>,
    /// The session read external text (theseus-9bp, `external.rs`): written
    /// in the frame that brings the text in, cleared only by the operator's
    /// trust. A turn never writes it (`take_turns_fields`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<theseus_protocol::ExternalText>,
    /// The run of failed turns the session is in (theseus-ljr), which decides
    /// what the next failure does: retry with backoff, retry once, or wait on
    /// input. A turn whose model answers ends the run, and so does new input.
    /// Absent in records written before it (session schema 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failing: Option<Failing>,
    /// Images the provider refused (theseus-0s4), by their blob: each one
    /// renders as its line in every later request of the session, whatever
    /// recompiles it, and the node that carries it stays as it was written.
    /// Absent in records written before it (session schema 3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_shown: Vec<NotShown>,
}

/// An image the provider refused (theseus-0s4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotShown {
    /// Its blob's digest (`blobs.rs`), which every node that carries the
    /// image names: the same image sent again is not shown either.
    pub digest: String,
    /// What the provider said of it, which its line repeats.
    pub why: String,
    pub at_ms: u64,
}

/// A run of failed turns (theseus-ljr), as the session keeps it, so that a
/// restart neither retries it again nor posts its notice twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failing {
    /// Failed turns in a row, the latest included.
    pub turns: u32,
    /// Of those, the ones whose class does not pass by waiting.
    pub lasting: u32,
    /// The latest one's class.
    pub class: String,
    /// The run's notice is out: no later failure of it posts another, but
    /// for the one that parks the execution.
    pub noticed: bool,
    /// The run parked the execution on input, and said so.
    pub parked: bool,
    /// When its first failure was.
    pub since_ms: u64,
}

/// What a failed turn's execution does next (theseus-ljr).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Then {
    /// A class that passes with time (overloaded, rate limited, a server
    /// error, a timeout, the network, a broken or cut stream): the driver
    /// retries it with its backoff for as long as it fails.
    Backoff,
    /// The first failure of the run whose class will not pass by waiting (a
    /// 400, a 401, a model the provider does not serve, an internal error):
    /// one retry, since a config or profile change may have cured it.
    Retry,
    /// Tried twice, or nothing a retry could change: the execution waits on
    /// its input, and the next message retries.
    Park,
}

impl Then {
    pub fn as_str(self) -> &'static str {
        match self {
            Then::Backoff => "backoff",
            Then::Retry => "retry",
            Then::Park => "park",
        }
    }
}

impl Failing {
    /// The rule (theseus-ljr): the run a failed turn extends, what its
    /// execution does next, and whether it posts the run's notice.
    /// - A transient class keeps the driver's backoff, and only the run's
    ///   first failure posts the notice.
    /// - A lasting class gets one retry, silently; its second failure in the
    ///   run parks the execution on input, with the notice that says so.
    /// - A turn that settled nothing of its own (`settled` false) failed
    ///   before any provider call returned (`over_limit`, an unpriced model,
    ///   the kernel's refusal to plan the call): a retry would change
    ///   nothing, so it parks at once. kks's `over_limit` is that case: its
    ///   turn is itself the retry of the call whose reset was approved.
    ///
    /// `prev` is the run so far: none after a model's answer or new input.
    pub fn after(
        prev: Option<Failing>,
        class: &str,
        transient: bool,
        settled: bool,
        now_ms: u64,
    ) -> (Failing, Then, bool) {
        let mut run = prev.unwrap_or(Failing {
            turns: 0,
            lasting: 0,
            class: String::new(),
            noticed: false,
            parked: false,
            since_ms: now_ms,
        });
        run.turns += 1;
        run.class = class.to_string();
        let then = if !settled {
            Then::Park
        } else if transient {
            Then::Backoff
        } else {
            run.lasting += 1;
            if run.lasting == 1 {
                Then::Retry
            } else {
                Then::Park
            }
        };
        let notice = match then {
            Then::Backoff => !run.noticed,
            Then::Retry => false,
            Then::Park => !run.parked,
        };
        run.noticed |= notice;
        run.parked |= then == Then::Park;
        (run, then, notice)
    }
}

/// Where a task session came from, and where it reports (DD7, theseus-qn2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOf {
    pub parent_session: String,
    pub parent_execution: String,
    /// The `task.create` call that opened it.
    pub by: String,
    /// Where its cards and its report go: the place its parent posted to
    /// when it started, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl SessionRecord {
    pub fn new(kind: SessionKind, label: Option<String>) -> Self {
        Self::with_id(crate::new_id("ses"), kind, label)
    }

    /// A record for a session whose id is decided elsewhere: a task's comes
    /// from the call that opens it (DD7).
    pub fn with_id(session_id: String, kind: SessionKind, label: Option<String>) -> Self {
        let now = theseus_protocol::now_unix_ms();
        Self {
            session_id,
            kind,
            label,
            created_at_unix_ms: now,
            turns: 0,
            last_turn_id: None,
            usage: Usage::default(),
            execution_id: None,
            compilation_id: None,
            last_target: None,
            last_active_ms: now,
            cost_usd: 0.0,
            tool_calls: 0,
            title: None,
            pending_recompile: None,
            task: None,
            external: None,
            failing: None,
            not_shown: Vec::new(),
        }
    }
    /// What a turn writes into the stored record (theseus-xeo): the fields it
    /// owns, from its own copy. Its books (turns, usage, cost, tool calls,
    /// last activity), its target, its compilation, its execution, its run
    /// of failures (theseus-ljr), the images its provider refused (added to,
    /// never taken away: theseus-0s4), and the title its first input gave.
    /// Never `pending_recompile`, which the operator sets while the turn runs;
    /// a turn takes that one by itself (`update_session`) when it starts.
    pub fn take_turns_fields(&mut self, turn: &SessionRecord) {
        self.turns = turn.turns;
        self.last_turn_id.clone_from(&turn.last_turn_id);
        self.usage = turn.usage.clone();
        self.last_active_ms = turn.last_active_ms;
        self.cost_usd = turn.cost_usd;
        self.tool_calls = turn.tool_calls;
        self.last_target.clone_from(&turn.last_target);
        self.compilation_id.clone_from(&turn.compilation_id);
        self.execution_id.clone_from(&turn.execution_id);
        self.failing.clone_from(&turn.failing);
        for n in &turn.not_shown {
            if !self.not_shown.iter().any(|m| m.digest == n.digest) {
                self.not_shown.push(n.clone());
            }
        }
        if self.title.is_none() {
            self.title.clone_from(&turn.title);
        }
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.session_id.clone(),
            kind: self.kind,
            label: self.label.clone(),
            created_at_unix_ms: self.created_at_unix_ms,
            turns: self.turns,
            usage: self.usage.clone(),
            execution_id: self.execution_id.clone(),
            execution_state: None,
            last_active_ms: self.last_active_ms.max(self.created_at_unix_ms),
            cost_usd: self.cost_usd,
            tool_calls: self.tool_calls,
            profile: self.last_target.as_ref().map(|t| t.profile.clone()),
            model: self.last_target.as_ref().map(|t| t.model.clone()),
            compilation_id: self.compilation_id.clone(),
            title: self.title.clone(),
            pending_confirms: 0,
            parent_session_id: self.task.as_ref().map(|t| t.parent_session.clone()),
            limit_usd: None,
            external_text: self.external.clone(),
        }
    }
}

/// A title from a prompt: the first line, trimmed to about sixty characters.
pub fn title_from(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() > 60 {
        format!("{}…", line.chars().take(60).collect::<String>())
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session record as a build before theseus-ljr wrote it (schema 2:
    /// T1's hold, kol's target), which this build must read with no run of
    /// failures, and write back without one.
    const SCHEMA_2: &str = r#"{"session_id":"ses_old2","kind":"conversation","label":"lighthouse log","created_at_unix_ms":1790000000000,"turns":3,"last_turn_id":"turn_c3","usage":{"input_tokens":1200,"output_tokens":900,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"execution_id":"exe_old2","compilation_id":"cmp_old2","last_target":{"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5"},"last_active_ms":1790000300000,"cost_usd":0.0114,"tool_calls":2,"title":"lighthouse log","external":{"since_ms":1790000200000,"tool":"http.fetch","url":"example.invalid/tides","node_id":"trs_old2"}}"#;

    #[test]
    fn a_session_record_written_before_its_run_of_failures_reads() {
        let r: SessionRecord = serde_json::from_str(SCHEMA_2).unwrap();
        assert_eq!(r.failing, None);
        assert_eq!(r.turns, 3);
        assert_eq!(r.last_target.as_ref().unwrap().profile, "sonnet");
        assert_eq!(r.external.as_ref().unwrap().tool, "http.fetch");
        let again = serde_json::to_value(&r).unwrap();
        assert!(again.get("failing").is_none(), "{again}");
        assert_eq!(
            again,
            serde_json::from_str::<serde_json::Value>(SCHEMA_2).unwrap(),
            "a record with no run of failures keeps its bytes"
        );
    }

    /// A session record as ljr's build wrote it (schema 3: a run of failures,
    /// parked), which this build reads with no image marked not shown, and
    /// writes back as it was.
    const SCHEMA_3: &str = r#"{"session_id":"ses_old3","kind":"conversation","label":null,"created_at_unix_ms":1790000000000,"turns":2,"last_turn_id":"turn_d4","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"execution_id":"exe_old3","last_target":{"profile":"lighthouse","provider":"anthropic","model":"claude-lighthouse-9"},"last_active_ms":1790000300000,"cost_usd":0.0,"tool_calls":0,"failing":{"turns":2,"lasting":2,"class":"invalid_request","noticed":true,"parked":true,"since_ms":1790000290000}}"#;

    #[test]
    fn a_session_record_written_before_its_images_not_shown_reads() {
        let r: SessionRecord = serde_json::from_str(SCHEMA_3).unwrap();
        assert!(r.not_shown.is_empty());
        assert_eq!(r.failing.as_ref().unwrap().turns, 2);
        let again = serde_json::to_value(&r).unwrap();
        assert!(again.get("not_shown").is_none(), "{again}");
        assert_eq!(
            again,
            serde_json::from_str::<serde_json::Value>(SCHEMA_3).unwrap()
        );
        // A turn's copy adds its marks; it never takes one away.
        let mut stored = r.clone();
        stored.not_shown.push(NotShown {
            digest: "a1".into(),
            why: "Could not process image".into(),
            at_ms: 1,
        });
        let mut turn = r;
        turn.not_shown.push(NotShown {
            digest: "b2".into(),
            why: "Could not process image".into(),
            at_ms: 2,
        });
        stored.take_turns_fields(&turn);
        let kept: Vec<&str> = stored.not_shown.iter().map(|n| n.digest.as_str()).collect();
        assert_eq!(kept, ["a1", "b2"]);
    }

    /// A session record as 0s4's build wrote it (schema 4: an image not
    /// shown, and a search's hold that names only the request), which this
    /// build reads with no query, names by the request, and writes back as
    /// it was (theseus-qiy's schema 5 keeps a search's query).
    const SCHEMA_4: &str = r#"{"session_id":"ses_old4","kind":"conversation","label":"tide watch","created_at_unix_ms":1790000000000,"turns":1,"last_turn_id":"turn_e5","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"last_active_ms":1790000300000,"cost_usd":0.0,"tool_calls":1,"external":{"since_ms":1790000200000,"tool":"web.search","url":"search.example.invalid/res?q=tide+tables","node_id":"trs_old4"},"not_shown":[{"digest":"c3","why":"Could not process image","at_ms":1790000250000}]}"#;

    #[test]
    fn a_session_record_written_before_a_searchs_query_reads() {
        let r: SessionRecord = serde_json::from_str(SCHEMA_4).unwrap();
        let h = r.external.as_ref().unwrap();
        assert_eq!(h.query, None);
        assert_eq!(
            h.what(),
            "web.search search.example.invalid/res?q=tide+tables"
        );
        assert_eq!(r.not_shown[0].digest, "c3");
        let again = serde_json::to_value(&r).unwrap();
        assert!(again["external"].get("query").is_none(), "{again}");
        assert_eq!(
            again,
            serde_json::from_str::<serde_json::Value>(SCHEMA_4).unwrap()
        );
    }

    /// The rule's answers, failure by failure: (class, transient, settled)
    /// in, (then, notice) out.
    fn run_of(failures: &[(&str, bool, bool)]) -> (Failing, Vec<(Then, bool)>) {
        let mut run = None;
        let mut out = Vec::new();
        for (class, transient, settled) in failures {
            let (r, then, notice) = Failing::after(run, class, *transient, *settled, 7);
            out.push((then, notice));
            run = Some(r);
        }
        (run.unwrap(), out)
    }

    #[test]
    fn a_lasting_failure_is_retried_once_then_parks_with_one_notice() {
        let (run, out) = run_of(&[("invalid_request", false, true); 2]);
        assert_eq!(out, [(Then::Retry, false), (Then::Park, true)]);
        assert_eq!((run.turns, run.lasting, run.parked), (2, 2, true));
        // A wake's turn that fails the same way later says nothing more.
        let (_, then, notice) = Failing::after(Some(run), "invalid_request", false, true, 9);
        assert_eq!((then, notice), (Then::Park, false));
    }

    #[test]
    fn a_transient_failure_keeps_the_backoff_and_posts_its_notice_once() {
        let (run, out) = run_of(&[("overloaded", true, true); 4]);
        assert_eq!(out[0], (Then::Backoff, true));
        assert!(
            out[1..].iter().all(|o| *o == (Then::Backoff, false)),
            "{out:?}"
        );
        assert_eq!((run.turns, run.lasting, run.since_ms), (4, 0, 7));
    }

    #[test]
    fn a_run_that_turns_lasting_retries_once_and_parks_with_its_second_notice() {
        let (run, out) = run_of(&[
            ("overloaded", true, true),
            ("rate_limited", true, true),
            ("auth", false, true),
            ("auth", false, true),
        ]);
        assert_eq!(
            out,
            [
                (Then::Backoff, true),
                (Then::Backoff, false),
                (Then::Retry, false),
                (Then::Park, true)
            ]
        );
        assert_eq!(run.class, "auth");
    }

    /// kks's `over_limit` fails before any provider call: nothing a retry
    /// would change, so it parks at once and says so.
    #[test]
    fn a_failure_before_any_call_parks_at_once() {
        let (_, out) = run_of(&[("over_limit", false, false)]);
        assert_eq!(out, [(Then::Park, true)]);
    }
}
