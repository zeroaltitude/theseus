//! The vector side against the tender (29c's test row): a tiny model of
//! Nomic BERT's architecture, from the same code, with seeded weights, so
//! every vector is deterministic and nothing is downloaded. Its files are
//! written as the real model's are (`model.safetensors`, `tokenizer.json`),
//! and pinned by their own SHA-256, so the load path is the real one.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::client::Client;
use crate::embedder::{cosine, Embedder, ModelSpec};
use crate::model::NomicConfig;
use crate::proto::{
    method, EmbedParams, EmbedResult, IndexStatus, NeighboursParams, NeighboursResult, QueryParams,
    QueryResult, Task, WarmResult, Weights,
};
use crate::server;
use crate::tender::{Config, Shared, Tender};
use crate::tests::{result, settle, user, Rig};
use crate::vectors::VectorConfig;
use crate::weights::{seeded, sha256_file, write_safetensors};
use crate::wordpiece::{WordPiece, SPECIALS};

/// The tiny vocabulary: the special tokens, every lower-case letter and
/// digit whole and as a `##` piece (so any ASCII word has a cover),
/// punctuation, and a few whole words.
pub(crate) fn tiny_vocab() -> Vec<String> {
    let mut v: Vec<String> = SPECIALS.iter().map(|s| s.to_string()).collect();
    let alnum = ('a'..='z').chain('0'..='9');
    v.extend(alnum.clone().map(String::from));
    v.extend(alnum.map(|c| format!("##{c}")));
    v.extend(
        "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"
            .chars()
            .map(String::from),
    );
    v.extend(
        [
            "the", "search", "document", "query", "cat", "dog", "port", "mat", "un", "##aff",
            "##able", "中", "café",
        ]
        .map(String::from),
    );
    v
}

/// `tokenizer.json` in the published file's shape, over [`tiny_vocab`].
pub(crate) fn tiny_tokenizer_json() -> Value {
    let vocab: serde_json::Map<String, Value> = tiny_vocab()
        .into_iter()
        .enumerate()
        .map(|(i, t)| (t, json!(i)))
        .collect();
    let id = |t: &str| vocab[t].as_u64().unwrap();
    let added: Vec<Value> = SPECIALS
        .iter()
        .map(|s| {
            json!({"id": id(s), "content": s, "single_word": false, "lstrip": false,
                   "rstrip": false, "normalized": false, "special": true})
        })
        .collect();
    json!({
        "version": "1.0",
        "truncation": null,
        "padding": null,
        "added_tokens": added,
        "normalizer": {"type": "BertNormalizer", "clean_text": true, "handle_chinese_chars": true,
                       "strip_accents": null, "lowercase": true},
        "pre_tokenizer": {"type": "BertPreTokenizer"},
        "post_processor": {
            "type": "TemplateProcessing",
            "single": [{"SpecialToken": {"id": "[CLS]", "type_id": 0}},
                       {"Sequence": {"id": "A", "type_id": 0}},
                       {"SpecialToken": {"id": "[SEP]", "type_id": 0}}],
            "pair": [{"SpecialToken": {"id": "[CLS]", "type_id": 0}},
                     {"Sequence": {"id": "A", "type_id": 0}},
                     {"SpecialToken": {"id": "[SEP]", "type_id": 0}},
                     {"Sequence": {"id": "B", "type_id": 1}},
                     {"SpecialToken": {"id": "[SEP]", "type_id": 1}}],
            "special_tokens": {
                "[CLS]": {"id": "[CLS]", "ids": [id("[CLS]")], "tokens": ["[CLS]"]},
                "[SEP]": {"id": "[SEP]", "ids": [id("[SEP]")], "tokens": ["[SEP]"]}
            }
        },
        "decoder": {"type": "WordPiece", "prefix": "##", "cleanup": true},
        "model": {"type": "WordPiece", "unk_token": "[UNK]", "continuing_subword_prefix": "##",
                  "max_input_chars_per_word": 100, "vocab": vocab}
    })
}

