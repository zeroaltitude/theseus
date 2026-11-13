//! What the vector side holds on the heap per chunk (theseus-agqn), measured
//! on a synthetic table: each chunk's row, its text's entry in the rows' map,
//! and its record in the vector file's memory (the int8 cut, its scale, its
//! hash, its entry in the file's index). The 768-d vectors stay on disk.

use super::Cache;

impl Cache {
    /// Give back what the read's doubling left unused: a file's memory is
    /// read whole at open, and its int8 cuts alone are 256 bytes a record.
    pub(super) fn shrink(&mut self) {
        self.hashes.shrink_to_fit();
        self.q.shrink_to_fit();
        self.scale.shrink_to_fit();
        self.dead.shrink_to_fit();
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::weights::SplitMix;

    fn stamp() -> Stamp {
        Stamp {
            model: "m".into(),
            weights: "w".into(),
            tokenizer: "t".into(),
            engine: "e".into(),
            precision: "f32".into(),
            dims: [256, 768],
        }
    }

    fn vector(rng: &mut SplitMix) -> Vector {
        let raw: Vec<f32> = (0..768).map(|_| rng.unit()).collect();
        let (full, cut) = embedder::finish(&raw, 256);
        Vector {
            full,
            cut,
            tokens: 1,
            windowed: false,
            truncated: false,
        }
    }

    /// The vector file of `nodes` nodes of three chunks each: every text
    /// has a vector but every tenth node's last chunk (it waits).
    fn write(dir: &Path, nodes: usize, seed: u64) {
        let mut rng = SplitMix(seed);
        let mut cache = Cache::open(dir, &stamp()).unwrap();
        let mut batch = Vec::new();
        for i in 0..nodes {
            for c in 0..3u32 {
                if !(i % 10 == 9 && c == 2) {
                    batch.push(((i as u128) << 8 | u128::from(c), vector(&mut rng)));
                }
            }
            if batch.len() >= 512 || i + 1 == nodes {
                let items: Vec<(u128, &Vector)> = batch.iter().map(|(h, v)| (*h, v)).collect();
                cache.append(&items).unwrap();
                batch.clear();
            }
        }
    }

    /// The table a tender's start makes of [`write`]'s file: the file read,
    /// then a row for every chunk, in 50 sessions.
    fn open(dir: &Path, nodes: usize) -> Table {
        let mut t = Table::default();
        t.caches.push(Cache::open(dir, &stamp()).unwrap());
        t.opened = true;
        for i in 0..nodes {
            t.add_node(&NodeChunks {
                node_id: format!("nd_{i:026}"),
                position: i as u64 * 7,
                session: format!("ses_{:026}", i % 50),
                kind: ["user_message", "tool_result", "assistant_message"][i % 3].into(),
                external: i % 5 == 0,
                chunks: (0..3)
                    .map(|c| ChunkKey {
                        chunk: c,
                        hash: (i as u128) << 8 | u128::from(c),
                        tokens: 100,
                    })
                    .collect(),
            });
        }
        t
    }

    fn build(dir: &Path, nodes: usize, seed: u64) -> Table {
        write(dir, nodes, seed);
        open(dir, nodes)
    }

    /// This process's peak resident set (`VmHWM`), in bytes. `clear_refs`
    /// resets it to what is resident now, as `tests_weights.rs` does.
    fn hwm() -> u64 {
        let s = fs::read_to_string("/proc/self/status").unwrap();
        s.lines()
            .find_map(|l| {
                let v = l.strip_prefix("VmHWM")?.strip_prefix(':')?;
                v.trim().trim_end_matches(" kB").trim().parse::<u64>().ok()
            })
            .unwrap_or(0)
            * 1024
    }

    /// A vector file's read takes its room once (theseus-xxac7): the peak
    /// resident set grows by about what its records hold in memory, not by
    /// twice it. **This is a guard, not the record of a fix**: the read's
    /// `Vec`s do double (nothing tells them how many records the file holds)
    /// and `shrink` then frees a block as large as the int8 cuts, and it was
    /// expected to leave that block resident, because nothing in the tender
    /// trims — `release_memory` runs only when the embedding model unloads or
    /// refuses. Measured on this base it does not: at 29,000 records the peak
    /// grew by 6,868,992 bytes for 8,295,144 bytes of room, because glibc
    /// grows and shrinks a chunk this size with `mremap`, which moves pages
    /// rather than copying them. The guard stands so a read that does start
    /// taking its room twice is caught.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn a_vector_files_read_takes_its_room_once() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), 10_000, 31);
        // SAFETY: no pointers.
        unsafe { libc::malloc_trim(0) };
        fs::write("/proc/self/clear_refs", "5").unwrap();
        let base = hwm();
        let c = Cache::open(tmp.path(), &stamp()).unwrap();
        let grew = hwm().saturating_sub(base);
        let held = c.held();
        let room = held.total();
        assert_eq!(c.len(), 29_000, "every text has a vector but a tenth");
        assert!(room > 7_000_000, "{room} bytes of room for 29,000 records");
        eprintln!("{room} bytes of room, the peak grew by {grew}\n{held}");
        assert!(
            grew < room + room / 2,
            "the read's peak grew by {grew} bytes for {room} bytes of room"
        );
    }

