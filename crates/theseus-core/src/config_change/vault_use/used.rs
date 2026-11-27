//! The items `vault.use`'s run form has given (issue 64), remembered by name so that the exits
//! keep masking their values after a restart.
//!
//! A job's output is kept as the job wrote it, and the exits mask what the secrets board knows
//! (`exits.rs`). A one-run value is on the board from its grant (`vault-use:<call>`), but a
//! restart empties the board, and a value only the run form read would then pass every exit. So
//! the run form remembers each item it gives, by the names the vault listed and the reference
//! the daemon built (never a value), BEFORE the value can reach a program; and at every start
//! ([`crate::Core::hold_vault_used`]), before the daemon serves, each remembered item is read
//! again, all at once within one bound ([`BOUND`]), and held under `vault-used:<n>`: the board
//! knows the item's CURRENT value before any turn, courier or push can send a copy. A value the
//! item held before it was changed in the vault is not known. An item unread at the bound, or
//! whose read failed, is named in one warning, taken off the board (no turn waits for it), and
//! tried again in the background ([`RETRIES`]); it is held the moment a try succeeds. A record
//! that cannot be read is not an empty list: the start holds nothing from it, and [`remember`]
//! gives nothing, so it is never replaced.
//!
//! Kept in one META record, `vault.used` (no store format: a new key), rewritten whole; one
//! entry per distinct reference. A start costs one keyed read of it, and the items' vault reads,
//! made together.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::check::Checked;
use crate::secrets::{OpReader, Secret, SecretBoard, SecretRef};
use crate::store::Store;

/// The META key the items are remembered under.
pub const KEY: &str = "vault.used";

/// The board's name of a remembered item's value, held at a start: its place in the list after
/// this. A grant's own hold is `broker::ONCE_HANDLE`'s.
pub const HANDLE: &str = "vault-used:";

/// How long a start waits, at most, for all of the remembered items' reads together.
pub const BOUND: Duration = Duration::from_secs(20);

/// When an item a start could not read is tried again, each after the one before: three more
/// times a minute apart, then once five minutes later, then no more.
#[cfg(not(test))]
const RETRIES: [Duration; 4] = [
    Duration::from_secs(60),
    Duration::from_secs(60),
    Duration::from_secs(60),
    Duration::from_secs(300),
];
#[cfg(test)]
const RETRIES: [Duration; 4] = [Duration::from_millis(200); 4];

/// The answer when the item could not be remembered.
const NOT_NOTED: &str = "Theseus could not note this item for masking after a restart, so it \
     did not give it: try again";

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

/// The items remembered in `store`: none when there is no record; an error when the record
/// cannot be read or decoded, which is never taken for an empty list.
pub(crate) fn remembered(store: &Store) -> anyhow::Result<Vec<Used>> {
    Ok(store.get_meta(KEY)?.unwrap_or_default())
}

/// The class of a record that could not be read, for a warning: it is JSON but not the list
/// (`decode`), or not even that (`read`). Never its content.
fn failure_class(store: &Store) -> &'static str {
    match store.get_meta::<serde_json::Value>(KEY) {
        Ok(_) => "decode",
        Err(_) => "read",
    }
}

/// Remember the item `c` names, once. An error is the result the model reads: the item is not
/// given when it could not be remembered, and a record that could not be read is not replaced.
pub(crate) fn remember(store: &Store, c: &Checked) -> Result<(), String> {
    let _w = WRITING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut all = remembered(store).map_err(|_| {
        tracing::warn!(
            key = KEY,
            class = failure_class(store),
            "the record of the items the run form gave could not be read: nothing is given"
        );
        NOT_NOTED.to_string()
    })?;
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
        NOT_NOTED.to_string()
    })
}

/// Read `u` once, until `until`: its value, or none.
async fn read_one(reader: &OpReader, u: &Used, until: tokio::time::Instant) -> Option<Secret> {
    let r = SecretRef::parse(&u.reference).ok()?;
    tokio::time::timeout_at(until, reader.read(&r))
        .await
        .ok()?
        .ok()
}

/// Try `u` again in the background, after each of [`RETRIES`]; held under `handle` the moment a
/// try succeeds, and one warning when the last one fails.
fn retry(board: Arc<SecretBoard>, reader: Arc<OpReader>, u: Used, handle: String) {
    tokio::spawn(async move {
        for wait in RETRIES {
            tokio::time::sleep(wait).await;
            let until = tokio::time::Instant::now() + BOUND;
            if let Some(v) = read_one(&reader, &u, until).await {
                board.hold(&handle, v);
                return;
            }
        }
        tracing::warn!(
            vault = %u.vault,
            item = %u.item,
            field = %u.field,
            "an item the run form gave could still not be read again: it is tried no more until \
             the next start, and its value is not masked"
        );
    });
}

/// The items a start reads again: none, with one warning, when the record cannot be read.
fn at_start(store: &Store) -> Vec<Used> {
    remembered(store).unwrap_or_else(|_| {
        tracing::warn!(
            key = KEY,
            class = failure_class(store),
            "the record of the items the run form gave could not be read: none of them is read \
             again at this start, and their values are not masked"
        );
        Vec::new()
    })
}

/// One item's read at a start, settled: held under `handle`, or warned, taken off the board, and
/// tried again in the background ([`retry`]).
fn settle(
    board: &Arc<SecretBoard>,
    reader: &Arc<OpReader>,
    (u, handle): (Used, String),
    read: Option<Secret>,
) {
    if let Some(v) = read {
        board.hold(&handle, v);
        return;
    }
    tracing::warn!(
        vault = %u.vault,
        item = %u.item,
        field = %u.field,
        "an item the run form gave could not be read again at this start: it is tried again in \
         the background, and its value is not masked until then"
    );
    board.forget(&handle);
    retry(board.clone(), reader.clone(), u, handle);
}

impl crate::Core {
    /// The start's step for the items the run form gave, awaited before the daemon serves: every
    /// remembered item is read again, all at once, within [`BOUND`], and held. An item unread by
    /// then, or whose read failed: one warning by its names, taken off the board so that no turn
    /// waits for it, and tried again in the background ([`RETRIES`]). A record that cannot be
    /// read: one warning (its key and the class of the failure), and nothing is held from it.
    pub async fn hold_vault_used(self: &Arc<Self>) {
        let used = at_start(&self.store);
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
        let until = tokio::time::Instant::now() + BOUND;
        let reads: Vec<_> = used
            .iter()
            .map(|u| {
                let (reader, u) = (reader.clone(), u.clone());
                tokio::spawn(async move { read_one(&reader, &u, until).await })
            })
            .collect();
        for (item, read) in used.into_iter().zip(handles).zip(reads) {
            let read = read.await.ok().flatten();
            settle(&self.secrets, &reader, item, read);
        }
    }
}
