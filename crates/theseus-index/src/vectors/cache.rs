//! One stamp's vectors on disk: the `.vec` file of records, and in memory or in
//! its scan file (`scan.rs`) each record's hash, int8 cut and scale.

use super::*;

const MAGIC: &[u8; 8] = b"thsvec01";
pub(super) const HEADER: u64 = 16;

/// One stamp's vectors: the file, and in memory or in its scan file each
/// record's hash, int8 cut, and scale (the 768-d vectors stay on disk, read
/// for the re-score).
pub struct Cache {
    pub stamp: Stamp,
    pub(super) key: String,
    pub(super) dir: PathBuf,
    pub(super) file: File,
    pub(super) cut: usize,
    pub(super) full: usize,
    pub(super) rec: usize,
    pub(super) by_hash: Entries,
    /// Each entry's text hash.
    pub(super) hashes: Vec<u128>,
    pub(super) q: Vec<i8>,
    pub(super) scale: Vec<f32>,
    /// Entries whose text no chunk holds (theseus-64x): no row points at
    /// them, so they answer nothing, and the next compaction drops them.
    pub(super) dead: Vec<bool>,
    pub(super) dead_count: usize,
    /// The records of the scan file `<key>.scan`, read in place (issue 21 of
    /// 1.0.7): entries `0..scan.len()`. The fields above then hold only the
    /// entries after them (`n_base` and on), numbered from 0 in the `Vec`s; a
    /// cache opened without a scan file holds every entry in them, as it
    /// always did.
    pub(super) scan: Option<Arc<ScanFile>>,
    /// Bumped each time records are dropped and the rest renumbered: a layout made before it is
    /// of numbers that no longer mean the same records.
    pub(super) epoch: u64,
    /// Whether a scan file is kept for this cache (a table's own caches).
    scanned: bool,
    /// The dead flags of the entries in `scan`.
    pub(super) base_dead: Bits,
}

pub(super) fn rec_len(cut: usize, full: usize) -> usize {
    16 + 4 + cut + 2 * full + 4
}

/// Read a vector file's records of `rec` bytes from entry `from` on, each
/// checked by its CRC, to `f`; the number of whole good records read. A torn
/// or corrupt record ends it.
fn stream_file(
    file: &File,
    rec: usize,
    from: usize,
    f: &mut dyn FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<usize> {
    let mut file = file.try_clone()?;
    file.seek(SeekFrom::Start(HEADER + (from * rec) as u64))?;
    let mut r = BufReader::with_capacity(1 << 20, file);
    let mut buf = vec![0u8; rec];
    let mut n = 0;
    loop {
        match r.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(n),
            Err(e) => return Err(e),
        }
        let crc = u32::from_le_bytes(buf[rec - 4..].try_into().unwrap_or_default());
        if crc32fast::hash(&buf[..rec - 4]) != crc {
            return Ok(n);
        }
        f(&buf)?;
        n += 1;
    }
}

/// What a compaction did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Compacted {
    /// Files rewritten.
    pub files: u64,
    /// Records dropped.
    pub dropped: u64,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

impl Compacted {
    pub(super) fn add(&mut self, o: Compacted) {
        self.files += o.files;
        self.dropped += o.dropped;
        self.bytes_before += o.bytes_before;
        self.bytes_after += o.bytes_after;
    }
}

/// A compaction's steps, at whose ends a test stops it as a crash would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Half the kept records copied.
    HalfCopied,
    /// The copy whole and synced, not yet renamed.
    Copied,
    /// Renamed over the file; the directory not yet synced.
    Renamed,
}

/// Make a rename in `dir` durable.
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

impl Cache {
    /// Open `<dir>/<key>.vec` for `stamp`, or start it. A torn or corrupt
    /// tail (a crash mid-append) is cut at the last whole record, and a
    /// compaction's copy that a crash left unrenamed is removed.
    pub fn open(dir: &Path, stamp: &Stamp) -> anyhow::Result<Cache> {
        Self::open_with(dir, stamp, false)
    }

