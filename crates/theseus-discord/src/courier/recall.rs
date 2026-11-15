//! A lane's record of which message belongs to which key, after a restart
//! (issue 8): the map is memory, and what a view needs must be in the durable
//! record, so it is rebuilt from the `discord.message.out` rows the lane's
//! creates wrote, the first time a key is asked for and not found.
//!
//! A row names the place, the lane's target (`discord:channel:<id>` or
//! `discord:dm:<id>`, which is unique where a place's label is not), the key
//! (`part`), the message and, since this change, the channel it went to. A row
//! an older build wrote has no target or channel and is skipped: its message cannot be edited, and the key is posted again,
//! as it was before this. Only a turn's keys are recalled (`turn_of_key`):
//! the board and the other lane-wide keys have their own way back
//! (`courier/board.rs`).

use std::collections::HashMap;

use theseus_protocol::{method, LedgerTailParams, LedgerTailResult};

use super::Lane;
use crate::runtime::route::turn_of_key;

/// How many of the newest rows of `discord.message.out` a lane reads to
/// rebuild. They are every place's, filtered here by the lane's target, so a
/// quiet place's rows can fall out of the read when others are busy: its keys
/// are then posted again, as before this change.
const ROWS_READ: usize = 1000;

impl Lane {
    /// The message `key` was written to, from the durable rows, when this
    /// lane has lost it with its memory. The rows are read once, at the first
    /// miss of a turn's key, and each key is taken from them as it is asked
    /// for, so what is held stays bounded (`msgs`'s bound).
    pub(super) async fn recall(&mut self, key: &str) -> Option<(u64, u64)> {
        turn_of_key(key)?;
        if self.recalled.is_none() {
            let rows = self.durable_messages().await;
            self.recalled = Some(rows);
        }
        let found = self.recalled.as_mut()?.remove(key)?;
        self.touch(key);
        self.msgs.insert(key.to_string(), found);
        Some(found)
    }

    /// The lane's messages the rows name, key → (channel, message), the
    /// newest row of a key winning. A read that fails finds none: the lane
    /// posts as it did, and asks again at no later miss (the read is the
    /// restart's, once).
    async fn durable_messages(&self) -> HashMap<String, (u64, u64)> {
        let read = self
            .shared
            .rpc
            .call::<_, LedgerTailResult>(
                method::LEDGER_TAIL,
                LedgerTailParams {
                    n: Some(ROWS_READ),
                    kind: Some(
                        theseus_protocol::LedgerKind::DiscordMessageOut
                            .as_str()
                            .into(),
                    ),
                    ..Default::default()
                },
            )
            .await;
        let rows = match read {
            Ok(r) => r.rows,
            Err(e) => {
                tracing::warn!(place = %self.label, error = %e.message, "the lane's messages were not read back");
                return HashMap::new();
            }
        };
        let mut out = HashMap::new();
        for r in rows {
            let d = &r.data;
            let ids = (
                d["target"].as_str() == Some(self.target.as_str()),
                d["part"].as_str(),
                d["channel"].as_str().and_then(|c| c.parse::<u64>().ok()),
                d["message_id"].as_str().and_then(|m| m.parse::<u64>().ok()),
            );
            if let (true, Some(part), Some(channel), Some(message)) = ids {
                out.insert(part.to_string(), (channel, message));
            }
        }
        out
    }
}
