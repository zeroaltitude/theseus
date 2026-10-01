//! Restore a store from a local WAL directory (spec P5: the restore path exists
//! from the first release; S3 arrives in M4). The WAL is the truth and the
//! index is rebuildable, so a restore is: copy the segments into a staging
//! store beside the live one, open it (recovery checks every frame's checksum,
//! cuts a torn tail, and rebuilds the index from the WAL), record the restore
//! in its ledger, then swap it in. The source is never written to, and a store
//! being replaced is moved aside, never deleted.
//!
//! Nothing is reported restored before it is durable (theseus-ez3): each
//! copied segment and blob is synced before the open, then the staging
//! store's `wal/` and `blobs/` and the staging store itself, and the state dir
//! after each rename. A power loss after "restored" loses nothing of it.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::json;

use crate::ledger::LedgerRow;
use crate::store::Store;

#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    pub from: String,
    pub into: String,
    /// Where the store that was there went (only with `force`).
    pub moved_aside: Option<String>,
    pub segments: u32,
    pub frames: u64,
    pub records: u64,
    pub last_position: u64,
    /// Bytes of a torn final frame that recovery cut off.
    pub truncated_bytes: u64,
    pub sessions: u64,
    pub nodes: u64,
    pub ledger_rows: u64,
    /// Image blobs copied from beside the WAL (theseus-9g2).
    pub blobs: u32,
}

/// What a restore makes durable, and how (theseus-ez3). The restore's syncs
/// all go through here, so a test can see each one.
trait Durable {
    /// A file's bytes and size, through `file`, open for writing.
    fn file(&mut self, file: &std::fs::File, path: &Path) -> std::io::Result<()>;
    /// A directory's entries: what was created in it or renamed into it.
    fn dir(&mut self, path: &Path) -> std::io::Result<()>;
}

/// The syncs themselves.
struct Sync;

impl Durable for Sync {
    fn file(&mut self, file: &std::fs::File, _: &Path) -> std::io::Result<()> {
        file.sync_all()
    }

    fn dir(&mut self, path: &Path) -> std::io::Result<()> {
        std::fs::File::open(path)?.sync_all()
    }
}

/// `from` may be a WAL directory (holding `*.seg`) or a store directory
/// (holding `wal/`). Only the segments are read: the index is rebuilt.
pub fn restore(from: &Path, state_dir: &Path, force: bool) -> Result<RestoreReport> {
    restore_with(from, state_dir, force, &mut Sync)
}