/// Nomic BERT's architecture, shrunk: 2 layers, 32 wide, 4 heads.
pub(crate) fn tiny_config() -> NomicConfig {
    NomicConfig {
        hidden: 32,
        heads: 4,
        inner: 64,
        layers: 2,
        vocab: tiny_vocab().len().next_multiple_of(64),
        types: 2,
        eps: 1e-12,
        rope_base: 1000.0,
        max_pos: crate::embedder::MAX_TOKENS,
    }
}

pub(crate) const TINY_SEED: u64 = 29;

/// The tiny model, built in memory.
pub(crate) fn tiny_embedder() -> Embedder {
    let spec = ModelSpec {
        name: "tiny-nomic".into(),
        revision: "seeded-29".into(),
        config: tiny_config(),
        cut: 16,
        weights_sha256: String::new(),
        tokenizer_sha256: String::new(),
    };
    let tok = WordPiece::from_json(tiny_tokenizer_json().to_string().as_bytes()).unwrap();
    Embedder::from_parts(spec, tok, seeded(&tiny_config(), TINY_SEED).unwrap()).unwrap()
}

/// The tiny model's files under `<weights>/tiny-nomic/`, and its spec,
/// pinned to them.
pub(crate) fn tiny_files(weights: &Path) -> ModelSpec {
    let dir = weights.join("tiny-nomic");
    std::fs::create_dir_all(&dir).unwrap();
    write_safetensors(
        &seeded(&tiny_config(), TINY_SEED).unwrap(),
        &dir.join("model.safetensors"),
    )
    .unwrap();
    std::fs::write(
        dir.join("tokenizer.json"),
        serde_json::to_vec_pretty(&tiny_tokenizer_json()).unwrap(),
    )
    .unwrap();
    ModelSpec {
        name: "tiny-nomic".into(),
        revision: "seeded-29".into(),
        config: tiny_config(),
        cut: 16,
        weights_sha256: sha256_file(&dir.join("model.safetensors")).unwrap(),
        tokenizer_sha256: sha256_file(&dir.join("tokenizer.json")).unwrap(),
    }
}

/// A rig whose tender has the tiny model.
pub(crate) struct VRig {
    pub(crate) rig: Rig,
    weights: PathBuf,
    spec: ModelSpec,
}

impl VRig {
    pub(crate) fn new() -> Self {
        let rig = Rig::new();
        let weights = rig._tmp.path().join("models");
        let spec = tiny_files(&weights);
        Self { rig, weights, spec }
    }

    pub(crate) fn cfg(&self, engine: &str) -> Config {
        let mut c = self.rig.cfg(&self.rig.index);
        c.vectors = VectorConfig {
            weights_dir: Some(self.weights.clone()),
            spec: self.spec.clone(),
            idle_unload: Duration::from_secs(600),
            engine: engine.into(),
        };
        c
    }

    pub(crate) fn open(&self) -> Tender {
        Tender::open(self.cfg("test-engine-1")).unwrap()
    }
}

/// Follow the WAL to its end, embed everything pending, and have the model
/// loaded (as the embedding thread would for a query that may wait).
pub(crate) fn settle_all(t: &mut Tender) {
    settle(t);
    let s = t.shared();
    s.vectors.settle(&s.engine).unwrap();
    // A turn may compact the files first (the vector side's housekeeping),
    // and the next one loads.
    for _ in 0..4 {
        if s.warm().model != "loading" {
            break;
        }
        s.vectors.work_once(&s.engine);
    }
}

fn vector_query(text: &str, k: usize) -> QueryParams {
    let mut p = QueryParams::new(text);
    p.sources = vec!["vector".into()];
    p.k = k;
    p.wait_ms = 10_000;
    p
}

pub(crate) fn status(s: &Shared) -> crate::proto::VectorStatus {
    s.status().vectors.unwrap()
}

/// The f16 the vector file keeps, back as f32.
fn as_stored(v: &[f32]) -> Vec<f32> {
    v.iter().map(|&x| half::f16::from_f32(x).to_f32()).collect()
}

