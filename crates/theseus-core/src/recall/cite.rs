//! A reply's citations (theseus-ejdk, provable memory): which recalled items
//! a reply cited, and the source each one shows. String work over what the
//! turn already holds: no Jev call, no new frame, no wait. A handle (`m:a1b2c3`,
//! `theseus_protocol::cite`) is in the item's frozen header; a reply's
//! handles are resolved against the turn's own `Recall` node, so a handle
//! the turn was not given is no citation and stays text.

use serde::{Deserialize, Serialize};
use theseus_memory::recall::Place;
use theseus_protocol::cite::{split_header, Citation, DiscordSource};
use theseus_protocol::term::words::cut_marked as clip;
use theseus_store::NewRecord;

use super::render::shown_text;
use crate::node::{Body, Node, RecalledRef};
use crate::turn::TurnRunner;

/// How much of a source a citation's excerpt holds, in characters.
pub const EXCERPT_CHARS: usize = 600;

/// The META key of a message's surface origin: `msg.origin.<node id>`.
const ORIGIN_PREFIX: &str = "msg.origin.";

/// Where a person's message came from on a surface that has message ids: kept
/// beside the node, since the node's own record has no field for it (a field
/// would be a store format change; a META key is not one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageOrigin {
    /// `discord`.
    pub surface: String,
    /// The surface's id of the message (a Discord message id).
    pub message_id: String,
}

/// The record that keeps `node_id`'s origin, for the frame that writes the
/// node.
pub fn origin_record(node_id: &str, surface: &str, message_id: &str) -> Option<NewRecord> {
    NewRecord::json(
        theseus_store::kinds::META,
        Some(&format!("{ORIGIN_PREFIX}{node_id}")),
        &MessageOrigin {
            surface: surface.into(),
            message_id: message_id.into(),
        },
    )
    .ok()
}

/// The item of `items` whose header carries `handle`.
pub fn item_for<'a>(items: &'a [RecalledRef], handle: &str) -> Option<&'a RecalledRef> {
    items
        .iter()
        .find(|i| split_header(&i.header).is_some_and(|(h, _)| h == handle))
}

impl TurnRunner {
    /// The citation for `item`: its handle, its frozen header, its source's
    /// excerpt, and where it was said when that was a Discord message. None
    /// for an item with no handle (recalled before handles), a source that
    /// cannot be read, or, when `shared`, a source a shared place may not
    /// show: one from a private place or imported. A shared place cites its
    /// own history and nothing else of the owner's.
    pub fn citation(&self, item: &RecalledRef, shared: bool) -> Option<Citation> {
        let (handle, header) = split_header(&item.header)?;
        if shared && !self.shows_in_shared(&item.session_id) {
            return None;
        }
        let source = self.memory.read_source(&self.store, item)?;
        Some(Citation {
            handle: handle.to_string(),
            node_id: item.node_id.clone(),
            session_id: item.session_id.clone(),
            position: item.position,
            header: header.to_string(),
            at_unix_ms: source.created_at_ms,
            excerpt: clip(&shown_text(&source, item.chunk), EXCERPT_CHARS),
            discord: self.discord_source(&source),
        })
    }

    /// A shared place's reply shows a source only from a shared place and
    /// never an imported one.
    fn shows_in_shared(&self, session_id: &str) -> bool {
        !crate::import::is_imported(session_id)
            && matches!(self.place_of(session_id), Place::Shared(_))
    }

    /// Where `source` was said, when it was a person's Discord message whose
    /// id its turn kept.
    fn discord_source(&self, source: &Node) -> Option<DiscordSource> {
        if !matches!(source.body, Body::UserMessage { .. }) {
            return None;
        }
        let origin: MessageOrigin = self
            .store
            .get_meta(&format!("{ORIGIN_PREFIX}{}", source.id))
            .ok()
            .flatten()?;
        if origin.surface != "discord" {
            return None;
        }
        let target = self
            .outbox
            .try_target(&source.session_id)
            .ok()
            .flatten()
            .or_else(|| {
                self.outbox
                    .try_wake_target(&source.session_id)
                    .ok()
                    .flatten()
            })?;
        let message_id = origin.message_id;
        target
            .starts_with("discord:")
            .then_some(DiscordSource { target, message_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_excerpt_is_cut_on_a_character() {
        assert_eq!(clip("héron nests", 5), "héron…");
        assert_eq!(clip("short", 10), "short");
    }

    #[test]
    fn an_item_is_found_by_the_handle_in_its_header() {
        let item = |node: &str, header: &str| RecalledRef {
            node_id: node.into(),
            session_id: "ses_a".into(),
            position: 1,
            chunk: (0, 1),
            header: header.into(),
            tokens: 1,
        };
        let items = [
            item("msg_1", "[m:a1b2c3] a message from cli"),
            item("msg_2", "a message from cli, recalled before handles"),
        ];
        assert_eq!(item_for(&items, "m:a1b2c3").unwrap().node_id, "msg_1");
        assert!(item_for(&items, "m:ffffff").is_none());
    }
}
