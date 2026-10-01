//! The embedder (M6 §2.2's "Embeddings", as the 29a spike settled them):
//! Nomic Embed Text v1.5 on candle 0.11, at f32, on the caller's thread.
//!
//! A text becomes a vector Nomic's way: its task prefix (`search_document: `
//! for chunks, `search_query: ` for queries), WordPiece, `[CLS] … [SEP]`, the
//! encoder, masked mean pooling, a layer norm without weights, then the full
//! 768-d vector L2-normalized, and the first 256 dimensions (Matryoshka's
//! cut) L2-normalized on their own. The 256-d one is scanned as int8; the
//! 768-d one re-scores, and is kept at f16.
//!
//! **Windows.** A text past 512 tokens is embedded in windows of 512, each
//! `[CLS]`, the prefix, the next stretch of the text, `[SEP]`, and its pooled
//! vector is the mean over every window's tokens. The index's chunks are cut
//! by an estimate (29b's), and on Eddie's store a quarter of them came out a
//! little past 512 word pieces: windows embed all of each, not its first 512.
//! At most [`MAX_WINDOWS`]; past that a text is cut, and says so.
//!
//! **One thread.** candle sizes gemm's parallelism from `RAYON_NUM_THREADS`
//! at every matmul, and its quantized pool from `CANDLE_NUM_THREADS`, both
//! every core when unset. The tender's binary sets both to `[index] threads`
//! (1) before anything starts; a core that spawns the tender should set them
//! in its environment too.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use sha2::{Digest, Sha256};

use crate::model::{NomicBert, NomicConfig};
use crate::proto::{Stamp, Task};
use crate::weights::{self, LoadError};
use crate::wordpiece::WordPiece;

/// The most tokens in one window, `[CLS]` and `[SEP]` included.
pub const MAX_TOKENS: usize = 512;

/// The most windows a text is embedded in (about 4,000 tokens).
pub const MAX_WINDOWS: usize = 8;

/// The engine, pinned in `Cargo.toml` (`=0.11.0`).
pub const ENGINE: &str = "candle-0.11.0";

/// Bumped when this code's vectors change for the same weights: the
/// prefixes, the windows, the pooling, the cut, the quantization.
pub const EMBED_VERSION: u32 = 2;

pub const PRECISION: &str = "f32";

/// A model the tender can load: where its files live under `weights_dir`,
/// its architecture, its cut, and the SHA-256 of its two files, pinned.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelSpec {
    /// The model's directory under `weights_dir`.
    pub name: String,
    pub revision: String,
    pub config: NomicConfig,
    /// The Matryoshka cut the scan reads.
    pub cut: usize,
    pub weights_sha256: String,
    pub tokenizer_sha256: String,
}

impl ModelSpec {
    /// nomic-embed-text-v1.5 at revision e9b6763023c6, as
    /// `tools/fetch-nomic.py` fetches and checks it.
    pub fn nomic_v1_5() -> Self {
        Self {
            name: "nomic-embed-text-v1.5".into(),
            revision: "e9b6763023c676ca8431644204f50c2b100d9aab".into(),
            config: NomicConfig::V1_5,
            cut: 256,
            weights_sha256: "9e7d262b1fe5ea350782829496efa831901b77486bbde1cea54a4c822d010d5c"
                .into(),
            tokenizer_sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66"
                .into(),
        }
    }

    pub fn dir(&self, weights_dir: &Path) -> PathBuf {
        weights_dir.join(&self.name)
    }

    pub fn weights_path(&self, weights_dir: &Path) -> PathBuf {
        self.dir(weights_dir).join("model.safetensors")
    }

    pub fn tokenizer_path(&self, weights_dir: &Path) -> PathBuf {
        self.dir(weights_dir).join("tokenizer.json")
    }

