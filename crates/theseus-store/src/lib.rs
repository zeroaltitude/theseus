//! The keel (spec §6, Part II M1).
//!
//! Two layers, one contract:
//!
//! - **WAL**: append-only segment files of checksummed frames. A frame is the
//!   atomic unit: one or more records written and fsynced together, so
//!   `settle(completion, continuation)` is one frame and either both records
//!   exist after a crash or neither does. Recovery scans segments, verifies
//!   every frame, and truncates a torn tail. Positions are monotonic u64s.
//! - **Index**: a rebuildable projection of the WAL in an embedded store
//!   (redb or fjall, behind one trait): position → location, (kind, key) →
//!   latest position, (kind, position) for per-kind scans, and the checkpoint.
//!   Index writes are non-durable; a **checkpoint** flushes them and records
//!   the position they are good to. Startup replays the WAL from the last
//!   checkpoint to rebuild whatever the index lost.
//!
//! The WAL is the truth. The index is a cache over it. Nothing in the WAL is
//! ever rewritten (append-only, §1); a torn tail that never committed is the
//! only thing recovery removes.

pub mod index;
pub mod record;
pub mod store;
pub mod wal;

pub use index::{Engine, Index, Location};
pub use record::{kinds, NewRecord, Record, RecordKind};
pub use store::{Store, StoreStats, WalStore};
pub use wal::{Wal, WalConfig, WalError};
