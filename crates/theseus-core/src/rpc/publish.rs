//! `place.publish` (the place rule, theseus-nbsh; graduation, light): the
//! owner, from a private place, puts a chosen item into a place's
//! conversation: a node by id, a file by path, or a message. It is written
//! there as the owner's message, said in the place as a notice, and recorded
//! as one `place.published` row. Nothing else carries the owner's material
//! into a shared place.

use anyhow::{anyhow, bail, Result};
use serde_json::json;
use theseus_protocol::{error_code, PlacePublishParams, PublishResult};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::node::{Body, Node};
use crate::session::SessionRecord;

/// The most of a published item the place's notice shows; the conversation
/// gets it whole.
const NOTICE_CHARS: usize = 1_500;

/// What is published: its words for the row and the notice, its source for
/// the row, and its text.
struct Item {
    what: String,
    source: serde_json::Value,
    text: String,
    /// The node it copies, for its `derived_from` edge.
    node: Option<String>,
}

impl Core {
    /// Publish `p` into the place it names, for `who`.
    pub fn publish(
        &self,
        p: &PlacePublishParams,
        who: impl Into<Answerer>,
    ) -> Result<PublishResult> {
        let who = who.into();
        let place = self.runner.place_rule.find(&p.to).ok_or_else(|| {
            anyhow!(
                "no bound place is named {}: `theseus places` lists them",
                p.to
            )
        })?;
        let key = place
            .target
            .strip_prefix("discord:")
            .ok_or_else(|| anyhow!("{} is not a Discord place", place.target))?;
        let sid = self.outbox.place_session(key)?.ok_or_else(|| {
            anyhow!(
                "{} has no session yet: it gets one when the binding starts",
                place.name
            )
        })?;
        // The owner, from a private place (`judge_act`, the place rule).
        self.judge_act(
            &who,
            Act::Publish {
                place: &place.target,
            },
        )?;
        // Read only once who asks may publish: a refusal says nothing of
        // what it named, not even whether a file exists, or its size.
        let item = self.item(p)?;
        let header = match p.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            Some(note) => format!(
                "[Published here by the owner ({}): {}. Their note: {note}]",
                who.who(),
                item.what
            ),
            None => format!(
                "[Published here by the owner ({}): {}]",
                who.who(),
                item.what
            ),
        };
        let node = Node::user_with(
            &sid,
            None,
            &who.label,
            &format!("{header}\n{}", item.text),
            vec![],
        );
        let digest = {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(item.text.as_bytes()))[..16].to_string()
        };
        let class = self.runner.place_rule.class(&self.cfg, Some(&place.target));
        let published = fact::place::Published {
            who: &who.who(),
            via: &who.via(),
            source: &item.source,
            what: &item.what,
            digest: &digest,
            bytes: item.text.len() as u64,
            place: &place.target,
            name: &place.name,
            node_id: &node.id,
        };
        self.write_published(&sid, &node, &item, &published, &place.target, &header)?;
        Ok(PublishResult {
            place: place.target.clone(),
            name: place.name.clone(),
            class,
            session_id: sid,
            node_id: node.id,
            what: item.what,
            bytes: item.text.len() as u64,
            digest,
        })
    }

    /// The item `p` names: exactly one of a node, a file, or a message.
    fn item(&self, p: &PlacePublishParams) -> Result<Item> {
        match (&p.node_id, &p.path, &p.text) {
            (Some(id), None, None) => {
                let (_, n) = self
                    .store
                    .get_node(id)?
                    .ok_or_else(|| anyhow!("no node is named {id}"))?;
                let (what, text) = match &n.body {
                    Body::ToolResult { tool, content, .. } => (format!("{tool}'s result {id}"), content.clone()),
                    Body::UserMessage { text, .. } => (format!("the message {id}"), text.clone()),
                    Body::AssistantMessage { blocks, .. } => {
                        (format!("the answer {id}"), crate::provider::text_of(blocks))
                    }
                    Body::ToolCall { tool, .. } => bail!(
                        "{id} is a call to {tool}, which has no content of its own: publish its result"
                    ),
                };
                Ok(Item {
                    what,
                    source: json!({"node_id": id, "session_id": n.session_id}),
                    text,
                    node: Some(id.clone()),
                })
            }
            (None, Some(path), None) => {
                let full = crate::config::expand(path);
                let meta = std::fs::metadata(&full).map_err(|e| anyhow!("{}: {e}", full.display()))?;
                if !meta.is_file() {
                    bail!("{} is not a regular file", full.display());
                }
                let max = self.cfg.tools.max_read_bytes;
                if meta.len() > max as u64 {
                    bail!("{} is {} bytes, over the {max} a read may take", full.display(), meta.len());
                }
                let bytes = std::fs::read(&full).map_err(|e| anyhow!("{}: {e}", full.display()))?;
                Ok(Item {
                    what: format!("the file {}", full.display()),
                    source: json!({"path": full}),
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                    node: None,
                })
            }
            (None, None, Some(text)) => Ok(Item {
                what: "a message".into(),
                source: json!({"text": true}),
                text: text.clone(),
                node: None,
            }),
            _ => bail!("name one thing to publish: a node, a file, or a message"),
        }
        .and_then(|i| match i.text.trim().is_empty() {
            true => bail!("{} is empty: nothing to publish", i.what),
            false => Ok(i),
        })
    }

    /// The published node (with its `derived_from` edge when it copies a
    /// node), its row, and the place's notice, in one frame under the
    /// place's execution's lock, once no turn holds its session: a turn owns
    /// its transcript until it ends.
    fn write_published(
        &self,
        sid: &str,
        node: &Node,
        item: &Item,
        f: &fact::place::Published<'_>,
        target: &str,
        header: &str,
    ) -> Result<()> {
        let rec = self
            .store
            .get_session::<SessionRecord>(sid)?
            .ok_or_else(|| anyhow!("no session is named {sid}"))?;
        let exec = rec.execution_id.ok_or_else(|| {
            anyhow!("session {sid} has never run a turn: say something there first")
        })?;
        let row = fact::row(f, Some(sid), None)?;
        let excerpt: String = item.text.chars().take(NOTICE_CHARS).collect();
        let more = if excerpt.len() < item.text.len() {
            format!(
                "\n… ({} bytes in all; Theseus has it whole)",
                item.text.len()
            )
        } else {
            String::new()
        };
        let notice = json!({"kind": "notice", "text": format!("📎 {header}\n{excerpt}{more}")});
        let mut post = None;
        self.kernel.frame(&[&exec], |k| {
            let e = k
                .execution(&exec)?
                .ok_or_else(|| anyhow!("session {sid}'s execution is not in the store"))?;
            if k.holds_turn(&exec) {
                bail!("a turn is running in {sid}: publish once it ends (`theseus wait {sid}`)");
            }
            if e.state.is_terminal() {
                bail!(
                    "session {sid} has ended ({}): nothing compiles it again",
                    e.state.as_str()
                );
            }
            let mut records = vec![node.record()?, row.clone()];
            if let Some(source) = &item.node {
                let edge = crate::graph::Edge::new(
                    crate::graph::EdgeKind::DerivedFrom,
                    &node.id,
                    source,
                    crate::graph::VIA_PUBLISH,
                );
                records.push(edge.record()?);
            }
            let (a, staged) = self.outbox.stage(sid, &exec, target, notice.clone())?;
            records.extend(staged);
            post = Some(a);
            k.stage(&records)?;
            Ok(())
        })?;
        if let Some(a) = &post {
            self.outbox.posted(a);
        }
        let rec = self.session_rec(sid);
        rec.announce(f);
        rec.announce(&fact::turn::NodeWritten {
            session_id: sid,
            node,
        });
        Ok(())
    }

    pub(super) fn place_publish(
        &self,
        p: PlacePublishParams,
        conn: Conn<'_>,
    ) -> Result<PublishResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.publish(&p, who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "publishing into {} from {} does not count: {}. Nothing was published.",
                        p.to, r.who, r.why
                    ),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })
    }
}