    /// The heap in use, glibc's count: its arenas' and its mapped chunks'.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    fn heap() -> usize {
        // SAFETY: no pointers.
        let m = unsafe { libc::mallinfo2() };
        m.uordblks + m.hblkhd
    }

    /// The heap the table took, its chunks, and what [`Table::held`] says it
    /// went on.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    fn per_chunk(nodes: usize) -> (f64, usize, usize, Held) {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), nodes, 29);
        let before = heap();
        let t = open(tmp.path(), nodes);
        let used = heap().saturating_sub(before);
        let chunks = t.rows.len();
        let held = t.held();
        drop(t);
        (used as f64 / chunks as f64, chunks, used, held)
    }

    /// The heap per chunk at 30,000 chunks, held under 600 bytes (665 before the maps were
    /// cut, theseus-agqn): the int8 cut
    /// (256 bytes) is most of it, and what is around it stays small.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn the_heap_per_chunk_stays_near_its_int8_cut() {
        let (b, chunks, _, held) = per_chunk(10_000);
        eprintln!(
            "{chunks} chunks: {b:.0} bytes of heap a chunk (a row {} bytes, a cut 256)\n{held}",
            std::mem::size_of::<Row>()
        );
        assert!(b < 600.0, "{b:.0} bytes a chunk");
    }

    /// The breakdown is not a story about the heap: its sum is within a
    /// seventh of what glibc says the same table took, it names every chunk
    /// and record (so an empty table cannot pass it), and its cache lines are
    /// exactly what the files' own [`Cache::held`] says.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn the_breakdown_accounts_for_the_heap_it_measures() {
        let (_, chunks, used, held) = per_chunk(10_000);
        eprintln!(
            "glibc {used} bytes, the breakdown {} bytes\n{held}",
            held.total()
        );
        assert_eq!((chunks, held.chunks), (30_000, 30_000));
        assert_eq!(held.records, 29_000, "every text has a vector but a tenth");
        let (sum, measured) = (held.total() as f64, used as f64);
        assert!(
            sum > measured * 6.0 / 7.0 && sum < measured * 8.0 / 7.0,
            "the breakdown says {sum:.0} bytes of the {measured:.0} glibc counted"
        );
        assert_eq!(
            held.lines()[0].0,
            "the int8 cuts",
            "the largest holder moved: {held}"
        );
        assert!(
            held.backlog > 0 && held.rest > 0,
            "a tenth of the texts wait, and the sessions are interned: {held}"
        );
    }

    /// Every file's own lines add up to the table's (theseus-xxac7): a table
    /// of two files counts both, and one of none counts no cut at all.
    #[test]
    fn a_tables_cache_lines_are_its_files_own() {
        let tmp = tempfile::tempdir().unwrap();
        let t = build(tmp.path(), 200, 11);
        let held = t.held();
        let mut cuts = 0u64;
        let mut records = 0u64;
        for c in &t.caches {
            cuts += c.held().cuts;
            records += c.held().records;
        }
        assert!(cuts > 0 && records > 0, "the fixture holds vectors");
        assert_eq!((held.cuts, held.records), (cuts, records));
        let empty = Table::default().held();
        assert_eq!((empty.cuts, empty.records, empty.chunks), (0, 0, 0));
        assert_eq!(empty.total(), 0);
    }

    /// The same at 100,000 nodes (300,000 chunks), printed:
    /// `cargo nextest run -p theseus-index --run-ignored only -E 'test(heap_at)' --no-capture`.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    #[ignore = "a measure: 300,000 chunks, about 600 MB of vector file"]
    fn bench_the_heap_at_300k_chunks() {
        let (b, chunks, _, held) = per_chunk(100_000);
        eprintln!("{chunks} chunks: {b:.0} bytes of heap a chunk\n{held}");
    }

    /// One fixture's best ten for three queries, as the table answered them
    /// before its maps were cut (theseus-agqn): the same rows, in order.
    #[test]
    fn a_query_answers_the_same_top_k_as_before() {
        let tmp = tempfile::tempdir().unwrap();
        let t = build(tmp.path(), 400, 5);
        let mut rng = SplitMix(77);
        let all = t.filter(None, &[], &Filters::default());
        let mut got = Vec::new();
        for _ in 0..3 {
            let q = vector(&mut rng);
            let top: Vec<u32> = t
                .search(&q.full, 256, &all, 10)
                .unwrap()
                .into_iter()
                .map(|(i, _)| i as u32)
                .collect();
            got.push(top);
        }
        assert_eq!(got, GOLDEN);
    }

    const GOLDEN: [[u32; 10]; 3] = [
        [844, 888, 828, 669, 718, 370, 956, 515, 185, 301],
        [43, 84, 381, 576, 909, 222, 127, 530, 662, 379],
        [219, 1088, 674, 900, 1072, 343, 431, 44, 487, 742],
    ];
}

