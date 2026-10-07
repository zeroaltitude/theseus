//! Image bytes, stored once (theseus-9g2), and every other file a model is
//! given, with what was made of it: a PDF's text by page and its parts
//! (theseus-c9l6).
//!
//! An image is up to 5 MiB, and a node is a WAL frame that every recovery
//! and index rebuild reads, so the bytes never go in a node. They go to
//! `<store dir>/blobs/<sha256>`, content-addressed and written atomically,
//! and the node holds the digest. The same image sent twice is one file.
//! The compiler renders an image block from its blob through a small
//! bounded cache of base64 strings, so a session's image is read and
//! encoded once, not on every loop. Nothing is read at startup.
//!
//! Each write goes to a temporary file of its own first, renamed once
//! synced. A crash inside a write leaves its file, and a crash inside a
//! batch every file of the batch (`put_many`); a later start sweeps a dead
//! writer's once it serves (`Blobs::sweep`, theseus-vipg).

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use base64::Engine as _;

/// Base64 the cache may hold: about four images at the 5 MiB limit, or
/// dozens of screenshots.
const CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;
const CACHE_MAX_ENTRIES: usize = 16;
/// The threads a batch's files are synced from at most (`put_many`).
const SYNC_THREADS: usize = 4;

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

/// The bytes a padded base64 string decodes to.
pub fn decoded_len(b64: &str) -> u64 {
    let pad = b64.bytes().rev().take_while(|b| *b == b'=').count();
    (b64.len() / 4 * 3).saturating_sub(pad) as u64
}

/// A kept file's text by section, as its blob holds it (theseus-c9l6): a
/// PDF's pages (a JSON list of strings, each `page N`), or a document's
/// sections with their labels and images (`{"sections": [...]}`).
pub type Texts = Arc<Vec<Section>>;

/// One section of a kept file's text.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Section {
    pub label: String,
    pub text: String,
    /// Its images (a notebook cell's outputs), each a blob.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<SectionImage>,
}

/// An image a section holds, stored in the blobs.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SectionImage {
    pub digest: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
}

/// A document's sections as their blob holds them.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sections {
    pub sections: Vec<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<String>,
}

/// A text blob's sections, in either of its shapes.
fn parse_texts(bytes: &[u8]) -> Option<Vec<Section>> {
    if let Ok(pages) = serde_json::from_slice::<Vec<String>>(bytes) {
        return Some(
            pages
                .into_iter()
                .enumerate()
                .map(|(i, text)| Section {
                    label: format!("page {}", i + 1),
                    text,
                    images: Vec::new(),
                })
                .collect(),
        );
    }
    serde_json::from_slice::<Sections>(bytes)
        .ok()
        .map(|s| s.sections)
}

/// Parsed texts the cache may hold: a few PDFs' worth.
const TEXT_CACHE_ENTRIES: usize = 8;

pub struct Blobs {
    dir: PathBuf,
    /// Most recently used last: (digest, base64).
    cache: Mutex<VecDeque<(String, Arc<str>)>>,
    /// Most recently used last: (digest, a PDF's texts) (theseus-c9l6).
    texts: Mutex<VecDeque<(String, Texts)>>,
    /// The syncs the writes have made, files' and the directory's
    /// (theseus-ehkp: what a batched write saves).
    syncs: AtomicU64,
    /// When this was built, as the store opened: a temporary file older
    /// than it, another process's, is a dead writer's (`sweep`).
    opened: SystemTime,
    /// A test's hold on every write, as a disk under a neighbour's IO holds
    /// a sync (`hold_puts`).
    #[cfg(test)]
    hold: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    /// A test's stand-in for a crash between a batch's syncs and its
    /// renames: called there.
    #[cfg(test)]
    at_renames: Mutex<Option<Box<dyn Fn() + Send>>>,
}

/// What a sweep of the temporary files did (`Blobs::sweep`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    /// A dead writer's, removed.
    pub removed: usize,
    /// Their bytes.
    pub bytes: u64,
    /// The temporary files left: this process's own, one newer than its
    /// open of the store, and one that could not be removed.
    pub kept: usize,
}

/// The writer's pid in a temporary file's name (`.<digest>.tmp-<pid>`, or
/// `.<digest>.tmp-<pid>-<n>` since each write gets its own), or `None` for
/// any other name: a blob's is its digest.
fn temporary_pid(name: &str) -> Option<u32> {
    let (d, rest) = name.strip_prefix('.')?.split_at_checked(64)?;
    let rest = rest.strip_prefix(".tmp-")?;
    let (pid, n) = rest.split_once('-').unwrap_or((rest, "0"));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !(is_digest(d) && digits(pid) && digits(n)) {
        return None;
    }
    pid.parse().ok()
}

