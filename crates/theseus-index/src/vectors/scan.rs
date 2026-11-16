//! A vector file's records, laid out in columns for the scan (issue 21 of
//! 1.0.7): `<stamp key>.scan`, beside `<stamp key>.vec`.
//!
//! The `.vec` file keeps a record whole (hash, scale, 256 int8 numbers, 768
//! half floats, CRC), which is what appends and the 768-d re-score want and
//! what a scan does not: reading the int8 cuts out of it touches every page
//! of the file. The scan file holds only what the scan and the lookups need,
//! each in a column of its own: the text hashes, the scales, the int8 cuts,
//! and an open-addressed table of entries by hash (the same table
//! [`super::entries::Entries`] keeps in memory, laid out in the file). It is
//! mapped read-only, so the process's memory does not grow with the file.
//!
//! **It is a projection of the `.vec` file, rebuilt from it with no
//! re-embedding**: absent, another stamp's, torn, or not the file it was made
//! from (a compaction a crash cut short), it is made again from the records.
//! What was appended after it was made is read from the `.vec` file into
//! memory at open (a cache's `tail`) and folded into a new scan file once
//! there are [`FOLD`] of them. It is written beside its name, synced and
//! renamed over it, so a crash leaves the old file or the new one; it is
//! never written in place, so a running search's map is of a file that does
//! not change.
//!
//! **What says it is the file it was made from**: its length, its stamp's
//! dimensions, its entry count against the `.vec`'s, and three records (the
//! first, the middle, the last it covers) read from the `.vec` and compared
//! by hash and CRC. A `.vec` file's records are only appended or rewritten
//! whole, so a prefix that matches at its end, its middle and its start is
//! the prefix.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::entries::{slot_start, Entries, EMPTY};
use super::store::{align, f32_at, u128_at, u32_at, u64_at, Out};
use crate::mapped::Map;

const MAGIC: &[u8; 8] = b"thsscn01";
const HEAD: u64 = 160;
const CHECKS: usize = 3;

/// A record read back from the `.vec` file to check a scan file against.
pub(super) type Check = (u64, u128, u32);

/// A file's appended records are folded into a new scan file at this many.
pub(super) const FOLD: usize = 16_384;

/// The sections of a scan file that covers up to `cap` records: where each
/// starts, and the file's length for `slots` slots.
fn layout(cap: u64, cut: u64, slots: u64) -> [u64; 5] {
    let hashes = HEAD;
    let scale = align(hashes + cap * 16);
    let q = align(scale + cap * 4);
    let table = align(q + cap * cut);
    [hashes, scale, q, table, table + slots * 4]
}

/// How many slots hold `n` entries: [`Entries::with_capacity`]'s.
fn slots_for(n: usize) -> usize {
    let mut cap = 16usize;
    while cap * 3 < n * 4 {
        cap *= 2;
    }
    cap
}

/// A scan file, mapped.
pub(super) struct ScanFile {
    map: Map,
    // Kept open so the map's file is one this process holds, whatever happens
    // to its name.
    _file: File,
    n: usize,
    cut: usize,
    hashes: usize,
    scale: usize,
    q: usize,
    table: usize,
    slots: usize,
}

impl ScanFile {
    pub fn len(&self) -> usize {
        self.n
    }

    pub fn hash(&self, e: u32) -> u128 {
        u128_at(self.map.bytes(), self.hashes + 16 * e as usize)
    }

    pub fn scale(&self, e: u32) -> f32 {
        f32_at(self.map.bytes(), self.scale + 4 * e as usize)
    }

    pub fn q(&self, e: u32) -> &[i8] {
        self.map.i8s(self.q + self.cut * e as usize, self.cut)
    }

    /// The entry whose hash is `hash`: the latest record that has it.
    pub fn get(&self, hash: u128) -> Option<u32> {
        let b = self.map.bytes();
        let mask = self.slots - 1;
        let mut i = slot_start(self.slots, hash);
        loop {
            let e = u32_at(b, self.table + 4 * i);
            if e == EMPTY {
                return None;
            }
            if self.hash(e) == hash {
                return Some(e);
            }
            i = (i + 1) & mask;
        }
    }