pub(crate) const TEXTS: [&str; 12] = [
    "the cat sat on the mat",
    "a dog barked at the mailman all night",
    "kumquats are small citrus fruits",
    "rust compiles to machine code ahead of time",
    "the web ui listens on port 7433",
    "sourdough bread needs a living starter",
    "the gate runs clippy and the tests",
    "glaciers carve valleys over millennia",
    "a quorum of three replicas acknowledged the write",
    "jazz improvisation follows the chord changes",
    "the kettle whistled as the water boiled",
    "tides follow the moon",
];

#[test]
fn the_vector_source_ranks_by_the_cosine_of_the_768_d_vectors() {
    let v = VRig::new();
    let nodes: Vec<_> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    let positions = v.rig.put(&nodes);
    let mut t = v.open();
    settle_all(&mut t);
    let shared = t.shared();
    let s = status(&shared);
    assert_eq!((s.chunks, s.vectors, s.pending), (12, 12, 0), "{s:?}");
    assert_eq!(s.backfill.texts, 12);
    assert_eq!(s.model, "loaded");

    let query = "a feline resting on a rug";
    let r = shared.query(&vector_query(query, 12)).unwrap();
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert_eq!(r.hits.len(), 12);
    // The order the tender must give: each text's stored vector against the
    // query's, computed here from the model itself.
    let emb = shared.vectors.model(Duration::ZERO).unwrap();
    let q = emb.embed(Task::SearchQuery, &[query]).unwrap().remove(0);
    let mut want: Vec<(f64, u64)> = TEXTS
        .iter()
        .zip(&positions)
        .map(|(text, &p)| {
            let d = emb.embed(Task::SearchDocument, &[text]).unwrap().remove(0);
            (cosine(&q.full, &as_stored(&d.full)), p)
        })
        .collect();
    want.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let got: Vec<u64> = r.hits.iter().map(|h| h.position).collect();
    assert_eq!(got, want.iter().map(|w| w.1).collect::<Vec<_>>());
    let weight = r.weights["vector"];
    for (i, (h, w)) in r.hits.iter().zip(&want).enumerate() {
        let src = &h.sources["vector"];
        assert_eq!(src.rank, i + 1);
        assert!(
            (src.score - w.0).abs() < 1e-9,
            "{} against {}",
            src.score,
            w.0
        );
        assert!((h.fused - weight / (60.0 + (i + 1) as f64)).abs() < 1e-12);
        assert_eq!(h.sources.len(), 1);
    }
    assert!(r.timings.embed_ms > 0.0);
}

#[test]
fn as_of_holds_for_vectors() {
    let v = VRig::new();
    let nodes = [
        user("ses_1", "kumquat one"),
        user("ses_1", "kumquat two"),
        user("ses_1", "kumquat three"),
    ];
    let positions = v.rig.put(&nodes);
    let mut t = v.open();
    settle_all(&mut t);
    let shared = t.shared();
    let at = |as_of: Option<u64>| {
        let mut p = vector_query("citrus", 10);
        p.as_of = as_of;
        let mut got: Vec<u64> = shared
            .query(&p)
            .unwrap()
            .hits
            .iter()
            .map(|h| h.position)
            .collect();
        got.sort_unstable();
        got
    };
    assert_eq!(at(None), positions);
    assert_eq!(at(Some(positions[2])), positions[..2].to_vec());
    assert_eq!(at(Some(positions[1])), positions[..1].to_vec());
    assert_eq!(at(Some(positions[0])), Vec::<u64>::new());
    // Neighbours too: only earlier nodes, and never the node itself.
    let n = |as_of: Option<u64>| {
        shared
            .neighbours(&NeighboursParams {
                node_id: nodes[2].id.clone(),
                k: 10,
                as_of,
            })
            .unwrap()
            .neighbours
            .iter()
            .map(|x| x.position)
            .collect::<Vec<_>>()
    };
    let mut all = n(None);
    all.sort_unstable();
    assert_eq!(all, positions[..2].to_vec());
    assert_eq!(n(Some(positions[1])), positions[..1].to_vec());
}