/// The live check's aid (theseus-agqn): a stopped tender's index given a
/// random vector of the pinned model's stamp for every chunk text it holds,
/// so a synthetic store of 100,000 chunks answers vector queries without
/// hours of embedding. `THESEUS_FILL_INDEX=<state>/index cargo nextest run
/// -p theseus-index --run-ignored only -E 'test(fill_an_index)'`.
#[cfg(test)]
mod fill {
    use super::super::*;
    use crate::engine::{open_or_create, Engine};
    use crate::weights::SplitMix;

    #[test]
    #[ignore = "a tool: fills the index THESEUS_FILL_INDEX names"]
    fn fill_an_index_with_random_vectors() {
        let Ok(dir) = std::env::var("THESEUS_FILL_INDEX") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let (index, fields) = open_or_create(&dir.join("bm25")).unwrap();
        let engine = Engine::new(index, fields).unwrap();
        let mut hashes: Vec<u128> = engine
            .all_nodes()
            .unwrap()
            .iter()
            .flat_map(|n| n.chunks.iter().map(|c| c.hash))
            .collect();
        hashes.sort_unstable();
        hashes.dedup();
        let stamp = VectorConfig::new(None).spec.stamp(&embedder::engine_tag());
        let vdir = dir.join("vectors");
        fs::create_dir_all(&vdir).unwrap();
        let mut cache = Cache::open(&vdir, &stamp).unwrap();
        let mut rng = SplitMix(2026);
        let [cut, full] = stamp.dims;
        for part in hashes.chunks(1000) {
            let vs: Vec<(u128, Vector)> = part
                .iter()
                .filter(|h| cache.get(**h).is_none())
                .map(|&h| {
                    let raw: Vec<f32> = (0..full).map(|_| rng.unit()).collect();
                    let (full, cut) = embedder::finish(&raw, cut);
                    let v = Vector {
                        full,
                        cut,
                        tokens: 1,
                        windowed: false,
                        truncated: false,
                    };
                    (h, v)
                })
                .collect();
            let items: Vec<(u128, &Vector)> = vs.iter().map(|(h, v)| (*h, v)).collect();
            cache.append(&items).unwrap();
        }
        eprintln!(
            "{} texts, {} records in {}",
            hashes.len(),
            cache.len(),
            vdir.display()
        );
    }
}

#[cfg(test)]
mod pacing_tests {
    use super::super::*;

    /// The embedding thread's default wait has no bound (theseus-195j): a
    /// busy machine is never given a batch because ten seconds passed.
    #[test]
    fn the_embedder_waits_out_pressure_by_default() {
        let cfg = VectorConfig::off();
        assert_eq!(cfg.yield_bound, theseus_store::pressure::PATIENT);
        assert!(cfg.yield_bound > theseus_store::pressure::BOUND * 1000);
    }
}
