//! `task.start` (theseus-7oq0w): the owner starts a task from any surface with no
//! model turn. Today a task exists only when a model calls `task.create`; this is
//! the same task, opened by the owner's word: the CLI's `theseus task start`, the
//! terminal UI's `N`, and the rows that follow call this one method.
//!
//! - **The parent** is the conversation `home` names, else the owner's Home (made
//!   on first use, as reminders make it: `wake_home.rs`). The carve is the
//!   parent's, as `task.create`'s is: `budget_usd`, or a quarter of what the
//!   parent has left. A `budget_usd` over what it has left is refused with the
//!   numbers, and a parent with nothing left carves nothing.
//! - **One frame.** `Kernel::open_task_owned` (the opening `task.create` shares
//!   through `open_task_at`, no turn required) writes the task's execution, its
//!   session, its brief, its graph record, its quotes' arrangement, and the rows,
//!   as `task.create` does, and the `task.started_by_owner` row beside them. No
//!   model call; a plain turn is untouched.
//! - **A deferred start** (`start_at_ms`, `start_after_ms`, or `start_when`) opens
//!   it asleep in `Waiting{DueAt}`, as `task.create { start_at }` does.
//! - **Quotes** become the task's arrangement (role `context`). An arrangement is no
//!   content item, so quotes that are outside text (`outside_quotes`, or an agent's)
//!   are also queued as one item in the task's session, a person's message, before its
//!   first turn: the pack asks of them what it asks of any other person's text, and an
//!   `injection` it finds freezes the task's calls that act. The owner's own quotes
//!   are the owner's own text and are not asked.
//! - **Authority** is `judge_act(Act::Task)`: the owner, from a private place. A
//!   shared place, an MCP client and a job's shell are refused, each told where
//!   to start it instead.
//! - **The report** goes to the parent's place (`report_to: home`, the default)
//!   or to nobody (`board`). It never starts a turn of the parent.

use anyhow::anyhow;
use serde_json::json;
use theseus_kernel::{
    micros_to_usd, task_ids, usd, usd_to_micros, KernelError, Opening, SessionKind,
};
use theseus_protocol::task_start::{ReportTo, TaskStartParams, TaskStartResult};
use theseus_protocol::{error_code, method};
use theseus_store::{kinds, NewRecord};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::arrangement::{Piece, Role, MAX_PIECES, PIECE_CHARS};
use crate::node::{Node, Origin};
use crate::session::{title_from, SessionRecord, TargetRef, TaskOf};
use crate::task::{short, DEFAULT_SHARE, MAX_BRIEF_CHARS};

/// Where a refused start says to start it instead.
const INSTEAD: &str =
    "Start it from the owner's own shell (`theseus task start`), the terminal UI \
                       (N), the web UI, or the owner's DM.";