    /// What vectors of this model made by `engine` are stamped with.
    pub fn stamp(&self, engine: &str) -> Stamp {
        Stamp {
            model: format!(
                "{}@{}",
                self.name,
                &self.revision[..self.revision.len().min(12)]
            ),
            weights: self.weights_sha256.clone(),
            tokenizer: self.tokenizer_sha256.clone(),
            engine: engine.to_string(),
            precision: PRECISION.into(),
            dims: [self.cut, self.config.hidden],
        }
    }
}

/// This build's engine and embedding code: `candle-0.11.0+embed.1`.
pub fn engine_tag() -> String {
    format!("{ENGINE}+embed.{EMBED_VERSION}")
}

/// A stamp's short name, for its files: 16 hex digits of the SHA-256 of its
/// JSON.
pub fn stamp_key(s: &Stamp) -> String {
    let json = serde_json::to_vec(s).unwrap_or_default();
    weights::hex(&Sha256::digest(&json))[..16].to_string()
}

/// One text's vectors.
#[derive(Debug, Clone, PartialEq)]
pub struct Vector {
    /// The 768-d vector, unit length.
    pub full: Vec<f32>,
    /// Its first `cut` dimensions, unit length.
    pub cut: Vec<f32>,
    /// Its tokens, over all its windows, each window's prefix and
    /// `[CLS]`/`[SEP]` included.
    pub tokens: usize,
    /// Embedded in more than one window.
    pub windowed: bool,
    /// Cut at [`MAX_WINDOWS`].
    pub truncated: bool,
}

/// A text as the model reads it: one window or more, each
/// `[CLS] prefix … [SEP]`, at most [`MAX_TOKENS`] long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Windows {
    pub ids: Vec<Vec<u32>>,
    /// Cut at [`MAX_WINDOWS`].
    pub truncated: bool,
}

impl Windows {
    pub fn tokens(&self) -> usize {
        self.ids.iter().map(Vec::len).sum()
    }
}

pub struct Embedder {
    spec: ModelSpec,
    tok: WordPiece,
    model: NomicBert,
    pad: u32,
    cls: u32,
    sep: u32,
    /// Each task's prefix, as ids.
    prefixes: [Vec<u32>; 4],
}

fn task_index(t: Task) -> usize {
    match t {
        Task::SearchDocument => 0,
        Task::SearchQuery => 1,
        Task::Clustering => 2,
        Task::Classification => 3,
    }
}

impl Embedder {
    /// The model's files from `weights_dir`, each checked against its pinned
    /// SHA-256 before it is used.
    pub fn load(weights_dir: &Path, spec: &ModelSpec) -> Result<Embedder, LoadError> {
        let tok_bytes =
            weights::read_pinned(&spec.tokenizer_path(weights_dir), &spec.tokenizer_sha256)?;
        let tok = WordPiece::from_json(&tok_bytes)?;
        let tensors =
            weights::read_safetensors(&spec.weights_path(weights_dir), &spec.weights_sha256)?;
        Ok(Embedder::from_parts(spec.clone(), tok, tensors)?)
    }

    /// From a tokenizer and tensors already in hand (the tests' tiny model).
    pub fn from_parts(
        spec: ModelSpec,
        tok: WordPiece,
        tensors: HashMap<String, Tensor>,
    ) -> anyhow::Result<Embedder> {
        anyhow::ensure!(
            tok.vocab_size() <= spec.config.vocab,
            "a vocabulary of {} for a table of {}",
            tok.vocab_size(),
            spec.config.vocab
        );
        anyhow::ensure!(spec.cut <= spec.config.hidden, "a cut past the vector");
        let vb = VarBuilder::from_tensors(tensors, DType::F32, &Device::Cpu);
        let model = NomicBert::load(vb, &spec.config).context("building the model")?;
        let id = |t: &str| tok.token_id(t).with_context(|| format!("no {t}"));
        let (pad, cls, sep) = (id("[PAD]")?, id("[CLS]")?, id("[SEP]")?);
        let prefix = |t: Task| tok.ids(t.prefix(), usize::MAX).0;
        let prefixes = [
            prefix(Task::SearchDocument),
            prefix(Task::SearchQuery),
            prefix(Task::Clustering),
            prefix(Task::Classification),
        ];
        anyhow::ensure!(
            spec.config.max_pos.min(MAX_TOKENS)
                > 2 + prefixes.iter().map(Vec::len).max().unwrap_or(0),
            "a model too short for its prefixes"
        );
        Ok(Embedder {
            spec,
            tok,
            model,
            pad,
            cls,
            sep,
            prefixes,
        })
    }

