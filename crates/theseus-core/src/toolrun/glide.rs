//! A glide's run (38b), the harness's side of `channel.post` and
//! `channel.read` (`crate::glide`): its places resolved again, the place
//! rule checked again, and its result node with its row, and its post or
//! its hold, written in the one frame that settles the call.
//!
//! The check again is for a call that waited: a place the binding rebound
//! while its question was open may have a stricter class by now, and a call
//! the rule would ask for runs only once approved.

use std::time::Instant;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{Accepted, Completion, Outcome};
use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind_session;
use theseus_store::{kinds, Page};
use theseus_tools::Tool;

use super::{CallOutcome, ResultNode, ToolRuntime, TurnCtx};
use crate::fact::place::{GlidePosted, GlideRead, Published, Where};
use crate::glide::{self, Resolved};
use crate::graph::{Edge, EdgeKind, VIA_GLIDE};
use crate::node::{Node, ResultStatus};
use crate::places::Glide;
use crate::policy::Posture;
use crate::provider::ToolUse;

impl ToolRuntime {
    /// Run a glide: its places and the rule, then the post or the read.
    pub(super) fn run_glide(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
        ran_at: Posture,
    ) -> Result<CallOutcome> {
        Self::harness_started(tc, correlation_id, tool, call);
        let t = (theseus_protocol::now_unix_ms(), Instant::now());
        let resolved = glide::resolve(tc, tool.name(), &call.input).and_then(|r| match &r.rule {
            Glide::AskFirst(why) if ran_at != Posture::Approve => Err(format!(
                "Not run: the place rule asks the owner first ({why}), and this call was not \
                 approved, so nothing moved. Call it again to ask."
            )),
            _ => Ok(r),
        });
        let r = match resolved {
            Ok(r) => r,
            Err(why) => {
                let none = crate::task_graph::tools::Done::default();
                return self.harness_done(tc, correlation_id, tool, call, t, (Err(why), none));
            }
        };
        match tool.name() {
            glide::POST => self.glide_post(tc, correlation_id, tool, call, &r, t),
            _ => self.glide_read(tc, correlation_id, tool, call, &r, t),
        }
    }

