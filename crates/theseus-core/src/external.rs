//! External text (theseus-9bp, spec §3.9): once a session has read text from
//! outside Theseus, a call that acts waits for the operator's approval, until
//! the operator trusts the session again. It is the interim, deterministic
//! floor for web text, before provenance labels (§3.9 "Exposure") and Jev's
//! `security.v1` (M5).
//!
//! - **Reading.** A session reads external text when a result node marked
//!   `external` (DD5) enters its context: `http.fetch` and `web.search` today.
//!   The first read since the session was last trusted is its hold
//!   (`SessionRecord.external`), written with a `session.external_read` row
//!   in the very frame that writes the node, so no crash leaves the text in
//!   the context without the hold. A later read writes nothing more.
//! - **By program** (theseus-b5cl). A `proc.run` of a program `[policy]
//!   external_programs` lists (`gh` by default: `gh issue view` prints a
//!   stranger's text), or of a shell or a launcher whose command names one,
//!   is marked `external` too (`Listed`), at L0 and in L1.
//! - **From another session.** A task that a session holding external text
//!   starts holds it from its first node, its brief, which may carry that
//!   text. A session that reads a report from a task that holds it holds it
//!   too, from the frame that writes the report. A session that a holding
//!   session's job opens, or sends a turn to, holds it too (theseus-b5cl):
//!   every job carries its session in `THESEUS_SESSION`, which the CLI sends
//!   as `opened_from` (`from_job`). A job can strip its environment, so this
//!   is a light guard under default trust, not a boundary.
//! - **What waits.** Every call whose class is not `Read` (writes, edits,
//!   patches, `proc.run`, `task.create`), after the whole order of §3.9, the
//!   allow list included: the stricter posture wins, as a granted secret's
//!   does. `Read` calls keep their posture, fetches included, so research
//!   goes on, and each fetch's notice names its URL (Eddie, 2026-09-30). A
//!   wake's turn and a turn that a task's report started are the session's
//!   own turns, so the hold covers them.
//! - **`wake.at` keeps its posture** (Eddie, 2026-09-30, T1b): the reminder's
//!   turn runs in this same session, so any call it makes that acts still
//!   waits. `task.create` still waits: a task spends its own budget and runs
//!   turns of its own.
//! - **Clearing.** Only the operator, with the trusted answer an approval
//!   takes: `policy.trust` (the CLI, the Observatory, Discord's `/trust`),
//!   or an approval with `trust`. Ledgered as `session.trusted`. A later read
//!   holds the session again.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_protocol::ExternalText;
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};
use theseus_tools::ToolClass;

use crate::ledger::LedgerRow;
use crate::policy::{Decision, Posture};
use crate::session::SessionRecord;

/// `[policy] external_text`: what a call that acts gets once its session has
/// read external text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// It waits for the operator's approval.
    #[default]
    Ask,
    /// It runs, with a notice at least.
    Notify,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Ask => "ask",
            Mode::Notify => "notify",
        }
    }

    fn posture(self) -> Posture {
        match self {
            Mode::Ask => Posture::Approve,
            Mode::Notify => Posture::Notify,
        }
    }
}

/// `task.create`: a task that a session holding external text started.
pub const VIA_TASK: &str = "task.create";
/// `task.report`: a report from a task that held external text.
pub const VIA_REPORT: &str = "task.report";
/// `job`: a session that a holding session's job opened, or sent a turn to
/// (theseus-b5cl), by the `opened_from` the CLI sends.
pub const VIA_JOB: &str = "job";
/// `program`: a job whose program `[policy] external_programs` lists
/// (theseus-b5cl). The hold's `url` is the job's command (`Listed`).
pub const VIA_PROGRAM: &str = "program";

/// The key of a job result's `meta` that names the listed program its job
/// ran (theseus-b5cl), so its hold says `via: program`.
pub const PROGRAM_KEY: &str = "external_program";