    /// [`Cache::open`], for a table to search: the records' cuts, scales and
    /// hashes are read from the scan file when it is the file the records
    /// made, and made from them when it is not (`scan.rs`); only what was
    /// appended since is read into memory.
    pub fn open_scanned(dir: &Path, stamp: &Stamp) -> anyhow::Result<Cache> {
        Self::open_with(dir, stamp, true)
    }

    pub(super) fn open_with(dir: &Path, stamp: &Stamp, scanned: bool) -> anyhow::Result<Cache> {
        let key = stamp_key(stamp);
        let path = dir.join(format!("{key}.vec"));
        let meta = dir.join(format!("{key}.json"));
        if !meta.exists() {
            crate::state::save(&meta, stamp).context("writing a stamp's file")?;
        }
        for stale in [
            path.with_extension("vec.tmp"),
            path.with_extension("scan.tmp"),
            path.with_extension("tier.tmp"),
            path.with_extension("tier.cand"),
        ] {
            match fs::remove_file(&stale) {
                Ok(()) => tracing::warn!(
                    file = %stale.display(),
                    "index: a copy a crash left, removed (the file beside it is whole)"
                ),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).context("removing a copy a crash left"),
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let [cut, full] = stamp.dims;
        let rec = rec_len(cut, full);
        let mut c = Cache {
            stamp: stamp.clone(),
            key,
            dir: dir.to_path_buf(),
            file,
            cut,
            full,
            rec,
            by_hash: Entries::default(),
            hashes: Vec::new(),
            q: Vec::new(),
            scale: Vec::new(),
            dead: Vec::new(),
            dead_count: 0,
            scan: None,
            epoch: 0,
            scanned,
            base_dead: Bits::default(),
        };
        let len = c.file.metadata()?.len();
        let mut head = [0u8; HEADER as usize];
        let fresh = len < HEADER || {
            c.file.read_exact_at(&mut head, 0)?;
            &head[..8] != MAGIC
                || u32::from_le_bytes(head[8..12].try_into()?) as usize != cut
                || u32::from_le_bytes(head[12..16].try_into()?) as usize != full
        };
        if fresh {
            c.file.set_len(0)?;
            let mut h = MAGIC.to_vec();
            h.extend_from_slice(&(cut as u32).to_le_bytes());
            h.extend_from_slice(&(full as u32).to_le_bytes());
            (&c.file).write_all(&h)?;
            return Ok(c);
        }
        let records = ((len - HEADER) / rec as u64) as usize;
        if scanned {
            c.take_scan(records);
        }
        let from = c.n_base();
        let reader = c.file.try_clone()?;
        stream_file(&reader, rec, from, &mut |b| {
            c.push_record(b);
            Ok(())
        })?;
        c.shrink();
        let good = HEADER + (c.len() * rec) as u64;
        if good != len {
            tracing::warn!(
                file = %path.display(),
                cut_bytes = len - good,
                "index: a vector file's torn or corrupt tail, cut"
            );
            c.file.set_len(good)?;
        }
        if c.scanned && c.scale.len() >= FOLD {
            c.fold_scan();
        }
        Ok(c)
    }

    /// [`stream_file`] over this cache's file.
    pub(super) fn stream(
        &self,
        from: usize,
        f: &mut dyn FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<usize> {
        stream_file(&self.file.try_clone()?, self.rec, from, f)
    }

    pub(super) fn scan_path(&self) -> PathBuf {
        self.dir.join(format!("{}.scan", self.key))
    }

    pub(super) fn tier_path(&self) -> PathBuf {
        super::tier::path(&self.dir, &self.key)
    }

    /// The records were renumbered: the tier file of the old numbers goes (a layout of them is of
    /// other records now), and a layout made before this is not to be installed.
    fn drop_tier(&mut self) {
        self.epoch += 1;
        match fs::remove_file(self.tier_path()) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(error = %e, "index: the old tier file was not removed after a compaction");
            }
        }
    }