    /// The post: one outbox post into the place's lane (`glide:<call>`, its
    /// key there), its `glide.posted` row, and, out of a private place into
    /// a shared one, the publish's `place.published` row, all in the frame
    /// that settles the call. Nothing is posted unless that frame is.
    fn glide_post(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
        r: &Resolved,
        (started, t0): (u64, Instant),
    ) -> Result<CallOutcome> {
        let text = r.text.as_deref().unwrap_or_default();
        let chars = text.chars().count() as u64;
        let body = json!({"kind": "glide", "call": correlation_id, "text": text,
                          "session": crate::task::short(tc.session_id)});
        let (post, mut records) =
            tc.outbox
                .stage(tc.session_id, tc.execution_id, &r.other.target, body)?;
        let posted = GlidePosted {
            session_id: tc.session_id,
            correlation_id,
            from: Where {
                place: r.own_target.as_deref(),
                name: &r.own_name,
            },
            to: Where {
                place: Some(&r.other.target),
                name: &r.other.name,
            },
            chars,
            allowed: r.allowed(),
            why: r.why(),
            post: &post.correlation_id,
        };
        records.push(tc.rec().row(&posted)?);
        let meta = json!({"to": r.other.target, "name": r.other.name, "class": r.other_class,
                          "chars": chars, "post": post.correlation_id, "allowed": r.allowed()});
        let node = self.result_node(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                duration_ms: Some(t0.elapsed().as_millis() as u64),
                meta: meta.clone(),
                ..ResultNode::new(
                    &call.id,
                    tool.name(),
                    ResultStatus::Ok,
                    posted_text(r, chars),
                )
            },
        );
        // Out of a private place into a shared one, the owner's approval made
        // it a publish, recorded as `place.published` is (the place rule).
        let publish = r.publishes().then(|| Publish::of(tc, correlation_id, text));
        let published = publish.as_ref().map(|p| p.fact(r, &node.id));
        if let Some(p) = &published {
            records.push(tc.rec().row(p)?);
        }
        let c = completion(correlation_id, tool, &node, started, &meta, t0);
        let mut frame = vec![node.record()?];
        frame.append(&mut records);
        if !written(&tc.kernel.accept_completion_with(&c, frame)?) {
            // A cancel settled it while it ran: the frame, the post with it,
            // was not written.
            return self.not_moved(tc, correlation_id, tool, call, "posted");
        }
        tc.outbox.posted(&post);
        tc.rec().announce(&posted);
        if let Some(p) = &published {
            tc.rec().announce(p);
        }
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done {
            status: ResultStatus::Ok,
        })
    }

    /// The read: the place's last messages as the call's result, marked
    /// `borrowed from <place>`; from a shared place, outside text, whose
    /// hold rides in the frame that settles it, with its `glide.read` row.
    fn glide_read(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
        r: &Resolved,
        (started, t0): (u64, Instant),
    ) -> Result<CallOutcome> {
        let last = r.last;
        let key = r
            .other
            .target
            .strip_prefix("discord:")
            .unwrap_or(&r.other.target);
        let Some(source) = tc.outbox.place_session(key)? else {
            let why = format!(
                "{} has no conversation yet: it gets one when the binding starts",
                r.other.name
            );
            let none = crate::task_graph::tools::Done::default();
            return self.harness_done(
                tc,
                correlation_id,
                tool,
                call,
                (started, t0),
                (Err(why), none),
            );
        };
        let nodes = tc.store.session_nodes(&source)?;
        let outside = glide::outside(r.other_class);
        let now = theseus_protocol::now_unix_ms();
        let (text, took) = glide::borrowed(&r.other.name, outside, &nodes, last, now);
        let (messages, chars) = (took.len(), text.chars().count() as u64);
        let meta = json!({"from": r.other.target, "name": r.other.name, "class": r.other_class,
                          "borrowed_from": r.other.name, "session_id": source,
                          "messages": messages, "chars": chars, "outside": outside,
                          "allowed": r.allowed()});
        let node = self.result_node(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                duration_ms: Some(t0.elapsed().as_millis() as u64),
                meta: meta.clone(),
                // A shared place's people wrote it: outside text, as a page
                // `http.fetch` read is.
                external: outside.then(|| theseus_tools::External {
                    url: r.other.name.clone(),
                }),
                ..ResultNode::new(&call.id, tool.name(), ResultStatus::Ok, text)
            },
        );
        let read = GlideRead {
            session_id: tc.session_id,
            correlation_id,
            from: Where {
                place: Some(&r.other.target),
                name: &r.other.name,
            },
            to: Where {
                place: r.own_target.as_deref(),
                name: &r.own_name,
            },
            messages,
            chars,
            allowed: r.allowed(),
            why: r.why(),
            outside,
            node_id: &node.id,
        };
        // Content into another context lands with its edges (P0's rule 3):
        // the borrowed node copies each message it took.
        let mut more = vec![tc.rec().row(&read)?];
        for to in &took {
            let edge = Edge::new(EdgeKind::DerivedFrom, &node.id, to, VIA_GLIDE);
            more.push(edge.record()?);
        }
        let c = completion(correlation_id, tool, &node, started, &meta, t0);
        let (accepted, newly) = crate::external::with_hold(
            tc.store,
            tc.session_id,
            Some(tc.turn_id),
            &node,
            |mut frame| {
                frame.append(&mut more);
                tc.kernel.accept_completion_with(&c, frame)
            },
        )?;
        if !written(&accepted) {
            return self.not_moved(tc, correlation_id, tool, call, "read");
        }
        tc.rec().announce(&read);
        Self::announce_end(tc, &node);
        self.held(tc, newly);
        Ok(CallOutcome::Done {
            status: ResultStatus::Ok,
        })
    }

    /// A glide a cancel settled while it ran: its frame was not written, so
    /// nothing moved, and its result says so on a frame of its own.
    fn not_moved(
        &self,
        tc: &TurnCtx<'_>,
        correlation_id: &str,
        tool: &dyn Tool,
        call: &ToolUse,
        what: &str,
    ) -> Result<CallOutcome> {
        let text = format!("Not {what}: the call was cancelled while it ran, so nothing moved.");
        let node = self.result_node(
            tc,
            ResultNode {
                correlation_id: Some(correlation_id),
                ..ResultNode::new(&call.id, tool.name(), ResultStatus::Cancelled, text)
            },
        );
        tc.store.append(&[node.record()?])?;
        Self::announce_end(tc, &node);
        Ok(CallOutcome::Done {
            status: ResultStatus::Cancelled,
        })
    }
}

