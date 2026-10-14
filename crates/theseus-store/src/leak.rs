//! Who still holds the store when the runtime is gone (theseus-8ub95). Every
//! handle on a store is gone by the process's end, or redb never closes it.
//! The stop's checkpoint is durable by itself (`WalStore::checkpoint_for_close`),
//! so a handle that outlives the runtime loses nothing; this says that one did,
//! and how many, so the thread or task that holds it is found and joined.

use std::sync::{Arc, Mutex, Weak};

use crate::store::WalStore;

static WATCHED: Mutex<Option<Weak<WalStore>>> = Mutex::new(None);

/// Remember the store, by `Weak`: this never keeps it open.
pub fn watch(store: &Arc<WalStore>) {
    *WATCHED.lock().unwrap() = Some(Arc::downgrade(store));
}

/// The handles on the watched store that are still alive (0: it has closed,
/// or none was watched).
pub fn holders() -> usize {
    WATCHED
        .lock()
        .unwrap()
        .as_ref()
        .map_or(0, Weak::strong_count)
}