    pub fn spec(&self) -> &ModelSpec {
        &self.spec
    }

    /// A text as the model reads it: `[CLS]`, its task's prefix, the text,
    /// `[SEP]`, in windows of at most [`MAX_TOKENS`], each with the prefix.
    /// The prefix ends in a space, so the text's word pieces are the same
    /// apart as after it.
    pub fn tokenize(&self, task: Task, text: &str) -> Windows {
        let prefix = &self.prefixes[task_index(task)];
        let room = MAX_TOKENS.min(self.spec.config.max_pos) - 2 - prefix.len();
        let (body, truncated) = self.tok.ids(text, room * MAX_WINDOWS);
        let window = |part: &[u32]| {
            let mut w = Vec::with_capacity(part.len() + prefix.len() + 2);
            w.push(self.cls);
            w.extend_from_slice(prefix);
            w.extend_from_slice(part);
            w.push(self.sep);
            w
        };
        let ids = if body.is_empty() {
            vec![window(&[])]
        } else {
            body.chunks(room).map(window).collect()
        };
        Windows { ids, truncated }
    }

    /// The texts' vectors: callers group texts of about one length (a batch
    /// costs its longest member's length times its size).
    pub fn embed(&self, task: Task, texts: &[&str]) -> anyhow::Result<Vec<Vector>> {
        let items: Vec<Windows> = texts.iter().map(|t| self.tokenize(task, t)).collect();
        self.embed_windows(&items)
    }

    /// The vectors of tokenized texts: one padded batch when each is one
    /// window; otherwise one window at a time (a long window gains nothing
    /// from a batch), each text's vector pooled over all its windows' tokens.
    pub fn embed_windows(&self, items: &[Windows]) -> anyhow::Result<Vec<Vector>> {
        let mut sums: Vec<(Vec<f64>, usize)> = Vec::with_capacity(items.len());
        if items.iter().all(|w| w.ids.len() == 1) {
            let rows: Vec<&[u32]> = items.iter().map(|w| w.ids[0].as_slice()).collect();
            sums = self.token_sums(&rows)?;
        } else {
            for w in items {
                let mut total = (vec![0f64; self.spec.config.hidden], 0usize);
                for win in &w.ids {
                    let (s, n) = self.token_sums(&[win.as_slice()])?.remove(0);
                    total.0.iter_mut().zip(&s).for_each(|(a, b)| *a += b);
                    total.1 += n;
                }
                sums.push(total);
            }
        }
        Ok(sums
            .iter()
            .zip(items)
            .map(|((sum, n), w)| {
                let pooled: Vec<f32> = sum
                    .iter()
                    .map(|&s| (s / (*n).max(1) as f64) as f32)
                    .collect();
                let (full, short) = finish(&pooled, self.spec.cut);
                Vector {
                    full,
                    cut: short,
                    tokens: w.tokens(),
                    windowed: w.ids.len() > 1,
                    truncated: w.truncated,
                }
            })
            .collect())
    }