/// What a post's result tells the model: where it went, and that it goes
/// out once.
fn posted_text(r: &Resolved, chars: u64) -> String {
    let shown = crate::narrative::count(chars, "character", "characters");
    let approved = match r.why() {
        Some(_) => ", with the owner's approval",
        None => "",
    };
    format!(
        "Posted {shown} to {}{approved}: it goes out once, in that place's order.",
        r.other.name
    )
}

/// What an approved post out of a private place records as a publish
/// (`place.published`): who approved it and through what, and its words by
/// digest.
struct Publish {
    who: String,
    via: String,
    source: Value,
    what: String,
    digest: String,
    bytes: u64,
}

impl Publish {
    fn of(tc: &TurnCtx<'_>, correlation_id: &str, text: &str) -> Self {
        use sha2::{Digest, Sha256};
        let (who, via) = approver(tc, correlation_id);
        Self {
            who,
            via,
            source: json!({"glide": correlation_id, "session_id": tc.session_id}),
            what: format!("a post from session {}", crate::task::short(tc.session_id)),
            digest: hex::encode(Sha256::digest(text.as_bytes()))[..16].to_string(),
            bytes: text.len() as u64,
        }
    }

    /// Its fact, for the post `r` resolved, whose result is `node_id`.
    fn fact<'a>(&'a self, r: &'a Resolved, node_id: &'a str) -> Published<'a> {
        Published {
            who: &self.who,
            via: &self.via,
            source: &self.source,
            what: &self.what,
            digest: &self.digest,
            bytes: self.bytes,
            place: &r.other.target,
            name: &r.other.name,
            node_id,
        }
    }
}

/// A glide's completion: it succeeded, with its result node.
fn completion(
    correlation_id: &str,
    tool: &dyn Tool,
    node: &Node,
    started: u64,
    meta: &Value,
    t0: Instant,
) -> Completion {
    Completion {
        correlation_id: correlation_id.into(),
        outcome: Outcome::Succeeded,
        result_ref: Some(node.id.clone()),
        external_op_id: None,
        started_at_ms: started,
        finished_at_ms: theseus_protocol::now_unix_ms(),
        producer: format!("harness:{}", tool.name()),
        signature: None,
        cost_micros: None,
        detail: Some(json!({"duration_ms": t0.elapsed().as_millis() as u64, "meta": meta})),
    }
}

/// Whether the kernel wrote a completion's frame: only when it settled the
/// action, or resolved an unknown one.
fn written(a: &Accepted) -> bool {
    matches!(
        a,
        Accepted::Settled { .. } | Accepted::ResolvedUnknown { .. }
    )
}

/// Who answered the question `correlation_id` asked, and through what, from
/// its `action.confirm_answered` row: a publish's `who` and `via`. One page
/// of the session's answers through the index; "the owner" when it cannot
/// say (its shape is built after serving).
fn approver(tc: &TurnCtx<'_>, correlation_id: &str) -> (String, String) {
    let page = Page {
        kind: kinds::LEDGER,
        tags: vec![ledger_kind_session(
            LedgerKind::ActionConfirmAnswered.as_str(),
            tc.session_id,
        )],
        limit: 50,
        ..Default::default()
    };
    let rows: Vec<crate::ledger::LedgerRow> = tc
        .store
        .ledger_page(&page)
        .ok()
        .flatten()
        .map(|p| p.records)
        .unwrap_or_default()
        .iter()
        .filter_map(|r| r.decode().ok())
        .collect();
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    rows.iter()
        .rev()
        .find(|r| r.data["correlation_id"] == correlation_id)
        .map_or_else(
            || ("the owner".into(), "an approval".into()),
            |r| (s(&r.data["by"]), s(&r.data["via"])),
        )
}
