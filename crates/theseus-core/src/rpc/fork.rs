//! `session.fork` (theseus-2qzx, R19): a new conversation from any moment of
//! another, by reference. `crate::fork` holds what it stands on.
//!
//! One frame writes the whole fork: the new execution, its session record
//! (`forked_from`), its first compilation (`inherits`: a run of the source's
//! nodes through the fork point, never a copy), the fork edge (`derived_from`
//! the source's compilation that held the node, `via = "fork"`), the source's
//! reverse entry (`includes`, one for the run: `node.reach` reads it) and a
//! `session.forked` row. The source's records are read and never written, so
//! a fork asks no lock of it and a turn there runs on.
//!
//! **Authority** (§3.3, "never broadens"): the fork's execution carries the
//! source execution's authority, and its external-text hold; its spend limit
//! is its own (`budget_usd`, pinned, else the daemon's). A caller on a
//! surface that reads no private text may fork only a session of a shared
//! place; an MCP client may fork only a conversation it opened. The fork
//! speaks in no place (the CLI's and the web UI's: private), so a fork of a
//! shared place's session copies nothing private there and nothing of the
//! fork reaches the shared place.

use std::time::Instant;

use serde_json::json;
use theseus_protocol::{
    error_code, ForkOrigin, Inherits, LedgerKind, SessionForkParams, SessionForkResult,
};
use theseus_store::{kinds, NewRecord};

use super::server::RpcFailure;
use super::Core;
use crate::approval::Surface;
use crate::compiler::{Compilation, Manifest, COMPILATION_SCHEMA};
use crate::graph::{Edge, EdgeKind, VIA_FORK};
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::node::{Body, Node};
use crate::session::{SessionRecord, TargetRef};

/// The most of a compilation's `derived_from` chain read to find the one
/// that held the fork point: past it the fork derives from none.
const CHAIN: usize = 512;

fn invalid(m: impl Into<String>) -> RpcFailure {
    RpcFailure::new(error_code::INVALID_PARAMS, m)
}

fn refused(m: impl Into<String>) -> RpcFailure {
    RpcFailure::new(error_code::REFUSED, m)
}

impl Core {
    /// `session.fork`.
    pub(crate) fn session_fork(
        &self,
        p: SessionForkParams,
        surface: Surface,
    ) -> Result<SessionForkResult, RpcFailure> {
        let started = Instant::now();
        let source = self.session(&p.session)?;
        if source.imported.is_some() {
            return Err(invalid(
                "an imported session is the owner's history, closed and read-only: it is not forked",
            ));
        }
        let src_exec = match &source.execution_id {
            Some(id) => self.kernel.execution(id)?,
            None => None,
        };
        self.fork_allowed(surface, &source, src_exec.as_ref())?;
        let (at_position, at_node) = self.fork_point(&source, &p.at)?;
        let base = self.compilation_holding(&at_node, at_position, &source)?;

        let (profile, limit) = self.fork_target(&p, &source)?;
        let authority = src_exec.map(|e| e.authority).unwrap_or_default();

        let title = p.title.unwrap_or_else(|| {
            crate::fork::default_title(source.title.as_deref(), source.label.as_deref())
        });
        let mut rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        rec.title = Some(title.clone());
        rec.dir.clone_from(&source.dir);
        rec.last_target = profile;
        // What the source read from outside holds the fork too: never less.
        rec.external.clone_from(&source.external);
        let found = |id: &str| self.store.get_node(id).ok().flatten().map(|(p, _)| p);
        rec.data = crate::fork::data_through(source.data.as_deref(), at_position, found);
        let now = theseus_protocol::now_unix_ms();
        let origin = ForkOrigin {
            session: source.session_id.clone(),
            node: at_node.id.clone(),
            position: at_position,
            compilation: base.as_ref().map(|c| c.id.clone()),
            at_ms: now,
        };
        let c = self.fork_compilation(&rec.session_id, &origin, base.as_ref())?;
        rec.compilation_id = Some(c.id.clone());
        rec.forked_from = Some(Box::new(origin));

        let (exec, bytes) = self.fork_frame(
            &mut rec,
            &c,
            (&source, &at_node),
            base.as_ref(),
            (authority, limit),
        )?;
        if self.narrator.on() {
            self.narrator.first_sight(&rec.session_id);
            narrate!(
                self.narrator,
                Session,
                Some(&rec.session_id),
                None,
                "Session {} forked from {} at position {}; its execution {} has a spend limit of {}.",
                crate::narrative::short(&rec.session_id),
                crate::narrative::short(&source.session_id),
                at_position,
                crate::narrative::short(&exec.id),
                crate::narrative::dollars(exec.budget.limit_micros)
            );
        }
        Ok(SessionForkResult {
            session_id: rec.session_id,
            title,
            source_session: source.session_id,
            at_node: at_node.id,
            at_position,
            compilation_id: c.id,
            derived_from: c.derived_from,
            bytes,
            ms: started.elapsed().as_millis() as u64,
            not_restored: crate::fork::NOT_RESTORED.into(),
        })
    }