    /// One padded batch through the encoder: each row's sum of its tokens'
    /// last hidden states, and how many tokens.
    fn token_sums(&self, rows: &[&[u32]]) -> anyhow::Result<Vec<(Vec<f64>, usize)>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let batch = rows.len();
        let seq = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut ids = vec![self.pad; batch * seq];
        let mut mask = vec![0u32; batch * seq];
        for (r, row) in rows.iter().enumerate() {
            ids[r * seq..r * seq + row.len()].copy_from_slice(row);
            mask[r * seq..r * seq + row.len()].fill(1);
        }
        let dev = Device::Cpu;
        let ids_t = Tensor::from_vec(ids, (batch, seq), &dev)?;
        let mask_t = Tensor::from_vec(mask.clone(), (batch, seq), &dev)?;
        let hidden = self.model.forward(&ids_t, &mask_t)?;
        let hidden: Vec<f32> = hidden.flatten_all()?.to_vec1()?;
        Ok(token_sums(
            &hidden,
            &mask,
            batch,
            seq,
            self.spec.config.hidden,
        ))
    }
}

/// Each row's sum of its unmasked token states, and their count. `hidden` is
/// row-major `(batch, seq, dim)`.
pub fn token_sums(
    hidden: &[f32],
    mask: &[u32],
    batch: usize,
    seq: usize,
    dim: usize,
) -> Vec<(Vec<f64>, usize)> {
    (0..batch)
        .map(|b| {
            let mut sum = vec![0f64; dim];
            let mut n = 0usize;
            for t in 0..seq {
                if mask[b * seq + t] == 0 {
                    continue;
                }
                n += 1;
                let row = &hidden[(b * seq + t) * dim..(b * seq + t + 1) * dim];
                for (s, &x) in sum.iter_mut().zip(row) {
                    *s += f64::from(x);
                }
            }
            (sum, n)
        })
        .collect()
}

/// `F.layer_norm(x, (dim,))`: no weight or bias, eps 1e-5 (Nomic's recipe).
pub fn layer_norm(x: &[f32]) -> Vec<f32> {
    let n = x.len() as f64;
    let mean = x.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let var = x
        .iter()
        .map(|&v| (f64::from(v) - mean).powi(2))
        .sum::<f64>()
        / n;
    let inv = 1.0 / (var + 1e-5).sqrt();
    x.iter()
        .map(|&v| ((f64::from(v) - mean) * inv) as f32)
        .collect()
}

pub fn l2_normalize(x: &[f32]) -> Vec<f32> {
    let norm = x.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>().sqrt();
    x.iter()
        .map(|&v| (f64::from(v) / norm.max(1e-12)) as f32)
        .collect()
}

/// A pooled vector's two forms: the full one and its first `cut`
/// dimensions, each layer-normed (together) and unit length (apart).
pub fn finish(pooled: &[f32], cut: usize) -> (Vec<f32>, Vec<f32>) {
    let normed = layer_norm(pooled);
    (l2_normalize(&normed), l2_normalize(&normed[..cut]))
}

pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| f64::from(x) * f64::from(y))
        .sum();
    let na: f64 = a.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>().sqrt();
    dot / (na * nb).max(1e-24)
}

/// A vector as int8 and its scale: each coordinate over the largest
/// magnitude, times 127, rounded, so `x ≈ q · scale` within `scale / 2`.
pub fn quantize(x: &[f32]) -> (Vec<i8>, f32) {
    let max = x.iter().fold(0f32, |m, v| m.max(v.abs()));
    if max == 0.0 || !max.is_finite() {
        return (vec![0; x.len()], 0.0);
    }
    let scale = max / 127.0;
    let q = x
        .iter()
        .map(|v| (v / scale).round().clamp(-127.0, 127.0) as i8)
        .collect();
    (q, scale)
}