#[test]
fn a_hybrid_query_fuses_bm25_entities_and_vectors() {
    let v = VRig::new();
    v.rig.put(&[
        user("ses_1", "zebra stripes in crates/theseus-store/src/wal.rs"),
        user("ses_2", "nothing in common with the question"),
        result("ses_3", "fs.read", "more zebra facts", false),
    ]);
    let mut t = v.open();
    settle_all(&mut t);
    let shared = t.shared();
    assert_eq!(shared.status().mode, "hybrid");
    // The default sources, in hybrid mode: all three.
    let r = shared
        .query(&QueryParams::new("zebra crates/theseus-store/src/wal.rs"))
        .unwrap();
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert_eq!(
        r.hits.len(),
        3,
        "every node, the vector source ranks them all"
    );
    assert_eq!(
        r.weights.keys().collect::<Vec<_>>(),
        ["bm25", "entity", "vector"]
    );
    for h in &r.hits {
        let want: f64 = h
            .sources
            .iter()
            .map(|(name, s)| r.weights[name] / (60.0 + s.rank as f64))
            .sum();
        assert!((h.fused - want).abs() < 1e-12);
        assert!(h.sources.contains_key("vector"));
    }
    // The node that shares no word with the question is found by its vector
    // alone, and ranks last.
    let last = r.hits.last().unwrap();
    assert_eq!(last.session_id, "ses_2");
    assert_eq!(last.sources.keys().collect::<Vec<_>>(), ["vector"]);
    // The node every source ranks first is the fused first.
    let top = &r.hits[0];
    assert_eq!(top.session_id, "ses_1");
    assert!(top.sources.contains_key("bm25") && top.sources.contains_key("entity"));
    // BM25 alone never sees it.
    let mut p = QueryParams::new("zebra");
    p.sources = vec!["bm25".into()];
    assert_eq!(shared.query(&p).unwrap().hits.len(), 2);
}

#[test]
fn vectors_survive_a_restart_and_a_rebuild_without_embedding_again() {
    let v = VRig::new();
    v.rig.put(&TEXTS.map(|t| user("ses_1", t)));
    let mut t = v.open();
    settle_all(&mut t);
    let before = t.shared().query(&vector_query("tides", 5)).unwrap();
    drop(t);

    // A restart: the rows come back from the index, the vectors from the file.
    let mut t = v.open();
    settle_all(&mut t);
    let s = status(&t.shared());
    assert_eq!(
        (s.vectors, s.pending, s.backfill.texts),
        (12, 0, 0),
        "{s:?}"
    );
    let after = t.shared().query(&vector_query("tides", 5)).unwrap();
    let ids = |r: &QueryResult| r.hits.iter().map(|h| h.node_id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&after), ids(&before));

    // `index.rebuild`: the index is dropped and refilled; the texts are the
    // same, so their vectors are found again, not embedded again.
    let shared = t.shared();
    shared.request_rebuild();
    let h = std::thread::spawn(move || t.run());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let s = shared.status();
        if s.rebuilds == 1 && s.state == "ready" && s.position > 0 && s.nodes == 12 {
            break;
        }
        assert!(Instant::now() < deadline, "rebuild never finished: {s:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    shared.vectors.settle(&shared.engine).unwrap();
    let s = status(&shared);
    assert_eq!(
        (s.vectors, s.pending, s.backfill.texts),
        (12, 0, 0),
        "{s:?}"
    );
    assert_eq!(
        ids(&shared.query(&vector_query("tides", 5)).unwrap()),
        ids(&before)
    );
    shared.request_stop();
    h.join().unwrap().unwrap();
}

