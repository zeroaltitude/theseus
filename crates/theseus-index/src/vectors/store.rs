//! What the vector table's two mapped files are made of (issue 21 of 1.0.7): a
//! bit set for the few flags that change, a buffered writer for a column that
//! is written once, little-endian reads out of a mapped slice, and a file that
//! has no name.
//!
//! The table's big parts (the int8 cuts, the rows, the node ids) are written
//! once into a file and mapped read-only, so the kernel pages them and the
//! process's own memory does not grow with the store. What changes after the
//! write (a row killed, a vector found for a row) lives in memory, at a bit
//! or four bytes a row.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// A set of bits, one a flag.
#[derive(Debug, Default, Clone)]
pub(super) struct Bits {
    words: Vec<u64>,
    len: usize,
}

impl Bits {
    pub fn filled(len: usize, value: bool) -> Bits {
        let word = if value { u64::MAX } else { 0 };
        Bits {
            words: vec![word; len.div_ceil(64)],
            len,
        }
    }

    pub fn get(&self, i: usize) -> bool {
        i < self.len && self.words[i / 64] >> (i % 64) & 1 == 1
    }

    pub fn set(&mut self, i: usize, value: bool) {
        if i >= self.len {
            return;
        }
        let (w, b) = (i / 64, i % 64);
        if value {
            self.words[w] |= 1 << b;
        } else {
            self.words[w] &= !(1 << b);
        }
    }

    /// The bytes it holds.
    pub fn bytes(&self) -> usize {
        self.words.capacity() * 8
    }
}

/// Round `x` up to a multiple of 16: where a mapped column starts.
pub(super) fn align(x: u64) -> u64 {
    x.div_ceil(16) * 16
}

pub(super) fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap_or_default())
}

pub(super) fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap_or_default())
}

pub(super) fn u128_at(b: &[u8], at: usize) -> u128 {
    u128::from_le_bytes(b[at..at + 16].try_into().unwrap_or_default())
}

pub(super) fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap_or_default())
}

/// One column's writer: bytes go to a buffer and, a megabyte at a time, to
/// the file at the column's own place.
pub(super) struct Out {
    at: u64,
    buf: Vec<u8>,
}

const FLUSH: usize = 1 << 20;

impl Out {
    pub fn new(at: u64) -> Out {
        Out {
            at,
            buf: Vec::with_capacity(FLUSH + (FLUSH >> 4)),
        }
    }

    /// Where the next byte will go.
    pub fn pos(&self) -> u64 {
        self.at + self.buf.len() as u64
    }

    pub fn put(&mut self, file: &File, b: &[u8]) -> io::Result<()> {
        self.buf.extend_from_slice(b);
        if self.buf.len() >= FLUSH {
            self.flush(file)?;
        }
        Ok(())
    }

    pub fn flush(&mut self, file: &File) -> io::Result<()> {
        file.write_all_at(&self.buf, self.at)?;
        self.at += self.buf.len() as u64;
        self.buf.clear();
        Ok(())
    }
}

/// A file with no name in `dir` (`O_TMPFILE`): nothing can open, truncate or
/// replace it by path, a crash leaves nothing behind, and the kernel gives
/// its blocks back when the last map and descriptor go. Where the filesystem
/// has none, a file made and unlinked at once.
pub(super) fn tmpfile(dir: &Path) -> io::Result<File> {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_TMPFILE)
        .mode(0o600)
        .open(dir)
    {
        Ok(f) => Ok(f),
        Err(e)
            if matches!(
                e.raw_os_error(),
                Some(libc::EOPNOTSUPP | libc::EISDIR | libc::EINVAL | libc::ENOENT)
            ) && dir.is_dir() =>
        {
            static N: AtomicU64 = AtomicU64::new(0);
            let path = dir.join(format!(
                ".scratch-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            std::fs::remove_file(&path)?;
            Ok(f)
        }
        Err(e) => Err(e),
    }
}
