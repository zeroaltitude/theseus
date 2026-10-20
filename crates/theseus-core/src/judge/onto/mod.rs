//! Jev files the session into the ontology (theseus-xzxqv; the design of
//! `docs/design/jev-ontology-growth.md`, sections 3.1 and 4): `onto.v1` rides
//! the inbound batch, and its answers act at the act band.
//!
//! - **Asked** ([`JudgeService::onto_wanted`], `inbound.rs`): a person's
//!   message in a private place, never a task's, while `[ontology] judge` is
//!   on. The state is the inbound state plus the ontology's fields
//!   ([`input`]); `route.v6`, which the turn waits on, never carries them.
//! - **Landed** ([`JudgeService::onto_landed`]): when the batch comes back the
//!   decision is made against the ontology as it stands ([`decide`]) and only
//!   its words are used: the reply's footer, and the naming call for a `new`
//!   no mined name fits ([`name`]). Nothing is written here and the turn
//!   waits for nothing of it.
//! - **Written** (`sink.rs`, [`stage`]): the sink frames the judgment's row;
//!   the decision is made again, under the ontology's writer lock, and its
//!   records, their rows and the owner's cards ride that same frame.
//!
//! Interpretations route context and never grant access: a membership takes
//! effect at the next recompile, and a shared place is never asked.

use std::sync::Mutex;

use serde_json::json;
use theseus_judge::builders::{InboundInput, OntoInput, OntoItem, OntoKind, OntoTable};
use theseus_judge::Judgment;
use theseus_ontology::{Basis, Ontology, Origin};
use tokio::time::Instant;

use super::inbound::Inbound;
use super::offer::{Book, Owed, Waiting};
use super::JudgeService;
use crate::rpc::Core;

pub mod decide;
pub mod mark;
mod name;
pub(super) mod stage;

pub use name::NAMES;

/// `onto.v1` (theseus-xzxqv), at `inbound`, in the batch beside `classify.v3`
/// and `role.v1`.
pub const ONTO_PACK: &str = "onto.v1";

/// What the footers and the naming calls hold: in memory, bounded.
#[derive(Default)]
pub struct Point {
    /// The `filed:` line of each turn, until its reply takes it; and the
    /// replies written before their judgment landed.
    filed: Mutex<Book>,
}

impl JudgeService {
    /// Whether `onto.v1` is asked about this message: a person's, in a
    /// private place, not a task's, and the switch on. A shared place's
    /// message files nothing.
    pub(super) fn onto_wanted(&self, msg: &Inbound) -> bool {
        !msg.task
            && msg.place_kind.ends_with("(private)")
            && self.core().is_some_and(|c| c.cfg.ontology.judge)
    }

    /// Whether `onto.v1` goes in a request of its own (`[ontology] request`).
    pub(super) fn onto_own(&self) -> bool {
        self.core()
            .is_some_and(|c| c.cfg.ontology.request == crate::config::ontology::Request::Own)
    }

    /// The batch found where `turn`'s session is filed. A reply already
    /// written without the line gets it late, as a notice; else it waits for
    /// the reply. Never waited on.
    pub(super) fn note_filed(&self, turn: &str, line: &str) {
        let late = {
            let mut book = self.onto.filed.lock().unwrap_or_else(|e| e.into_inner());
            book.note(turn, line, Instant::now())
        };
        if let Some(o) = late {
            self.filed_late(o, line.to_string());
        }
    }

    /// The footer line of `turn`'s reply, when its judgment had landed by the
    /// time the reply was written. When it had not, the line comes as a
    /// notice when it does (within the offer's two minutes).
    pub fn take_filed(&self, w: Waiting<'_>) -> Option<String> {
        let mut book = self.onto.filed.lock().unwrap_or_else(|e| e.into_inner());
        book.take(&w, Instant::now())
    }

    fn filed_late(&self, o: Owed, line: String) {
        let Some(core) = self.core() else { return };
        tokio::task::spawn_blocking(move || {
            let body = json!({"kind": "notice", "text": line});
            if let Err(e) = core.outbox.post(&o.session, &o.execution, &o.target, body) {
                tracing::warn!(error = %format!("{e:#}"), "the late filing line was not posted");
            }
        });
    }

    /// `onto.v1`'s judgment, landed: the reply's footer, and the naming call
    /// where Jev said `new` and no mined name fit. Only a judgment that acts
    /// (live or canary, answered by the pinned model) does either.
    pub(super) fn onto_landed(&self, j: &Judgment, turn: &str, said: &str) {
        if !(j.pack == ONTO_PACK && j.actionable() && j.mode != theseus_judge::Mode::Shadow) {
            return;
        }
        let Some(core) = self.core() else { return };
        let Some(o) = core.runner.ontology.held() else {
            return;
        };
        let reasons = mark::decide(j, core.cfg.ontology.private_mark, &stage::bands(&core));
        if !reasons.is_empty() {
            // The mark's write and undo wait for the session-data state
            // (theseus-a1wvq.5): until then the decision is a trace.
            tracing::info!(
                session = %j.id,
                reasons = %reasons.iter().map(|r| r.word()).collect::<Vec<_>>().join(","),
                "onto: the private mark would apply"
            );
        }
        let plan = decide::decide(j, &o, stage::bands(&core), theseus_protocol::now_unix_ms());
        if let Some(line) = plan.footer() {
            self.note_filed(turn, &line);
        }
        for kind in plan.unnamed() {
            name::name_when_due(&core, &plan.session, kind, said);
        }
    }
}