#[test]
fn a_stamp_change_re_embeds_while_the_old_vectors_still_answer() {
    let v = VRig::new();
    v.rig.put(&TEXTS.map(|t| user("ses_1", t)));
    let mut t = v.open();
    settle_all(&mut t);
    let old_stamp = status(&t.shared()).stamp.unwrap();
    drop(t);

    // The same weights under another engine: the same space, a new stamp.
    let mut t = Tender::open(v.cfg("test-engine-2")).unwrap();
    settle(&mut t);
    let shared = t.shared();
    shared.vectors.open_files().unwrap();
    shared.vectors.reconcile(&shared.engine).unwrap();
    let s = status(&shared);
    assert_ne!(s.stamp.as_ref(), Some(&old_stamp));
    assert_eq!((s.chunks, s.vectors, s.pending), (12, 12, 12), "{s:?}");
    let re = s.reembed.unwrap();
    assert_eq!((re.from, re.done, re.total), (vec![old_stamp], 0, 12));
    // The model loaded, nothing re-embedded yet, and the old vectors answer
    // a query the new engine embeds.
    assert_eq!(shared.warm().model, "loading");
    shared.vectors.work_once(&shared.engine);
    assert_eq!(status(&shared).model, "loaded");
    assert_eq!(status(&shared).backfill.texts, 0);
    assert_eq!(
        shared.query(&vector_query("tides", 12)).unwrap().hits.len(),
        12
    );

    // Part way: one batch (all 12 are short, so up to 8 go together).
    let mut turns = 0;
    while status(&shared).backfill.texts == 0 {
        shared.vectors.work_once(&shared.engine);
        turns += 1;
        assert!(turns < 10);
    }
    let s = status(&shared);
    assert!(s.pending > 0 && s.pending < 12, "{s:?}");
    assert_eq!(s.vectors, 12);
    assert_eq!(s.reembed.as_ref().unwrap().done, 12 - s.pending);
    assert_eq!(
        shared.query(&vector_query("tides", 12)).unwrap().hits.len(),
        12
    );

    // Done: the old stamp's file is gone, and every vector is the new one's.
    shared.vectors.settle(&shared.engine).unwrap();
    let s = status(&shared);
    assert_eq!(
        (s.vectors, s.pending, s.backfill.texts),
        (12, 0, 12),
        "{s:?}"
    );
    assert!(s.reembed.is_none());
    let files: Vec<String> = std::fs::read_dir(v.rig.index.join("vectors"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len(), 2, "{files:?}");
    assert_eq!(
        shared.query(&vector_query("tides", 12)).unwrap().hits.len(),
        12
    );
}

#[test]
fn a_wrong_weights_file_refuses_to_load_and_the_tender_stays_bm25_only() {
    let v = VRig::new();
    v.rig.put(&[user("ses_1", "zebra crossing")]);
    // One byte of one tensor changed: the file still reads as a checkpoint.
    let path = v.weights.join("tiny-nomic").join("model.safetensors");
    let mut bytes = std::fs::read(&path).unwrap();
    let n = bytes.len();
    bytes[n - 7] ^= 0x40;
    std::fs::write(&path, &bytes).unwrap();
    let mut t = v.open();
    settle_all(&mut t);
    let shared = t.shared();
    let s = shared.status();
    assert_eq!(s.mode, "bm25_only");
    let vs = s.vectors.unwrap();
    assert_eq!(vs.model, "refused");
    assert!(
        vs.last_error.as_deref().unwrap().contains("SHA-256"),
        "{vs:?}"
    );
    assert_eq!(vs.vectors, 0);
    // BM25 still answers; vectors, asked for, say why not.
    let r = shared.query(&QueryParams::new("zebra")).unwrap();
    assert_eq!(r.hits.len(), 1);
    assert!(!r.hits[0].sources.contains_key("vector"));
    let r = shared.query(&vector_query("zebra", 5)).unwrap();
    assert!(r.hits.is_empty());
    assert!(
        r.skipped["vector"].starts_with("bm25_only"),
        "{:?}",
        r.skipped
    );
    drop(t);

    // A tokenizer that is not the pinned one is refused too.
    let v = VRig::new();
    let tok = v.weights.join("tiny-nomic").join("tokenizer.json");
    let mut j: Value = serde_json::from_slice(&std::fs::read(&tok).unwrap()).unwrap();
    j["model"]["vocab"]["zebra"] = json!(999);
    std::fs::write(&tok, j.to_string()).unwrap();
    v.rig.put(&[user("ses_1", "zebra crossing")]);
    let mut t = v.open();
    settle_all(&mut t);
    assert_eq!(t.shared().status().mode, "bm25_only");
    drop(t);

    // No weights at all: bm25_only from the start, nothing tried.
    let v = VRig::new();
    std::fs::remove_dir_all(v.weights.join("tiny-nomic")).unwrap();
    let t = v.open();
    let s = t.shared().status();
    assert_eq!(s.mode, "bm25_only");
    assert_eq!(s.vectors.unwrap().model, "no_weights");
}

#[test]
fn the_model_unloads_after_idle_and_warm_loads_it_again() {
    let v = VRig::new();
    v.rig.put(&[user("ses_1", "tides follow the moon")]);
    let mut cfg = v.cfg("test-engine-1");
    cfg.vectors.idle_unload = Duration::from_millis(50);
    let mut t = Tender::open(cfg).unwrap();
    settle_all(&mut t);
    let shared = t.shared();
    assert_eq!(status(&shared).model, "loaded");
    std::thread::sleep(Duration::from_millis(60));
    // The embedding thread's next turn unloads it.
    shared.vectors.work_once(&shared.engine);
    let s = status(&shared);
    assert_eq!((s.model.as_str(), s.unloads, s.loads), ("unloaded", 1, 1));
    // A query that may not wait answers without vectors, and starts a load.
    let mut p = vector_query("moon", 5);
    p.wait_ms = 0;
    let r = shared.query(&p).unwrap();
    assert_eq!(r.skipped["vector"], "the model is loading");
    assert_eq!(shared.warm().model, "loading");
    shared.vectors.work_once(&shared.engine);
    let s = status(&shared);
    assert_eq!((s.model.as_str(), s.loads), ("loaded", 2));
    assert_eq!(shared.warm().model, "loaded");
    assert_eq!(shared.query(&p).unwrap().hits.len(), 1);
}

#[test]
fn a_torn_vector_file_is_cut_and_its_last_texts_embedded_again() {
    let v = VRig::new();
    v.rig.put(&TEXTS.map(|t| user("ses_1", t)));
    let mut t = v.open();
    settle_all(&mut t);
    drop(t);
    let file = std::fs::read_dir(v.rig.index.join("vectors"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "vec"))
        .unwrap();
    let len = std::fs::metadata(&file).unwrap().len();
    // Half of the last record gone, as a crash mid-append leaves it.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_len(len - 50)
        .unwrap();
    let mut t = v.open();
    settle_all(&mut t);
    let s = status(&t.shared());
    assert_eq!(
        (s.vectors, s.pending, s.backfill.texts),
        (12, 0, 1),
        "{s:?}"
    );
    assert_eq!(std::fs::metadata(&file).unwrap().len(), len);
}

#[test]
fn the_socket_answers_neighbours_embed_and_warm() {
    let v = VRig::new();
    v.rig.put(&TEXTS.map(|t| user("ses_1", t)));
    let t = v.open();
    let shared = t.shared();
    let sock = t.paths().socket();
    drop(server::spawn(&sock, shared.clone()).unwrap());
    let ingest = std::thread::spawn(move || t.run());
    let s2 = shared.clone();
    let embedding = std::thread::spawn(move || s2.vectors.run(&s2.engine));
    let mut c = Client::connect(&sock, Duration::from_secs(10)).unwrap();
    let w: WarmResult = c.call(method::WARM, ()).unwrap();
    assert_eq!(w.mode, "hybrid");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let s: IndexStatus = c.call(method::STATUS, ()).unwrap();
        let vs = s.vectors.clone().unwrap();
        if s.state == "ready" && vs.vectors == 12 && vs.pending == 0 {
            assert_eq!(vs.model, "loaded");
            assert!(vs.backfill.batches >= 2 && vs.backfill.tokens > 0, "{vs:?}");
            break;
        }
        assert!(Instant::now() < deadline, "never embedded: {s:?}");
        std::thread::sleep(Duration::from_millis(10));
    }

    let r: QueryResult = c.call(method::QUERY, vector_query("tides", 3)).unwrap();
    let node = r.hits[0].node_id.clone();
    let n: NeighboursResult = c
        .call(
            method::NEIGHBOURS,
            NeighboursParams {
                node_id: node.clone(),
                k: 5,
                as_of: None,
            },
        )
        .unwrap();
    assert_eq!(n.neighbours.len(), 5);
    assert!(n.neighbours.iter().all(|x| x.node_id != node));
    assert!(n.neighbours.windows(2).all(|w| w[0].score >= w[1].score));
    assert!(c
        .call::<NeighboursResult>(
            method::NEIGHBOURS,
            NeighboursParams {
                node_id: "nd_none".into(),
                k: 5,
                as_of: None
            }
        )
        .is_err());

    let e: EmbedResult = c
        .call(
            method::EMBED,
            EmbedParams {
                texts: vec!["tides follow the moon".into(), "a cat".into()],
                task: Task::SearchDocument,
                dims: None,
                wait_ms: 5_000,
            },
        )
        .unwrap();
    assert_eq!((e.vectors.len(), e.dims), (2, 32));
    for x in &e.vectors {
        let norm: f32 = x.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }
    // The index's own vector of that text is the one `index.embed` gives.
    let stored = shared
        .vectors
        .model(Duration::ZERO)
        .unwrap()
        .embed(Task::SearchDocument, &["tides follow the moon"])
        .unwrap();
    assert!(cosine(&e.vectors[0], &stored[0].full) > 0.999_999);
    let cut: EmbedResult = c
        .call(
            method::EMBED,
            EmbedParams {
                texts: vec!["a cat".into()],
                task: Task::Clustering,
                dims: Some(16),
                wait_ms: 5_000,
            },
        )
        .unwrap();
    assert_eq!(cut.vectors[0].len(), 16);
    let too_many = EmbedParams {
        texts: vec!["x".into(); 65],
        task: Task::SearchDocument,
        dims: None,
        wait_ms: 0,
    };
    assert!(c.call::<EmbedResult>(method::EMBED, too_many).is_err());

    shared.request_stop();
    ingest.join().unwrap().unwrap();
    embedding.join().unwrap();
}

