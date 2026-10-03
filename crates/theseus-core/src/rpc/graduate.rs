//! Graduation (M4 19c; design m4-boundaries §2.7): the only way an audience
//! widens. Relabelling in place is not an operation (Appendix F): the operator
//! writes a new node, with the source's content, wider readers, the source's
//! integrity, and a warrant (who, how, why, and when), and the session's next
//! compile admits it as an append. The source keeps its own label, and its
//! placeholder stays where it was, since its call's pairing needs it.
//!
//! It widens what the model may draw on for its audience, so it is judged as
//! an approval is (`judge_act`): never from a Theseus job's process (J1), and
//! under `[approval]` only from a trusted user through a trusted channel.
//! Ledgered as `label.graduated`, in the frame that writes the node.

use anyhow::{anyhow, bail, Result};
use serde_json::json;
use theseus_protocol::{error_code, GraduateResult, LabelGraduateParams, Readers, Warrant};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::labels::{self, GraduateTo};
use crate::node::{Attachment, Body, Node};
use crate::session::SessionRecord;

/// What a graduation carries from its source: the text the model reads, and
/// any files with it.
struct Carried {
    what: String,
    text: String,
    attachments: Vec<Attachment>,
}

/// What a node's content is, as a graduated copy carries it. A call has none
/// of its own: its result, or the answer that made it, is what is graduated.
fn carried(n: &Node) -> Result<Carried> {
    let c = match &n.body {
        Body::ToolResult {
            tool,
            content,
            image,
            ..
        } => Carried {
            what: format!("{tool}'s result"),
            text: content.clone(),
            attachments: image.clone().into_iter().collect(),
        },
        Body::UserMessage { text, attachments } => Carried {
            what: "a message".into(),
            text: text.clone(),
            attachments: attachments.clone(),
        },
        Body::AssistantMessage { blocks, .. } => Carried {
            what: "an earlier answer".into(),
            text: crate::provider::text_of(blocks),
            attachments: Vec::new(),
        },
        Body::ToolCall { tool, .. } => bail!(
            "{} is a call to {tool}, which has no content of its own: graduate its result, or \
             the answer that made it",
            n.id
        ),
    };
    if c.text.trim().is_empty() && c.attachments.is_empty() {
        bail!("{} has nothing to graduate: it is empty", n.id);
    }
    Ok(c)
}

/// The text the model reads in the graduated node: what it is, who widened
/// it to whom and why, then the content itself.
fn graduated_text(source: &Node, c: &Carried, readers: &Readers, w: &Warrant) -> String {
    format!(
        "[Graduated by the operator ({}): {} ({}), now readable {}. Why: {}]\n{}",
        w.who,
        c.what,
        source.id,
        readers.describe(),
        w.why,
        c.text
    )
}

impl Core {
    /// `label.graduate`: a new node in the source's session, with its
    /// content, `to`'s readers, and the warrant. Refused, with nothing
    /// written, for a node that is not there, a call, an empty node, readers
    /// that widen nothing, an empty reason, a `place` the session does not
    /// have, a running turn, or an answer that does not count.
    pub fn graduate(
        &self,
        node_id: &str,
        to: &str,
        why: &str,
        by: impl Into<Answerer>,
    ) -> Result<GraduateResult> {
        let who = by.into();
        let (_, source) = self
            .store
            .get_node(node_id)?
            .ok_or_else(|| anyhow!("no node is named {node_id}"))?;
        let c = carried(&source)?;
        let sid = source.session_id.clone();
        let place = self.outbox.target(&sid);
        let readers = match labels::parse_graduate_to(to)? {
            GraduateTo::Public => Readers::Public,
            GraduateTo::People(p) => Readers::People(p),
            GraduateTo::Place if place.is_none() => bail!(
                "session {sid} posts nowhere, so it has no place to graduate to: name the \
                 people, or `public`"
            ),
            GraduateTo::Place => labels::readers_of(place.as_deref()),
        };
        let now_readers = source.label.as_ref().map_or_else(
            || labels::readers_of(place.as_deref()),
            |l| l.readers.clone(),
        );
        if now_readers == Readers::Public {
            bail!("{node_id} is public already: anyone may read it");
        }
        if now_readers == readers {
            bail!("{node_id} is readable {} already", readers.describe());
        }
        let why = why.trim();
        if why.is_empty() {
            bail!("a graduation needs its reason: say why with --why \"…\"");
        }
        let asker = self.judge_act(
            &who,
            Act::Graduate {
                node: node_id,
                session: &sid,
            },
        )?;
        let warrant = Warrant {
            graduated_from: node_id.to_string(),
            who: who.who(),
            how: who.via(),
            why: why.to_string(),
            at_ms: theseus_protocol::now_unix_ms(),
        };
        let label = labels::graduated(
            source.label.as_ref(),
            held_by_marker(&source),
            readers.clone(),
            warrant.clone(),
        );
        let text = graduated_text(&source, &c, &readers, &warrant);
        let node = Node::user_with(&sid, None, &who.label, &text, c.attachments).labeled(label);
        self.write_graduated(&sid, &node, &source, &asker)?;
        let judge = self.runner.judge(&sid, place.as_deref());
        Ok(GraduateResult {
            node_id: node.id,
            session_id: sid,
            covers: judge.covers(&readers),
            audience: judge.audience,
            readers,
            warrant,
        })
    }

