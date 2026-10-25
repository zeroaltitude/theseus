//! SPIKE (theseus-xxac7), off unless `THESEUS_INDEX_VIEW_WEIGHTS` is set:
//! the model's f32 tensors used straight from the read-only map.
//!
//! candle's CPU storage is a `Vec<f32>`, so a tensor cannot borrow a map. This
//! module builds each tensor's `Vec` over the map's bytes (`Vec::from_raw_parts`)
//! and makes that sound with a global allocator, [`ViewAlloc`], that knows the
//! mapped ranges: a `dealloc` (or the free half of a `realloc`) of a pointer
//! inside one is a no-op. The weights are read-only to candle (every op makes a
//! new tensor), the mapping lives for the process (it is never unmapped, so a
//! tensor that outlives the embedder cannot fault), and the pages are
//! file-backed: the kernel reclaims them under pressure, and [`release_pages`]
//! hands them back at once. Compute stays f32 on the same bytes, so the
//! embeddings are bit-identical to the copying load's (`tests_view.rs`).
//!
//! - Alignment: a tensor whose bytes do not start 4-aligned is copied
//!   (`from_le_bytes`), as on a big-endian target; every other tensor is
//!   viewed. safetensors pads its header to 8 bytes, so a real checkpoint
//!   views all of them.
//! - Spike code: drop this module, its `mod` line, the `[global_allocator]`
//!   lines in `main.rs` and `lib.rs`, and the call at the top of
//!   `weights::read_safetensors`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::{ensure, Context as _};
use candle_core::{Device, Tensor};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::mapped;
use crate::weights::{hex, LoadError};

const SLOTS: usize = 8;

/// The mapped ranges `(start, end)` whose pointers the allocator never frees.
static RANGES: [(AtomicUsize, AtomicUsize); SLOTS] =
    [const { (AtomicUsize::new(0), AtomicUsize::new(0)) }; SLOTS];

fn inside(p: *mut u8) -> bool {
    let a = p as usize;
    RANGES.iter().any(|(s, e)| {
        let (s, e) = (s.load(Ordering::Acquire), e.load(Ordering::Acquire));
        s != 0 && a >= s && a < e
    })
}

/// The system allocator, except that it leaves a mapped weight tensor's
/// "allocation" alone.
pub struct ViewAlloc;

// SAFETY: every call goes to `System`, but a pointer inside a registered
// mapping, which `System` never returned, is not passed to it.
unsafe impl GlobalAlloc for ViewAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if !inside(p) {
            unsafe { System.dealloc(p, l) }
        }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if !inside(p) {
            return unsafe { System.realloc(p, l, n) };
        }
        // A viewed tensor grown: a copy, and the map's bytes stay.
        let q = unsafe { System.alloc(Layout::from_size_align_unchecked(n, l.align())) };
        if !q.is_null() {
            unsafe { std::ptr::copy_nonoverlapping(p, q, l.size().min(n)) };
        }
        q
    }
}

/// Whether the spike's load is asked for: `THESEUS_INDEX_VIEW_WEIGHTS` set.
pub fn enabled() -> bool {
    std::env::var_os("THESEUS_INDEX_VIEW_WEIGHTS").is_some()
}

/// A file mapped for the life of the process.
struct View {
    ptr: *mut libc::c_void,
    len: usize,
}

// SAFETY: a read-only private mapping, never unmapped or written.
unsafe impl Send for View {}
unsafe impl Sync for View {}

impl View {
    fn bytes(&self) -> &[u8] {
        // SAFETY: `ptr` maps `len` readable bytes for the process's life.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
    }

    fn release(&self, at: usize, len: usize) {
        let page = 4096usize;
        let start = at / page * page;
        let end = (at + len).min(self.len);
        if end > start {
            // SAFETY: a page-aligned range of a private read-only file
            // mapping: DONTNEED only drops pages the file gives back.
            unsafe {
                libc::madvise(
                    self.ptr.cast::<u8>().add(start).cast(),
                    end - start,
                    libc::MADV_DONTNEED,
                );
            }
        }
    }
}

fn views() -> &'static Mutex<HashMap<PathBuf, (mapped::Sig, &'static View)>> {
    static V: OnceLock<Mutex<HashMap<PathBuf, (mapped::Sig, &'static View)>>> = OnceLock::new();
    V.get_or_init(Mutex::default)
}

fn map(path: &Path) -> Result<(mapped::Sig, &'static View), LoadError> {
    let f = File::open(path).map_err(|e| LoadError::Other(e.into()))?;
    let md = f.metadata().map_err(|e| LoadError::Other(e.into()))?;
    if md.len() == 0 {
        return Err(LoadError::Other(anyhow::anyhow!("an empty weights file")));
    }
    let sig = mapped::Sig::of(&md);
    let mut all = views().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((s, v)) = all.get(path) {
        if *s == sig {
            return Ok((sig, v));
        }
    }
    let len = md.len() as usize;
    // SAFETY: a fresh read-only private mapping, checked for failure.
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
        return Err(LoadError::Other(std::io::Error::last_os_error().into()));
    }
    let v: &'static View = Box::leak(Box::new(View { ptr, len }));
    let slot = RANGES
        .iter()
        .find(|(s, _)| s.load(Ordering::Acquire) == 0)
        .ok_or_else(|| LoadError::Other(anyhow::anyhow!("too many mapped weight files")))?;
    slot.1.store(ptr as usize + len, Ordering::Release);
    slot.0.store(ptr as usize, Ordering::Release);
    all.insert(path.to_path_buf(), (sig, v));
    Ok((sig, v))
}