/// theseus-jz8: a query's weights override the tender's defaults source by
/// source; weights of 1 give 29c's fusion exactly (`Σ 1 / (60 + rank)`, to
/// the bit, and its order); a weight that is negative or names no source is
/// refused; status says the defaults; and the socket carries them.
#[test]
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn fusion_weights_are_the_querys_over_the_tenders_and_ones_are_the_old_fusion() {
    let v = VRig::new();
    v.rig.put(&TEXTS.map(|t| user("ses_1", t)));
    v.rig.put(&[
        user("ses_2", "the cat and crates/theseus-store/src/wal.rs"),
        result(
            "ses_2",
            "fs.read",
            "port 7433 in crates/theseus-store/src/wal.rs",
            false,
        ),
    ]);
    let mut cfg = v.cfg("test-engine-1");
    let defaults = Weights {
        bm25: 1.0,
        entity: 1.0,
        vector: 3.0,
    };
    cfg.weights = defaults;
    let mut t = Tender::open(cfg).unwrap();
    settle_all(&mut t);
    let shared = t.shared();
    assert_eq!(shared.status().weights, Some(defaults));
    let ask = |w: &[(&str, f64)]| {
        let mut p = QueryParams::new("the cat on port 7433 crates/theseus-store/src/wal.rs");
        p.k = 14;
        p.wait_ms = 10_000;
        p.weights = w.iter().map(|(s, x)| (s.to_string(), *x)).collect();
        shared.query(&p)
    };
    let fused_with = |r: &QueryResult, w: &Weights| -> Vec<f64> {
        r.hits
            .iter()
            .map(|h| {
                h.sources
                    .iter()
                    .map(|(name, s)| w.get(name) / (60.0 + s.rank as f64))
                    .sum()
            })
            .collect()
    };

    // Ones: each hit's score is Σ 1 / (60 + rank) to the bit, best first.
    let ones = ask(&[("bm25", 1.0), ("entity", 1.0), ("vector", 1.0)]).unwrap();
    assert_eq!(ones.hits.len(), 14);
    assert!(ones.hits.iter().any(|h| h.sources.len() == 3));
    let want = fused_with(&ones, &Weights::EQUAL);
    for (h, w) in ones.hits.iter().zip(&want) {
        assert_eq!(h.fused.to_bits(), w.to_bits(), "{h:?}");
    }
    assert!(ones.hits.windows(2).all(|p| p[0].fused >= p[1].fused));
    assert_eq!(
        ones.weights,
        Weights::EQUAL.of(&["bm25", "entity", "vector"])
    );

    // No weights: the tender's (vector 3); one named: it alone changes.
    let dflt = ask(&[]).unwrap();
    assert_eq!(dflt.weights, defaults.of(&["bm25", "entity", "vector"]));
    for (h, w) in dflt.hits.iter().zip(fused_with(&dflt, &defaults)) {
        assert!((h.fused - w).abs() < 1e-15);
    }
    let half = ask(&[("vector", 0.5)]).unwrap();
    assert_eq!(half.weights["vector"], 0.5);
    assert_eq!(half.weights["bm25"], 1.0);
    // The same hits, ranked in each source alike, but fused differently.
    let ranks = |r: &QueryResult| {
        let mut v: Vec<_> = r
            .hits
            .iter()
            .map(|h| (h.node_id.clone(), h.chunk, h.sources.clone()))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        v
    };
    assert_eq!(ranks(&ones), ranks(&dflt));
    let one: std::collections::BTreeMap<_, f64> = ones
        .hits
        .iter()
        .map(|h| ((h.node_id.clone(), h.chunk), h.fused))
        .collect();
    for h in &dflt.hits {
        let before = one[&(h.node_id.clone(), h.chunk)];
        if h.sources.contains_key("vector") {
            assert!(h.fused > before, "{h:?}");
        } else {
            assert_eq!(h.fused, before);
        }
    }

    // Refused: a negative weight, a source that is not one.
    for bad in [&[("vector", -1.0)][..], &[("recency", 1.0)][..]] {
        let e = format!("{:#}", ask(bad).unwrap_err());
        assert!(e.contains("weight") || e.contains("weigh"), "{e}");
    }
    assert!(Weights::EQUAL
        .with(&[("vector".to_string(), f64::NAN)].into())
        .is_err());
    assert_eq!(
        Weights::EQUAL.parse_over("vector=2, bm25=0.5").unwrap(),
        Weights {
            bm25: 0.5,
            entity: 1.0,
            vector: 2.0
        }
    );
    assert!(Weights::EQUAL.parse_over("vector").is_err());
    // The defaults the exam's held-in grid chose (the lane's report), held
    // here so they never change unnoticed.
    assert_eq!(
        Weights::default(),
        Weights {
            bm25: 1.0,
            entity: 1.0,
            vector: 6.0
        }
    );

    // Through the socket, as JSON.
    let sock = t.paths().socket();
    drop(server::spawn(&sock, shared.clone()).unwrap());
    let mut c = Client::connect(&sock, Duration::from_secs(10)).unwrap();
    let r: QueryResult = c
        .call(
            method::QUERY,
            json!({"text": "tides", "k": 3, "wait_ms": 5000, "weights": {"vector": 2.0}}),
        )
        .unwrap();
    assert_eq!(r.weights["vector"], 2.0);
    let e = c
        .call::<QueryResult>(
            method::QUERY,
            json!({"text": "tides", "weights": {"vector": -2.0}}),
        )
        .unwrap_err();
    assert!(format!("{e:#}").contains("finite number"), "{e:#}");
}