    /// The graduated node, its `derived_from` edge to the source, and its
    /// row, in one frame under the execution's lock, once no turn holds the
    /// session: a turn owns its transcript until it ends. Then the session's
    /// watchers hear of the node.
    fn write_graduated(
        &self,
        sid: &str,
        node: &Node,
        source: &Node,
        asker: &crate::peer::Traced,
    ) -> Result<()> {
        let rec = self
            .store
            .get_session::<SessionRecord>(sid)?
            .ok_or_else(|| anyhow!("no session is named {sid}"))?;
        let exec = rec
            .execution_id
            .ok_or_else(|| anyhow!("session {sid} has never run a turn"))?;
        let f = fact::label::Graduated {
            node,
            source,
            asker: asker.json(),
        };
        let row = fact::row(&f, Some(sid), None)?;
        let edge = crate::graph::Edge::new(
            crate::graph::EdgeKind::DerivedFrom,
            &node.id,
            &source.id,
            crate::graph::VIA_GRADUATE,
        );
        self.kernel.frame(&[&exec], |k| {
            let e = k
                .execution(&exec)?
                .ok_or_else(|| anyhow!("session {sid}'s execution is not in the store"))?;
            if k.holds_turn(&exec) {
                bail!(
                    "a turn is running in session {sid}: graduate once it ends (`theseus wait \
                     {sid}`)"
                );
            }
            if e.state.is_terminal() {
                bail!(
                    "session {sid} has ended ({}): nothing compiles it again",
                    e.state.as_str()
                );
            }
            k.stage(&[node.record()?, edge.record()?, row.clone()])?;
            Ok(())
        })?;
        let rec = self.session_rec(sid);
        rec.announce(&f);
        rec.announce(&fact::turn::NodeWritten {
            session_id: sid,
            node,
        });
        Ok(())
    }

    pub(super) fn label_graduate(
        &self,
        p: LabelGraduateParams,
        conn: Conn<'_>,
    ) -> Result<GraduateResult, RpcFailure> {
        // Named as a trust names its surface or person (`Conn::answerer`).
        let who = conn.answerer(p.author, p.discord);
        self.graduate(&p.node_id, &p.to, &p.why, who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "graduating {} from {} does not count: {}. Who may read it is unchanged.",
                        p.node_id, r.who, r.why
                    ),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })
    }
}

/// A source from before labels that DD5 marked external: where its text came
/// from, so its graduated copy stays untrusted (graduation never touches
/// integrity).
fn held_by_marker(n: &Node) -> Option<theseus_protocol::ExternalText> {
    if n.label.is_some() {
        return None;
    }
    match &n.body {
        Body::ToolResult {
            tool,
            external: Some(e),
            ..
        } => Some(crate::external::read(
            &n.id,
            tool,
            &e.url,
            None,
            n.created_at_ms,
        )),
        _ => None,
    }
}
