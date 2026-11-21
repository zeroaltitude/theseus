//! A piece's text, read back from the store for the owner at his own terminal (`theseus view show --text`).
//! An exchange's text is its ask and its final reply, through the exchange record (`exchange.<turn>`, the
//! ledger's) and their nodes. A space whose text this build does not read yet (a fold node, a fact, a card,
//! a task, code's own text) gives none: the reader says so rather than guess.

use serde::Deserialize;

use super::super::{Piece, Space};
use super::show::PieceText;
use crate::node::Body;
use crate::store::Store;

/// The two fields of an exchange record this reads: its ask and its reply, by node id.
#[derive(Deserialize)]
struct Ends {
    #[serde(default)]
    ask: Option<String>,
    #[serde(default)]
    reply: Option<String>,
}

/// The store, as a reader of pieces' text.
pub struct StoreText<'a>(pub &'a Store);

impl StoreText<'_> {
    /// A node's text: a message's, or the text blocks of a reply.
    fn node_text(&self, id: &str) -> Option<String> {
        let (_, node) = self.0.get_node(id).ok()??;
        match node.body {
            Body::UserMessage { text, .. } => Some(text),
            Body::AssistantMessage { blocks, .. } => {
                let parts: Vec<&str> = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect();
                (!parts.is_empty()).then(|| parts.join("\n"))
            }
            _ => None,
        }
    }

    /// One exchange's ask and reply.
    fn exchange(&self, turn: &str) -> Option<String> {
        let ends: Ends = self.0.get_meta(&format!("exchange.{turn}")).ok()??;
        let ask = ends.ask.as_deref().and_then(|n| self.node_text(n));
        let reply = ends.reply.as_deref().and_then(|n| self.node_text(n));
        match (ask, reply) {
            (None, None) => None,
            (a, r) => Some(format!(
                "asked: {}\nreplied: {}",
                a.unwrap_or_else(|| "(no text)".into()),
                r.unwrap_or_else(|| "(no reply yet)".into())
            )),
        }
    }
}

impl PieceText for StoreText<'_> {
    fn text(&self, piece: &Piece) -> Option<String> {
        let Space::Exchanges { .. } = &piece.range.space else {
            return None;
        };
        let (first, last) = (&piece.range.first.id, &piece.range.last.id);
        let mut out = vec![self.exchange(first)?];
        if last != first {
            out.push(self.exchange(last)?);
        }
        Some(out.join("\n…\n"))
    }
}