    /// Read the int8 cuts in now, in one run (see `Map::will_need`): a scan is
    /// about to touch every one.
    pub fn prefetch(&self) {
        self.map.will_need(self.q, self.n * self.cut);
    }

    /// Let the process's copy of every page go (the kernel keeps them): the
    /// next read faults them back in. For measuring a search that finds its
    /// file cold, and for a long idle.
    pub fn release(&self) {
        self.map.release(0, self.map.bytes().len());
    }

    /// The pages out of the process and out of the kernel's cache (for measuring
    /// a search that finds its file cold).
    #[cfg(test)]
    pub fn evict(&self) {
        use std::os::fd::AsRawFd;
        self.release();
        // SAFETY: a valid descriptor; advice only.
        unsafe { libc::posix_fadvise(self._file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
    }

    /// Open `path`, if it is a scan file of `cut`-by-`full` records that
    /// covers a prefix of `vec`'s `records` records of `rec` bytes each.
    /// `Err` says why it is not (and it is made again).
    pub fn open(
        path: &Path,
        cut: usize,
        full: usize,
        vec: &File,
        rec: usize,
        records: usize,
        header: u64,
    ) -> Result<ScanFile, String> {
        let file = File::open(path).map_err(|e| format!("not opened: {e}"))?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut head = [0u8; HEAD as usize];
        file.read_exact_at(&mut head, 0)
            .map_err(|_| "no header".to_string())?;
        if &head[..8] != MAGIC {
            return Err("not a scan file".into());
        }
        if u32_at(&head, 8) as usize != cut || u32_at(&head, 12) as usize != full {
            return Err("another stamp's dimensions".into());
        }
        let n = usize::try_from(u64_at(&head, 16)).map_err(|e| e.to_string())?;
        let cap = u64_at(&head, 24);
        let slots = usize::try_from(u64_at(&head, 32)).map_err(|e| e.to_string())?;
        if n > records || (n as u64) > cap || !slots.is_power_of_two() || slots * 3 < n * 4 {
            return Err("more records than the vector file holds".into());
        }
        let [hashes, scale, q, table, total] = layout(cap, cut as u64, slots as u64);
        if len != total {
            return Err(format!("a length of {len}, not {total}"));
        }
        for i in 0..CHECKS {
            let at = 40 + 32 * i;
            let idx = u64_at(&head, at);
            if idx == u64::MAX {
                continue;
            }
            let want = (u128_at(&head, at + 8), u32_at(&head, at + 24));
            let mut r = vec![0u8; 20];
            let hash_at = header + idx * rec as u64;
            vec.read_exact_at(&mut r[..16], hash_at)
                .map_err(|e| e.to_string())?;
            vec.read_exact_at(&mut r[16..], hash_at + rec as u64 - 4)
                .map_err(|e| e.to_string())?;
            let got = (u128_at(&r, 0), u32_at(&r, 16));
            if got != want || idx >= n as u64 {
                return Err("not the vector file it was made from".into());
            }
        }
        let map = Map::random(&file).map_err(|e| format!("not mapped: {e}"))?;
        // Every slot of the entry table is empty or an entry the file has: a lookup trusts it.
        let b = map.bytes();
        if (0..slots).any(|i| {
            let e = u32_at(b, table as usize + 4 * i);
            e != EMPTY && e as usize >= n
        }) {
            return Err("an entry table with an entry the file does not have".into());
        }
        Ok(ScanFile {
            map,
            _file: file,
            n,
            cut,
            hashes: hashes as usize,
            scale: scale as usize,
            q: q as usize,
            table: table as usize,
            slots,
        })
    }
}

/// A scan file being written, one record at a time, in entry order.
pub(super) struct ScanWriter {
    file: File,
    path: PathBuf,
    tmp: PathBuf,
    cut: usize,
    full: usize,
    cap: usize,
    hashes: Vec<u128>,
    scales: Vec<f32>,
    q: Out,
    q_at: u64,
}

impl ScanWriter {
    /// A writer for up to `cap` records, of the scan file `path`: written
    /// beside it, and renamed over it by [`ScanWriter::finish`].
    pub fn new(path: &Path, cut: usize, full: usize, cap: usize) -> io::Result<ScanWriter> {
        let [_, _, q, _, _] = layout(cap as u64, cut as u64, 16);
        let tmp = path.with_extension("scan.tmp");
        Ok(ScanWriter {
            file: OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?,
            path: path.to_path_buf(),
            tmp,
            cut,
            full,
            cap,
            hashes: Vec::with_capacity(cap),
            scales: Vec::with_capacity(cap),
            q: Out::new(q),
            q_at: q,
        })
    }

    pub fn push(&mut self, hash: u128, scale: f32, q: &[i8]) -> io::Result<()> {
        if self.hashes.len() == self.cap || q.len() != self.cut {
            return Err(io::Error::other("a record the scan file has no room for"));
        }
        self.hashes.push(hash);
        self.scales.push(scale);
        // SAFETY: the same bytes: `i8` and `u8` have one layout.
        let bytes = unsafe { std::slice::from_raw_parts(q.as_ptr().cast::<u8>(), q.len()) };
        self.q.put(&self.file, bytes)
    }

    /// Write the rest, sync, rename it over the scan file's name, and map it.
    /// `checks` are records of the `.vec` file by their index: at most three.
    pub fn finish(mut self, checks: &[Check]) -> io::Result<ScanFile> {
        let n = self.hashes.len();
        let mut entries = Entries::with_capacity(n);
        for e in 0..n {
            entries.insert(e as u32, &self.hashes);
        }
        let slots = slots_for(n);
        let [hashes, scale, _, table, total] =
            layout(self.cap as u64, self.cut as u64, slots as u64);
        self.q.flush(&self.file)?;
        let mut col: Vec<u8> = Vec::with_capacity(n * 16);
        for h in &self.hashes {
            col.extend_from_slice(&h.to_le_bytes());
        }
        self.file.write_all_at(&col, hashes)?;
        col.clear();
        for s in &self.scales {
            col.extend_from_slice(&s.to_le_bytes());
        }
        self.file.write_all_at(&col, scale)?;
        col.clear();
        for s in entries.into_slots() {
            col.extend_from_slice(&s.to_le_bytes());
        }
        self.file.write_all_at(&col, table)?;
        let mut head = [0u8; HEAD as usize];
        head[..8].copy_from_slice(MAGIC);
        head[8..12].copy_from_slice(&(self.cut as u32).to_le_bytes());
        head[12..16].copy_from_slice(&(self.full as u32).to_le_bytes());
        head[16..24].copy_from_slice(&(n as u64).to_le_bytes());
        head[24..32].copy_from_slice(&(self.cap as u64).to_le_bytes());
        head[32..40].copy_from_slice(&(slots as u64).to_le_bytes());
        for i in 0..CHECKS {
            let at = 40 + 32 * i;
            let (idx, hash, crc) = checks.get(i).copied().unwrap_or((u64::MAX, 0, 0));
            head[at..at + 8].copy_from_slice(&idx.to_le_bytes());
            head[at + 8..at + 24].copy_from_slice(&hash.to_le_bytes());
            head[at + 24..at + 28].copy_from_slice(&crc.to_le_bytes());
        }
        self.file.write_all_at(&head, 0)?;
        self.file.set_len(total)?;
        self.file.sync_data()?;
        std::fs::rename(&self.tmp, &self.path)?;
        let map = Map::random(&self.file)?;
        // What was built here (the hashes, the table) is freed: give it back,
        // so no long-lived allocation made next sits above it.
        drop(col);
        self.hashes = Vec::new();
        self.scales = Vec::new();
        super::release_memory();
        Ok(ScanFile {
            map,
            _file: self.file,
            n,
            cut: self.cut,
            hashes: hashes as usize,
            scale: scale as usize,
            q: self.q_at as usize,
            table: table as usize,
            slots,
        })
    }
}
