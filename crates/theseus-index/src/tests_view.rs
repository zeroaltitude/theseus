//! The spike's view of the weights (theseus-xxac7, `spike_view.rs`): the
//! tensors used straight from the map must give the copying load's
//! embeddings bit for bit, a misaligned tensor is copied and still right, and
//! a viewed load holds no anonymous memory for its tensors.

use std::fs;
use std::io::Write;

use crate::embedder::Embedder;
use crate::proto::Task;
use crate::spike_view;
use crate::vtests::{tiny_files, tiny_tokenizer_json};
use crate::weights::{read_safetensors, sha256_file};
use crate::wordpiece::WordPiece;

const TEXTS: [&str; 4] = [
    "the cat sat on the mat",
    "un-affable search document 42",
    "a",
    "café port query, dog! the dog and the cat and the port and the search and the document",
];

fn bits(e: &Embedder) -> Vec<Vec<u32>> {
    e.embed(Task::SearchDocument, &TEXTS)
        .unwrap()
        .iter()
        .flat_map(|v| [&v.full, &v.cut])
        .map(|x| x.iter().map(|f| f.to_bits()).collect())
        .collect()
}

#[test]
fn viewed_weights_embed_bit_identically_to_copied_ones() {
    let dir = tempfile::tempdir().unwrap();
    let spec = tiny_files(dir.path());
    let copied = Embedder::load(dir.path(), &spec).unwrap();
    let path = spec.weights_path(dir.path());
    let tensors = spike_view::read_safetensors(&path, &spec.weights_sha256).unwrap();
    let tok = WordPiece::from_json(tiny_tokenizer_json().to_string().as_bytes()).unwrap();
    let viewed = Embedder::from_parts(spec, tok, tensors).unwrap();
    assert_eq!(bits(&copied), bits(&viewed));
    // Twice, and after the pages are handed back: the same bits.
    spike_view::release_pages();
    assert_eq!(bits(&copied), bits(&viewed));
}

#[test]
fn a_view_of_another_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let spec = tiny_files(dir.path());
    let path = spec.weights_path(dir.path());
    assert!(matches!(
        spike_view::read_safetensors(&path, "00"),
        Err(crate::weights::LoadError::Refused { .. })
    ));
}

/// A header of 10 bytes puts the data at byte 18: not 4-aligned.
#[test]
fn a_misaligned_tensor_is_copied_and_reads_right() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.safetensors");
    let mut header = String::from(r#"{"a":{"dtype":"F32","shape":[3],"data_offsets":[0,12]}}"#);
    while header.len() % 8 != 2 {
        header.push(' ');
    }
    let mut f = fs::File::create(&path).unwrap();
    f.write_all(&(header.len() as u64).to_le_bytes()).unwrap();
    f.write_all(header.as_bytes()).unwrap();
    for x in [1.5f32, -2.25, 1e-30] {
        f.write_all(&x.to_le_bytes()).unwrap();
    }
    drop(f);
    assert_ne!((8 + header.len()) % 4, 0);
    let want = sha256_file(&path).unwrap();
    let t = spike_view::read_safetensors(&path, &want).unwrap();
    assert_eq!(t["a"].to_vec1::<f32>().unwrap(), [1.5, -2.25, 1e-30]);
    // The copying load agrees.
    let c = read_safetensors(&path, &want).unwrap();
    assert_eq!(c["a"].to_vec1::<f32>().unwrap(), [1.5, -2.25, 1e-30]);
}