fn restore_with(
    from: &Path,
    state_dir: &Path,
    force: bool,
    sync: &mut dyn Durable,
) -> Result<RestoreReport> {
    let wal_src = if from.join("wal").is_dir() {
        from.join("wal")
    } else {
        from.to_path_buf()
    };
    let mut segments: Vec<PathBuf> = std::fs::read_dir(&wal_src)
        .with_context(|| format!("reading {}", wal_src.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "seg"))
        .collect();
    segments.sort();
    if segments.is_empty() {
        bail!("no WAL segments (*.seg) in {}", wal_src.display());
    }

    let target = state_dir.join("store");
    let occupied = target.is_dir()
        && std::fs::read_dir(&target)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);
    if occupied && !force {
        bail!(
            "{} already holds a store; pass --force to move it aside (it is kept, never deleted)",
            target.display()
        );
    }
    let src_canon = std::fs::canonicalize(&wal_src)?;
    if src_canon.starts_with(std::fs::canonicalize(state_dir)?.join("store")) {
        bail!("the source is inside the store it would replace; copy it elsewhere first");
    }

    let ts = theseus_protocol::now_unix_ms();
    let staging = state_dir.join(format!("store.restoring-{ts}"));
    std::fs::create_dir_all(staging.join("wal"))?;
    for s in &segments {
        let name = s.file_name().context("segment without a name")?;
        copy_synced(s, &staging.join("wal").join(name), sync)?;
    }
    sync.dir(&staging.join("wal"))
        .context("syncing the restored WAL's directory")?;
    // Image bytes live beside the WAL, in the store's `blobs/` (theseus-9g2).
    let blobs_src = (wal_src.file_name().is_some_and(|n| n == "wal"))
        .then(|| wal_src.parent().map(|p| p.join("blobs")))
        .flatten()
        .filter(|p| p.is_dir());
    let mut blobs = 0u32;
    if let Some(src) = &blobs_src {
        std::fs::create_dir_all(staging.join("blobs"))?;
        for e in std::fs::read_dir(src)? {
            let p = e?.path();
            let Some(name) = p.file_name() else { continue };
            if p.is_file() && !name.to_string_lossy().starts_with('.') {
                copy_synced(&p, &staging.join("blobs").join(name), sync)?;
                blobs += 1;
            }
        }
        sync.dir(&staging.join("blobs"))
            .context("syncing the restored blobs' directory")?;
    }

    let mut report = RestoreReport {
        from: wal_src.display().to_string(),
        into: target.display().to_string(),
        moved_aside: None,
        segments: segments.len() as u32,
        frames: 0,
        records: 0,
        last_position: 0,
        truncated_bytes: 0,
        sessions: 0,
        nodes: 0,
        ledger_rows: 0,
        blobs,
    };
    {
        let store = Store::open(&staging)
            .with_context(|| format!("opening the restored WAL in {}", staging.display()))?;
        let rec = store.inner().recovery().clone();
        let st = store.stats()?;
        report.frames = rec.frames;
        report.records = rec.records;
        report.last_position = st.last_position;
        report.truncated_bytes = st.truncated_bytes;
        report.sessions = store.session_count()?;
        report.nodes = store.node_count()?;
        report.ledger_rows = store.ledger_len()?;
        store.append_ledger(&LedgerRow::new(
            "store.restored",
            None,
            None,
            json!({"from": report.from, "segments": report.segments, "frames": report.frames,
                   "records": report.records, "last_position": report.last_position,
                   "truncated_bytes": report.truncated_bytes, "sessions": report.sessions,
                   "blobs": report.blobs}),
        ))?;
        store.checkpoint()?;
    }
    // The staging store's entries: `wal/`, `blobs/`, and what the open wrote
    // beside them (the manifest and the index).
    sync.dir(&staging)
        .context("syncing the restored store's directory")?;

    if occupied {
        let aside = state_dir.join(format!("store.before-restore-{ts}"));
        std::fs::rename(&target, &aside)
            .with_context(|| format!("moving {} aside", target.display()))?;
        sync.dir(state_dir)
            .context("syncing the state dir after moving the old store aside")?;
        report.moved_aside = Some(aside.display().to_string());
    }
    std::fs::rename(&staging, &target)
        .with_context(|| format!("moving the restored store into {}", target.display()))?;
    sync.dir(state_dir)
        .context("syncing the state dir after the restore's rename")?;
    Ok(report)
}