    /// Use the scan file if it is the file these records made, else make it.
    /// A cache that cannot (a disk that refuses) reads them into memory.
    pub(super) fn take_scan(&mut self, records: usize) {
        let path = self.scan_path();
        match ScanFile::open(
            &path, self.cut, self.full, &self.file, self.rec, records, HEADER,
        ) {
            Ok(s) => {
                self.base_dead = Bits::filled(s.len(), false);
                self.scan = Some(Arc::new(s));
                return;
            }
            Err(why) => tracing::info!(
                file = %path.display(),
                why,
                "index: a vector file's scan file is made again from its records"
            ),
        }
        if let Err(e) = self.build_scan(records) {
            tracing::warn!(error = %e, "index: a vector file's scan file was not made; its records stay in memory");
        }
    }

    /// Make the scan file from the records (a pass over the whole file).
    pub(super) fn build_scan(&mut self, records: usize) -> io::Result<()> {
        let mut w = ScanWriter::new(&self.scan_path(), self.cut, self.full, records)?;
        let cut = self.cut;
        let n = self.stream(0, &mut |b| {
            let hash = u128::from_le_bytes(b[..16].try_into().unwrap_or_default());
            let scale = f32::from_le_bytes(b[16..20].try_into().unwrap_or_default());
            let q: Vec<i8> = b[20..20 + cut].iter().map(|&x| x as i8).collect();
            w.push(hash, scale, &q)
        })?;
        let checks = self.checks(n)?;
        let s = w.finish(&checks)?;
        self.base_dead = Bits::filled(s.len(), false);
        self.scan = Some(Arc::new(s));
        Ok(())
    }

    /// Three of the first `n` records, to say later that a scan file of `n`
    /// is of these.
    pub(super) fn checks(&self, n: usize) -> io::Result<Vec<Check>> {
        let mut out = Vec::new();
        if n == 0 {
            return Ok(out);
        }
        for idx in [0, n / 2, n - 1] {
            if out.iter().any(|c: &Check| c.0 == idx as u64) {
                continue;
            }
            let at = HEADER + (idx * self.rec) as u64;
            let mut head = [0u8; 16];
            let mut crc = [0u8; 4];
            self.file.read_exact_at(&mut head, at)?;
            self.file
                .read_exact_at(&mut crc, at + self.rec as u64 - 4)?;
            out.push((
                idx as u64,
                u128::from_le_bytes(head),
                u32::from_le_bytes(crc),
            ));
        }
        Ok(out)
    }

    /// Let the process's copy of the scan file's pages go.
    pub(super) fn release_pages(&self) {
        if let Some(s) = &self.scan {
            s.release();
        }
    }

    /// The alive records of the scan file: what a layout is made of (it reads the scan file, not
    /// the entries embedded since the last fold, which wait in memory until [`scan::FOLD`] have
    /// gathered). The count `layout_due` and the status compare with the budget, so that it and
    /// the layout pass are one count.
    pub(super) fn scanned_alive(&self) -> usize {
        let tail_dead = self.dead.iter().filter(|d| **d).count();
        self.n_base()
            .saturating_sub(self.dead_count.saturating_sub(tail_dead))
    }

    /// As a fold the disk refused leaves the cache: no more folds until a restart.
    #[cfg(test)]
    pub(super) fn refuse_folds(&mut self) {
        self.scanned = false;
    }

    /// Entries in the scan file.
    pub(super) fn n_base(&self) -> usize {
        self.scan.as_ref().map_or(0, |s| s.len())
    }

    /// Write every entry into a new scan file, the appended ones too.
    pub(super) fn fold_scan(&mut self) {
        let r = self.rewrite_scan(&(0..self.len() as u32).collect::<Vec<_>>());
        if let Err(e) = r {
            // Not again until the tender restarts: a disk that refused a file
            // would otherwise be asked at every batch.
            self.scanned = false;
            tracing::warn!(error = %e, "index: a vector file's scan file was not written out again; the new records stay in memory");
        }
    }

