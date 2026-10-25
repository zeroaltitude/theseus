//! SPIKE (theseus-xxac7): how much recall each stored-vector layout keeps,
//! each scored in f32, against an f32 brute-force reference, on synthetic
//! vectors (clustered, with a Matryoshka-like decay of energy across the
//! dimensions, so the 256-d cut means something). No file format changes: this
//! only measures. Synthetic vectors are not the model's, so the numbers say
//! how layouts rank on a structured set, and the local half re-runs the same
//! code on the owner's real embeddings. Drop this file and the `mod` line to
//! remove the spike.
//!
//! `cargo test -p theseus-index --release -- --ignored --nocapture spike_recall`
//! (`THESEUS_SPIKE_N`, `THESEUS_SPIKE_Q` size it; 200,000 and 200 by default).

use crate::embedder::{dot_i8, l2_normalize, quantize};
use crate::weights::SplitMix;

/// A standard normal from two uniforms (Box-Muller).
fn gauss(r: &mut SplitMix) -> f32 {
    let u1 = (f64::from(r.unit()) * 0.5 + 0.5).max(1e-9);
    let u2 = f64::from(r.unit()) * 0.5 + 0.5;
    ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
}

/// `n` unit vectors of `dim` dimensions in `clusters` clusters.
pub fn dataset(n: usize, dim: usize, clusters: usize, seed: u64) -> Vec<Vec<f32>> {
    let mut r = SplitMix(seed);
    let decay: Vec<f32> = (0..dim).map(|j| 1.0 / (1.0 + j as f32 / 96.0)).collect();
    let centers: Vec<Vec<f32>> = (0..clusters)
        .map(|_| (0..dim).map(|j| gauss(&mut r) * decay[j]).collect())
        .collect();
    (0..n)
        .map(|_| {
            let c = &centers[(r.next_u64() % clusters as u64) as usize];
            let v: Vec<f32> = (0..dim)
                .map(|j| c[j] + 0.9 * gauss(&mut r) * decay[j])
                .collect();
            l2_normalize(&v)
        })
        .collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn top(n: usize, scores: impl Iterator<Item = (usize, f32)>) -> Vec<usize> {
    crate::vectors::best(n, scores)
        .into_iter()
        .map(|(i, _)| i)
        .collect()
}

/// The fraction of `want` that `got` holds.
fn overlap(got: &[usize], want: &[usize]) -> f64 {
    want.iter().filter(|w| got.contains(w)).count() as f64 / want.len() as f64
}

fn f16(x: &[f32]) -> Vec<f32> {
    x.iter().map(|&v| half::f16::from_f32(v).to_f32()).collect()
}

/// The layouts, each as the candidates it would pick and the order it ends
/// with, for one query. Returns recall@k for each k in `ks`.
pub fn recall_table(n: usize, queries: usize, ks: &[usize]) -> Vec<(String, Vec<f64>)> {
    let dim = 768;
    let data = dataset(n, dim, 2000, 11);
    let q: Vec<Vec<f32>> = {
        let mut r = SplitMix(99);
        let qs = dataset(queries * 3, dim, 2000, 11);
        (0..queries)
            .map(|i| {
                // A data-like point nudged a little: a query near its cluster.
                let base = &qs[i * 3];
                let v: Vec<f32> = base.iter().map(|x| x + 0.02 * gauss(&mut r)).collect();
                l2_normalize(&v)
            })
            .collect()
    };
    // Stored forms.
    let cut = 256;
    let cuts: Vec<(Vec<i8>, f32)> = data
        .iter()
        .map(|v| quantize(&l2_normalize(&v[..cut])))
        .collect();
    let cut128: Vec<(Vec<i8>, f32)> = data
        .iter()
        .map(|v| quantize(&l2_normalize(&v[..128])))
        .collect();
    let q768: Vec<(Vec<i8>, f32)> = data.iter().map(|v| quantize(v)).collect();
    let h16: Vec<Vec<f32>> = data.iter().map(|v| f16(v)).collect();
    let kmax = *ks.iter().max().unwrap();
    let names = [
        "base: int8-256 scan, top 100, f16-768 re-score",
        "int8-256 scan alone (no re-score)",
        "int8-256 scan, top 30, f16-768 re-score",
        "int8-256 scan, top 300, f16-768 re-score",
        "int8-128 scan, top 100, f16-768 re-score",
        "int8-768 scan alone",
        "int8-768 scan, top 100, f16-768 re-score",
        "f16-768 scan (brute force)",
    ];
    let mut acc = vec![vec![0f64; ks.len()]; names.len()];
    for qv in &q {
        let reference = top(kmax, data.iter().enumerate().map(|(i, v)| (i, dot(qv, v))));
        let q256 = quantize(&l2_normalize(&qv[..cut]));
        let q128 = quantize(&l2_normalize(&qv[..128]));
        let q768i = quantize(qv);
        let scan256 = |m: usize| {
            top(
                m,
                cuts.iter()
                    .enumerate()
                    .map(|(i, (c, s))| (i, dot_i8(c, &q256.0) as f32 * s)),
            )
        };
        let rescore =
            |cands: Vec<usize>| top(kmax, cands.into_iter().map(|i| (i, dot(qv, &h16[i]))));
        let results = [
            rescore(scan256(100)),
            scan256(kmax),
            rescore(scan256(30)),
            rescore(scan256(300)),
            rescore(top(
                100,
                cut128
                    .iter()
                    .enumerate()
                    .map(|(i, (c, s))| (i, dot_i8(c, &q128.0) as f32 * s)),
            )),
            top(
                kmax,
                q768.iter()
                    .enumerate()
                    .map(|(i, (c, s))| (i, dot_i8(c, &q768i.0) as f32 * s)),
            ),
            rescore(top(
                100,
                q768.iter()
                    .enumerate()
                    .map(|(i, (c, s))| (i, dot_i8(c, &q768i.0) as f32 * s)),
            )),
            top(kmax, h16.iter().enumerate().map(|(i, v)| (i, dot(qv, v)))),
        ];
        for (a, got) in acc.iter_mut().zip(&results) {
            for (j, &k) in ks.iter().enumerate() {
                a[j] += overlap(&got[..k.min(got.len())], &reference[..k]);
            }
        }
    }
    names
        .iter()
        .zip(acc)
        .map(|(n, a)| {
            (
                n.to_string(),
                a.iter().map(|x| x / queries as f64).collect(),
            )
        })
        .collect()
}

#[test]
fn the_reference_ranks_itself_first() {
    let t = recall_table(2000, 10, &[6, 10]);
    let (name, r) = t.last().unwrap();
    assert!(name.contains("brute"));
    assert!(r.iter().all(|&x| x > 0.95), "{r:?}");
}

#[test]
#[ignore = "a measure: prints recall@k per layout"]
fn spike_recall_by_layout() {
    let n = std::env::var("THESEUS_SPIKE_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200_000);
    let q = std::env::var("THESEUS_SPIKE_Q")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let ks = [1, 6, 10, 20];
    println!("n={n} queries={q}; recall@k against the f32 brute-force top-k (k = {ks:?})");
    for (name, r) in recall_table(n, q, &ks) {
        println!(
            "{name:52} {}",
            r.iter()
                .map(|x| format!("{x:6.3}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
}

/// The bench's aid: a stopped tender's index given a random vector of the
/// serving model's stamp for every chunk text but the last
/// `THESEUS_SPIKE_BACKLOG` (by hash order), so a large synthetic store answers
/// vector queries at once and keeps a backlog for the embedding thread.
/// `THESEUS_INDEX_SPIKE_UNPINNED=1 THESEUS_SPIKE_INDEX=<state>/index
/// THESEUS_SPIKE_WEIGHTS=<weights dir> cargo test -p theseus-index --release
/// -- --ignored spike_fill_vectors`.
#[test]
#[ignore = "a tool: fills the index THESEUS_SPIKE_INDEX names"]
fn spike_fill_vectors() {
    use super::*;
    use crate::engine::{open_or_create, Engine};
    let (Ok(dir), Ok(w)) = (
        std::env::var("THESEUS_SPIKE_INDEX"),
        std::env::var("THESEUS_SPIKE_WEIGHTS"),
    ) else {
        return;
    };
    let backlog: usize = std::env::var("THESEUS_SPIKE_BACKLOG")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
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
    hashes.truncate(hashes.len().saturating_sub(backlog));
    let stamp = VectorConfig::new(Some(PathBuf::from(w)))
        .spec
        .stamp(&embedder::engine_tag());
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
    eprintln!("{} records ({} texts held back)", cache.len(), backlog);
}