/// The hold a result marked external gives its session: a search keeps its
/// query, which the hold names (theseus-qiy). A `proc.run` result is marked
/// when its job connected out of L1 (18c), so its hold says it came `via:
/// egress`, and its `url` names the hosts the job reached; or when its job
/// ran a listed program (theseus-b5cl), which `by_program` then says.
pub fn read(
    node_id: &str,
    tool: &str,
    url: &str,
    query: Option<&str>,
    now_ms: u64,
) -> ExternalText {
    ExternalText {
        since_ms: now_ms,
        tool: tool.into(),
        url: url.into(),
        node_id: node_id.into(),
        from_session: None,
        via: (tool == crate::sandbox::PROC_RUN).then(|| theseus_protocol::VIA_EGRESS.into()),
        query: query.map(str::to_string),
    }
}

/// A search's query, from its result's `meta` (`web.search` writes the query
/// it sent there): what its hold names. None for any other tool.
pub fn search_query<'a>(tool: &str, meta: &'a serde_json::Value) -> Option<&'a str> {
    (tool == "web.search")
        .then(|| meta.get("query").and_then(serde_json::Value::as_str))
        .flatten()
}

/// A hold from a result whose `meta` names the listed program its job ran
/// (`PROGRAM_KEY`): it came `via: program`, where `read` alone would take a
/// `proc.run`'s mark for 18c's egress.
pub fn by_program(h: &mut ExternalText, meta: &serde_json::Value) {
    if meta
        .get(PROGRAM_KEY)
        .is_some_and(serde_json::Value::is_string)
    {
        h.via = Some(VIA_PROGRAM.into());
    }
}

/// A job that runs a program `[policy] external_programs` lists
/// (theseus-b5cl): what its result is marked with and says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The listed program, as the list names it.
    pub program: String,
    /// The job's command as its hold names it: the program and its
    /// subcommand (`gh issue`), never the rest of its argv; or a launcher
    /// and what it names (`sh, whose command names gh`).
    pub command: String,
}

impl Listed {
    /// The listed program a `proc.run` of `input` runs: its `argv[0]`, by
    /// file name; or, when that is a shell or a launcher (`sh -c`, `env`,
    /// `xargs`, an interpreter, a runner: `broker::launcher`), any word of
    /// its command that names one. It reads words, not what runs (`sh -c
    /// 'echo gh'` counts), which is the cheap side to err on. None when it
    /// names none, or `input` has no argv.
    pub fn of(input: &serde_json::Value, programs: &[String]) -> Option<Listed> {
        let argv: Vec<String> = input
            .get("argv")?
            .as_array()?
            .iter()
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect();
        let named = |word: &str| {
            programs
                .iter()
                .find(|p| !p.is_empty() && file_name(p) == file_name(word))
        };
        let first = argv.first()?;
        if let Some(p) = named(first) {
            let shown = crate::narrative::subject("", Some(&argv), None, std::path::Path::new("/"));
            return Some(Listed {
                program: p.clone(),
                command: shown.trim_start().to_string(),
            });
        }
        crate::broker::launcher(file_name(first))?;
        let p = argv[1..].iter().flat_map(|a| words(a)).find_map(named)?;
        Some(Listed {
            program: p.clone(),
            command: format!("{}, whose command names {p}", file_name(first)),
        })
    }

    /// DD5's marker on its result: its `url` the command.
    pub fn marker(&self) -> theseus_tools::External {
        theseus_tools::External {
            url: self.command.clone(),
        }
    }

    /// The result's line that says why its text counts as outside text.
    pub fn line(&self) -> String {
        format!(
            "[it runs {}, which [policy] external_programs lists: what it printed may hold \
             outside text]\n",
            self.program
        )
    }
}

