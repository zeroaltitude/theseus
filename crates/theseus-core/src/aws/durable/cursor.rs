//! The tender's durable state, beside the store (`<state>/durability/`):
//!
//! - `cursor.json`: where the WAL is shipped to (the follower's cursor), the
//!   last sealed segment shipped whole, how far the open segment's tail is
//!   shipped, the last position whose index rows are written, and the object
//!   in flight, with a multipart upload's id and its parts, and the rows of
//!   objects shipped whose rows are not yet written. It is written
//!   whole, synced, and renamed over the last, after every object, so a
//!   restart resumes where the last one stopped: nothing shipped twice, and
//!   nothing skipped.
//! - `blobs.shipped`: each blob shipped, a line per digest, appended and
//!   synced once its object and its row are written.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use theseus_follow::Cursor;

/// This file's layout; another is read as no cursor, and shipping starts
/// again from the log's start (an object already there is found by its
/// checksum, not sent again).
pub const LAYOUT: u32 = 1;

/// The tender's files.
#[derive(Clone, Debug)]
pub struct Paths {
    pub wal: PathBuf,
    pub blobs: PathBuf,
    /// Its own directory.
    pub state: PathBuf,
}

impl Paths {
    /// A store's: `<state>/store` ships from `<state>/store/{wal,blobs}`
    /// and keeps its cursor in `<state>/durability` (a `store-x` in
    /// `durability-x`).
    pub fn for_store(store_dir: &Path) -> Self {
        let name = store_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "store".into());
        Self {
            wal: store_dir.join("wal"),
            blobs: store_dir.join("blobs"),
            state: store_dir.with_file_name(name.replacen("store", "durability", 1)),
        }
    }

    pub fn cursor(&self) -> PathBuf {
        self.state.join("cursor.json")
    }

    pub fn shipped_blobs(&self) -> PathBuf {
        self.state.join("blobs.shipped")
    }
}

/// One part of a multipart upload, as S3 took it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub n: u32,
    pub etag: String,
    /// Its SHA-256, base64, as S3's `ChecksumSHA256` says it.
    pub sha256: String,
}

/// A multipart upload begun: its id, its part size, and the parts S3 took.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    pub id: String,
    pub part_bytes: u64,
    pub parts: Vec<Part>,
}

/// The object being shipped, written before its first request: a restart
/// that finds it asks S3 whether the object is there already (by its
/// checksum) or resumes its upload, instead of sending it again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inflight {
    pub key: String,
    /// The object's SHA-256, base64 (a single put's `ChecksumSHA256`).
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload: Option<Upload>,
}

/// `cursor.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub layout: u32,
    /// Everything before it is shipped: its bytes, its sealed segments, and
    /// its records' index rows.
    pub follow: Cursor,
    /// The last sealed segment shipped whole (0: none).
    pub sealed_to: u32,
    /// The open segment's bytes shipped as tails: (segment, to).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail: Option<(u32, u64)>,
    /// The last position whose index row is written.
    pub rows_to: u64,
    /// The objects in flight, by key: a few at most (a sealed segment's
    /// upload, and the tail or blob a crash cut short).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inflight: Vec<Inflight>,
    /// The rows of objects shipped whose rows are not written yet: written
    /// with the batch's, so a crash between the two loses none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<serde_json::Value>,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            layout: LAYOUT,
            follow: Cursor::start(),
            sealed_to: 0,
            tail: None,
            rows_to: 0,
            inflight: Vec::new(),
            pending: Vec::new(),
        }
    }
}

impl Saved {
    /// The object in flight under `key`.
    pub fn inflight(&self, key: &str) -> Option<&Inflight> {
        self.inflight.iter().find(|i| i.key == key)
    }

    /// Record `i` in flight, in place of what was under its key.
    pub fn set_inflight(&mut self, i: Inflight) {
        self.done(&i.key);
        self.inflight.push(i);
    }

    /// `key` is shipped: no longer in flight.
    pub fn done(&mut self, key: &str) {
        self.inflight.retain(|i| i.key != key);
    }
}

/// The saved cursor, or none: a missing file, or one of another layout or
/// that does not read (said in the log, and shipping starts again).
pub fn load(paths: &Paths) -> Option<Saved> {
    let bytes = match fs::read(paths.cursor()) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(error = %e, "durability: the cursor does not read; shipping from the start");
            return None;
        }
    };
    match serde_json::from_slice::<Saved>(&bytes) {
        Ok(s) if s.layout == LAYOUT => Some(s),
        Ok(s) => {
            tracing::warn!(
                layout = s.layout,
                "durability: a cursor of another layout; shipping from the start"
            );
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "durability: the cursor does not parse; shipping from the start");
            None
        }
    }
}

/// Write `saved` whole: a temporary file, synced, renamed over the cursor,
/// and the directory synced, so a crash leaves the old cursor or the new.
pub fn save(paths: &Paths, saved: &Saved) -> io::Result<()> {
    fs::create_dir_all(&paths.state)?;
    let tmp = paths.state.join("cursor.json.tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(saved).map_err(io::Error::other)?)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, paths.cursor())?;
    File::open(&paths.state)?.sync_all()
}

/// The blobs shipped, from `blobs.shipped`.
pub fn shipped_blobs(paths: &Paths) -> io::Result<BTreeSet<String>> {
    match fs::read_to_string(paths.shipped_blobs()) {
        Ok(s) => Ok(s
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(e),
    }
}

/// Add a blob to `blobs.shipped`, synced.
pub fn mark_blob(paths: &Paths, digest: &str) -> io::Result<()> {
    fs::create_dir_all(&paths.state)?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.shipped_blobs())?;
    writeln!(f, "{digest}")?;
    f.sync_data()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_cursor_reads_back_and_another_layout_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::for_store(&dir.path().join("store"));
        assert_eq!(paths.state, dir.path().join("durability"));
        assert!(load(&paths).is_none());
        let mut s = Saved {
            sealed_to: 3,
            tail: Some((4, 900)),
            rows_to: 77,
            ..Saved::default()
        };
        s.inflight = vec![Inflight {
            key: "durability/x/wal/000000004.seg".into(),
            sha256: "abc=".into(),
            upload: Some(Upload {
                id: "up-1".into(),
                part_bytes: 64,
                parts: vec![Part {
                    n: 1,
                    etag: "\"e1\"".into(),
                    sha256: "p1=".into(),
                }],
            }),
        }];
        save(&paths, &s).unwrap();
        assert_eq!(load(&paths), Some(s.clone()));
        s.layout = LAYOUT + 1;
        save(&paths, &s).unwrap();
        assert!(load(&paths).is_none());
        // A stray temporary file from a crash is overwritten, not read.
        assert!(!paths.state.join("cursor.json.tmp").exists());
        mark_blob(&paths, "aa").unwrap();
        mark_blob(&paths, "bb").unwrap();
        assert_eq!(
            shipped_blobs(&paths).unwrap(),
            BTreeSet::from(["aa".to_string(), "bb".to_string()])
        );
    }
}