/// Copy `from` to `to`, then sync the copy (theseus-ez3): `std::fs::copy`
/// alone leaves its bytes in the page cache.
fn copy_synced(from: &Path, to: &Path, sync: &mut dyn Durable) -> Result<()> {
    std::fs::copy(from, to).with_context(|| format!("copying {}", from.display()))?;
    let copy = std::fs::OpenOptions::new()
        .write(true)
        .open(to)
        .with_context(|| format!("opening {} to sync it", to.display()))?;
    sync.file(&copy, to)
        .with_context(|| format!("syncing {}", to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionRecord;

    /// A store with `n` sessions; returns its last position and the last session id.
    fn store_with_sessions(dir: &Path, n: usize) -> (u64, String) {
        let store = Store::open(&dir.join("store")).unwrap();
        let mut last = String::new();
        for i in 0..n {
            let rec = SessionRecord::new(
                theseus_protocol::SessionKind::Conversation,
                Some(format!("s{i}")),
            );
            store.put_session(&rec.session_id, &rec).unwrap();
            last = rec.session_id;
        }
        store
            .append_ledger(&LedgerRow::new("test.row", None, None, json!({})))
            .unwrap();
        (store.last_position(), last)
    }

    #[test]
    fn a_wal_directory_restores_into_an_empty_state_dir_and_is_left_untouched() {
        let live = tempfile::tempdir().unwrap();
        let (last, last_session) = store_with_sessions(live.path(), 3);
        let wal = live.path().join("store/wal");
        let before: Vec<_> = std::fs::read(wal.join("000000001.seg")).unwrap();

        let fresh = tempfile::tempdir().unwrap();
        let r = restore(&wal, fresh.path(), false).unwrap();
        assert_eq!(r.sessions, 3);
        assert_eq!(r.last_position, last);
        assert_eq!(r.truncated_bytes, 0);
        assert!(r.moved_aside.is_none());
        assert_eq!(
            std::fs::read(wal.join("000000001.seg")).unwrap(),
            before,
            "source untouched"
        );

        let reopened = Store::open(&fresh.path().join("store")).unwrap();
        assert_eq!(reopened.session_count().unwrap(), 3);
        let rows: Vec<(u64, LedgerRow)> = reopened.ledger_tail(1).unwrap();
        assert_eq!(rows[0].1.kind, "store.restored");
        assert!(reopened
            .get_session::<SessionRecord>(&last_session)
            .unwrap()
            .is_some());
    }

    #[test]
    fn an_occupied_store_needs_force_and_is_moved_aside_not_deleted() {
        let src = tempfile::tempdir().unwrap();
        store_with_sessions(src.path(), 2);
        let dst = tempfile::tempdir().unwrap();
        store_with_sessions(dst.path(), 5);

        let e = restore(&src.path().join("store"), dst.path(), false).unwrap_err();
        assert!(e.to_string().contains("--force"), "{e}");

        let r = restore(&src.path().join("store"), dst.path(), true).unwrap();
        assert_eq!(r.sessions, 2);
        let aside = PathBuf::from(r.moved_aside.unwrap());
        let old = Store::open(&aside).unwrap();
        assert_eq!(
            old.session_count().unwrap(),
            5,
            "the replaced store is kept"
        );
    }

    #[test]
    fn a_torn_tail_is_cut_and_reported() {
        let src = tempfile::tempdir().unwrap();
        store_with_sessions(src.path(), 2);
        let seg = src.path().join("store/wal/000000001.seg");
        let mut bytes = std::fs::read(&seg).unwrap();
        bytes.extend_from_slice(&[0xAB; 37]); // half a frame after the last good one
        let wal_copy = tempfile::tempdir().unwrap();
        std::fs::write(wal_copy.path().join("000000001.seg"), &bytes).unwrap();

        let dst = tempfile::tempdir().unwrap();
        let r = restore(wal_copy.path(), dst.path(), false).unwrap();
        assert_eq!(r.truncated_bytes, 37);
        assert_eq!(r.sessions, 2);
    }

    #[test]
    fn nothing_to_restore_is_an_error() {
        let empty = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let e = restore(empty.path(), dst.path(), false).unwrap_err();
        assert!(e.to_string().contains("no WAL segments"), "{e}");
    }

    /// Image bytes live beside the WAL (theseus-9g2), so a restore carries
    /// them: an image node restored without its blob would show as missing.
    #[test]
    fn a_restore_carries_the_image_blobs() {
        let live = tempfile::tempdir().unwrap();
        store_with_sessions(live.path(), 1);
        let digest = {
            let store = Store::open(&live.path().join("store")).unwrap();
            store.blobs().put(b"\x89PNG image bytes").unwrap()
        };
        let fresh = tempfile::tempdir().unwrap();
        let r = restore(&live.path().join("store"), fresh.path(), false).unwrap();
        assert_eq!(r.blobs, 1);
        let back = Store::open(&fresh.path().join("store")).unwrap();
        let b64 = back.blobs().base64(&digest).unwrap();
        assert_eq!(
            crate::blobs::decode(&b64).unwrap(),
            b"\x89PNG image bytes".to_vec()
        );
    }

    /// A sync the restore made, as the test renders it: `file` or `dir`, the
    /// path under the state dir with the staging store named `staging`, and,
    /// for the state dir, what it held then. `opened` once the staging store
    /// has been opened (its index exists).
    struct Recorder {
        state_dir: PathBuf,
        syncs: Vec<String>,
    }

    impl Recorder {
        fn new(state_dir: &Path) -> Self {
            Self {
                state_dir: state_dir.to_path_buf(),
                syncs: Vec::new(),
            }
        }

        /// The names in the state dir, without their timestamps.
        fn state(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.state_dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .map(|n| match n.rsplit_once('-') {
                    Some((head, ts)) if ts.chars().all(|c| c.is_ascii_digit()) => head.to_string(),
                    _ => n,
                })
                .collect();
            names.sort();
            names
        }

        fn record(&mut self, what: &str, path: &Path) {
            let staging = self.state().contains(&"store.restoring".to_string());
            let opened = staging
                && std::fs::read_dir(&self.state_dir).unwrap().any(|e| {
                    let p = e.unwrap().path();
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("store.restoring-"))
                        && p.join("index.redb").exists()
                });
            let rel = path.strip_prefix(&self.state_dir).unwrap();
            let mut parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            if let Some(first) = parts.first_mut() {
                if first.starts_with("store.restoring-") {
                    *first = "staging".into();
                }
            }
            if parts.len() == 3 && parts[1] == "blobs" {
                parts[2] = "<blob>".into();
            }
            let mut line = format!("{what} {}", parts.join("/"));
            if path == self.state_dir {
                line = format!("{what} state: {}", self.state().join(", "));
            } else if opened {
                line.push_str(", opened");
            }
            self.syncs.push(line);
        }
    }

    impl Durable for Recorder {
        fn file(&mut self, file: &std::fs::File, path: &Path) -> std::io::Result<()> {
            self.record("file", path);
            Sync.file(file, path)
        }

        fn dir(&mut self, path: &Path) -> std::io::Result<()> {
            self.record("dir", path);
            Sync.dir(path)
        }
    }

    /// A restore reports only what is durable (theseus-ez3): each copied
    /// segment and blob is synced before the restored WAL is opened, then the
    /// restored WAL's and blobs' directories; the staging store once the open
    /// has written its index and manifest; and the state dir after the old
    /// store is moved aside and after the restored one takes its name. A
    /// restore that drops any of these, or makes it too late, fails here.
    #[test]
    fn a_restore_syncs_every_copy_and_directory_before_it_reports() {
        use theseus_store::{kinds, NewRecord, Store as _, WalConfig, WalStore};
        let src = tempfile::tempdir().unwrap();
        // Three segments, and a blob beside them.
        {
            let wal = WalStore::open(
                &src.path().join("store"),
                WalConfig {
                    segment_bytes: 2048,
                    ..WalConfig::default()
                },
            )
            .unwrap();
            for i in 0..60u32 {
                wal.append(&[
                    NewRecord::json(kinds::LEDGER, None, &format!("row {i:040}")).unwrap(),
                ])
                .unwrap();
            }
        }
        let segments: Vec<String> = {
            let mut s: Vec<String> = std::fs::read_dir(src.path().join("store/wal"))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".seg"))
                .collect();
            s.sort();
            s
        };
        assert_eq!(segments.len(), 3, "{segments:?}");
        std::fs::create_dir_all(src.path().join("store/blobs")).unwrap();
        std::fs::write(
            src.path().join("store/blobs/abc123"),
            b"\x89PNG image bytes",
        )
        .unwrap();

        // An occupied state dir: the old store is moved aside.
        let dst = tempfile::tempdir().unwrap();
        store_with_sessions(dst.path(), 2);
        let mut rec = Recorder::new(dst.path());
        let r = restore_with(&src.path().join("store"), dst.path(), true, &mut rec).unwrap();
        assert_eq!((r.segments, r.blobs), (3, 1));

        let mut want: Vec<String> = segments
            .iter()
            .map(|s| format!("file staging/wal/{s}"))
            .collect();
        want.extend(
            [
                "dir staging/wal",
                "file staging/blobs/<blob>",
                "dir staging/blobs",
                "dir staging, opened",
                "dir state: store.before-restore, store.restoring",
                "dir state: store, store.before-restore",
            ]
            .map(String::from),
        );
        assert_eq!(rec.syncs, want);
        let restored = Store::open(&dst.path().join("store")).unwrap();
        assert_eq!(
            restored.ledger_len().unwrap(),
            61,
            "60 rows and store.restored"
        );
    }
}
