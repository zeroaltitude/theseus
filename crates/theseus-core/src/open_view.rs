//! The open view (theseus-a1wvq.7): the prefix a request for open data is
//! compiled under, whatever model it goes to. A model of restricted origin
//! may read only content judged open (the owner, 2026-10-17), so such a
//! request carries none of the owner's material. The shared view already
//! says what that is (the place rule), and the open view reuses it: its
//! content readers read `PlaceView::content_class`, the shared class, while
//! the session keeps its own class for approvals, cards, asks and gliding.
//!
//! - **Content**: only the context files marked `readers = "public"`; the
//!   standing orders, never the preferences or the facts; the ontology's
//!   guidance down to the transport's memberships; the session block only
//!   for a directory under the open roots; no thread card; recall only of
//!   open sessions' open stretch (`Place::Open`); no task view or project
//!   slice. A prefix of the open view is keyed `shared`, never `private`.
//! - **Tools** (`Open::offers`, and `Open::refusal` at the gate, so a tool
//!   it is not offered is refused if called): the public tools; the file
//!   tools under the open roots; a program (`runs_tool`) only for a session
//!   whose directory is under them, and only there; nothing else, so no
//!   `memory.lookup`, no glide, no MCP tool, no `aws.*`.
//! - **Which requests**: every turn of a session whose `data.opened` is set
//!   (`TurnRunner::open_of`, read from the record the turn holds), and one
//!   request of any other session through `TurnRunner::open_request`.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use theseus_protocol::Plan;
use theseus_tools::paths;

use crate::ceiling::PlaceView;
use crate::compiler::{Compiled, RequestSpec};
use crate::places::{files_tool, public_tool, shown};
use crate::session::SessionRecord;
use crate::turn::{Target, TurnRunner};

/// What an open request's tools reach: the open roots, and whether its
/// session's directory is within them. Kept once per value and never freed
/// (`Open::of`), so a view stays `Copy` and a catalog keys it by address:
/// one per open-roots value and side, a handful a run.
#[derive(Debug, PartialEq, Eq)]
pub struct Open {
    pub roots: Vec<PathBuf>,
    /// The session's directory is under the roots: its block is kept, and
    /// its programs may run there.
    pub within: bool,
}

/// The tools that run a program: `proc.run`, its jobs, and the terminal.
/// The open view offers them only in a directory under the open roots.
pub fn runs_tool(name: &str) -> bool {
    name == "proc.run" || name.starts_with("job.") || name.starts_with("term.")
}

