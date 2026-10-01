//! The keel (spec §6, Part II M1).
//!
//! Two layers, one contract:
//!
//! - **WAL**: append-only segment files of checksummed frames. A frame is the
//!   atomic unit: one or more records written and fsynced together, so
//!   `settle(completion, continuation)` is one frame and either both records
//!   exist after a crash or neither does. Recovery checks the frames after
//!   the index's checkpoint and truncates a torn tail; the history before it
//!   is checked after serving (theseus-8ni). Positions are monotonic u64s.
//! - **Index**: a rebuildable projection of the WAL in an embedded store
//!   (redb): position → location, (kind, key) →
//!   latest position, (kind, position) for per-kind scans, and the checkpoint.
//!   Index writes are non-durable; a **checkpoint** flushes them and records
//!   the position they are good to. Startup replays the WAL from the last
//!   checkpoint to rebuild whatever the index lost.
//!
//! The WAL is the truth. The index is a cache over it. Nothing in the WAL is
//! ever rewritten (append-only, §1); a torn tail that never committed is the
//! only thing recovery removes.
//!
//! **Formats and schemas** (theseus-qa0 F4a). Every record carries its kind
//! and schema; the manifest names the store's format and the newest schema
//! written for each kind, and a build older than the store refuses to open
//! it. The standing rule, in `record.rs`: a new on-disk layout lands with
//! the reader for the layout it replaces, and bumps its kind's schema.

pub mod index;
pub mod record;
pub mod store;
pub mod wal;

pub use index::{Engine, Location, MovedAside};
pub use record::{kinds, NewRecord, Record, RecordKind};
pub use store::{Store, StoreStats, WalStore};
pub use wal::{History, HistoryCheck, Wal, WalConfig, WalError};
