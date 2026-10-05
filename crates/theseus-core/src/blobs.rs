//! Image bytes, stored once (theseus-9g2).
//!
//! An image is up to 5 MiB, and a node is a WAL frame that every recovery
//! and index rebuild reads, so the bytes never go in a node. They go to
//! `<store dir>/blobs/<sha256>`, content-addressed and written atomically,
//! and the node holds the digest. The same image sent twice is one file.
//! The compiler renders an image block from its blob through a small
//! bounded cache of base64 strings, so a session's image is read and
//! encoded once, not on every loop. Nothing is read at startup.

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine as _;

/// Base64 the cache may hold: about four images at the 5 MiB limit, or
/// dozens of screenshots.
const CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;
const CACHE_MAX_ENTRIES: usize = 16;

/// Standard base64, padded: what an image block's `data` and an
/// attachment's `data` carry.
pub fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn decode(s: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| e.to_string())
}

/// The full SHA-256 of `bytes`, as lowercase hex: a blob's name.
pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

fn is_digest(d: &str) -> bool {
    d.len() == 64
        && d.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

pub struct Blobs {
    dir: PathBuf,
    /// Most recently used last: (digest, base64).
    cache: Mutex<VecDeque<(String, Arc<str>)>>,
    /// A test's hold on every write, as a disk under a neighbour's IO holds
    /// a sync (`hold_puts`).
    #[cfg(test)]
    hold: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl Blobs {
    /// The blobs of the store at `store_dir`. Creates nothing until the
    /// first `put`.
    pub fn new(store_dir: &Path) -> Self {
        Self {
            dir: store_dir.join("blobs"),
            cache: Mutex::new(VecDeque::new()),
            #[cfg(test)]
            hold: Mutex::new(None),
        }
    }

    /// Hold every new blob's write until the sender is dropped (a test's
    /// stand-in for a slow sync, theseus-otny).
    #[cfg(test)]
    pub(crate) fn hold_puts(&self) -> std::sync::mpsc::Sender<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        *self.hold.lock().unwrap() = Some(rx);
        tx
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self, digest: &str) -> PathBuf {
        self.dir.join(digest)
    }

    /// Store `bytes` and return their digest. A blob already stored is
    /// left as it is; a new one is written to a temporary file, synced, and
    /// renamed, so a reader never sees half of one.
    pub fn put(&self, bytes: &[u8]) -> std::io::Result<String> {
        let d = digest(bytes);
        let path = self.path(&d);
        if path.exists() {
            return Ok(d);
        }
        #[cfg(test)]
        if let Some(rx) = self.hold.lock().unwrap().as_ref() {
            let _ = rx.recv();
        }
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".{d}.tmp-{}", std::process::id()));
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &path)?;
        if let Ok(dir) = std::fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(d)
    }

    /// A blob's bytes as base64, from the cache or its file; `None` when it
    /// is missing or its bytes no longer match its name.
    pub fn base64(&self, digest: &str) -> Option<Arc<str>> {
        if !is_digest(digest) {
            return None;
        }
        {
            let mut c = self.cache.lock().unwrap();
            if let Some(i) = c.iter().position(|(d, _)| d == digest) {
                let hit = c.remove(i).expect("present");
                let b64 = hit.1.clone();
                c.push_back(hit);
                return Some(b64);
            }
        }
        let bytes = std::fs::read(self.path(digest)).ok()?;
        if self::digest(&bytes) != digest {
            tracing::warn!(digest, "an image blob does not match its digest; not shown");
            return None;
        }
        let b64: Arc<str> = encode(&bytes).into();
        let mut c = self.cache.lock().unwrap();
        c.push_back((digest.to_string(), b64.clone()));
        let mut held: usize = c.iter().map(|(_, s)| s.len()).sum();
        while c.len() > 1 && (c.len() > CACHE_MAX_ENTRIES || held > CACHE_MAX_BYTES) {
            if let Some((_, gone)) = c.pop_front() {
                held -= gone.len();
            }
        }
        Some(b64)
    }

    /// How many blobs the cache holds (tests).
    pub fn cached(&self) -> usize {
        self.cache.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_is_stored_once_and_read_back_through_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        assert!(
            !blobs.dir().exists(),
            "nothing is created before the first put"
        );
        let bytes = b"\x89PNG not really".to_vec();
        let d1 = blobs.put(&bytes).unwrap();
        let d2 = blobs.put(&bytes).unwrap();
        assert_eq!(d1, d2);
        assert_eq!(
            std::fs::read_dir(blobs.dir()).unwrap().count(),
            1,
            "one file"
        );
        assert_eq!(std::fs::read(blobs.path(&d1)).unwrap(), bytes);
        let b64 = blobs.base64(&d1).unwrap();
        assert_eq!(decode(&b64).unwrap(), bytes);
        assert_eq!(blobs.cached(), 1);
        // A fresh instance (a restart) reads the same bytes from the file.
        assert_eq!(Blobs::new(dir.path()).base64(&d1).unwrap(), b64);
        // A name that is not a digest never reaches the file system.
        assert!(blobs.base64("../../etc/passwd").is_none());
        assert!(blobs.base64(&"0".repeat(64)).is_none(), "missing");
    }

    #[test]
    fn the_cache_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        for i in 0..(CACHE_MAX_ENTRIES + 4) {
            let d = blobs.put(format!("blob {i}").as_bytes()).unwrap();
            assert!(blobs.base64(&d).is_some());
        }
        assert_eq!(blobs.cached(), CACHE_MAX_ENTRIES);
    }
}