/// A path's file name: `gh` of `/usr/bin/gh`.
fn file_name(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// The words of a launcher's argument, split at spaces and the shell's
/// punctuation: `cd x && gh issue view 1 | head` has `gh` among them.
fn words(arg: &str) -> impl Iterator<Item = &str> {
    arg.split(|c: char| c.is_whitespace() || "|&;()<>`'\"$={}[],!*?\\".contains(c))
        .filter(|w| !w.is_empty())
}

/// The hold a session takes from `from`, the session whose job opened it or
/// sent it a turn (`opened_from`, theseus-b5cl), as a task takes its
/// parent's: `from`'s own, with `from` named and no node yet. None when
/// `from` holds none, or names no session: a job could as well strip its
/// environment, so an unknown id is no reason to refuse. A record that
/// cannot be read is an error, and the open or the turn fails.
pub fn from_job(
    store: &crate::store::Store,
    from: Option<&str>,
    now_ms: u64,
) -> Result<Option<ExternalText>> {
    let Some(from) = from.filter(|f| !f.is_empty()) else {
        return Ok(None);
    };
    let held = store
        .get_session::<SessionRecord>(from)?
        .and_then(|r| r.external);
    Ok(held.map(|h| taken(&h, from, VIA_JOB, "", now_ms)))
}

/// The hold a session takes from another, `from_session`, which holds
/// `from`: its first source, and the node that brought it here.
pub fn taken(
    from: &ExternalText,
    from_session: &str,
    via: &str,
    node_id: &str,
    now_ms: u64,
) -> ExternalText {
    ExternalText {
        since_ms: now_ms,
        tool: from.tool.clone(),
        url: from.url.clone(),
        node_id: node_id.into(),
        from_session: Some(from_session.into()),
        via: Some(via.into()),
        query: from.query.clone(),
    }
}

/// When a hold began, in the daemon's local time as the reason says it
/// (theseus-qiy): `12:55:01`, or `Sep 30 12:55:01` on another day than
/// `now_ms`'s. Health and a trust's result say it so, never in UTC.
pub fn since_local(h: &ExternalText, now_ms: u64) -> String {
    crate::wake::local(h.since_ms).hms_on(&crate::wake::local(now_ms))
}

/// What the session read, as a reason's parenthesis says it: `http.fetch
/// <url>, at 13:05`, or a search by its query, `web.search "tokio JoinSet
/// documentation", at 13:05` (theseus-qiy), and how it came, when it came
/// through another session.
pub fn source(h: &ExternalText) -> String {
    let at = crate::wake::local(h.since_ms).hm();
    let what = h.what();
    let from = h.from_session.as_deref().map(crate::task::short);
    match (h.via.as_deref(), from) {
        (Some(VIA_TASK), Some(s)) => {
            format!("{what}, which session {s} had read before it started this task, at {at}")
        }
        (Some(VIA_REPORT), Some(s)) => format!("{what}, in task {s}'s report, at {at}"),
        (Some(VIA_JOB), Some(s)) => {
            format!("{what}, which session {s} had read before its job reached this one, at {at}")
        }
        _ => format!("{what}, at {at}"),
    }
}

/// Why a call that acts waits (or is notified): the confirm's words.
pub fn why(h: &ExternalText, mode: Mode) -> String {
    match mode {
        Mode::Ask => format!(
            "this session read external text ({}), and a call that acts waits for approval \
             after that (§3.9)",
            source(h)
        ),
        Mode::Notify => format!(
            "this session read external text ({}), and a call that acts is notified after that \
             ([policy] external_text = notify)",
            source(h)
        ),
    }
}

/// The session's hold, read for the gate: none, or what it read. A record
/// that cannot be read is `Err`, and the call waits (the floor fails closed).
pub fn held(store: &crate::store::Store, session_id: &str) -> Result<Option<ExternalText>, String> {
    store
        .get_session::<SessionRecord>(session_id)
        .map(|r| r.and_then(|r| r.external))
        .map_err(|e| format!("{e:#}"))
}

/// A call the hold leaves at its own posture, whose gate reads no record: a
/// `Read`, and `wake.at`, whose turn runs in this same session, where the
/// hold still covers what it does.
pub fn exempt(class: ToolClass, tool: &str) -> bool {
    class == ToolClass::Read || tool == crate::wake::AT
}

/// The gate's decision for a call of `class` in a session whose hold is
/// `held`, after the whole order of §3.9: a call that acts runs at no looser
/// a posture than the mode's, and an exempt call keeps its own. The decision
/// names the hold only when the hold is what raised it.
pub fn gate(
    d: Decision,
    class: ToolClass,
    held: &Result<Option<ExternalText>, String>,
    mode: Mode,
    tool: &str,
    summary: &str,
) -> Decision {
    if exempt(class, tool) {
        return d;
    }
    let setting = format!("[policy] external_text = {}", mode.as_str());
    match held {
        Ok(None) => d,
        Ok(Some(h)) if mode.posture() > d.posture => Decision {
            external: Some(h.clone()),
            ..d.at_least(mode.posture(), &why(h, mode), &setting, tool, summary)
        },
        Ok(Some(_)) => d,
        Err(e) => d.at_least(
            Posture::Approve,
            &format!("its session's record could not be read to check for external text: {e}"),
            &setting,
            tool,
            summary,
        ),
    }
}

/// A session comes to hold `h`: its record changed, and the
/// `session.external_read` row, for the frame that brings the text in. None
/// when it holds external text already, so a later read writes nothing.
pub fn hold(
    mut rec: SessionRecord,
    h: ExternalText,
    turn_id: Option<&str>,
) -> Result<Option<Vec<NewRecord>>> {
    if rec.external.is_some() {
        return Ok(None);
    }
    let row = LedgerRow::new(
        LedgerKind::SessionExternalRead,
        Some(&rec.session_id),
        turn_id,
        json!({"node_id": h.node_id, "tool": h.tool, "url": h.url, "query": h.query,
               "since_ms": h.since_ms, "from_session": h.from_session, "via": h.via,
               "task": rec.task.is_some()}),
    );
    rec.external = Some(h);
    Ok(Some(vec![
        NewRecord::json(kinds::LEDGER, None, &row)?,
        NewRecord::json(kinds::SESSION, Some(&rec.session_id), &rec)?,
    ]))
}

/// The hold a result node brings a session that holds none yet: a result
/// marked `external` (DD5's fetch and search, 18c's job that connected out,
/// a job of a listed program) read as the session's first outside text since
/// it was last trusted. None for any other node, or a session that holds one
/// already. Its records ride in the frame that writes the node.
pub fn brought(
    rec: SessionRecord,
    node: &crate::node::Node,
    turn_id: Option<&str>,
) -> Result<Option<(ExternalText, Vec<NewRecord>)>> {
    let crate::node::Body::ToolResult {
        external: Some(e),
        tool,
        meta,
        ..
    } = &node.body
    else {
        return Ok(None);
    };
    let now = theseus_protocol::now_unix_ms();
    let mut h = read(&node.id, tool, &e.url, search_query(tool, meta), now);
    by_program(&mut h, meta);
    Ok(hold(rec, h.clone(), turn_id)?.map(|more| (h, more)))
}

/// Whether `node` is a result marked as outside text.
pub fn is_outside(node: &crate::node::Node) -> bool {
    matches!(
        &node.body,
        crate::node::Body::ToolResult {
            external: Some(_),
            ..
        }
    )
}

/// A frame that would write outside text without its session's lock: built
/// again under the lock, so the hold rides in it (`under_hold`).
#[derive(Debug)]
pub struct NeedsHold;

impl std::fmt::Display for NeedsHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a result that is outside text writes its session's hold in its own frame")
    }
}