impl Blobs {
    /// The blobs of the store at `store_dir`. Creates nothing until the
    /// first `put`.
    pub fn new(store_dir: &Path) -> Self {
        Self {
            dir: store_dir.join("blobs"),
            cache: Mutex::new(VecDeque::new()),
            texts: Mutex::new(VecDeque::new()),
            syncs: AtomicU64::new(0),
            opened: SystemTime::now(),
            #[cfg(test)]
            hold: Mutex::new(None),
            #[cfg(test)]
            at_renames: Mutex::new(None),
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

    /// A blob's bytes, when it is there and they match its name.
    pub fn read(&self, digest: &str) -> Option<Vec<u8>> {
        if !is_digest(digest) {
            return None;
        }
        let bytes = std::fs::read(self.path(digest)).ok()?;
        (self::digest(&bytes) == digest).then_some(bytes)
    }

    /// A kept file's text by section, from the JSON blob a `File` names,
    /// read once and kept in a small cache: a request renders it on every
    /// loop.
    pub fn texts(&self, digest: &str) -> Option<Texts> {
        {
            let mut c = self.texts.lock().unwrap();
            if let Some(i) = c.iter().position(|(d, _)| d == digest) {
                let hit = c.remove(i).expect("present");
                let t = hit.1.clone();
                c.push_back(hit);
                return Some(t);
            }
        }
        let t: Texts = Arc::new(parse_texts(&self.read(digest)?)?);
        let mut c = self.texts.lock().unwrap();
        c.push_back((digest.to_string(), t.clone()));
        while c.len() > TEXT_CACHE_ENTRIES {
            c.pop_front();
        }
        Some(t)
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
        let tmp = self.tmp(&d);
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            self.syncs.fetch_add(1, Ordering::Relaxed);
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &path)?;
        self.sync_dir();
        Ok(d)
    }

    /// Store many blobs at once (theseus-ehkp: a sink frame's staged
    /// states, a stop's), each as `put` stores one, with their syncs
    /// batched: every new one written to its temporary file, then every file
    /// synced, then each renamed, then the directory synced once, where
    /// `put` syncs the directory once a blob. Returns each one's digest, or
    /// why it was not stored, in order. What a crash leaves at each step:
    /// before the renames, temporary files no name reads (as `put`'s); after
    /// some renames and before the directory's sync, each blob there whole
    /// or not at all; after it, every one. The caller writes no row naming
    /// any of them until this returns, so no row outlives its blob.
    pub fn put_many(&self, all: &[&[u8]]) -> Vec<std::io::Result<String>> {
        let digests: Vec<String> = all.iter().map(|b| digest(b)).collect();
        let mut failed: std::collections::HashMap<String, String> = Default::default();
        // The new ones, each once, in order: (digest, temporary, file).
        let mut fresh: Vec<(String, PathBuf, std::fs::File)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let made = std::fs::create_dir_all(&self.dir);
        for (bytes, d) in all.iter().zip(&digests) {
            if !seen.insert(d.as_str()) || self.path(d).exists() {
                continue;
            }
            if let Err(e) = &made {
                failed.insert(d.clone(), e.to_string());
                continue;
            }
            #[cfg(test)]
            if let Some(rx) = self.hold.lock().unwrap().as_ref() {
                let _ = rx.recv();
            }
            let tmp = self.tmp(d);
            let written = std::fs::File::create(&tmp).and_then(|mut f| {
                f.write_all(bytes)?;
                Ok(f)
            });
            match written {
                Ok(f) => fresh.push((d.clone(), tmp, f)),
                Err(e) => {
                    failed.insert(d.clone(), e.to_string());
                }
            }
        }
        let synced = self.sync_files(fresh.iter().map(|(_, _, f)| f));
        #[cfg(test)]
        if let Some(at) = self
            .at_renames
            .lock()
            .unwrap()
            .as_ref()
            .filter(|_| !fresh.is_empty())
        {
            at();
        }
        let mut renamed = 0;
        for ((d, tmp, _), r) in fresh.iter().zip(synced) {
            match r.and_then(|()| std::fs::rename(tmp, self.path(d))) {
                Ok(()) => renamed += 1,
                Err(e) => {
                    let _ = std::fs::remove_file(tmp);
                    failed.insert(d.clone(), e.to_string());
                }
            }
        }
        if renamed > 0 {
            self.sync_dir();
        }
        digests
            .into_iter()
            .map(|d| match failed.get(&d) {
                Some(why) => Err(std::io::Error::other(why.clone())),
                None => Ok(d),
            })
            .collect()
    }

    /// Sync each file; each one's outcome, in order. More than a few go out
    /// together from up to [`SYNC_THREADS`] threads, so the journal commits
    /// them together (theseus-ehkp: a stop's batch). Never `syncfs`, which
    /// flushes every dirty page of the filesystem, a neighbour build's too:
    /// beside a writer of 256 MiB at a time, its first sync of 700 small
    /// files took 273 ms where four threads took 72 to 458 ms, alone 19 ms
    /// against 78 (2026-10-07, a 4-core VM).
    fn sync_files<'a>(
        &self,
        files: impl Iterator<Item = &'a std::fs::File>,
    ) -> Vec<std::io::Result<()>> {
        let files: Vec<&std::fs::File> = files.collect();
        self.syncs.fetch_add(files.len() as u64, Ordering::Relaxed);
        if files.len() <= SYNC_THREADS {
            return files.iter().map(|f| f.sync_all()).collect();
        }
        let per = files.len().div_ceil(SYNC_THREADS);
        std::thread::scope(|s| {
            let parts: Vec<_> = files
                .chunks(per)
                .map(|part| {
                    let n = part.len();
                    (
                        n,
                        s.spawn(move || part.iter().map(|f| f.sync_all()).collect::<Vec<_>>()),
                    )
                })
                .collect();
            parts
                .into_iter()
                .flat_map(|(n, p)| {
                    p.join().unwrap_or_else(|_| {
                        (0..n)
                            .map(|_| Err(std::io::Error::other("a sync's thread panicked")))
                            .collect()
                    })
                })
                .collect()
        })
    }