impl Core {
    /// The wire method (`rpc_prefixed`'s `task.start`).
    pub(super) fn task_start_rpc(
        &self,
        p: TaskStartParams,
        conn: Conn<'_>,
    ) -> Result<TaskStartResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.task_start(&p, who)
    }

    /// Where the task reports: its parent's place, or where the binding homes
    /// the tasks of that place (`Outbox::home_tasks`); a board-only task, nowhere.
    fn reports_to_of(
        &self,
        report_to: ReportTo,
        home_session: &str,
        home_target: Option<String>,
    ) -> Option<String> {
        match report_to {
            ReportTo::Home => {
                let at = self.outbox.target(home_session).or(home_target);
                self.outbox.report_target(at).0
            }
            ReportTo::Board => None,
        }
    }

    /// Start the task `p` describes, for `who`.
    pub fn task_start(
        &self,
        p: &TaskStartParams,
        who: impl Into<Answerer>,
    ) -> Result<TaskStartResult, RpcFailure> {
        self.task_start_as(p, who, false)
    }

    /// `task_start`, with the task's session labelled `confidential` from its
    /// first turn when `private` (its quotes are the owner's Google content).
    pub(crate) fn task_start_as(
        &self,
        p: &TaskStartParams,
        who: impl Into<Answerer>,
        private: bool,
    ) -> Result<TaskStartResult, RpcFailure> {
        let who: Answerer = who.into();
        // Quotes an outside hand wrote: an agent's, or the start said so (a mail, someone else's message).
        let outside = who.agent.is_some() || p.outside_quotes;
        let what = format!("starting a task ({})", title_from(&p.brief));
        self.judge_act(&who, Act::Task { what: &what })
            .map_err(failure)?;
        let brief = checked(p)?;
        let (quotes, now) = (quotes_of(&p.quotes)?, self.kernel.now_ms());
        let start = self.start_of(p, now)?;
        let (home_session, home_exec, parent_rec, home_target) = self.parent_of(p)?;
        let parent = self
            .kernel
            .execution(&home_exec)?
            .ok_or_else(|| invalid(format!("{home_session} has no conversation to carve from")))?;
        let want = self.carve_of(p, &parent, &home_session)?;
        let target = self.target_of(p, &parent_rec)?;
        let report_to = p.report_to.unwrap_or_default();
        let reports_to = self.reports_to_of(report_to, &home_session, home_target);
        let title = title_of(p, brief);
        let correlation_id = theseus_kernel::new_id("act");
        let (_, task_session) = task_ids(&correlation_id);
        let (brief_node, arrangement) = nodes_of(
            &task_session,
            &home_session,
            brief,
            &quotes,
            &who.label,
            now,
        );
        let graph = self.graph_of(&task_session, &title, brief, &home_session, &home_exec);
        let created = crate::fact::task_graph::Change {
            session_id: home_session.clone(),
            task: graph.clone(),
            from: None,
            detail: json!({"session": task_session}),
        };
        let home_rec = self.rec(Some(&home_session));
        let created_row = crate::fact::task_graph::row_of(&home_rec, "created", &created)?;
        let (by, via) = (who.who(), who.via());
        let common = Common {
            title: &title,
            home_session: &home_session,
            by: &by,
            via: &via,
            quotes: quotes.len(),
            board: report_to == ReportTo::Board,
            starts_at_ms: start,
        };
        let opening = Opening {
            reports_to: reports_to.clone(),
            start_at_ms: start,
            ..Default::default()
        };
        let draft = Draft {
            title: &title,
            parent_rec: &parent_rec,
            target: target.as_ref(),
            by_brief: p.profile.is_none(),
            home_exec: &home_exec,
            correlation_id: &correlation_id,
            reports_to: reports_to.as_deref(),
            arrangement: arrangement.as_ref(),
            brief_node: &brief_node,
            graph: &graph,
            created_row: &created_row,
            start,
            now,
            common: &common,
            available: parent.budget.available(),
            expect: p.expect,
            private,
        };
        let opened =
            self.kernel
                .open_task_owned(&home_exec, &correlation_id, want, opening, |task| {
                    self.owned_task_records(task, draft)
                });
        let opened = opened.map_err(open_refusal)?;
        let task = &opened.task;
        if let (true, Some(a)) = (outside, arrangement.as_ref()) {
            self.queue_quotes(&task.session_id, a, &quotes, now);
        }
        self.told(
            &opened,
            &common,
            reports_to.as_deref(),
            (&home_rec, &created),
        );
        Ok(TaskStartResult {
            task_id: task.session_id.clone(),
            session_id: task.session_id.clone(),
            short: short(&task.session_id),
            home_session,
            limit_usd: micros_to_usd(task.budget.limit_micros),
            starts_at_ms: start,
        })
    }

    /// When the task starts: none for now, else the one time the params give,
    /// inside `task.create { start_at }`'s bounds.
    fn start_of(&self, p: &TaskStartParams, now: u64) -> Result<Option<u64>, RpcFailure> {
        self.start_time(p.start_at_ms, p.start_after_ms, &p.start_when, now)
    }

    /// [`start_of`](Self::start_of)'s parse over the three times themselves,
    /// which `task.reschedule` shares.
    pub(super) fn start_time(
        &self,
        start_at_ms: Option<u64>,
        start_after_ms: Option<u64>,
        start_when: &Option<String>,
        now: u64,
    ) -> Result<Option<u64>, RpcFailure> {
        let kc = self.kernel.config();
        let max = kc.wakes.max_ahead_ms;
        let due = match (start_at_ms, start_after_ms, start_when) {
            (None, None, None) => return Ok(None),
            (Some(at), None, None) => {
                if at <= now {
                    return Err(invalid(format!(
                        "`start_at_ms` is {} ago; it is {} now. Give a later time",
                        crate::wake::span(now - at),
                        crate::wake::local(now).full()
                    )));
                }
                at
            }
            (None, Some(after), None) => now.saturating_add(after),
            (None, None, Some(when)) => {
                crate::wake_when::parse_when(when, now, &kc.zone, max).map_err(invalid)?
            }
            _ => {
                return Err(invalid(
                    "give one of `start_at_ms`, `start_after_ms` and `start_when`, not several",
                ))
            }
        };
        crate::task_start::bounded(due, now, max)
            .map(Some)
            .map_err(|e| invalid(format!("Not started: {e}")))
    }

    /// The conversation the task goes under: the one `home` names, else the
    /// owner's Home. Its id, its execution, its record, and Home's DM target.
    fn home_of(
        &self,
        p: &TaskStartParams,
    ) -> Result<(String, String, SessionRecord, Option<String>), RpcFailure> {
        match p.home.as_deref() {
            Some(id) => {
                let rec = self.session(id)?;
                if rec.kind == SessionKind::Task || rec.task.is_some() {
                    return Err(refused(
                        "Not started: that session is itself a task, and a task cannot start \
                         tasks (depth one). Start it under a conversation."
                            .into(),
                    ));
                }
                if rec.retired.is_some() {
                    return Err(refused(format!(
                        "Not started: session {} is retired. Reopen it (`theseus sessions \
                         reopen`) or start the task under another conversation.",
                        short(id)
                    )));
                }
                let Some(exec) = rec.execution_id.clone() else {
                    return Err(invalid(format!(
                        "session {id} has no conversation to carve a task's budget from"
                    )));
                };
                Ok((rec.session_id.clone(), exec, rec, None))
            }
            None => {
                let h = self.home()?;
                let rec = self.session(&h.session_id)?;
                Ok((h.session_id, h.execution_id, rec, h.target))
            }
        }
    }

    /// [`Self::home_of`], for this task.
    fn parent_of(
        &self,
        p: &TaskStartParams,
    ) -> Result<(String, String, SessionRecord, Option<String>), RpcFailure> {
        self.home_of(p)
    }

    /// The model the task starts on: the profile asked for (a pin), else the
    /// parent's, which its brief's verdict replaces on its first turn
    /// (`turn/brief_step.rs`: the one place that rule lives).
    fn target_of(
        &self,
        p: &TaskStartParams,
        parent_rec: &SessionRecord,
    ) -> Result<Option<TargetRef>, RpcFailure> {
        match &p.profile {
            Some(name) => self.profile_target(name).map(Some),
            None => Ok(parent_rec.last_target.clone()),
        }
    }

    /// The profile `name` names, as a task's model.
    fn profile_target(&self, name: &str) -> Result<TargetRef, RpcFailure> {
        self.tools.profiles.get(name).cloned().ok_or_else(|| {
            invalid(format!(
                "no profile is named `{name}`; configured: {}",
                self.tools
                    .profiles
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
    }
}

/// Everything the open's frame is built from.
struct Draft<'a> {
    title: &'a str,
    parent_rec: &'a SessionRecord,
    target: Option<&'a TargetRef>,
    /// No profile was asked for: the brief's verdict picks the model.
    by_brief: bool,
    home_exec: &'a str,
    correlation_id: &'a str,
    reports_to: Option<&'a str>,
    arrangement: Option<&'a Node>,
    brief_node: &'a Node,
    graph: &'a crate::task_graph::TaskRecord,
    created_row: &'a NewRecord,
    start: Option<u64>,
    now: u64,
    common: &'a Common<'a>,
    available: theseus_kernel::Micros,
    expect: Option<theseus_protocol::task_start::Expect>,
    private: bool,
}

impl Core {
    /// The records of the open's one frame, beside the kernel's own: the
    /// task's session, its brief and
    /// quotes, where it reports, its graph record, a deferred start, and the
    /// owner's row.
    fn owned_task_records(
        &self,
        task: &theseus_kernel::Execution,
        d: Draft<'_>,
    ) -> anyhow::Result<Vec<NewRecord>> {
        let mut rec = SessionRecord::with_id(
            task.session_id.clone(),
            SessionKind::Task,
            Some("task".into()),
        );
        rec.title = Some(d.title.to_string());
        rec.execution_id = Some(task.id.clone());
        // A task carries its parent's labels from its first call
        // (theseus-gyjyo.5), and a mail watch's, whose brief quotes the owner's
        // mail, the label that mail raises.
        rec.data = d.parent_rec.data.as_deref().and_then(|f| f.inherited());
        if d.private {
            let mut f = rec.data.take().unwrap_or_default();
            f.raise(crate::content::deterministic::label(
                crate::content::Flag::Confidential,
                &d.brief_node.id,
                crate::content::ItemKind::TaskBrief,
                crate::google::private::source("mail.watch"),
                d.now,
            ));
            rec.data = Some(f);
        }
        // A task works where its parent does (theseus-aab7).
        rec.dir = d.parent_rec.dir.clone();
        rec.last_target = d.target.cloned();
        rec.task = Some(TaskOf {
            parent_session: d.common.home_session.to_string(),
            parent_execution: d.home_exec.to_string(),
            by: d.correlation_id.to_string(),
            target: d.reports_to.map(str::to_string),
            arrangement: d.arrangement.map(|a| a.id.clone()),
            check: None,
            notify_done: false,
        });
        let mut records = vec![NewRecord::json(
            kinds::SESSION,
            Some(&rec.session_id),
            &rec,
        )?];
        records.push(d.brief_node.record()?);
        if d.by_brief {
            records.push(crate::turn::brief_step::mark(&task.session_id)?);
        }
        if let Some(a) = d.arrangement {
            records.push(a.record()?);
            records.push(
                crate::graph::Edge::new(
                    crate::graph::EdgeKind::DerivedFrom,
                    &a.id,
                    &d.brief_node.id,
                    crate::graph::VIA_ARRANGEMENT,
                )
                .record()?,
            );
        }
        if let Some(t) = d.reports_to {
            records.push(self.outbox.task_record(&task.session_id, t)?);
        }
        records.push(crate::task_graph::record(d.graph)?);
        records.push(d.created_row.clone());
        records.extend(crate::task::expect_record(&task.session_id, d.expect)?);
        if let Some(due) = d.start {
            let times = crate::task_start::Deferred {
                set_at_ms: d.now,
                due_at_ms: due,
                started_ms: None,
            };
            records.push(crate::task_start::record(&task.session_id, &times)?);
        }
        let short_id = short(&task.session_id);
        let fact = started_fact(
            d.common,
            &short_id,
            &task.session_id,
            (task.budget.limit_micros, d.available),
        );
        records.push(self.rec(Some(&task.session_id)).row(&fact)?);
        Ok(records)
    }
}

/// The task's title: the one given, else the brief's first line.
fn title_of(p: &TaskStartParams, brief: &str) -> String {
    match p.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => t.to_string(),
        None => title_from(brief),
    }
}

/// The brief's node, and the node that carries the quotes after it (none
/// without quotes).
fn nodes_of(
    task_session: &str,
    home_session: &str,
    brief: &str,
    quotes: &[String],
    author_label: &str,
    now: u64,
) -> (Node, Option<Node>) {
    let author = format!("session:{home_session}");
    let text = brief_text(task_session, home_session, brief);
    let brief_node = Node::relayed(task_session, None, Origin::Agent, &author, &text);
    let arrangement = (!quotes.is_empty()).then(|| {
        let pieces = quotes
            .iter()
            .map(|q| piece_of(q, &brief_node.id, task_session, author_label, now))
            .collect();
        Node::arrangement(task_session, &author, pieces, false)
    });
    (brief_node, arrangement)
}

/// What a refused open says.
fn open_refusal(e: anyhow::Error) -> RpcFailure {
    match e.downcast_ref::<KernelError>() {
        Some(KernelError::TaskDepth { .. }) => {
            refused("Not started: a task cannot start tasks (depth one).".into())
        }
        Some(KernelError::NothingToCarve { .. }) => refused(format!(
            "Not started: {e}. Start it under another conversation (`home`)."
        )),
        _ => RpcFailure::from(e),
    }
}

impl Core {
    /// Queue quotes that are outside text for the content pack, in the task's session, as one person's
    /// message: the arrangement holds them but is no item, so without this nothing would ask of them the
    /// injection question. It is asked before the task's first turn and nothing waits on it here.
    fn queue_quotes(&self, task_session: &str, arrangement: &Node, quotes: &[String], now: u64) {
        let item = crate::judge::content::ContentItem::new(
            &arrangement.id,
            crate::content::ItemKind::PersonMessage,
            "a quote the task was started with",
            &quotes.join("\n\n"),
            now,
        );
        self.runner.judge.ask_content(task_session, vec![item]);
    }

    /// Once the open's frame is written: bind where the task reports, and
    /// announce the owner's row's sentence and the task graph's creation.
    fn told(
        &self,
        opened: &theseus_kernel::TaskOpen,
        common: &Common<'_>,
        reports_to: Option<&str>,
        (home_rec, created): (&crate::fact::Rec<'_>, &crate::fact::task_graph::Change),
    ) {
        let task = &opened.task;
        if let Some(t) = reports_to {
            self.outbox.task_bound(&task.session_id, t);
        }
        let short_id = short(&task.session_id);
        let fact = started_fact(
            common,
            &short_id,
            &task.session_id,
            (task.budget.limit_micros, opened.available_before),
        );
        self.rec(Some(&task.session_id)).announce(&fact);
        crate::fact::task_graph::announce(home_rec, "created", created);
    }
}

/// The brief, trimmed, once it and the budget are in bounds.
fn checked(p: &TaskStartParams) -> Result<&str, RpcFailure> {
    if p.open {
        return Err(invalid(crate::task::OPEN_RETIRED));
    }
    let brief = p.brief.trim();
    if brief.is_empty() {
        return Err(invalid("the brief is empty: say what the task should do"));
    }
    let n = brief.chars().count();
    if n > MAX_BRIEF_CHARS {
        return Err(invalid(format!(
            "the brief is {n} characters, over the {MAX_BRIEF_CHARS} a task takes"
        )));
    }
    if let Some(b) = p.budget_usd {
        if !(b.is_finite() && b > 0.0) {
            return Err(invalid(format!("budget_usd must be more than $0, not {b}")));
        }
    }
    Ok(brief)
}

impl Core {
    /// What the task is carved: `task.create`'s rule (under a limit that
    /// notifies, from the parent's whole limit), with an explicit budget over
    /// it refused with the numbers.
    fn carve_of(
        &self,
        p: &TaskStartParams,
        parent: &theseus_kernel::Execution,
        home_session: &str,
    ) -> Result<theseus_kernel::Micros, RpcFailure> {
        let left = parent.budget.available();
        let pool = match self.kernel.overdraws(parent) {
            true => left.max(parent.budget.limit_micros),
            false => left,
        };
        match p.budget_usd {
            Some(b) if usd_to_micros(b) > pool => Err(refused(format!(
                "Not started: a budget of {} is more than conversation {} has left: {} \
                 (its limit is {}, and {} is spent or carved). Ask for less, or start it \
                 under another conversation (`home`).",
                usd(usd_to_micros(b)),
                short(home_session),
                usd(pool),
                usd(parent.budget.limit_micros),
                usd(parent.budget.limit_micros.saturating_sub(left)),
            ))),
            Some(b) => Ok(usd_to_micros(b)),
            None => Ok(pool / DEFAULT_SHARE.1 * DEFAULT_SHARE.0),
        }
    }

    /// The task's record in the task graph.
    fn graph_of(
        &self,
        task_session: &str,
        title: &str,
        brief: &str,
        home_session: &str,
        home_exec: &str,
    ) -> crate::task_graph::TaskRecord {
        crate::task_graph::NewTask {
            id: crate::task_graph::of_session(task_session),
            title,
            objective: brief.to_string(),
            acceptance: vec![],
            parent: None,
            deps: vec![],
            session: Some(task_session.to_string()),
            origin: crate::task_graph::TaskOrigin {
                session: home_session.to_string(),
                principal: crate::task_graph::principal_of(&self.kernel, home_exec),
                by_model: false,
                ..Default::default()
            },
            state: crate::task_graph::TaskState::InProgress,
        }
        .build(theseus_protocol::now_unix_ms())
    }
}

/// The quotes, trimmed, with the empty ones refused and the count and size capped.
fn quotes_of(quotes: &[String]) -> Result<Vec<String>, RpcFailure> {
    let out: Vec<String> = quotes
        .iter()
        .map(|q| q.trim().to_string())
        .filter(|q| !q.is_empty())
        .collect();
    if out.len() > MAX_PIECES {
        return Err(invalid(format!(
            "{} quotes, over the {MAX_PIECES} a task takes",
            out.len()
        )));
    }
    if let Some(q) = out.iter().find(|q| q.chars().count() > PIECE_CHARS) {
        return Err(invalid(format!(
            "a quote is {} characters, over the {PIECE_CHARS} a piece takes",
            q.chars().count()
        )));
    }
    Ok(out)
}

/// A quote as the arrangement keeps it: context, said by the owner.
fn piece_of(quote: &str, node: &str, session: &str, author: &str, at_ms: u64) -> Piece {
    Piece {
        role: Role::Context,
        node: node.into(),
        session_id: session.into(),
        origin: Origin::Operator,
        author: Some(author.into()),
        at_ms,
        text: Some(quote.into()),
        first_line: title_from(quote),
        trusted: false,
        superseded_by: None,
    }
}

/// What the task's first turn reads: the owner's brief.
fn brief_text(task_session: &str, home_session: &str, brief: &str) -> String {
    format!(
        "[Task {}: a background task the owner started, under session {}. Work on the brief \
         below on your own. Nobody reads this session while you work, and nobody answers \
         questions asked in it: decide, and say what you decided. Your last message is your \
         report, so end with what you did and what you found. The task graph is the system's \
         business: your own task's record closes when you report, so do not report on it; \
         open your report with the result. You cannot start tasks of your own.]\n\n{brief}",
        short(task_session),
        crate::narrative::short(home_session)
    )
}

#[cfg(test)]
pub(crate) fn brief_text_for_tests(task: &str, home: &str, brief: &str) -> String {
    brief_text(task, home, brief)
}

/// What every row and sentence of one start shares.
struct Common<'a> {
    title: &'a str,
    home_session: &'a str,
    by: &'a str,
    via: &'a str,
    quotes: usize,
    board: bool,
    starts_at_ms: Option<u64>,
}

fn started_fact<'a>(
    c: &Common<'a>,
    short: &'a str,
    session_id: &'a str,
    (limit, available_before): (theseus_kernel::Micros, theseus_kernel::Micros),
) -> crate::fact::task_start::TaskStartedByOwner<'a> {
    crate::fact::task_start::TaskStartedByOwner {
        short,
        title: c.title,
        session_id,
        home_session: c.home_session,
        limit,
        available_before,
        quotes: c.quotes,
        board: c.board,
        starts_at_ms: c.starts_at_ms,
        by: c.by,
        via: c.via,
    }
}

fn invalid(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::INVALID_PARAMS, e.to_string())
}

fn refused(e: String) -> RpcFailure {
    RpcFailure::new(error_code::REFUSED, e)
}

/// A judged refusal is `REFUSED`, with where to start it instead.
fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a task start from {} does not count: {}. {INSTEAD}",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why, "method": method::TASK_START}),
        },
        Err(e) => RpcFailure::invalid(anyhow!("{e:#}")),
    }
}