/// The dot product of two int8 vectors, in i32 (256 products of at most
/// 127² each cannot overflow it).
pub fn dot_i8(a: &[i8], b: &[i8]) -> i32 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| i32::from(x) * i32::from(y))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vtests::tiny_embedder;

    fn norm(v: &[f32]) -> f64 {
        v.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>().sqrt()
    }

    /// The tiny model's vectors are fixed by its seed: these were taken from
    /// it once, and a change to the model's code, the pooling, or the
    /// tokenizer moves them.
    const FIXED: [[f32; 6]; 2] = [
        [
            0.238_608_17,
            -0.113_484_29,
            -0.258_982_7,
            0.101_060_26,
            -0.415_663_54,
            0.072_414_3,
        ],
        [
            0.199_146_39,
            -0.053_965_88,
            -0.305_956,
            0.104_661_12,
            -0.403_083_26,
            0.127_022_43,
        ],
    ];

    #[test]
    fn the_tiny_seeded_model_gives_fixed_vectors() {
        let emb = tiny_embedder();
        let texts = ["the cat sat on the mat", "kumquat"];
        let v = emb.embed(Task::SearchDocument, &texts).unwrap();
        let got: Vec<&[f32]> = v.iter().map(|x| &x.full[..6]).collect();
        for (x, want) in v.iter().zip(FIXED) {
            assert_eq!((x.full.len(), x.cut.len()), (32, 16));
            assert!((norm(&x.full) - 1.0).abs() < 1e-5);
            assert!((norm(&x.cut) - 1.0).abs() < 1e-5);
            for (g, w) in x.full.iter().zip(want) {
                assert!((g - w).abs() < 1e-4, "{got:?}");
            }
        }
        // The same seed, the same model, the same vectors, bit for bit.
        assert_eq!(
            tiny_embedder().embed(Task::SearchDocument, &texts).unwrap(),
            v
        );
        // Padding is exact: a text alone gives what it gave beside a longer one.
        let alone = emb.embed(Task::SearchDocument, &["kumquat"]).unwrap();
        assert!(cosine(&alone[0].full, &v[1].full) > 0.999_999);
        // The prefix is part of the text: a query's vector is not a document's.
        let q = emb.embed(Task::SearchQuery, &["kumquat"]).unwrap();
        assert!(cosine(&q[0].full, &v[1].full) < 0.999);
        let w = emb.tokenize(Task::SearchDocument, texts[0]);
        assert_eq!((w.ids.len(), v[0].tokens), (1, w.tokens()));
        assert!(!v[0].windowed && !v[0].truncated);
    }

    #[test]
    fn a_long_text_is_embedded_in_windows_and_pooled_over_all_its_tokens() {
        let emb = tiny_embedder();
        // "word" is four pieces (w ##o ##r ##d) in the tiny vocabulary: 2,400
        // pieces, in windows of 506 after `[CLS]`, the prefix's 4, `[SEP]`.
        let long = "word ".repeat(600);
        let w = emb.tokenize(Task::SearchDocument, &long);
        assert_eq!(w.ids.len(), 5);
        assert!(w.ids.iter().all(|x| x.len() <= MAX_TOKENS));
        assert_eq!(w.ids[0].len(), MAX_TOKENS);
        assert_eq!(w.tokens(), 2_400 + 5 * 6);
        let prefix = &w.ids[0][..5];
        assert!(w
            .ids
            .iter()
            .all(|x| &x[..5] == prefix && x.last() == w.ids[0].last()));
        assert!(!w.truncated);
        // Its vector is the mean over every window's tokens, as one pass
        // over all of them would pool them.
        let v = emb.embed(Task::SearchDocument, &[&long]).unwrap().remove(0);
        assert!(v.windowed && !v.truncated);
        assert_eq!(v.tokens, w.tokens());
        let (mut sum, mut n) = (vec![0f64; 32], 0usize);
        for win in &w.ids {
            let (s, k) = emb.token_sums(&[win.as_slice()]).unwrap().remove(0);
            sum.iter_mut().zip(&s).for_each(|(a, b)| *a += b);
            n += k;
        }
        let pooled: Vec<f32> = sum.iter().map(|&s| (s / n as f64) as f32).collect();
        assert_eq!(v.full, finish(&pooled, 16).0);
        // Not the first window's vector alone: the rest of the text counts.
        let first = emb
            .embed_windows(&[Windows {
                ids: vec![w.ids[0].clone()],
                truncated: false,
            }])
            .unwrap();
        assert!(cosine(&first[0].full, &v.full) < 0.999_999);
        // Past the cap of windows, the text is cut, and says so.
        let longer = "word ".repeat(1_200);
        let w = emb.tokenize(Task::SearchDocument, &longer);
        assert_eq!((w.ids.len(), w.truncated), (MAX_WINDOWS, true));
        assert_eq!(w.tokens(), MAX_WINDOWS * MAX_TOKENS);
    }

    /// The 29a spike's sentences: six queries, each with a paraphrase that
    /// shares few of its words, and eight distractors (all names invented).
    const SPIKE: [(&str, &str); 20] = [
        ("q-port", "Which port does the web interface listen on?"),
        ("q-build", "Why did the nightly build fail?"),
        (
            "q-weights",
            "Where are the embedding model's weights kept on disk?",
        ),
        ("q-tests", "How can I make the test suite finish sooner?"),
        (
            "q-review",
            "Who has to approve changes to the storage crate?",
        ),
        ("q-forgot", "The agent forgot what we decided yesterday."),
        (
            "d-port",
            "The dashboard is served locally on TCP 8080, bound to the loopback address only.",
        ),
        (
            "d-build",
            "Last night's compile broke because the linker ran out of memory halfway through.",
        ),
        (
            "d-weights",
            "The vector model's files live in the cache directory, under a folder named models.",
        ),
        (
            "d-tests",
            "Running the checks in parallel on four threads cut the wall-clock time in half.",
        ),
        (
            "d-review",
            "Mira signs off on every edit to the write-ahead log before it merges.",
        ),
        (
            "d-forgot",
            "Recall missed the earlier decision because the note used different words for it.",
        ),
        (
            "x-recipe",
            "The recipe calls for two cups of flour, a pinch of salt, and warm water.",
        ),
        (
            "x-tide",
            "At low tide the rock pools fill with anemones and small green crabs.",
        ),
        (
            "x-orchestra",
            "The orchestra rehearsed the second movement twice before lunch.",
        ),
        (
            "x-fox",
            "A red fox crossed the snowy field just after dawn.",
        ),
        (
            "x-port-wine",
            "The port wine was served after dinner, listening to the harbour bells.",
        ),
        (
            "x-build-house",
            "They will build the house on a concrete slab before the rains arrive.",
        ),
        (
            "x-weights-gym",
            "She keeps the gym weights on a rack beside the door.",
        ),
        (
            "x-test-exam",
            "The exam asked students to approve or reject the proposed amendment.",
        ),
    ];

    /// The real model, loaded the tender's way (pinned hashes, read not
    /// mapped), against the vectors the 29a spike's probe stored for the
    /// same sentences (`THESEUS_SPIKE_VECTORS`: its `agree-candle-f32-t4.json`),
    /// and the spike's paraphrase ranking. The live check runs it.
    #[test]
    #[ignore = "loads the real weights from ~/.cache/theseus/models (the live check runs it)"]
    fn the_real_model_gives_the_spike_s_vectors() {
        let home = std::env::var("HOME").unwrap();
        let weights = std::path::Path::new(&home).join(".cache/theseus/models");
        let t0 = std::time::Instant::now();
        let emb = Embedder::load(&weights, &ModelSpec::nomic_v1_5()).unwrap();
        eprintln!("loaded in {:?}", t0.elapsed());
        let vectors: Vec<Vector> = SPIKE
            .iter()
            .map(|(id, text)| {
                let task = if id.starts_with('q') {
                    Task::SearchQuery
                } else {
                    Task::SearchDocument
                };
                emb.embed(task, &[text]).unwrap().remove(0)
            })
            .collect();
        if let Ok(path) = std::env::var("THESEUS_SPIKE_VECTORS") {
            let j: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            let theirs = j["agree"]["vectors"].as_array().unwrap();
            let mut worst: f64 = 1.0;
            for ((id, _), v) in SPIKE.iter().zip(&vectors) {
                let t = theirs.iter().find(|x| x["id"] == *id).unwrap();
                let f: Vec<f32> = serde_json::from_value(t["full"].clone()).unwrap();
                let c: Vec<f32> = serde_json::from_value(t["cut"].clone()).unwrap();
                worst = worst.min(cosine(&v.full, &f)).min(cosine(&v.cut, &c));
            }
            eprintln!("worst cosine against the spike's vectors: {worst:.12}");
            assert!(worst > 0.999_999, "{worst}");
        }
        // Each query's paraphrase, ranked among the fourteen documents.
        let mut top1 = [0, 0];
        for (qi, (qid, _)) in SPIKE.iter().enumerate().take(6) {
            let answer = qid.replacen("q-", "d-", 1);
            for (k, top) in top1.iter_mut().enumerate() {
                let pick = |v: &Vector| {
                    if k == 0 {
                        v.cut.clone()
                    } else {
                        v.full.clone()
                    }
                };
                let q = pick(&vectors[qi]);
                let best = (6..20)
                    .max_by(|&a, &b| {
                        cosine(&q, &pick(&vectors[a])).total_cmp(&cosine(&q, &pick(&vectors[b])))
                    })
                    .unwrap();
                if SPIKE[best].0 == answer {
                    *top += 1;
                }
            }
        }
        eprintln!(
            "paraphrase top-1: {}/6 at 256-d, {}/6 at 768-d",
            top1[0], top1[1]
        );
        assert_eq!(
            top1,
            [6, 5],
            "the spike's ranking: 6/6 at 256-d, 5/6 at 768-d"
        );
    }

    #[test]
    fn the_matryoshka_cut_and_int8_quantization() {
        let pooled: Vec<f32> = (0..768)
            .map(|i| ((i * 37 % 101) as f32 - 50.0) / 7.0)
            .collect();
        let (full, cut) = finish(&pooled, 256);
        assert_eq!((full.len(), cut.len()), (768, 256));
        assert!((norm(&full) - 1.0).abs() < 1e-6 && (norm(&cut) - 1.0).abs() < 1e-6);
        // Layer-normed first: the full vector's mean is 0.
        assert!(full.iter().map(|&x| f64::from(x)).sum::<f64>().abs() < 1e-5);
        // The cut is the full vector's first 256 dimensions, made unit again.
        for (a, b) in l2_normalize(&full[..256]).iter().zip(&cut) {
            assert!((a - b).abs() < 1e-6);
        }
        // int8: the largest magnitude is 127, every coordinate within half a
        // step, and the dot product near the cosine.
        let (q, s) = quantize(&cut);
        assert_eq!(q.iter().map(|x| x.unsigned_abs()).max(), Some(127));
        for (x, qx) in cut.iter().zip(&q) {
            assert!((x - f32::from(*qx) * s).abs() <= s / 2.0 + 1e-7);
        }
        let other: Vec<f32> = (0..768)
            .map(|i| ((i * 53 % 97) as f32 - 48.0) / 5.0)
            .collect();
        let (_, cut2) = finish(&other, 256);
        let (q2, s2) = quantize(&cut2);
        let approx = f64::from(dot_i8(&q, &q2)) * f64::from(s) * f64::from(s2);
        assert!((approx - cosine(&cut, &cut2)).abs() < 0.01, "{approx}");
        let self_dot = f64::from(dot_i8(&q, &q)) * f64::from(s) * f64::from(s);
        assert!((self_dot - 1.0).abs() < 0.01);
        assert_eq!(quantize(&[0.0; 4]), (vec![0; 4], 0.0));
    }
}