    /// A new blob's temporary file: its own for each write, so two writes of
    /// the same bytes at once (two judgments of one state) each rename their
    /// own file, where a name shared by the process let one rename the
    /// other's away and fail with "no such file".
    fn tmp(&self, d: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        self.dir
            .join(format!(".{d}.tmp-{}-{n}", std::process::id()))
    }

    /// The directory's sync, which makes its renames durable.
    fn sync_dir(&self) {
        if let Ok(dir) = std::fs::File::open(&self.dir) {
            self.syncs.fetch_add(1, Ordering::Relaxed);
            let _ = dir.sync_all();
        }
    }

    /// The syncs the writes have made since this was built, files' and the
    /// directory's.
    pub fn syncs(&self) -> u64 {
        self.syncs.load(Ordering::Relaxed)
    }

    /// The temporary files a dead writer left (theseus-vipg): a crash
    /// inside a batch (`put_many`) leaves every file of it, whole, synced
    /// and never renamed, and a crash inside a `put` its one. A name of a
    /// temporary's shape goes when its pid is another process's and its file
    /// is older than this process's open of the store: the store has one
    /// writer at a time (its lock), so that writer is dead. A blob (its name
    /// is its digest) and any other name are never touched, nor this
    /// process's own temporaries, so its own batch between its syncs and its
    /// renames still finds its files; a dead writer whose pid this process
    /// reused (an exec in place keeps it) leaves its files to a later start.
    /// It lists the directory, so the daemon runs it once it serves
    /// (`Core::sweep_blobs_after_serving`), never on the start path.
    pub fn sweep(&self) -> Swept {
        let mut out = Swept::default();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return out;
        };
        let me = std::process::id();
        for e in entries.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(temporary_pid) else {
                continue;
            };
            let meta = e.metadata().ok();
            let older = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .is_some_and(|t| t < self.opened);
            if pid == me || !older {
                out.kept += 1;
                continue;
            }
            match std::fs::remove_file(e.path()) {
                Ok(()) => {
                    out.removed += 1;
                    out.bytes += meta.map_or(0, |m| m.len());
                }
                Err(err) => {
                    tracing::warn!(error = %err, file = %e.path().display(), "blobs: a dead writer's temporary file was not removed");
                    out.kept += 1;
                }
            }
        }
        out
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

    /// What was read of a blob on request (a recording's transcript), kept
    /// beside the blobs by its digest and `what`: `<blobs>/derived/<digest>.<what>`.
    pub fn derived(&self, digest: &str, what: &str) -> Option<Vec<u8>> {
        if !is_digest(digest) {
            return None;
        }
        std::fs::read(self.dir.join("derived").join(format!("{digest}.{what}"))).ok()
    }

    /// Keep what was read of a blob on request, written whole or not at all.
    pub fn put_derived(&self, digest: &str, what: &str, bytes: &[u8]) -> std::io::Result<()> {
        if !is_digest(digest) {
            return Err(std::io::Error::other("not a digest"));
        }
        let dir = self.dir.join("derived");
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!(".{digest}.{what}.tmp-{}", std::process::id()));
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, dir.join(format!("{digest}.{what}")))
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

    /// Many blobs at once: each stored as `put` stores one, a blob already
    /// there and one named twice written once, with one sync a new file and
    /// one for the directory, where `put` makes two a blob.
    #[test]
    fn many_blobs_are_written_with_one_directory_sync() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let there = blobs.put(b"already there").unwrap();
        assert_eq!(blobs.syncs(), 2, "put: the file's sync and the directory's");
        let all: Vec<Vec<u8>> = (0..5).map(|i| format!("state {i}").into_bytes()).collect();
        let mut asked: Vec<&[u8]> = all.iter().map(Vec::as_slice).collect();
        asked.push(b"already there");
        asked.push(all[0].as_slice());
        let before = blobs.syncs();
        let got = blobs.put_many(&asked);
        assert_eq!(blobs.syncs() - before, 5 + 1, "five files, one directory");
        let got: Vec<String> = got.into_iter().map(Result::unwrap).collect();
        assert_eq!(got[5], there);
        assert_eq!(got[6], got[0]);
        for (bytes, d) in asked.iter().zip(&got) {
            assert_eq!(blobs.read(d).as_deref(), Some(*bytes));
        }
        let names: Vec<String> = std::fs::read_dir(blobs.dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 6, "no temporary file is left: {names:?}");
        let before = blobs.syncs();
        assert!(blobs.put_many(&asked).iter().all(Result::is_ok));
        assert_eq!(blobs.syncs(), before, "nothing new, nothing synced");
    }

    /// The same bytes put from several threads at once: every put returns
    /// the digest, and one file is left.
    #[test]
    fn the_same_blob_put_at_once_from_many_threads_is_stored_once() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let bytes = vec![7u8; 64 * 1024];
        for _ in 0..20 {
            let d = digest(&bytes);
            let _ = std::fs::remove_file(blobs.path(&d));
            std::thread::scope(|s| {
                let each: Vec<_> = (0..8).map(|_| s.spawn(|| blobs.put(&bytes))).collect();
                for h in each {
                    assert_eq!(h.join().unwrap().unwrap(), d);
                }
            });
            assert_eq!(std::fs::read_dir(blobs.dir()).unwrap().count(), 1);
        }
    }

    /// Where the sweep test's stand-in writes its batch.
    const STAND_IN_DIR: &str = "THESEUS_TEST_BLOBS_DIR";
    /// The blobs of the stand-in's batch.
    const BATCH: usize = 40;

    /// The temporary files under the blobs of the store at `store_dir`.
    fn temporaries(store_dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(store_dir.join("blobs"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with('.'))
            .collect();
        names.sort();
        names
    }

    /// Where the stand-in stops: between its batch's syncs and its renames,
    /// until the SIGKILL.
    fn stop_here() {
        loop {
            // SAFETY: a signal to this process itself.
            unsafe { libc::raise(libc::SIGSTOP) };
        }
    }

    /// The writer's stand-in for the sweep's test: run again by it, it puts
    /// a batch of [`BATCH`] blobs and stops itself with SIGSTOP between the
    /// batch's syncs and its renames, where it waits for the SIGKILL. Run on
    /// its own, with no environment, it does nothing.
    #[test]
    fn a_writer_stand_in_for_the_sweep_test() {
        let Some(dir) = std::env::var_os(STAND_IN_DIR) else {
            return;
        };
        let blobs = Blobs::new(Path::new(&dir));
        *blobs.at_renames.lock().unwrap() = Some(Box::new(stop_here));
        let all: Vec<Vec<u8>> = (0..BATCH)
            .map(|i| format!("a batch's state {i}").into_bytes())
            .collect();
        blobs.put_many(&all.iter().map(Vec::as_slice).collect::<Vec<_>>());
        unreachable!("stopped between the batch's syncs and its renames");
    }

    /// Run the stand-in on `store_dir`, wait until it has stopped between its
    /// batch's syncs and its renames, and kill it there with SIGKILL.
    fn kill_a_writer_mid_batch(store_dir: &Path) {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "blobs::tests::a_writer_stand_in_for_the_sweep_test",
                "--exact",
                "--nocapture",
            ])
            .env(STAND_IN_DIR, store_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        let mut status = 0;
        // SAFETY: waits for our own child to stop; it is not reaped while stopped.
        let got = unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED) };
        assert_eq!(got, pid, "{}", std::io::Error::last_os_error());
        assert!(
            libc::WIFSTOPPED(status),
            "the stand-in ended before its renames: status {status:#x}"
        );
        // SAFETY: our own stopped child.
        unsafe { libc::kill(pid, libc::SIGKILL) };
        child.wait().unwrap();
    }

    /// A crash between a batch's syncs and its renames (theseus-vipg): the
    /// writer, this test binary run again, is killed there with SIGKILL, and
    /// leaves its batch's files, whole and synced, under its pid. The next
    /// open's sweep removes every one, and nothing else: not a blob put before
    /// the crash; not this process's own temporaries, a batch of an earlier
    /// open held between its syncs and its renames across the next open,
    /// whose renames then find their files; nor another process's file newer
    /// than the open.
    #[test]
    fn a_crash_mid_batch_leaves_temporaries_the_next_opens_sweep_removes() {
        let dir = tempfile::tempdir().unwrap();
        let earlier = Blobs::new(dir.path());
        let finished = earlier.put(b"a blob put before the crash").unwrap();
        kill_a_writer_mid_batch(dir.path());
        let left = temporaries(dir.path());
        assert_eq!(left.len(), BATCH, "the batch's files are left: {left:?}");
        assert!(left.iter().all(|n| temporary_pid(n).is_some()), "{left:?}");
        // This process's own batch, of the earlier open, held between its
        // syncs and its renames until after the next open's sweep.
        let (at, arrived) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel::<()>();
        *earlier.at_renames.lock().unwrap() = Some(Box::new(move || {
            let _ = at.send(());
            let _ = held.recv();
        }));
        let own: [&[u8]; 2] = [b"this process's own, mid-batch", b"and its second"];
        std::thread::scope(|s| {
            let batch = s.spawn(|| earlier.put_many(&own));
            arrived.recv().unwrap();
            let next = Blobs::new(dir.path());
            // Another process's, newer than the open: as far as the sweep can
            // tell, a live writer's.
            let newer = next.dir().join(format!(
                ".{}.tmp-{}-0",
                digest(b"newer"),
                std::process::id() + 1
            ));
            std::fs::File::create(&newer)
                .unwrap()
                .set_modified(next.opened + std::time::Duration::from_secs(1))
                .unwrap();
            let swept = next.sweep();
            assert_eq!(
                (swept.removed, swept.kept),
                (BATCH, 3),
                "the dead writer's, every one: {swept:?}"
            );
            let left = temporaries(dir.path());
            assert_eq!(
                left.len(),
                3,
                "this process's own two, and the newer: {left:?}"
            );
            drop(release);
            let got = batch.join().unwrap();
            assert!(
                got.iter().all(Result::is_ok),
                "its renames found their files: {got:?}"
            );
            std::fs::remove_file(&newer).unwrap();
            assert_eq!(temporaries(dir.path()), Vec::<String>::new());
            assert_eq!(
                next.read(&finished).as_deref(),
                Some(&b"a blob put before the crash"[..])
            );
            for b in own {
                assert_eq!(next.read(&digest(b)).as_deref(), Some(b));
            }
            assert_eq!(next.sweep(), Swept::default(), "nothing left to sweep");
        });
    }

    /// A temporary file's name gives its writer's pid, in either shape; a
    /// blob's, a derived file's, and any other name give none.
    #[test]
    fn a_temporarys_name_gives_its_writers_pid() {
        let d = digest(b"a state");
        assert_eq!(temporary_pid(&format!(".{d}.tmp-4242-17")), Some(4242));
        assert_eq!(temporary_pid(&format!(".{d}.tmp-4242")), Some(4242));
        for other in [
            d.clone(),
            format!(".{d}"),
            format!(".{d}.tmp-"),
            format!(".{d}.tmp-42x"),
            format!(".{d}.tmp-42-"),
            format!(".{d}.tmp--1"),
            format!(".{d}.transcript.tmp-4242"),
            format!(".{}.tmp-4242", d.to_uppercase()),
            format!(".{}.tmp-4242", &d[..63]),
            "derived".to_string(),
        ] {
            assert_eq!(temporary_pid(&other), None, "{other}");
        }
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