/// The ontology fields of `input`, for a message in `msg`'s session: the
/// kinds Jev may assign, with every live category of each, the thread's and
/// project's titles, and what the naming call suggested. False when the
/// ontology cannot be read: nothing is asked then.
pub(super) fn fill(core: &Core, msg: &Inbound, input: &mut InboundInput) -> bool {
    let Ok(o) = core.runner.ontology.snapshot(&core.store) else {
        return false;
    };
    input.onto = Some(Box::new(onto_input(core, &o, msg)));
    true
}

/// The input: see [`fill`]. A category's last use is when a session last held it.
fn onto_input(core: &Core, o: &Ontology, msg: &Inbound) -> OntoInput {
    let sid = &msg.session_id;
    let threads = core.threads();
    let thread = threads.of_session(sid);
    let project = core
        .store
        .get_session::<crate::session::SessionRecord>(sid)
        .ok()
        .flatten()
        .and_then(|r| r.project);
    let project_title = project.as_deref().and_then(|id| {
        crate::task_graph::all(&core.store)
            .ok()?
            .into_iter()
            .find(|t| t.id == id)
            .map(|t| t.title)
    });
    // When each category was last held, from every session's lists.
    let mut used: std::collections::HashMap<&theseus_ontology::CategoryId, u64> =
        Default::default();
    for l in o.lists() {
        for m in &l.members {
            let at = used.entry(&m.category).or_default();
            *at = (*at).max(m.as_of_ms);
        }
    }
    let kinds: Vec<OntoKind> = o
        .kinds()
        .into_iter()
        .filter(|k| {
            k.basis == Basis::Interpreted
                && k.retired_ms.is_none()
                && k.assigned_by.contains(&Origin::Jev)
                && theseus_judge::builders::ONTO_KINDS.contains(&k.name.as_str())
        })
        // The thread has a project: `project` is not asked.
        .filter(|k| !(k.name == "project" && project.is_some()))
        .map(|k| {
            let held = o.member_list(sid, &k.name);
            let pool = o
                .categories()
                .filter(|c| c.kind() == k.name)
                .map(|c| OntoItem {
                    id: c.id.to_string(),
                    name: o
                        .path(&c.id)
                        .iter()
                        .map(|p| p.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" › "),
                    description: c.description.clone(),
                    session: held.is_some_and(|l| l.members.iter().any(|m| m.category == c.id)),
                    thread: k.name == "topic"
                        && thread.is_some_and(|t| t.topics.iter().any(|x| *x == c.id.as_str())),
                    used_ms: used.get(&c.id).copied().unwrap_or(0),
                })
                .collect();
            OntoKind {
                kind: k.name.clone(),
                meaning: meaning(&k.description, &k.examples),
                pool,
                mined_from: k
                    .mined_from
                    .iter()
                    .filter_map(|m| serde_json::to_value(m).ok()?.as_str().map(str::to_string))
                    .collect(),
            }
        })
        .collect();
    let table = o
        .kinds()
        .into_iter()
        .filter(|k| k.basis == Basis::Interpreted && k.retired_ms.is_none())
        .map(|k| OntoTable {
            name: k.name.clone(),
            meaning: meaning(&k.description, &k.examples),
        })
        .collect();
    OntoInput {
        thread_title: thread.map(|t| t.title.clone()),
        project_title,
        kinds,
        table,
        suggested: name::suggested(core, sid),
        ..OntoInput::default()
    }
}

/// What a kind is for, with its examples when it has some.
fn meaning(description: &str, examples: &[String]) -> String {
    match examples.is_empty() {
        true => description.to_string(),
        false => format!("{description} (such as {})", examples.join(", ")),
    }
}

/// The context `onto.v1`'s judgment carries, from which its answers read:
/// the category ids each kind's options name (`c1` is the first), the mined
/// names (`n1`), as the builder offered them, and the one-line descriptions
/// the naming call gave its suggestions.
pub(super) fn context(core: &Core, session: &str, input: &InboundInput) -> serde_json::Value {
    let Some(o) = &input.onto else {
        return serde_json::Value::Null;
    };
    let kinds: serde_json::Map<String, serde_json::Value> = o
        .pick(&input.message)
        .iter()
        .map(|p| {
            let ids: Vec<&str> = p.candidates.iter().map(|c| c.id.as_str()).collect();
            (p.kind.clone(), json!(ids))
        })
        .collect();
    json!({"kinds": kinds, "names": o.names(&input.message, input.previous_human_message.as_deref()),
           "described": name::described(core, session)})
}