impl std::error::Error for NeedsHold {}

/// What a frame that may write outside text knows of its session's hold.
pub enum Hold {
    /// Not locked: a frame with outside text fails with `NeedsHold`.
    Ask,
    /// Under the session record's lock, with the record.
    With(Box<SessionRecord>),
    /// The session has no record, so no hold can be kept.
    Without,
}

impl Hold {
    /// The hold the first of `nodes` that is outside text brings, and its
    /// records, for the frame that writes them: None when none is, or when
    /// the session holds one already.
    pub fn of(
        &self,
        nodes: &[crate::node::Node],
        turn_id: Option<&str>,
    ) -> Result<Option<(ExternalText, Vec<NewRecord>)>> {
        let Some(first) = nodes.iter().find(|n| is_outside(n)) else {
            return Ok(None);
        };
        match self {
            Hold::Ask => Err(NeedsHold.into()),
            Hold::With(rec) => brought((**rec).clone(), first, turn_id),
            Hold::Without => {
                tracing::warn!("external text read in a session with no record: no hold is kept");
                Ok(None)
            }
        }
    }
}

/// Builds and writes a frame with `take`, first without the session's lock;
/// a frame with outside text (a late result, a cancelled job's, 18c) is
/// built again under it, so its hold rides in the same frame. So the common
/// frame, which brings none, costs no lock and no read of the record.
pub fn under_hold<T>(
    store: &crate::store::Store,
    session_id: &str,
    mut take: impl FnMut(Hold) -> Result<T>,
) -> Result<T> {
    match take(Hold::Ask) {
        Err(e) if e.is::<NeedsHold>() => {
            match store.with_session(session_id, |rec| take(Hold::With(Box::new(rec))))? {
                Some(t) => Ok(t),
                None => take(Hold::Without),
            }
        }
        t => t,
    }
}