    /// Make the scan file of the entries `kept`, in that order, from what is in
    /// memory and in the old scan file, and put it in place of both. The dead
    /// flags go with their entries.
    pub(super) fn rewrite_scan(&mut self, kept: &[u32]) -> io::Result<()> {
        let mut w = ScanWriter::new(&self.scan_path(), self.cut, self.full, kept.len())?;
        for &e in kept {
            w.push(self.hash_of(e), self.scale_of(e), self.q(e))?;
        }
        let checks = self.checks(kept.len())?;
        let s = w.finish(&checks)?;
        let mut dead = Bits::filled(kept.len(), false);
        for (n, &e) in kept.iter().enumerate() {
            dead.set(n, self.is_dead(e));
        }
        self.scan = Some(Arc::new(s));
        self.base_dead = dead;
        self.by_hash = Entries::default();
        self.hashes = Vec::new();
        self.q = Vec::new();
        self.scale = Vec::new();
        self.dead = Vec::new();
        Ok(())
    }

    pub(super) fn push_record(&mut self, b: &[u8]) {
        let hash = u128::from_le_bytes(b[..16].try_into().unwrap_or_default());
        let scale = f32::from_le_bytes(b[16..20].try_into().unwrap_or_default());
        let entry = self.scale.len() as u32;
        self.hashes.push(hash);
        self.by_hash.insert(entry, &self.hashes);
        self.scale.push(scale);
        self.q.extend(b[20..20 + self.cut].iter().map(|&x| x as i8));
        self.dead.push(false);
    }

    pub fn len(&self) -> usize {
        self.n_base() + self.scale.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, hash: u128) -> Option<u32> {
        let nb = self.n_base() as u32;
        match self.by_hash.get(hash, &self.hashes) {
            Some(t) => Some(nb + t),
            None => self.scan.as_ref()?.get(hash),
        }
    }

    /// Records whose text no chunk holds.
    pub fn dead(&self) -> usize {
        self.dead_count
    }

    /// The file's length: its header and its whole records.
    pub fn bytes(&self) -> u64 {
        HEADER + (self.len() * self.rec) as u64
    }

    /// Entry `e`'s text hash.
    pub(super) fn hash_of(&self, e: u32) -> u128 {
        match (e as usize).checked_sub(self.n_base()) {
            None => self.scan.as_ref().map_or(0, |s| s.hash(e)),
            Some(t) => self.hashes[t],
        }
    }

    pub(super) fn scale_of(&self, e: u32) -> f32 {
        match (e as usize).checked_sub(self.n_base()) {
            None => self.scan.as_ref().map_or(0.0, |s| s.scale(e)),
            Some(t) => self.scale[t],
        }
    }

    pub(super) fn is_dead(&self, e: u32) -> bool {
        match (e as usize).checked_sub(self.n_base()) {
            None => self.base_dead.get(e as usize),
            Some(t) => self.dead[t],
        }
    }

    pub(super) fn flag(&mut self, e: u32, dead: bool) {
        match (e as usize).checked_sub(self.n_base()) {
            None => self.base_dead.set(e as usize, dead),
            Some(t) => self.dead[t] = dead,
        }
    }

    /// Mark the record of `hash`, if this file holds one, dead or alive.
    pub(super) fn set_dead(&mut self, hash: u128, dead: bool) {
        if let Some(e) = self.get(hash) {
            if self.is_dead(e) != dead {
                self.flag(e, dead);
                if dead {
                    self.dead_count += 1;
                } else {
                    self.dead_count -= 1;
                }
            }
        }
    }

    /// Every record dead: a count of the alive follows, by `set_dead(.., false)`.
    pub(super) fn all_dead(&mut self) {
        self.base_dead = Bits::filled(self.n_base(), true);
        self.dead.iter_mut().for_each(|d| *d = true);
        self.dead_count = self.len();
    }

