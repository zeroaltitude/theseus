//! Jev files the session into the ontology (theseus-xzxqv; the design of
//! `docs/design/jev-ontology-growth.md`, sections 3.1 and 4): `onto.v2` rides
//! the inbound batch, and its answers act at the act band.
//!
//! - **Asked** ([`JudgeService::onto_wanted`], `inbound.rs`): a person's
//!   message in any place, never a task's, while `[ontology] judge` is
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
//! - **Swept** ([`sweep`], theseus-63tfk): nightly, a machine membership
//!   unconfirmed for 30 days is dropped and a duplicate category a machine
//!   wrote is merged into its twin; three sessions' `other` answers about one
//!   name mint a kind ([`mint`], theseus-81zx7); and Jev judges pairs that look
//!   alike and categories too large to say anything ([`judged`], theseus-nzzz4,
//!   under its own daily cap); each an undoable write.
//!
//! Interpretations route context and never grant access: a membership takes
//! effect at the next turn, regardless of place.

use std::sync::Mutex;

use serde_json::json;
use theseus_judge::builders::{InboundInput, OntoInput, OntoItem, OntoKind, OntoTable, OntoText};
use theseus_judge::pic::Picture;
use theseus_judge::Judgment;
use theseus_ontology::{Basis, Ontology, Origin};
use tokio::time::Instant;

use super::inbound::Inbound;
use super::offer::{Book, Owed, Waiting};
use super::JudgeService;
use crate::node::AttachmentContent;
use crate::rpc::Core;

pub(crate) mod class;
pub mod decide;
pub mod digest;
pub mod history;
pub mod judged;
pub mod mint;
pub(crate) mod name;
pub mod renamed;
pub(crate) mod stage;
pub mod sweep;
mod tidy;
pub(crate) mod words;

#[cfg(test)]
mod tests_history;

pub use name::NAMES;

/// `onto.v2` (issue 46; `onto.v1`, theseus-xzxqv, without the questions that
/// fed the private mark), at `inbound`, in the batch beside `classify.v5`.
pub const ONTO_PACK: &str = "onto.v2";

/// What the footers and the naming calls hold: in memory, bounded.
#[derive(Default)]
pub struct Point {
    /// The `filed:` line of each turn, until its reply takes it; and the
    /// replies written before their judgment landed.
    filed: Mutex<Book>,
    naming: Mutex<std::collections::HashSet<(String, String)>>,
}

impl JudgeService {
    /// Label a person's message in every place, never a task's or our own.
    pub(super) fn onto_wanted(&self, msg: &Inbound) -> bool {
        !msg.task && !msg.own && self.core().is_some_and(|c| c.cfg.ontology.judge)
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

    /// The onto pack's judgment, landed: the reply's footer, and the naming call
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
/// kinds Jev may assign, with every live category of each, the
/// project's title, and what the naming call suggested. False when the
/// ontology cannot be read: nothing is asked then.
pub(super) fn fill(core: &Core, msg: &Inbound, input: &mut InboundInput) -> bool {
    let Ok(o) = core.runner.ontology.snapshot(&core.store) else {
        return false;
    };
    let mut onto = onto_input(core, &o, msg);
    let (pictures, texts) = attached(core, msg);
    onto.pictures = pictures;
    onto.texts = texts;
    onto.shared = !msg.place_kind.ends_with("(private)");
    input.onto = Some(Box::new(onto));
    true
}

/// The message's pictures, prepared from the blobs' bytes (a picture whose
/// blob is gone is a skipped row), and its text attachments.
fn attached(core: &Core, msg: &Inbound) -> (Vec<Picture>, Vec<OntoText>) {
    let mut images: Vec<(String, Vec<u8>)> = Vec::new();
    let mut texts = Vec::new();
    for a in &msg.attachments {
        match &a.content {
            AttachmentContent::Image { digest, .. } => {
                let bytes = core.store.blobs().read(digest).unwrap_or_default();
                images.push((a.name.clone(), bytes));
            }
            AttachmentContent::Text { text, .. } => texts.push(OntoText {
                name: a.name.clone(),
                text: text
                    .chars()
                    .take(theseus_judge::builders::ONTO_EXCERPT_CHARS)
                    .collect(),
            }),
            _ => {}
        }
    }
    (theseus_judge::pic::prepare_all(&images), texts)
}

/// The input: see [`fill`]. A category's last use is when a session last held it.
pub(crate) fn onto_input(core: &Core, o: &Ontology, msg: &Inbound) -> OntoInput {
    let sid = &msg.session_id;
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
    // When each category was last held, across every session's lists.
    let used = o.last_held();
    let mut kinds: Vec<OntoKind> = o
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
                    session: held
                        .as_ref()
                        .is_some_and(|l| l.members.iter().any(|m| m.category == c.id)),
                    thread: false,
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
    // The thread has no project: Jev may attach one that exists. A project is
    // the owner's to make, so only a listed one is offered and `new` files
    // nothing (theseus-jrf4c).
    if project.is_none() {
        kinds.extend(project_kind(core, o));
    }
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
        thread_title: None,
        project_title,
        kinds,
        table,
        suggested: name::suggested(core, sid),
        private_dirs: vec![core
            .tools
            .self_pr
            .state_dir()
            .to_string_lossy()
            .into_owned()],
        ..OntoInput::default()
    }
}

/// The `project` question's kind: every open project, as a candidate. None
/// when there is none, or the kind is not in the table.
fn project_kind(core: &Core, o: &Ontology) -> Option<OntoKind> {
    let k = o.kind("project").filter(|k| k.retired_ms.is_none())?;
    let pool: Vec<OntoItem> = crate::task_graph::all(&core.store)
        .ok()?
        .into_iter()
        .filter(|t| crate::project::is_project(t) && !t.state.is_closed())
        .map(|t| OntoItem {
            id: t.id,
            name: t.title,
            description: t.objective,
            session: false,
            thread: false,
            used_ms: 0,
        })
        .collect();
    (!pool.is_empty()).then(|| OntoKind {
        kind: "project".into(),
        meaning: meaning(&k.description, &k.examples),
        pool,
        mined_from: Vec::new(),
    })
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
    // The projects a candidate id names, for the footer and the card.
    let projects: serde_json::Map<String, serde_json::Value> = o
        .pick(&input.message)
        .iter()
        .filter(|p| p.kind == "project")
        .flat_map(|p| p.candidates.iter().map(|c| (c.id.clone(), json!(c.name))))
        .collect();
    let mut context = json!({"shared": o.shared, "kinds": kinds, "names": o.names(&input.message, input.previous_human_message.as_deref()),
           "described": name::described(core, session)});
    if !projects.is_empty() {
        context["projects"] = projects.into();
    }
    context
}
