//! A reply's turn on Discord (theseus-rha2), and "typing…".
//!
//! - **"Typing…"** shows while the place's turn runs ([`typing`]).
//! - **A reply's turn** ([`Shared::reply_turn_of`]): a reply to one of this
//!   process's replies names the turn. A reply to one sent before a restart,
//!   which this process does not hold, finds its turn by the message's durable
//!   `discord.message.out` row ([`sent_turn_of`], issue 44).
//!
//! A child of `runtime`, apart from it for the shape budget's file ceiling.

use theseus_protocol::{method, LedgerKind, LedgerTailParams, LedgerTailResult};

use super::Shared;
use crate::rpc_client::RpcClient;
use crate::runtime::route::turn_of_key;

/// The rows of `discord.message.out` read back for a reply's message, at
/// most, newest first.
const SENT_READ: usize = 5000;

/// The turn whose reply `message` was, by the message's `discord.message.out`
/// row, the durable record a restart keeps: its key names the turn
/// (`turn_of_key`). None when no row of the newest `SENT_READ` names it, its
/// key is no turn's, or the rows cannot be read: the message then carries no
/// reply's turn, as before.
pub(super) async fn sent_turn_of(rpc: &RpcClient, message: u64) -> Option<String> {
    let id = message.to_string();
    // `ledger.tail` gives the next page's cursor (`older`) only to a read
    // that named `before`: the walk starts at the log's end, named.
    let (mut before, mut read) = (Some(u64::MAX), 0);
    while read < SENT_READ {
        let page = rpc
            .call::<_, LedgerTailResult>(
                method::LEDGER_TAIL,
                LedgerTailParams {
                    n: Some(1000),
                    kind: Some(LedgerKind::DiscordMessageOut.as_str().into()),
                    before,
                    ..Default::default()
                },
            )
            .await;
        let page = match page {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e.message, "a reply's message was not read back");
                return None;
            }
        };
        if page.rows.is_empty() {
            return None;
        }
        read += page.rows.len();
        if let Some(r) = page
            .rows
            .iter()
            .find(|r| r.data["message_id"].as_str() == Some(id.as_str()))
        {
            return turn_of_key(r.data["part"].as_str()?).map(str::to_string);
        }
        before = Some(page.older?);
    }
    None
}

/// Whether the place shows "typing…" now: while its turn runs, or while its
/// submit is outstanding and not stopping.
pub(super) fn typing(inflight: bool, stopping: bool, busy: bool) -> bool {
    busy || (inflight && !stopping)
}

impl super::Place {
    /// [`typing`] for this place.
    pub(super) fn typing_due(&self) -> bool {
        typing(self.turning(), self.stopping, self.renderer.busy())
    }
}

impl Shared {
    /// The turn a message replies to, when it replies to one of this
    /// process's replies (theseus-rha2).
    pub(super) fn reply_turn_of(&self, m: &twilight_model::channel::Message) -> Option<String> {
        m.referenced_message
            .as_ref()
            .and_then(|r| self.replies.turn(r.id.get()))
    }

    /// The message a reply answers when it is one of this bot's whose turn
    /// this process does not hold: [`sent_turn_of`] reads it back.
    pub(super) fn reply_unheld_of(&self, m: &twilight_model::channel::Message) -> Option<u64> {
        let r = m.referenced_message.as_ref()?;
        (r.author.id.get() == self.bot_id() && self.replies.turn(r.id.get()).is_none())
            .then_some(r.id.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Typing…" shows while a turn runs, and while a submit is outstanding;
    /// not once it is stopping or done.
    #[test]
    fn typing_shows_while_a_turn_runs() {
        assert!(typing(true, false, false), "submitted");
        assert!(typing(false, false, true), "running");
        assert!(!typing(true, true, false), "stopping");
        assert!(!typing(false, false, false), "done");
    }

    /// A reply to a message sent before a restart finds the turn it answered
    /// by the message's durable row (issue 44); a message no row names, or
    /// whose key is no turn's, finds none.
    #[tokio::test]
    async fn a_message_sent_before_a_restart_names_its_turn_by_its_row() {
        let d = tempfile::tempdir().unwrap();
        let secrets = theseus_core::secrets::SecretBoard::new([], std::time::Instant::now());
        let core = crate::runtime::tests::core_with(d.path(), secrets, |_| {});
        let shared = crate::runtime::shared_for_tests(&core);
        for (id, part) in [("4242", "turn_old:0"), ("4343", "board")] {
            core.binding_ledger(
                LedgerKind::DiscordMessageOut,
                None,
                serde_json::json!({"place": "#harbor", "target": "discord:channel:7",
                                   "message_id": id, "channel": "7", "part": part}),
            );
        }
        let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(t) = sent_turn_of(&shared.rpc, 4242).await {
                    break t;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the row is read back");
        assert_eq!(found, "turn_old");
        assert_eq!(sent_turn_of(&shared.rpc, 4343).await, None, "no turn's key");
        assert_eq!(sent_turn_of(&shared.rpc, 4444).await, None, "no row");
    }

    /// A message sent long before, past the first page of `ledger.tail`'s
    /// answer, is still found: the walk pages back (issue 44).
    #[tokio::test]
    async fn a_message_past_the_first_page_is_found() {
        let d = tempfile::tempdir().unwrap();
        let secrets = theseus_core::secrets::SecretBoard::new([], std::time::Instant::now());
        let core = crate::runtime::tests::core_with(d.path(), secrets, |_| {});
        let shared = crate::runtime::shared_for_tests(&core);
        let out = |id: String, part: String| {
            core.binding_ledger(
                LedgerKind::DiscordMessageOut,
                None,
                serde_json::json!({"place": "#harbor", "target": "discord:channel:7",
                                   "message_id": id, "channel": "7", "part": part}),
            );
        };
        out("4242".into(), "turn_long_ago:0".into());
        for i in 0..1_500u64 {
            out((10_000 + i).to_string(), format!("turn_n{i}:0"));
        }
        let found = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                if let Some(t) = sent_turn_of(&shared.rpc, 4242).await {
                    break t;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("the row past the first page is read back");
        assert_eq!(found, "turn_long_ago");
    }
}
