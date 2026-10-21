//! The bench's own copies (theseus-vncz5): the swap's other build, and a copied
//! store. Each is written out before the bench goes on, so the kernel has
//! nothing of the bench's to write back while it times a phase. The swap's
//! copy of a debug `theseusd` is over 400 MB: left dirty, its writeback landed
//! in the stops, swaps and cancels the bench timed, and on ext4's data=ordered
//! each of the store's fsyncs there waited for it, 100 to 530 ms at a time.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// `from`'s files and directories into `to`, each file written out.
pub(crate) fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            copy_synced(&e.path(), &dest)?;
        }
    }
    Ok(())
}

/// Copy a file, written out before this returns, as an install writes its
/// binary (`theseusd install`'s `write_whole`: filled, synced, renamed).
fn copy_synced(from: &Path, to: &Path) -> Result<u64> {
    let n = std::fs::copy(from, to)?;
    std::fs::File::open(to)?.sync_all()?;
    Ok(n)
}

/// The swap's other build when none is given: a copy of `theseusd` in
/// `work/swap`, with a copy of the index tender beside it as an install puts
/// one (row 51), each written out before any phase is timed.
pub(super) fn swap_build(work: &Path, theseusd: &Path) -> Result<PathBuf> {
    let dir = work.join("swap");
    std::fs::create_dir_all(&dir)?;
    let copy = dir.join("theseusd");
    copy_synced(theseusd, &copy)
        .with_context(|| format!("copying {} for the swap", theseusd.display()))?;
    let tender = theseusd.with_file_name("theseus-index");
    if tender.is_file() {
        copy_synced(&tender, &dir.join("theseus-index"))
            .with_context(|| format!("copying {} for the swap", tender.display()))?;
    }
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cachestat(2)`'s answer (Linux 6.5): a file's pages in the page cache.
    #[repr(C)]
    #[derive(Debug, Default)]
    struct CacheStat {
        nr_cache: u64,
        nr_dirty: u64,
        nr_writeback: u64,
        nr_evicted: u64,
        nr_recently_evicted: u64,
    }

    /// `path`'s pages in the page cache, or none where the kernel cannot say
    /// (before 6.5), or on tmpfs, whose pages stay dirty however it syncs.
    fn cachestat(path: &Path) -> Option<CacheStat> {
        use std::os::fd::AsRawFd;
        // The same number on x86_64 and in the generic table (aarch64).
        const SYS_CACHESTAT: libc::c_long = 451;
        let f = std::fs::File::open(path).unwrap();
        // SAFETY: statfs is plain data, valid all zeroes.
        let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: an open file's fd and a buffer of the struct's size.
        if unsafe { libc::fstatfs(f.as_raw_fd(), &mut fs) } != 0 || fs.f_type == libc::TMPFS_MAGIC {
            return None;
        }
        // From offset 0, to the end of the file.
        let range = [0u64, 0u64];
        let mut cs = CacheStat::default();
        // SAFETY: an open file's fd, the range and the answer's buffers of
        // the kernel's sizes, and no flags.
        let r = unsafe {
            libc::syscall(
                SYS_CACHESTAT,
                f.as_raw_fd(),
                range.as_ptr(),
                &mut cs as *mut CacheStat,
                0u32,
            )
        };
        (r == 0).then_some(cs)
    }

    /// The bench's copies leave no page for the kernel to write back while it
    /// measures: the swap's other build and its tender, and a copied store.
    #[test]
    fn the_benchs_copies_are_written_out_before_it_measures() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir_all(bin.join("store")).unwrap();
        let theseusd = bin.join("theseusd");
        for (p, n) in [
            (theseusd.clone(), 3 << 20),
            (bin.join("theseus-index"), 1 << 20),
            (bin.join("store").join("seg"), 1 << 20),
        ] {
            std::fs::write(&p, vec![7u8; n]).unwrap();
            // Written out first: what is dirty after is the copy's.
            std::fs::File::open(&p).unwrap().sync_all().unwrap();
        }
        let work = d.path().join("work");
        let copy = swap_build(&work, &theseusd).unwrap();
        copy_dir(&bin.join("store"), &work.join("store")).unwrap();
        for (p, from) in [
            (copy.clone(), theseusd),
            (
                copy.with_file_name("theseus-index"),
                bin.join("theseus-index"),
            ),
            (
                work.join("store").join("seg"),
                bin.join("store").join("seg"),
            ),
        ] {
            assert_eq!(std::fs::read(&p).unwrap(), std::fs::read(&from).unwrap());
            let Some(cs) = cachestat(&p) else {
                eprintln!("no cachestat(2) here (before Linux 6.5, or tmpfs): not checked");
                return;
            };
            assert_eq!(
                (cs.nr_dirty, cs.nr_writeback),
                (0, 0),
                "{} was left for the kernel to write back: {cs:?}",
                p.display()
            );
        }
    }
}