    pub(super) fn q(&self, e: u32) -> &[i8] {
        match (e as usize).checked_sub(self.n_base()) {
            None => self.scan.as_ref().map_or(&[], |s| s.q(e)),
            Some(t) => &self.q[t * self.cut..(t + 1) * self.cut],
        }
    }

    /// An entry's 768-d vector, read from the file.
    pub fn full(&self, e: u32) -> io::Result<Vec<f32>> {
        let mut b = vec![0u8; 2 * self.full];
        let at = HEADER + e as u64 * self.rec as u64 + 20 + self.cut as u64;
        self.file.read_exact_at(&mut b, at)?;
        let (pairs, _) = b.as_chunks::<2>();
        Ok(pairs
            .iter()
            .map(|x| half::f16::from_bits(u16::from_le_bytes(*x)).to_f32())
            .collect())
    }

    pub(super) fn record(&self, hash: u128, v: &Vector) -> Vec<u8> {
        let (q, scale) = quantize(&v.cut);
        let mut b = Vec::with_capacity(self.rec);
        b.extend_from_slice(&hash.to_le_bytes());
        b.extend_from_slice(&scale.to_le_bytes());
        b.extend(q.iter().map(|&x| x as u8));
        for &x in &v.full {
            b.extend_from_slice(&half::f16::from_f32(x).to_bits().to_le_bytes());
        }
        let crc = crc32fast::hash(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        b
    }

    /// Append vectors, in one write; their entries. A failed write leaves the
    /// file as it was.
    pub fn append(&mut self, items: &[(u128, &Vector)]) -> io::Result<Vec<u32>> {
        let mut buf = Vec::with_capacity(items.len() * self.rec);
        for (h, v) in items {
            if v.cut.len() != self.cut || v.full.len() != self.full {
                return Err(io::Error::other("a vector of another stamp's dimensions"));
            }
            buf.extend(self.record(*h, v));
        }
        if let Err(e) = (&self.file).write_all(&buf) {
            let _ = self.file.set_len(HEADER + (self.len() * self.rec) as u64);
            return Err(e);
        }
        let mut out = Vec::with_capacity(items.len());
        for r in buf.chunks_exact(self.rec) {
            self.push_record(r);
            out.push(self.len() as u32 - 1);
        }
        if self.scanned && self.scale.len() >= FOLD {
            self.fold_scan();
        }
        Ok(out)
    }

    pub(super) fn paths(&self) -> (PathBuf, PathBuf) {
        (
            self.dir.join(format!("{}.vec", self.key)),
            self.dir.join(format!("{}.json", self.key)),
        )
    }

    /// Rewrite the file with only the records whose text `keep` admits
    /// (theseus-64x), atomically: a copy beside it (mode 0600) synced, then
    /// renamed over it, then the directory synced. A crash leaves the old
    /// file or the new one, never part of either: the copy is renamed only
    /// whole, and the next open removes one a crash left. Nothing is
    /// rewritten when every record stays.
    pub fn compact(&mut self, keep: &dyn Fn(u128) -> bool) -> anyhow::Result<Compacted> {
        self.rewrite(keep, &mut |_| Ok(()))
    }

    /// [`Cache::compact`], calling `at` at each step's end: a test returns an
    /// error there to stop it as a crash would.
    pub(crate) fn rewrite(
        &mut self,
        keep: &dyn Fn(u128) -> bool,
        at: &mut dyn FnMut(Step) -> io::Result<()>,
    ) -> anyhow::Result<Compacted> {
        let kept = self.plan(keep);
        self.apply(&kept, at)
    }

    /// The entries `keep` admits, in order.
    pub(crate) fn plan(&self, keep: &dyn Fn(u128) -> bool) -> Vec<u32> {
        (0..self.len() as u32)
            .filter(|&e| keep(self.hash_of(e)))
            .collect()
    }

    /// Rewrite the file with the entries `kept` (from [`Cache::plan`]) alone.
    pub(crate) fn apply(
        &mut self,
        kept: &[u32],
        at: &mut dyn FnMut(Step) -> io::Result<()>,
    ) -> anyhow::Result<Compacted> {
        if kept.len() == self.len() {
            return Ok(Compacted::default());
        }
        let before = self.bytes();
        let (path, _) = self.paths();
        let tmp = path.with_extension("vec.tmp");
        let mut out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("creating {}", tmp.display()))?;
        {
            let mut w = io::BufWriter::with_capacity(1 << 20, &mut out);
            w.write_all(MAGIC)?;
            w.write_all(&(self.cut as u32).to_le_bytes())?;
            w.write_all(&(self.full as u32).to_le_bytes())?;
            let mut rec = vec![0u8; self.rec];
            for (n, &e) in kept.iter().enumerate() {
                if n == kept.len() / 2 {
                    w.flush()?;
                    at(Step::HalfCopied)?;
                }
                self.file
                    .read_exact_at(&mut rec, HEADER + u64::from(e) * self.rec as u64)?;
                w.write_all(&rec)?;
            }
            w.flush()?;
        }
        out.sync_all()?;
        drop(out);
        at(Step::Copied)?;
        // The records are about to renumber, so a layout of the old numbers will mean other
        // records: it goes first, so that no crash can leave it beside the renamed file (and a
        // forgotten record's cut stays in no copy of the cuts).
        self.drop_tier();
        fs::rename(&tmp, &path).with_context(|| format!("renaming over {}", path.display()))?;
        at(Step::Renamed)?;
        sync_dir(&self.dir)?;
        // The new file, and in memory or in a new scan file its records as the
        // copy wrote them.
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let dropped = (self.len() - kept.len()) as u64;
        let old = std::mem::replace(&mut self.file, file);
        if self.scanned {
            match self.rewrite_scan(kept) {
                Ok(()) => {
                    self.dead_count = (0..kept.len()).filter(|&n| self.base_dead.get(n)).count();
                    return Ok(Compacted {
                        files: 1,
                        dropped,
                        bytes_before: before,
                        bytes_after: self.bytes(),
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "index: a vector file's scan file was not made after a compaction; its records stay in memory");
                }
            }
        }
        // The records this file dropped must not stay in a scan file beside it: one that was never
        // opened here (another space's stamp compacted for a forget) or whose rewrite just failed. It
        // is made again from the records when the stamp is next opened scanned.
        match fs::remove_file(self.scan_path()) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(error = %e, "index: the old scan file was not removed after a compaction");
            }
        }
        let mut by_hash = Entries::with_capacity(kept.len());
        let mut hashes = Vec::with_capacity(kept.len());
        let mut q = Vec::with_capacity(kept.len() * self.cut);
        let mut scale = Vec::with_capacity(kept.len());
        let mut dead = Vec::with_capacity(kept.len());
        for (n, &e) in kept.iter().enumerate() {
            hashes.push(self.hash_of(e));
            by_hash.insert(n as u32, &hashes);
            q.extend_from_slice(self.q(e));
            scale.push(self.scale_of(e));
            dead.push(self.is_dead(e));
        }
        drop(old);
        self.dead_count = dead.iter().filter(|d| **d).count();
        (self.by_hash, self.hashes, self.q, self.scale, self.dead) =
            (by_hash, hashes, q, scale, dead);
        self.scan = None;
        self.base_dead = Bits::default();
        Ok(Compacted {
            files: 1,
            dropped,
            bytes_before: before,
            bytes_after: self.bytes(),
        })
    }

    pub(super) fn remove(self) {
        let (vec, json) = self.paths();
        let _ = fs::remove_file(self.scan_path());
        let _ = fs::remove_file(self.tier_path());
        let _ = fs::remove_file(vec);
        let _ = fs::remove_file(json);
    }
}