fn anon() -> u64 {
    let s = fs::read_to_string("/proc/self/status").unwrap();
    s.lines()
        .find_map(|l| {
            l.strip_prefix("RssAnon:")?
                .trim()
                .trim_end_matches(" kB")
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

/// 32 MiB of tensors: viewed, they add no anonymous memory (the copy adds
/// them all).
#[test]
fn a_viewed_load_adds_no_anonymous_memory_for_its_tensors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.safetensors");
    let (n, elems) = (8usize, 1usize << 20);
    let mut header = String::from("{");
    for k in 0..n {
        if k > 0 {
            header.push(',');
        }
        header.push_str(&format!(
            r#""t{k}":{{"dtype":"F32","shape":[{elems}],"data_offsets":[{},{}]}}"#,
            k * elems * 4,
            (k + 1) * elems * 4
        ));
    }
    header.push('}');
    while header.len() % 8 != 0 {
        header.push(' ');
    }
    let mut f = std::io::BufWriter::new(fs::File::create(&path).unwrap());
    f.write_all(&(header.len() as u64).to_le_bytes()).unwrap();
    f.write_all(header.as_bytes()).unwrap();
    for k in 0..n {
        for j in 0..elems {
            f.write_all(&((k * 7 + j % 13) as f32).to_le_bytes())
                .unwrap();
        }
    }
    f.flush().unwrap();
    drop(f);
    let want = sha256_file(&path).unwrap();
    let before = anon();
    let t = spike_view::read_safetensors(&path, &want).unwrap();
    let grew = anon().saturating_sub(before);
    assert!(
        grew < 8 * 1024,
        "{grew} kB of anonymous memory for a view of 32 MiB"
    );
    // Read through the view, then drop it all: no free of the map's bytes.
    let s = t["t3"].sum_all().unwrap().to_scalar::<f32>().unwrap();
    assert!(s > 0.0);
    drop(t);
}

/// The bench's model (theseus-xxac7): the published model's architecture at
/// full size with seeded weights (the same compute and the same 547 MB as
/// the real one, none of its meaning), and a tokenizer whose vocabulary
/// holds the synthetic store's invented words (`theseus-sim synth-memory`'s
/// seed-1 vocabulary), so a text is about a token a word, as English nearly is.
/// `THESEUS_SPIKE_MODEL_DIR=<weights dir> cargo test -p theseus-index --release
/// -- --ignored spike_write_model`, then serve with
/// `THESEUS_INDEX_SPIKE_UNPINNED=1 --weights-dir <weights dir>`.
#[test]
#[ignore = "a tool: writes THESEUS_SPIKE_MODEL_DIR's seeded full-size model"]
fn spike_write_model() {
    let Ok(dir) = std::env::var("THESEUS_SPIKE_MODEL_DIR") else {
        return;
    };
    let dir = std::path::Path::new(&dir).join("nomic-embed-text-v1.5");
    fs::create_dir_all(&dir).unwrap();
    let mut json = tiny_tokenizer_json();
    let vocab = json["model"]["vocab"].as_object_mut().unwrap();
    let mut next = vocab.len();
    for w in synthetic_words(30_000).into_iter().take(30528 - next - 8) {
        if !vocab.contains_key(&w) {
            vocab.insert(w, serde_json::json!(next));
            next += 1;
        }
    }
    fs::write(
        dir.join("tokenizer.json"),
        serde_json::to_vec(&json).unwrap(),
    )
    .unwrap();
    let t = crate::weights::seeded(&crate::model::NomicConfig::V1_5, 2026).unwrap();
    crate::weights::write_safetensors(&t, &dir.join("model.safetensors")).unwrap();
    eprintln!(
        "{} tokens in the vocabulary, model in {}",
        next,
        dir.display()
    );
}

/// `theseus-sim`'s `memsynth::vocabulary(&mut Rng(1))`, copied (the two
/// crates share no code): SplitMix64 over syllables.
fn synthetic_words(n: usize) -> Vec<String> {
    const SYL: [&str; 24] = [
        "ba", "ko", "ri", "tu", "me", "sa", "lo", "ne", "pi", "da", "vo", "qui", "zan", "fel",
        "dor", "mir", "tek", "bra", "sol", "gu", "han", "yo", "wex", "cri",
    ];
    let mut r = crate::weights::SplitMix(1);
    (0..n)
        .map(|i| {
            let k = 2 + (r.next_u64() % 3) as usize;
            let mut w = String::new();
            for _ in 0..k {
                w.push_str(SYL[(r.next_u64() % 24) as usize]);
            }
            w.push_str(&format!("{i:x}"));
            w
        })
        .collect()
}
