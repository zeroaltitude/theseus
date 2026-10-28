//! A China-origin draft's view (theseus-a1wvq.7, theseus-a1wvq.13; narrowed by
//! theseus-9ymz0): the prefix and the tool reach a draft session's requests
//! take. The open view this began as is gone, replaced by content flags
//! (`content.rs`), which decide whether a session may use a model of
//! restricted origin at all; what stays is what a draft still needs, which no
//! label gives it: a draft is a model of restricted origin reading a session,
//! so it carries a stripped prefix and a bounded tool reach, or it would be
//! offered `memory.lookup`, glides, MCP tools and the owner's context files.
//!
//! - **Content**: the shared view's (the place rule): only the context files
//!   marked `readers = "public"`; the standing orders, never the preferences
//!   or the facts; the ontology's guidance down to the transport's
//!   memberships; the session block only for a directory under the draft's
//!   tree; no thread card; recall only of public sessions (`Place::Open`); no
//!   task view or project slice. A prefix of the draft view is keyed
//!   `shared`, never `private`.
//! - **Tools** (`DraftReach::offers`, and `DraftReach::refusal` at the gate,
//!   so a tool it is not offered is refused if called): the public tools that
//!   start no later work (`open_public`); the file tools under the draft's
//!   tree; never a program (its tree is under the roots, yet a draft runs
//!   none until its programs run in a view that sees only them,
//!   theseus-a1wvq.16); nothing else, so no `memory.lookup`, no glide, no MCP
//!   tool, no `aws.*`.
//! - **Which requests**: every turn of a draft session (`TurnRunner::reach_of`,
//!   read from the draft's own META record, `draft::is_draft`).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use theseus_protocol::Plan;
use theseus_tools::paths;

use crate::places::{files_tool, shown};
use crate::session::SessionRecord;
use crate::turn::TurnRunner;

/// What a draft's tools reach: its tree, and whether its session's directory
/// is within it. Kept once per value and never freed (`DraftReach::of`), so a
/// view stays `Copy` and a catalog keys it by address: one per tree, a
/// handful a run.
#[derive(Debug, PartialEq, Eq)]
pub struct DraftReach {
    pub roots: Vec<PathBuf>,
    /// The session's directory is under the roots: its block is kept.
    pub within: bool,
}

/// The tools that run a program: `proc.run`, its jobs, and the terminal. A
/// draft is offered none.
pub fn runs_tool(name: &str) -> bool {
    name == "proc.run" || name.starts_with("job.") || name.starts_with("term.")
}

/// The public tools a draft is offered (theseus-qul8y): those that read, or
/// act on a task's own record, by id (`task.update`, `task.close`: the gate
/// refuses another task's, `toolrun::batch`), but none that starts or
/// schedules work outside this request (`task.create`, `task.split`,
/// `task.claim`, `wake.at`), which would run later, on another model, with the
/// owner's trust. An allowlist, so a public tool added later is not offered to
/// a draft until it is judged here. A shared place keeps every public tool.
pub fn open_public(name: &str) -> bool {
    matches!(
        name,
        "web.search"
            | "http.fetch"
            | "tools.find"
            | crate::task_graph::tools::UPDATE
            | crate::task_graph::tools::CLOSE
    )
}

impl DraftReach {
    /// The reach of a draft session working in `cwd` under `roots`, kept.
    pub fn of(roots: Vec<PathBuf>, cwd: &Path) -> &'static DraftReach {
        static KEPT: Mutex<Vec<&'static DraftReach>> = Mutex::new(Vec::new());
        let cwd = paths::canonical_best_effort(cwd);
        let within = roots.iter().any(|r| paths::within(&cwd, r));
        let reach = DraftReach { roots, within };
        let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(r) = kept.iter().find(|r| ***r == reach) {
            return r;
        }
        let r: &'static DraftReach = Box::leak(Box::new(reach));
        kept.push(r);
        r
    }

    /// Whether a draft is offered the tool `name`.
    pub fn offers(&self, name: &str) -> bool {
        open_public(name) || (files_tool(name) && !self.roots.is_empty())
    }

    /// Why the gate refuses a call of a draft, or None: a tool it is not
    /// offered, or a file's path outside its tree (each canonical, so a link
    /// out of it is outside it), or git's own files in it.
    pub fn refusal(&self, name: &str, plan: &Plan) -> Option<String> {
        if !self.offers(name) {
            return Some(format!(
                "{name} is not offered in an open request: one that may go to a model outside \
                 the owner's trust gets only the public tools that start no later work, and files \
                 under its tree"
            ));
        }
        if !files_tool(name) {
            return None;
        }
        let reached: Vec<PathBuf> = plan
            .resources
            .iter()
            .map(|r| paths::canonical_best_effort(&r.path))
            .collect();
        // A draft never reaches git's own files (theseus-a1wvq.13): the core's
        // git calls in its tree read them.
        if let Some(g) = reached
            .iter()
            .find(|p| p.components().any(|c| c.as_os_str() == ".git"))
        {
            return Some(format!(
                "{} is git's own, and a draft edits only its tree's files",
                crate::places::home_shown(g)
            ));
        }
        let outside = reached
            .iter()
            .find(|p| !self.roots.iter().any(|root| paths::within(p, root)))?;
        Some(format!(
            "{} is outside the open paths, and this request is open: here {name} reaches only {}",
            crate::places::home_shown(outside),
            shown(&self.roots)
        ))
    }

    /// The tools paragraph of a draft's request: what it is offered, and why.
    pub fn note(&self, postures: &str, ceiling_note: &str) -> String {
        let reach = match self.roots.is_empty() {
            true => "no files".to_string(),
            false => format!(
                "files only under {} (give their absolute paths)",
                shown(&self.roots)
            ),
        };
        format!(
            "Tools. You act through tools; every call is recorded, checked against policy, and may wait for the operator's confirmation.\n\
             - This request is open: it may go to a model outside the operator's trust, so it carries nothing of theirs. You are offered only the public tools here, and {reach}.\n\
             - Postures (open runs; notify runs and tells the operator; approve waits for the operator's approval): {postures}.\n\
             - A declined call is final for that request: tell the operator and do not route around it.{ceiling_note}"
        )
    }
}

/// A draft session's roots: its own tree alone, from its id
/// (theseus-a1wvq.13). The owner's directory is never among them: only a
/// reviewed change reaches it.
pub fn draft_roots(cfg: &crate::Config, draft_session: &str) -> Vec<PathBuf> {
    let tree = crate::draft::tree_of(cfg, draft_session);
    vec![paths::canonical_best_effort(&tree)]
}

impl TurnRunner {
    /// The reach of `session`'s turns when it is a draft's, read from its META
    /// record (`draft::is_draft`); None for every other session, which pays
    /// one test of `task` and, for a task's, one read.
    pub(crate) fn reach_of(&self, session: &SessionRecord) -> Option<&'static DraftReach> {
        if !crate::draft::is_draft(&self.store, session) {
            return None;
        }
        let cwd = self.tools.cwd_for(session.dir.as_deref());
        let roots = draft_roots(&self.cfg, &session.session_id);
        Some(DraftReach::of(roots, &cwd))
    }
}
