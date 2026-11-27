//! The items `vault.use`'s run form has given (issue 64), remembered by name so that the exits
//! keep masking their values after a restart.
//!
//! A job's output is kept as the job wrote it, and the exits mask what the secrets board knows
//! (`exits.rs`). A one-run value is on the board from its grant (`vault-use:<call>`), but a
//! restart empties the board, and a value only the run form read would then pass every exit. So
//! the run form remembers each item it gives, by the names the vault listed and the reference
//! the daemon built (never a value), BEFORE the value can reach a program; and at every start
//! ([`crate::Core::hold_vault_used`]) each remembered item is put on the board as resolving,
//! under `vault-used:<n>`, and read again. Until its read settles, a turn waits for it as it
//! waits for every configured secret (`turn::await_secrets`), so no request leaves before the
//! value is known. An item that cannot be read is named in one warning and taken off the board,
//! and the start goes on.
//!
//! Kept in one META record, `vault.used` (no store format: a new key), rewritten whole; one
//! entry per distinct reference. A start costs one keyed read of it, and one vault read per item.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::check::Checked;
use crate::secrets::SecretRef;
use crate::store::Store;

/// The META key the items are remembered under.
pub const KEY: &str = "vault.used";

/// The board's name of a remembered item's value, held at a start: its place in the list after
/// this. A grant's own hold is `broker::ONCE_HANDLE`'s.
pub const HANDLE: &str = "vault-used:";

/// One item the run form gave, by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Used {
    pub vault: String,
    pub item: String,
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// The reference the daemon built from the vault's own lists.
    pub reference: String,
}

/// One writer of the record at a time: two run forms at once must not lose an item.
static WRITING: Mutex<()> = Mutex::new(());

/// The items remembered in `store`; none when there are none or the record cannot be read.
pub(crate) fn remembered(store: &Store) -> Vec<Used> {
    store.get_meta(KEY).ok().flatten().unwrap_or_default()
}

/// Remember the item `c` names, once. An error is the result the model reads: the item is not
/// given when it could not be remembered.
pub(crate) fn remember(store: &Store, c: &Checked) -> Result<(), String> {
    let _w = WRITING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut all = remembered(store);
    if all.iter().any(|u| u.reference == c.reference) {
        return Ok(());
    }
    all.push(Used {
        vault: c.vault.clone(),
        item: c.item.clone(),
        field: c.field.clone(),
        section: c.section.clone(),
        reference: c.reference.clone(),
    });
    store.put_meta(KEY, &all).map_err(|e| {
        tracing::warn!(error = %format!("{e:#}"), "the run form's item could not be remembered");
        "Theseus could not note this item for masking after a restart, so it did not give it: \
         try again"
            .to_string()
    })
}

impl crate::Core {
    /// The start's step for the items the run form gave: each is on the board as resolving at
    /// once, so a turn waits for it, and is read again in the background and held. Called once
    /// the vault's reader is set, before serving. An item that cannot be read: one warning by its
    /// names, its entry taken off the board, and the start goes on.
    pub fn hold_vault_used(self: &Arc<Self>) {
        let used = remembered(&self.store);
        if used.is_empty() {
            return;
        }
        let Some(reader) = self.tools.look.reader().filter(|r| !r.has_no_vault()) else {
            tracing::warn!(
                items = used.len(),
                "the items the run form gave cannot be read again: this Theseus has no vault \
                 session, so their values are not masked"
            );
            return;
        };
        let handles: Vec<String> = (0..used.len()).map(|i| format!("{HANDLE}{i}")).collect();
        for h in &handles {
            self.secrets.pending(h);
        }
        let board = self.secrets.clone();
        tokio::spawn(async move {
            for (u, h) in used.iter().zip(&handles) {
                let read = match SecretRef::parse(&u.reference) {
                    Ok(r) => reader.read(&r).await.ok(),
                    Err(_) => None,
                };
                match read {
                    Some(v) => board.hold(h, v),
                    None => {
                        tracing::warn!(
                            vault = %u.vault,
                            item = %u.item,
                            field = %u.field,
                            "an item the run form gave could not be read again at this start: \
                             its value is not masked"
                        );
                        board.forget(h);
                    }
                }
            }
        });
    }
}