    /// The profile the fork's turns run on (the one named, else the
    /// source's last) and its own spend limit, when one is asked.
    fn fork_target(
        &self,
        p: &SessionForkParams,
        source: &SessionRecord,
    ) -> Result<(Option<TargetRef>, Option<theseus_kernel::Micros>), RpcFailure> {
        let profile = match &p.profile {
            Some(name) => {
                let (live, _) = self.live_profile();
                let t = self
                    .runner
                    .resolve_target(&live, Some(name), None, None)
                    .map_err(|e| invalid(format!("profile {name:?}: {e:#}")))?;
                Some(TargetRef::from(&t))
            }
            None => source.last_target.clone(),
        };
        if p.budget_usd.is_some_and(|b| !b.is_finite() || b <= 0.0) {
            return Err(invalid("budget_usd must be a positive number of dollars"));
        }
        Ok((profile, p.budget_usd.map(theseus_kernel::usd_to_micros)))
    }

    /// The fork's one frame: its execution, its session record, its first
    /// compilation, the reverse entry into the source's session, the fork
    /// edge, and its row. Returns the execution and the frame's bytes.
    fn fork_frame(
        &self,
        rec: &mut SessionRecord,
        c: &Compilation,
        (source, at): (&SessionRecord, &Node),
        base: Option<&Compilation>,
        (authority, limit): (theseus_kernel::Authority, Option<theseus_kernel::Micros>),
    ) -> Result<(theseus_kernel::Execution, u64), RpcFailure> {
        let mut bytes = 0u64;
        let exec = self.kernel.frame(&[], |k| {
            let exec = k.open_execution(&rec.session_id, rec.kind, authority, limit, None)?;
            rec.execution_id = Some(exec.id.clone());
            bytes += serde_json::to_vec(&exec).map_or(0, |v| v.len() as u64);
            let mut frame = vec![
                NewRecord::json(kinds::SESSION, Some(&rec.session_id), &*rec)?,
                NewRecord::json(kinds::COMPILATION, Some(&c.id), c)?.scoped(&c.session_id),
                // The reverse entry: the source's nodes through the fork
                // point were admitted by this compilation, one for the run.
                Edge::new(EdgeKind::Includes, &c.id, &source.session_id, VIA_FORK).record()?,
            ];
            if let Some(from) = base {
                frame.push(Edge::new(EdgeKind::DerivedFrom, &c.id, &from.id, VIA_FORK).record()?);
            }
            let row = LedgerRow::new(
                LedgerKind::SessionForked,
                Some(&rec.session_id),
                None,
                json!({
                    "source": source.session_id,
                    "node": at.id,
                    "compilation_id": c.id,
                    "derived_from": c.derived_from,
                    "execution_id": exec.id,
                }),
            );
            frame.push(NewRecord::json(kinds::LEDGER, None, &row)?);
            bytes += frame.iter().map(|r| r.payload.len() as u64).sum::<u64>();
            k.stage(&frame)?;
            Ok(exec)
        })?;
        Ok((exec, bytes))
    }