impl Open {
    /// The reach of a session working in `cwd` under `roots`, kept.
    pub fn of(roots: Vec<PathBuf>, cwd: &Path) -> &'static Open {
        static KEPT: Mutex<Vec<&'static Open>> = Mutex::new(Vec::new());
        let cwd = paths::canonical_best_effort(cwd);
        let within = roots.iter().any(|r| paths::within(&cwd, r));
        let open = Open { roots, within };
        let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(o) = kept.iter().find(|o| ***o == open) {
            return o;
        }
        let o: &'static Open = Box::leak(Box::new(open));
        kept.push(o);
        o
    }

    /// Whether an open request is offered the tool `name`.
    pub fn offers(&self, name: &str) -> bool {
        public_tool(name)
            || (files_tool(name) && !self.roots.is_empty())
            || (runs_tool(name) && self.within)
    }

    /// Why the gate refuses a call of an open request, or None: a tool it is
    /// not offered, or a file's or a program's path outside every open root
    /// (each canonical, so a link out of one is outside it).
    pub fn refusal(&self, name: &str, plan: &Plan) -> Option<String> {
        if !self.offers(name) {
            return Some(format!(
                "{name} is not offered in an open request: one that may go to a model outside \
                 the owner's trust gets only the public tools, and files and programs under the \
                 open paths"
            ));
        }
        if !(files_tool(name) || runs_tool(name)) {
            return None;
        }
        let outside = plan
            .resources
            .iter()
            .map(|r| paths::canonical_best_effort(&r.path))
            .find(|p| !self.roots.iter().any(|root| paths::within(p, root)))?;
        Some(format!(
            "{} is outside the open paths, and this request is open: here {name} reaches only {}",
            outside.display(),
            shown(&self.roots)
        ))
    }

    /// The tools paragraph of an open request: what it is offered, and why.
    pub fn note(&self, postures: &str, ceiling_note: &str) -> String {
        let reach = match (self.roots.is_empty(), self.within) {
            (true, _) => "no files".to_string(),
            (false, false) => format!(
                "files only under {} (give their absolute paths)",
                shown(&self.roots)
            ),
            (false, true) => format!(
                "files and programs only under {} (give their absolute paths)",
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

/// The open roots of `session`: `[routing.data] open_paths`, expanded and
/// canonical, as `places::public_roots` makes `[places] public_paths`. A
/// function of the session, never a stored field: theseus-a1wvq.13 adds a
/// draft task's own tree here, from the task's id.
pub fn open_roots(cfg: &crate::Config, _session: &SessionRecord) -> Vec<PathBuf> {
    cfg.routing
        .data
        .open_paths
        .iter()
        .map(|p| paths::canonical_best_effort(&crate::config::expand(p)))
        .collect()
}

/// One request compiled in the open view (`TurnRunner::open_request`): the
/// view its calls are gated in, its spec (the tools it is offered), and the
/// compiled request.
pub struct OpenRequest {
    pub view: PlaceView,
    pub spec: RequestSpec,
    pub compiled: Compiled,
}

impl TurnRunner {
    /// The reach of `session`'s turns when its data state is open
    /// (`data.opened`): read from the record the turn holds, never the store.
    pub(crate) fn open_of(&self, session: &SessionRecord) -> Option<&'static Open> {
        session.data.as_ref()?.opened.as_ref()?;
        Some(self.reach_of(session))
    }

    fn reach_of(&self, session: &SessionRecord) -> &'static Open {
        let cwd = self.tools.cwd_for(session.dir.as_deref());
        Open::of(open_roots(&self.cfg, session), &cwd)
    }

    /// One request of `session` on `target` in the open view, for a session
    /// that is not open (theseus-a1wvq.8 calls it for a detour in the
    /// owner's DM, once Jev has judged the message open): the session's
    /// newest message alone (no earlier exchange, no recall, no summary),
    /// compiled outside its compilation as a detour is, under the open
    /// view's content and tools. It reads and writes nothing of the session
    /// or the turn: the next compile is the session's own view again. The
    /// calls its answer makes are gated in `view`. Its caller is
    /// theseus-a1wvq.8's; until then its test is.
    pub fn open_request(
        &self,
        session: &SessionRecord,
        target: &Target,
    ) -> Result<OpenRequest> {
        let sid = session.session_id.as_str();
        let view = PlaceView {
            open: Some(self.reach_of(session)),
            ..self.view_of(sid)
        };
        let class = view.content_class();
        let (mut spec, _) = self.request_spec(target, session.kind, view);
        spec.session_text = self.session_block(session.dir.as_deref(), view);
        spec.walk = self.walk(sid, class);
        if let Some(b) = self.standing_block(sid, class) {
            b.apply(&mut spec);
        }
        // The thread's tools as a turn takes them; its card is the owner's.
        self.thread_spec(&mut spec, sid, None);
        spec.card = None;
        spec.defer = self.deferral(sid, session.task.is_some(), &spec);
        let nodes = self.store.transcript(sid)?;
        let Some(at) = nodes.iter().rposition(|(_, n)| {
            n.origin == crate::node::Origin::Operator && n.kind == crate::stub::Kind::UserMessage
        }) else {
            anyhow::bail!("{sid} has no message to compile in the open view");
        };
        let compiled = crate::compiler::compile(crate::compiler::CompileInput {
            session_id: sid,
            current: None,
            nodes: &nodes[at..=at],
            last_position: self.store.last_position(),
            spec: &spec,
            catalog: &self.catalog,
            force: None,
            window_override: self.working_window(&spec),
            blobs: Some(self.store.blobs()),
            hidden: &session.not_shown,
            strip: None,
            overflowed: None,
            sources: &crate::recall::render::Sources::default(),
            signals: None,
            assembled: None,
            situation: &crate::compiler::situation::Situation::Detour,
            kept: None,
        });
        Ok(OpenRequest {
            view,
            spec,
            compiled,
        })
    }
}
