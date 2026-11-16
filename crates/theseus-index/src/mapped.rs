//! The model's weights file, mapped and checked once (theseus-agqn).
//!
//! - **Mapped, read a window at a time.** `model.safetensors` (547 MB) is
//!   mapped read-only, and each tensor copied out of the map into its own f32
//!   buffer (compute stays f32: the owner's D-3), a window of [`WINDOW`] bytes
//!   at a time; each window's pages leave the process's resident set as soon
//!   as it is copied ([`Map::release`]: the page cache keeps them). So a load
//!   holds the tensors and at most a window of the file, never the file
//!   beside its copy, and no heap buffer the file's size.
//! - **Hashed once per file.** The SHA-256 is taken while the first load
//!   copies, and kept by the file's signature ([`Sig`]: its device, inode,
//!   size, and mtime, read from the descriptor that is mapped). A later load
//!   of the same signature (the idle unload undone) trusts it and only
//!   copies; a file replaced or touched has another signature and is hashed
//!   again. [`hashes_of`] counts the passes: the tests' seam.

use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The bytes copied or hashed between two releases (a multiple of 4, so an
/// f32 never straddles two windows of a tensor).
pub const WINDOW: usize = 1 << 20;

/// A file mapped read-only, unmapped on drop.
pub struct Map {
    ptr: *mut libc::c_void,
    len: usize,
}

impl Map {
    /// Map all of `f` (an empty file maps to no bytes), for a read from
    /// front to back.
    pub fn of(f: &File) -> io::Result<Map> {
        Self::with_advice(f, libc::MADV_SEQUENTIAL)
    }

    /// Map all of `f` for reads wherever they fall: no readahead advice, so a
    /// scan or a lookup pulls in only the pages it touches (the vector
    /// table's columns, issue 21 of 1.0.7).
    pub fn random(f: &File) -> io::Result<Map> {
        Self::with_advice(f, libc::MADV_NORMAL)
    }

    fn with_advice(f: &File, advice: libc::c_int) -> io::Result<Map> {
        let len = usize::try_from(f.metadata()?.len()).map_err(io::Error::other)?;
        if len == 0 {
            return Ok(Map {
                ptr: std::ptr::null_mut(),
                len: 0,
            });
        }
        // SAFETY: a fresh read-only private mapping of an open descriptor,
        // checked for failure; nothing else aliases it.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                f.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the range just mapped. Advice only: its failure is harmless.
        unsafe { libc::madvise(ptr, len, advice) };
        Ok(Map { ptr, len })
    }

    /// `len` bytes at `at` as int8 numbers (the vector table's cuts). The
    /// range is checked; an `i8` and a `u8` have one layout.
    pub fn i8s(&self, at: usize, len: usize) -> &[i8] {
        let b = &self.bytes()[at..at + len];
        // SAFETY: the same bytes, the same length, and `i8` has no alignment
        // or validity requirement beyond `u8`'s.
        unsafe { std::slice::from_raw_parts(b.as_ptr().cast::<i8>(), b.len()) }
    }

    pub fn bytes(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: `ptr` maps `len` readable bytes for as long as `self` lives.
        // A file truncated under the map would fault (SIGBUS): the weights
        // directory is the tender's, written only by the fetch.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
    }

    /// Ask the kernel to read `[at, at + len)` in now, in one run, rather than
    /// a page at a time as a scan reaches it: a scan over a file the kernel has
    /// dropped from its cache costs one sequential read, not a fault a page.
    /// A page already cached costs the call a look and no read.
    pub fn will_need(&self, at: usize, len: usize) {
        if len == 0 || self.len == 0 {
            return;
        }
        let page = page_size();
        let start = at / page * page;
        let end = (at + len).min(self.len);
        if end <= start {
            return;
        }
        // SAFETY: a page-aligned range inside the mapping; advice only.
        unsafe {
            libc::madvise(
                self.ptr.cast::<u8>().add(start).cast(),
                end - start,
                libc::MADV_WILLNEED,
            );
        }
    }

    /// Let `[at, at + len)`'s pages go from the resident set: the next read
    /// of them faults them back in from the page cache.
    pub fn release(&self, at: usize, len: usize) {
        if len == 0 || self.len == 0 {
            return;
        }
        let page = page_size();
        let start = at / page * page;
        let end = (at + len).min(self.len);
        if end <= start {
            return;
        }
        // SAFETY: a page-aligned range inside the mapping; MADV_DONTNEED on a
        // private read-only file mapping only drops pages, which the file
        // gives back unchanged.
        unsafe {
            libc::madvise(
                self.ptr.cast::<u8>().add(start).cast(),
                end - start,
                libc::MADV_DONTNEED,
            );
        }
    }
}

// SAFETY: a `Map` is a read-only mapping that nothing writes through; the bytes
// it hands out are shared immutably, so it may move between threads and be read
// from several at once. (A file truncated under it would fault, as `bytes` says.)
unsafe impl Send for Map {}
// SAFETY: as above.
unsafe impl Sync for Map {}

impl Drop for Map {
    fn drop(&mut self) {
        if self.len > 0 {
            // SAFETY: the mapping `of` made, unmapped once.
            unsafe { libc::munmap(self.ptr, self.len) };
        }
    }
}

fn page_size() -> usize {
    // SAFETY: no pointers.
    let p = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(p).unwrap_or(4096).max(1)
}

/// What says a file is the one hashed before: where it is on disk, its size,
/// and when it was last written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sig {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub mtime_s: i64,
    pub mtime_ns: i64,
}

impl Sig {
    pub fn of(m: &Metadata) -> Sig {
        Sig {
            dev: m.dev(),
            ino: m.ino(),
            size: m.size(),
            mtime_s: m.mtime(),
            mtime_ns: m.mtime_nsec(),
        }
    }
}

/// Per path: the signature last hashed, its SHA-256, and the passes so far.
#[derive(Debug, Default)]
struct Seen {
    last: Option<(Sig, String)>,
    hashes: u64,
}

fn seen() -> &'static Mutex<HashMap<PathBuf, Seen>> {
    static SEEN: OnceLock<Mutex<HashMap<PathBuf, Seen>>> = OnceLock::new();
    SEEN.get_or_init(Mutex::default)
}

/// The SHA-256 hashed before for `path` at `sig`, if it is that file still.
pub fn known(path: &Path, sig: &Sig) -> Option<String> {
    let seen = seen()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match &seen.get(path)?.last {
        Some((s, sha)) if s == sig => Some(sha.clone()),
        _ => None,
    }
}

/// A whole pass hashed `path` at `sig`: its SHA-256 is `sha`.
pub fn remember(path: &Path, sig: Sig, sha: &str) {
    let mut seen = seen()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let e = seen.entry(path.to_path_buf()).or_default();
    e.last = Some((sig, sha.to_string()));
    e.hashes += 1;
}

/// How many times this process has hashed the whole of `path`.
pub fn hashes_of(path: &Path) -> u64 {
    seen()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(path)
        .map_or(0, |s| s.hashes)
}