    /// Whether this caller may fork this session. A fork reads the source's
    /// conversation on its owner's behalf: a surface that reads no private
    /// text may fork only a shared place's session (its words are the
    /// place's already), and an MCP client only a conversation it opened.
    fn fork_allowed(
        &self,
        surface: Surface,
        source: &SessionRecord,
        exec: Option<&theseus_kernel::Execution>,
    ) -> Result<(), RpcFailure> {
        if surface == Surface::Mcp {
            return match exec.is_some_and(|e| crate::mcp_server::own(&e.authority)) {
                true => Ok(()),
                false => Err(refused(format!(
                    "{} is not a conversation an MCP client opened: an MCP client forks only its own",
                    source.session_id
                ))),
            };
        }
        if !surface.reads_private()
            && self.runner.class_of(&source.session_id) == crate::places::PlaceClass::Private
        {
            return Err(refused(format!(
                "{} speaks in a private place, which {} does not read: fork it from the CLI or \
                 the web UI",
                source.session_id,
                surface.name()
            )));
        }
        Ok(())
    }

    /// The fork point: the node `at` names (or the last node at or before
    /// the WAL position it gives) in the source's transcript, which must
    /// leave no tool call without its result: a message of the person, or an
    /// answer that made no call.
    fn fork_point(&self, source: &SessionRecord, at: &str) -> Result<(u64, Node), RpcFailure> {
        let sid = &source.session_id;
        let at = at.trim();
        let digits: String = at.trim_start_matches('@').replace([',', '_'], "");
        let position = digits.parse::<u64>().ok();
        // A whole id is one lookup; a name that is its end walks the index's
        // keys (`reach::resolve`), as `node.reach` does.
        if let Some(found) = self.store.get_node(at)?.filter(|_| !at.starts_with('@')) {
            return self.fork_closed(sid, found);
        }
        let named = match crate::reach::resolve(&self.store, at)? {
            crate::reach::Named::One(id) => Some(id),
            crate::reach::Named::Refused(m) => return Err(invalid(m)),
            crate::reach::Named::Unknown(_) if position.is_some() => None,
            crate::reach::Named::Unknown(m) => return Err(invalid(m)),
        };
        let found = match (named, position) {
            (Some(id), _) if !at.starts_with('@') => self.store.get_node(&id)?,
            (_, Some(pos)) => crate::fork::node_at_or_before(&self.store, sid, pos)?,
            _ => None,
        };
        let Some((pos, node)) = found else {
            return Err(invalid(format!("no node of session {sid} is at {at}")));
        };
        self.fork_closed(sid, (pos, node))
    }

    /// `node` as a fork point of session `sid`: in its transcript, and
    /// closing its calls.
    fn fork_closed(&self, sid: &str, (pos, node): (u64, Node)) -> Result<(u64, Node), RpcFailure> {
        if !crate::fork::in_transcript(&self.store, sid, &node.session_id, pos)? {
            return Err(invalid(format!(
                "{} is not in the transcript of session {sid}: it can be forked only at one of \
                 its own nodes or one it inherited",
                node.id
            )));
        }
        let closed = match &node.body {
            Body::UserMessage { .. } => true,
            Body::AssistantMessage { blocks, .. } => !blocks
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use")),
            _ => false,
        };
        if !closed {
            return Err(invalid(format!(
                "{} is not a place to fork: its calls would be left without their results. Fork \
                 at a message of the person, or at an answer that made no tool call",
                node.id
            )));
        }
        Ok((pos, node))
    }

    /// The source's compilation that held the fork point: the one a loop's
    /// answer names, else the latest made before the node, else the oldest
    /// the chain reaches (the node before any compile). None past `CHAIN`.
    fn compilation_holding(
        &self,
        at: &Node,
        position: u64,
        source: &SessionRecord,
    ) -> Result<Option<Compilation>, RpcFailure> {
        if let Body::AssistantMessage {
            compilation_id: Some(id),
            ..
        } = &at.body
        {
            if let Some(c) = self.store.get_compilation(id)? {
                return Ok(Some(c));
            }
        }
        let Some(mut cur) = source
            .compilation_id
            .as_deref()
            .map(|id| self.store.get_compilation(id))
            .transpose()?
            .flatten()
        else {
            return Err(invalid(format!(
                "session {} has no compilation yet: it has taken no turn to fork",
                source.session_id
            )));
        };
        for _ in 0..CHAIN {
            if cur.as_of < position {
                return Ok(Some(cur));
            }
            let Some(prev) = cur
                .derived_from
                .as_deref()
                .map(|id| self.store.get_compilation(id))
                .transpose()?
                .flatten()
            else {
                return Ok(Some(cur));
            };
            cur = prev;
        }
        Ok(None)
    }