/// Writes `node`'s frame through `write`: the node alone, or, for a result
/// that brings outside text, the node and the hold it gives its session,
/// under the session record's lock, so no crash leaves the text in the
/// context without the hold (theseus-9bp). What `write` returned, and the
/// hold when this node began it.
pub fn with_hold<R>(
    store: &crate::store::Store,
    session_id: &str,
    turn_id: Option<&str>,
    node: &crate::node::Node,
    write: impl FnOnce(Vec<NewRecord>) -> Result<R>,
) -> Result<(R, Option<ExternalText>)> {
    let frame = vec![node.record()?];
    if !is_outside(node) {
        return Ok((write(frame)?, None));
    }
    let mut once = Some((write, frame));
    let mut newly = None;
    let done = store.with_session(session_id, |rec| {
        let (write, mut frame) = once.take().expect("called once");
        if let Some((h, more)) = brought(rec, node, turn_id)? {
            frame.extend(more);
            newly = Some(h);
        }
        write(frame)
    })?;
    match (done, once) {
        (Some(r), _) => Ok((r, newly)),
        // Every session a surface opens has a record before its first turn;
        // one without cannot keep a hold.
        (None, Some((write, frame))) => {
            tracing::warn!(
                session_id,
                "external text read in a session with no record: no hold is kept"
            );
            Ok((write(frame)?, None))
        }
        (None, None) => unreachable!("with_session returns None only before it calls"),
    }
}

/// The sessions of `sessions` that hold external text, the longest-held
/// first: health's list, from the records it reads anyway. Each hold's time
/// is in the daemon's local time, as its reason says it (theseus-qiy).
pub fn listed(sessions: &[SessionRecord], now_ms: u64) -> Vec<theseus_protocol::ExternalTextInfo> {
    let mut v: Vec<theseus_protocol::ExternalTextInfo> = sessions
        .iter()
        .filter_map(|r| {
            let held = r.external.clone()?;
            Some(theseus_protocol::ExternalTextInfo {
                session_id: r.session_id.clone(),
                title: r.title.clone(),
                task: r.task.as_ref().map(|_| crate::task::short(&r.session_id)),
                since_local: since_local(&held, now_ms),
                held,
            })
        })
        .collect();
    v.sort_by_key(|i| i.held.since_ms);
    v
}