/// With `THESEUS_INDEX_VIEW_WEIGHTS=release`, the pages go back after every
/// backfill batch, so a busy tender's resident set holds the weights only
/// while a batch runs (the next batch faults them from the page cache).
pub fn after_batch() {
    static RELEASE: OnceLock<bool> = OnceLock::new();
    if *RELEASE
        .get_or_init(|| std::env::var("THESEUS_INDEX_VIEW_WEIGHTS").is_ok_and(|v| v == "release"))
    {
        release_pages();
    }
}

/// Hand every mapped page back (the next use faults them from the page cache).
pub fn release_pages() {
    let all = views().lock().unwrap_or_else(|e| e.into_inner());
    for (_, v) in all.values() {
        v.release(0, v.len);
    }
}

/// The f32 tensors of a safetensors file, viewed from its map, if its
/// SHA-256 is `want` (taken once per file signature, as `weights` does).
pub fn read_safetensors(path: &Path, want: &str) -> Result<HashMap<String, Tensor>, LoadError> {
    let (sig, v) = map(path)?;
    let got = match mapped::known(path, &sig) {
        Some(g) => g,
        None => {
            let mut h = Sha256::new();
            let mut i = 0;
            while i < v.len {
                let n = (v.len - i).min(mapped::WINDOW);
                h.update(&v.bytes()[i..i + n]);
                v.release(i, n);
                i += n;
            }
            let g = hex(&h.finalize());
            mapped::remember(path, sig, &g);
            g
        }
    };
    if got != want {
        return Err(LoadError::Refused {
            file: path.to_path_buf(),
            want: want.to_string(),
            got,
        });
    }
    tensors(v).map_err(|e| LoadError::Other(e.context(format!("reading {}", path.display()))))
}

fn tensors(v: &View) -> anyhow::Result<HashMap<String, Tensor>> {
    let bytes = v.bytes();
    ensure!(bytes.len() >= 8, "shorter than its header's length");
    let n = u64::from_le_bytes(bytes[..8].try_into()?) as usize;
    ensure!(n <= 100 << 20 && bytes.len() >= 8 + n, "a bad header");
    let base = 8 + n;
    let header: serde_json::Map<String, Value> =
        serde_json::from_slice(&bytes[8..base]).context("the header is not JSON")?;
    let mut out = HashMap::new();
    for (name, h) in header {
        if name == "__metadata__" {
            continue;
        }
        ensure!(h["dtype"] == "F32", "{name}: only f32 checkpoints load");
        let shape: Vec<usize> = h["shape"]
            .as_array()
            .context("no shape")?
            .iter()
            .map(|d| d.as_u64().map(|d| d as usize).context("a bad dimension"))
            .collect::<anyhow::Result<_>>()?;
        let off = h["data_offsets"].as_array().context("no data_offsets")?;
        let (b, e) = (
            off.first().and_then(Value::as_u64).context("an offset")? as usize,
            off.get(1).and_then(Value::as_u64).context("an offset")? as usize,
        );
        let elems: usize = shape.iter().product();
        ensure!(
            e >= b && e - b == elems * 4,
            "{name}'s bytes do not match its shape"
        );
        ensure!(base + e <= bytes.len(), "{name} runs past the file's end");
        let at = &bytes[base + b..base + e];
        let t = if cfg!(target_endian = "little") && (at.as_ptr() as usize).is_multiple_of(4) {
            // SAFETY: `at` is `elems` aligned f32s of a mapping that lives for
            // the process; `ViewAlloc` never frees it, and nothing writes it.
            let data = unsafe { Vec::from_raw_parts(at.as_ptr() as *mut f32, elems, elems) };
            Tensor::from_vec(data, shape, &Device::Cpu)?
        } else {
            let (quads, _) = at.as_chunks::<4>();
            let data: Vec<f32> = quads.iter().map(|q| f32::from_le_bytes(*q)).collect();
            Tensor::from_vec(data, shape, &Device::Cpu)?
        };
        out.insert(name, t);
    }
    Ok(out)
}

/// The bench's seeded model (theseus-xxac7): with `THESEUS_INDEX_SPIKE_UNPINNED`
/// set, the spec pins the files found under `weights_dir` instead of the
/// published ones, so a full-size model of seeded weights loads. Off, nothing
/// changes.
pub fn unpin(spec: &mut crate::embedder::ModelSpec, dir: Option<&Path>) {
    let Some(dir) = dir else { return };
    if std::env::var_os("THESEUS_INDEX_SPIKE_UNPINNED").is_none() {
        return;
    }
    if let Ok(h) = crate::weights::sha256_file(&spec.weights_path(dir)) {
        spec.weights_sha256 = h;
    }
    if let Ok(h) = crate::weights::sha256_file(&spec.tokenizer_path(dir)) {
        spec.tokenizer_sha256 = h;
    }
}