    /// The fork's first compilation: the source's manifest (the next turn
    /// recompiles only if the system, the tools or the model changed since),
    /// no thinking carried across (the context it was thought in is another
    /// conversation's), and the inherited run.
    fn fork_compilation(
        &self,
        session: &str,
        origin: &ForkOrigin,
        base: Option<&Compilation>,
    ) -> Result<Compilation, RpcFailure> {
        let Some(base) = base else {
            return Err(invalid(
                "the fork point is older than any compilation the session's chain still names",
            ));
        };
        Ok(Compilation {
            id: crate::new_id("cmp"),
            schema: COMPILATION_SCHEMA,
            session_id: session.into(),
            created_at_ms: theseus_protocol::now_unix_ms(),
            trigger: "fork".into(),
            strategy: "transcript".into(),
            as_of: self.store.last_position(),
            includes: Vec::new(),
            derived_from: Some(base.id.clone()),
            manifest: Manifest {
                strip_thinking: true,
                ..base.manifest.clone()
            },
            budget: None,
            recall_id: None,
            situation: Some(theseus_protocol::Situation::Recompile {
                trigger: "fork".into(),
            }),
            inherits: Some(Inherits {
                session: origin.session.clone(),
                through_node: origin.node.clone(),
                through_position: origin.position,
            }),
        })
    }
}

impl Core {
    /// A fork's history (`session.history`): the nodes it inherits, then its
    /// own, one list by WAL position, paged as any session's is. Only the
    /// page's records are decoded; each inherited node keeps its source's
    /// `session_id`, which every surface reads as "from the source".
    pub(super) fn fork_history_page(
        &self,
        session: &str,
        origin: &ForkOrigin,
        (n, after, before): (Option<usize>, Option<u64>, Option<u64>),
    ) -> anyhow::Result<super::pages::HistoryPage> {
        let paged = after.is_some() || before.is_some();
        let want = n.or(paged.then_some(super::pages::HISTORY_PAGE));
        let within = |r: &theseus_store::Record| {
            after.is_none_or(|a| r.position > a) && before.is_none_or(|b| r.position < b)
        };
        let mut all: Vec<_> = self
            .store
            .scope_after(session, 0)?
            .into_iter()
            .filter(|r| r.kind == kinds::NODE && within(r))
            .collect();
        // The fork's own nodes fill a page of the newest, or one before a
        // cursor, without the source's: the prefix is read only when the
        // page reaches back into it, or forward from before it.
        let enough = after.is_none() && want.is_some_and(|n| all.len() > n);
        if !enough && after.is_none_or(|a| a < origin.position) {
            let mut prefix = crate::fork::prefix_records(&self.store, origin)?;
            prefix.retain(within);
            prefix.append(&mut all);
            all = prefix;
        }
        let (mut next, mut older) = (None, None);
        if let Some(n) = want {
            let more = all.len() > n;
            if after.is_some() {
                all.truncate(n);
                next = more.then(|| all.last().map(|r| r.position)).flatten();
            } else {
                all.drain(..all.len().saturating_sub(n));
                older = (more && before.is_some())
                    .then(|| all.first().map(|r| r.position))
                    .flatten();
            }
        }
        let nodes = all
            .iter()
            .map(|r| Ok((r.position, r.decode()?)))
            .collect::<anyhow::Result<Vec<(u64, Node)>>>()?;
        Ok(super::pages::HistoryPage {
            read: nodes.len(),
            nodes,
            next,
            older,
        })
    }
}