/// The narrative's line for a session that came to hold `h`.
pub fn narrated(h: &ExternalText, mode: Mode) -> String {
    let then = match mode {
        Mode::Ask => "a call that acts waits for approval",
        Mode::Notify => "a call that acts is notified",
    };
    format!(
        "This session read external text ({}): from now on {then}, until the operator trusts \
         it again (`theseus policy trust`, `/trust` on Discord, or \"Approve + trust session\" \
         on a card).",
        source(h)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::SessionKind;

    fn hold_of(tool: &str) -> ExternalText {
        read(
            "nod_1",
            tool,
            "https://example.test/page",
            None,
            1_759_266_720_000,
        )
    }

    /// A call that acts after external text waits, with the reason the brief
    /// gives; a read and `wake.at` keep their postures; a call that waits
    /// already keeps its own reason; and `notify` notifies instead.
    #[test]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn a_call_that_acts_waits_and_a_read_keeps_its_posture() {
        let h = Ok(Some(hold_of("http.fetch")));
        let notify = Decision::at_least(
            Decision {
                posture: Posture::Open,
                reason: "proc.run — open (x)".into(),
                notify: None,
                floor: false,
                granted: None,
                external: None,
            },
            Posture::Notify,
            "enforcement = notify",
            "enforcement = notify",
            "proc.run",
            "run echo hi",
        );
        let d = gate(
            notify.clone(),
            ToolClass::Run,
            &h,
            Mode::Ask,
            "proc.run",
            "run echo hi",
        );
        assert_eq!(d.posture, Posture::Approve);
        assert!(d.external.is_some());
        assert!(
            d.reason.starts_with(
                "run echo hi: proc.run — approve (this session read external text (http.fetch \
                 https://example.test/page, at "
            ),
            "{}",
            d.reason
        );
        assert!(d
            .reason
            .ends_with("), and a call that acts waits for approval after that (§3.9))"));
        let read = gate(
            notify.clone(),
            ToolClass::Read,
            &h,
            Mode::Ask,
            "http.fetch",
            "fetch",
        );
        assert_eq!(read.posture, Posture::Notify);
        assert!(read.external.is_none());
        // `wake.at` keeps its posture and its own reason (T1b), even where
        // the record cannot be read; `task.create` still waits.
        for held in [&h, &Err("disk".into())] {
            let wake = gate(
                notify.clone(),
                ToolClass::Write,
                held,
                Mode::Ask,
                "wake.at",
                "wake in 2m",
            );
            assert_eq!(
                (wake.posture, &wake.reason, wake.external.is_none()),
                (Posture::Notify, &notify.reason, true),
                "wake.at is exempt"
            );
        }
        let task = gate(
            notify.clone(),
            ToolClass::Run,
            &h,
            Mode::Ask,
            "task.create",
            "start a task",
        );
        assert_eq!(task.posture, Posture::Approve);
        assert!(task.external.is_some());
        assert!(exempt(ToolClass::Write, "wake.at") && exempt(ToolClass::Read, "fs.read"));
        assert!(!exempt(ToolClass::Run, "task.create") && !exempt(ToolClass::Write, "fs.write"));
        let clean = gate(
            notify.clone(),
            ToolClass::Write,
            &Ok(None),
            Mode::Ask,
            "fs.write",
            "write",
        );
        assert_eq!(clean.posture, Posture::Notify);
        assert!(clean.external.is_none());
        let waits = Decision::at_least(
            notify.clone(),
            Posture::Approve,
            "`sudo` matches the approve list",
            "",
            "proc.run",
            "run sudo ls",
        );
        let kept = gate(
            waits.clone(),
            ToolClass::Run,
            &h,
            Mode::Ask,
            "proc.run",
            "run sudo ls",
        );
        assert_eq!(kept.reason, waits.reason);
        assert!(kept.external.is_none(), "the hold did not raise it");
        let open = Decision {
            posture: Posture::Open,
            reason: "proc.run — open (`ls` matches the allow list entry `ls`)".into(),
            notify: None,
            floor: false,
            granted: None,
            external: None,
        };
        let n = gate(open, ToolClass::Run, &h, Mode::Notify, "proc.run", "run ls");
        assert_eq!(n.posture, Posture::Notify);
        assert_eq!(
            n.notify.as_ref().map(|x| x.setting.as_str()),
            Some("[policy] external_text = notify")
        );
        let unread = gate(
            notify,
            ToolClass::Run,
            &Err("disk".into()),
            Mode::Notify,
            "proc.run",
            "run echo hi",
        );
        assert_eq!(unread.posture, Posture::Approve, "fails closed");
    }

    /// A hold is taken once: a session that holds one writes nothing for a
    /// later read, and one taken from another session says where it came from.
    #[test]
    fn a_hold_is_taken_once_and_says_where_it_came_from() {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        let first = hold(rec.clone(), hold_of("http.fetch"), Some("turn_1"))
            .unwrap()
            .expect("the first read holds it");
        assert_eq!(first.len(), 2);
        let mut held = rec;
        held.external = Some(hold_of("http.fetch"));
        assert!(hold(held, hold_of("web.search"), None).unwrap().is_none());
        let task = taken(
            &hold_of("web.search"),
            "ses_0000aa1b2c3",
            VIA_TASK,
            "nod_brief",
            1_759_266_780_000,
        );
        assert!(
            source(&task).starts_with(
                "web.search https://example.test/page, which session a1b2c3 had read before it \
                 started this task, at "
            ),
            "{}",
            source(&task)
        );
        let report = taken(&task, "ses_0000dd4e5f6", VIA_REPORT, "nod_r", 0);
        assert!(source(&report).contains(", in task d4e5f6's report, at "));
        assert_eq!(report.tool, "web.search", "the first source");
    }

    /// A search's hold names its query, not the request's URL, which it keeps
    /// for the record; a hold taken from it names the query too; a fetch, and
    /// a search's hold written before the query was kept, name the URL. Health
    /// says when, in the same local time as the reason (theseus-qiy).
    #[test]
    fn a_searchs_hold_names_its_query_and_health_says_when_in_local_time() {
        let at = 1_759_266_720_000;
        let search = read(
            "nod_s",
            "web.search",
            "https://search.example.test/res?q=lantern+tide+tables&count=5",
            Some("lantern tide tables"),
            at,
        );
        let hm = crate::wake::local(at).hm();
        assert_eq!(
            source(&search),
            format!("web.search \"lantern tide tables\", at {hm}")
        );
        assert!(
            search.url.contains("q=lantern+tide+tables"),
            "the URL stays"
        );
        let task = taken(&search, "ses_0000aa1b2c3", VIA_TASK, "nod_brief", at);
        assert!(
            source(&task).starts_with(
                "web.search \"lantern tide tables\", which session a1b2c3 had read before it"
            ),
            "{}",
            source(&task)
        );
        let old = ExternalText {
            query: None,
            ..search.clone()
        };
        assert_eq!(
            source(&old),
            format!(
                "web.search https://search.example.test/res?q=lantern+tide+tables&count=5, at {hm}"
            )
        );
        assert_eq!(
            source(&hold_of("http.fetch")),
            format!("http.fetch https://example.test/page, at {hm}")
        );
        // Health: the reason's local time, to the second, and the day when
        // the hold is older than today.
        let mut rec = SessionRecord::new(SessionKind::Conversation, None);
        rec.external = Some(search);
        let listed_now = listed(std::slice::from_ref(&rec), at + 5_000);
        assert_eq!(listed_now[0].since_local, crate::wake::local(at).hms());
        assert!(listed_now[0].since_local.starts_with(&hm));
        assert!(!listed_now[0].since_local.ends_with('Z'), "never UTC");
        let later = listed(std::slice::from_ref(&rec), at + 3 * 86_400_000);
        let l = crate::wake::local(at);
        assert!(
            later[0]
                .since_local
                .ends_with(&format!(" {} {}", l.day, l.hms())),
            "{}",
            later[0].since_local
        );
    }

    fn run_of(argv: &[&str]) -> serde_json::Value {
        json!({ "argv": argv })
    }

    /// `[policy] external_programs` (theseus-b5cl): a listed program is
    /// named by its `argv[0]`'s file name; a shell or a launcher by any word
    /// of its command; an unlisted program, or one that only mentions a
    /// listed one in its arguments, is not.
    #[test]
    fn a_listed_program_is_named_by_its_file_name_or_by_a_launchers_words() {
        let gh = ["gh".to_string()];
        let of = |argv: &[&str]| Listed::of(&run_of(argv), &gh);
        let direct = of(&["gh", "issue", "view", "12"]).expect("gh is listed");
        assert_eq!(
            (direct.program.as_str(), direct.command.as_str()),
            ("gh", "gh issue")
        );
        assert_eq!(
            of(&["/usr/local/bin/gh", "pr", "list"]).map(|l| l.command),
            Some("gh pr".into())
        );
        for launched in [
            &["sh", "-c", "cd work && gh issue view 1 | head -5"][..],
            &["env", "GH_REPO=invented/x", "gh", "api", "user"],
            &["xargs", "-I{}", "gh", "issue", "view", "{}"],
            &["bash", "-c", "$(command -v gh) version"],
            &["python3", "-c", "import os; os.system('gh issue view 1')"],
            &["timeout", "30", "/usr/bin/gh", "api", "user"],
        ] {
            let l = of(launched).unwrap_or_else(|| panic!("{launched:?} names gh"));
            assert_eq!(l.program, "gh");
            let shell = launched[0].rsplit('/').next().unwrap();
            assert_eq!(l.command, format!("{shell}, whose command names gh"));
        }
        for unlisted in [
            &["git", "log"][..],
            &["grep", "gh", "notes.txt"],
            &["sh", "-c", "echo hi"],
            &["sh", "-c", "ghost --version"],
        ] {
            assert_eq!(of(unlisted), None, "{unlisted:?}");
        }
        assert_eq!(Listed::of(&json!({"cmd": "gh"}), &gh), None, "no argv");
        assert_eq!(Listed::of(&run_of(&["gh"]), &[]), None, "an empty list");
        let path = ["/opt/bin/glab".to_string()];
        assert_eq!(
            Listed::of(&run_of(&["glab", "mr", "view"]), &path).map(|l| l.program),
            Some("/opt/bin/glab".into())
        );
    }

    /// A listed program's result is marked with its command, its line says
    /// why, and its hold says `via: program`, where a `proc.run`'s
    /// mark alone is 18c's egress.
    #[test]
    fn a_listed_programs_hold_says_it_came_by_program() {
        let l = Listed::of(&run_of(&["gh", "issue", "view", "12"]), &["gh".into()]).unwrap();
        assert_eq!(l.marker().url, "gh issue");
        assert_eq!(
            l.line(),
            "[it runs gh, which [policy] external_programs lists: what it printed may hold \
             outside text]\n"
        );
        let at = 1_759_266_720_000;
        let mut h = read("nod_r", crate::sandbox::PROC_RUN, &l.marker().url, None, at);
        assert_eq!(h.via.as_deref(), Some(theseus_protocol::VIA_EGRESS));
        by_program(&mut h, &json!({ PROGRAM_KEY: "gh", "exit_code": 0 }));
        assert_eq!(h.via.as_deref(), Some(VIA_PROGRAM));
        let hm = crate::wake::local(at).hm();
        assert_eq!(source(&h), format!("proc.run gh issue, at {hm}"));
        let mut egress = read(
            "nod_e",
            crate::sandbox::PROC_RUN,
            "api.github.com:443",
            None,
            at,
        );
        by_program(&mut egress, &json!({"exit_code": 0}));
        assert_eq!(egress.via.as_deref(), Some(theseus_protocol::VIA_EGRESS));
    }

    /// `[policy] external_programs` is `["gh"]` by default, so a note from
    /// before it lists gh; the template says the same; a note may list
    /// others, or none (theseus-b5cl).
    #[test]
    fn external_programs_is_gh_by_default_and_in_the_template() {
        let policy = |p: &str| {
            let note = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[policy]\n{p}\n");
            crate::Config::parse(&note)
                .unwrap()
                .0
                .policy
                .external_programs
        };
        assert_eq!(policy(""), ["gh"]);
        let (t, _) = crate::Config::parse(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(t.policy.external_programs, ["gh"], "the template says gh");
        assert_eq!(
            policy("external_programs = [\"gh\", \"glab\"]"),
            ["gh", "glab"]
        );
        assert!(policy("external_programs = []").is_empty());
    }

    /// A hold taken through a job (theseus-b5cl) names the session whose job
    /// it was, and no node: the session held it before any node came.
    #[test]
    fn a_hold_taken_through_a_job_names_its_session() {
        let h = taken(
            &hold_of("http.fetch"),
            "ses_0000aa1b2c3",
            VIA_JOB,
            "",
            1_759_266_780_000,
        );
        assert_eq!(
            (h.from_session.as_deref(), h.node_id.as_str()),
            (Some("ses_0000aa1b2c3"), "")
        );
        assert!(
            source(&h).starts_with(
                "http.fetch https://example.test/page, which session a1b2c3 had read before its \
                 job reached this one, at "
            ),
            "{}",
            source(&h)
        );
    }
}
