//! The tokenizer: BERT's WordPiece, as Hugging Face's `tokenizers` runs
//! nomic-embed-text-v1.5's `tokenizer.json`, step for step, with the same
//! Unicode tables (`unicode-normalization-alignments` and
//! `unicode_categories`, the two that crate uses). A test holds it to
//! `tokenizers` id for id; the live check does the same on the real file.
//!
//! 1. **Special tokens** (`[CLS]`, `[SEP]`, …) written in the text are
//!    matched first, leftmost and longest, and become their ids.
//! 2. **Normalize** each stretch between them: drop NUL, U+FFFD and control
//!    characters, and turn whitespace into spaces; pad CJK ideographs with
//!    spaces; decompose (NFD) and drop non-spacing marks (accents); then
//!    lower-case one character at a time.
//! 3. **Pre-tokenize**: split on whitespace, and give each punctuation mark
//!    its own word.
//! 4. **WordPiece** each word: the longest vocabulary entry from its start,
//!    then the longest `##` entry from there on; a word with no full cover,
//!    or over 100 characters, is one `[UNK]`.
//! 5. `[CLS]`, the ids, `[SEP]`.
//!
//! Hand-written, not the `tokenizers` crate: that crate costs the static
//! binary about 3 MiB (the 29a spike), for a pipeline this short.

use std::collections::HashMap;

use anyhow::{bail, ensure, Context as _};
use serde_json::Value;
use unicode_categories::UnicodeCategories;
use unicode_normalization_alignments::UnicodeNormalization;

/// The special tokens BERT's vocabulary holds, in its order.
pub const SPECIALS: [&str; 5] = ["[PAD]", "[UNK]", "[CLS]", "[SEP]", "[MASK]"];

#[derive(Debug, Clone)]
pub struct WordPiece {
    vocab: HashMap<String, u32>,
    unk: u32,
    cls: u32,
    sep: u32,
    /// Matched in the raw text before anything else.
    specials: Vec<(String, u32)>,
    /// A longer word is one `[UNK]`.
    max_chars: usize,
}

impl WordPiece {
    /// From a vocabulary, ids in order: the tests' tiny tokenizer. It must
    /// hold the special tokens.
    pub fn from_vocab(tokens: &[&str]) -> anyhow::Result<Self> {
        let vocab = tokens
            .iter()
            .enumerate()
            .map(|(i, t)| (t.to_string(), i as u32))
            .collect();
        Self::new(vocab, 100)
    }

    fn new(vocab: HashMap<String, u32>, max_chars: usize) -> anyhow::Result<Self> {
        let id = |t: &str| {
            vocab
                .get(t)
                .copied()
                .with_context(|| format!("the vocabulary has no {t}"))
        };
        let specials = SPECIALS
            .iter()
            .map(|s| Ok((s.to_string(), id(s)?)))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            unk: id("[UNK]")?,
            cls: id("[CLS]")?,
            sep: id("[SEP]")?,
            vocab,
            specials,
            max_chars,
        })
    }

    /// From a `tokenizer.json`, after checking that its pipeline is the one
    /// this code runs: BERT's normalizer and pre-tokenizer, WordPiece with
    /// `##`, `[CLS] $A [SEP]`, the five special tokens, and no truncation or
    /// padding of its own.
    pub fn from_json(bytes: &[u8]) -> anyhow::Result<Self> {
        let j: Value = serde_json::from_slice(bytes).context("tokenizer.json is not JSON")?;
        let norm = &j["normalizer"];
        ensure!(
            norm["type"] == "BertNormalizer"
                && norm["clean_text"] == true
                && norm["handle_chinese_chars"] == true
                && norm["lowercase"] == true
                && (norm["strip_accents"].is_null() || norm["strip_accents"] == true),
            "tokenizer.json's normalizer is not BERT's lower-casing one: {norm}"
        );
        ensure!(
            j["pre_tokenizer"]["type"] == "BertPreTokenizer",
            "tokenizer.json's pre-tokenizer is not BERT's"
        );
        ensure!(
            j["truncation"].is_null() && j["padding"].is_null(),
            "tokenizer.json truncates or pads by itself"
        );
        let model = &j["model"];
        ensure!(
            model["type"] == "WordPiece"
                && model["continuing_subword_prefix"] == "##"
                && model["unk_token"] == "[UNK]",
            "tokenizer.json's model is not WordPiece with ##"
        );
        let max_chars = model["max_input_chars_per_word"]
            .as_u64()
            .context("no max_input_chars_per_word")? as usize;
        let vocab: HashMap<String, u32> = model["vocab"]
            .as_object()
            .context("no vocabulary")?
            .iter()
            .map(|(k, v)| {
                let id = v.as_u64().context("an id that is not a number")?;
                Ok((k.clone(), u32::try_from(id)?))
            })
            .collect::<anyhow::Result<_>>()?;
        let wp = Self::new(vocab, max_chars)?;
        let added: Vec<(String, u32)> = j["added_tokens"]
            .as_array()
            .context("no added_tokens")?
            .iter()
            .map(|t| {
                ensure!(
                    t["special"] == true
                        && t["normalized"] == false
                        && t["lstrip"] == false
                        && t["rstrip"] == false
                        && t["single_word"] == false,
                    "an added token this tokenizer cannot match: {t}"
                );
                let content = t["content"].as_str().context("no content")?;
                let id = t["id"].as_u64().context("no id")? as u32;
                Ok((content.to_string(), id))
            })
            .collect::<anyhow::Result<_>>()?;
        let mut want = wp.specials.clone();
        let mut got = added;
        want.sort();
        got.sort();
        ensure!(
            want == got,
            "tokenizer.json's special tokens are {got:?}, not {want:?}"
        );
        let post = &j["post_processor"];
        let single: Vec<String> = post["single"]
            .as_array()
            .context("no post_processor.single")?
            .iter()
            .map(|p| {
                if let Some(s) = p.get("SpecialToken") {
                    s["id"].as_str().unwrap_or_default().to_string()
                } else if let Some(s) = p.get("Sequence") {
                    format!("${}", s["id"].as_str().unwrap_or_default())
                } else {
                    String::new()
                }
            })
            .collect();
        if post["type"] != "TemplateProcessing" || single != ["[CLS]", "$A", "[SEP]"] {
            bail!("tokenizer.json's template is not [CLS] $A [SEP]: {single:?}");
        }
        Ok(wp)
    }

    pub fn vocab_size(&self) -> usize {
        self.vocab.len()
    }

    pub fn token_id(&self, token: &str) -> Option<u32> {
        self.vocab.get(token).copied()
    }

    /// `[CLS]`, the text's tokens, `[SEP]`: at most `max` ids, cutting the
    /// text's tokens at `max - 2`. Also whether it was cut.
    pub fn encode(&self, text: &str, max: usize) -> (Vec<u32>, bool) {
        let budget = max.max(2) - 2;
        let mut ids = vec![self.cls];
        let mut rest = text;
        let mut cut = false;
        while !rest.is_empty() {
            let (before, special, after) = self.next_special(rest);
            if !before.is_empty() {
                let norm = normalize(before);
                for word in words(&norm) {
                    self.word_piece(word, &mut ids);
                    if ids.len() > budget + 1 {
                        break;
                    }
                }
            }
            if let Some(id) = special {
                ids.push(id);
            }
            if ids.len() > budget + 1 {
                cut = true;
                break;
            }
            rest = after;
        }
        if ids.len() > budget + 1 {
            cut = true;
            ids.truncate(budget + 1);
        }
        ids.push(self.sep);
        (ids, cut)
    }

    /// The text before the first special token, its id, and the text after;
    /// leftmost, and the longest of those that start there.
    fn next_special<'a>(&self, text: &'a str) -> (&'a str, Option<u32>, &'a str) {
        for (i, c) in text.char_indices() {
            let mut best: Option<&(String, u32)> = None;
            for s in &self.specials {
                if s.0.starts_with(c)
                    && text[i..].starts_with(s.0.as_str())
                    && best.is_none_or(|b| s.0.len() > b.0.len())
                {
                    best = Some(s);
                }
            }
            if let Some((s, id)) = best {
                return (&text[..i], Some(*id), &text[i + s.len()..]);
            }
        }
        (text, None, "")
    }

    fn word_piece(&self, word: &str, out: &mut Vec<u32>) {
        if word.chars().count() > self.max_chars {
            out.push(self.unk);
            return;
        }
        let first = out.len();
        let mut start = 0;
        let mut buf = String::new();
        while start < word.len() {
            let mut end = word.len();
            let mut found = None;
            while start < end {
                let sub = &word[start..end];
                let id = if start > 0 {
                    buf.clear();
                    buf.push_str("##");
                    buf.push_str(sub);
                    self.vocab.get(buf.as_str())
                } else {
                    self.vocab.get(sub)
                };
                if let Some(&id) = id {
                    found = Some(id);
                    break;
                }
                end -= sub.chars().last().map_or(1, char::len_utf8);
            }
            match found {
                Some(id) => {
                    out.push(id);
                    start = end;
                }
                None => {
                    out.truncate(first);
                    out.push(self.unk);
                    return;
                }
            }
        }
    }
}

/// BERT's `is_whitespace`: tab, newline and return count, as Unicode's
/// white space does.
fn is_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || c.is_whitespace()
}

/// BERT's `is_control`: Unicode's other categories (Cc, Cf, Co, Cn, Cs),
/// less tab, newline and return.
fn is_control(c: char) -> bool {
    !matches!(c, '\t' | '\n' | '\r') && c.is_other()
}

/// The CJK blocks `tokenizers` pads, as it writes them (its fourth from last
/// starts at U+2B920, where BERT's Python has U+2B820; this follows the
/// Rust, which is what the published tokenizer runs).
fn is_chinese_char(c: char) -> bool {
    matches!(
        c as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B920..=0x2CEAF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
}

fn is_bert_punc(c: char) -> bool {
    c.is_ascii_punctuation() || c.is_punctuation()
}

/// BERT's normalizer, in `tokenizers`' order: clean, pad CJK, strip
/// accents (NFD, less non-spacing marks), lower-case each character.
fn normalize(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\0' || c == '\u{fffd}' || is_control(c) {
            continue;
        }
        if is_whitespace(c) {
            clean.push(' ');
        } else if is_chinese_char(c) {
            clean.push(' ');
            clean.push(c);
            clean.push(' ');
        } else {
            clean.push(c);
        }
    }
    let mut out = String::with_capacity(clean.len());
    for (c, _) in clean.as_str().nfd() {
        if c.is_mark_nonspacing() {
            continue;
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// BERT's pre-tokenizer: split on whitespace, and isolate each punctuation
/// mark.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(char::is_whitespace)
        .filter(|w| !w.is_empty())
        .flat_map(|w| {
            let mut out = Vec::new();
            let mut start = 0;
            for (i, c) in w.char_indices() {
                if is_bert_punc(c) {
                    if start < i {
                        out.push(&w[start..i]);
                    }
                    out.push(&w[i..i + c.len_utf8()]);
                    start = i + c.len_utf8();
                }
            }
            if start < w.len() {
                out.push(&w[start..]);
            }
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Texts that exercise every step: punctuation, accents and lone marks,
    /// CJK (and the block `tokenizers` writes from U+2B920), other scripts,
    /// control and format characters, every kind of white space, special
    /// tokens written in the text, case, ligatures, code, paths.
    const TORTURE: &[&str] = &[
        "",
        "   ",
        "Hello, World!",
        "the cat sat on the mat.",
        "unaffable UNAFFABLE Unaffable un-affable",
        "Café naïve Ångström façade résumé",
        "e\u{301}cole \u{301} a\u{308}\u{301}",
        "中文字 and 中",
        "\u{2B820}\u{2B91F}\u{2B920} edge",
        "한국어 テスト ελληνικά русский עברית العربية",
        "emoji 🦀 crab 👩‍👩‍👧",
        "zero\u{200b}width\u{feff}bom\u{200d}joiner",
        "bell\u{7}nul\u{0}fffd\u{fffd}end\u{85}nel",
        "tabs\tand\nnew\r\nlines\u{b}vt\u{c}ff",
        "[CLS] [SEP] [MASK] [PAD] [UNK]",
        "a[SEP]b[[MASK]]c [UNK [CLS [cls]",
        "ß straße İstanbul ΣΊΣΥΦΟΣ ǅ",
        "ﬁne ligature ﬀ",
        "fn main() { println!(\"hi\"); }",
        "crates/theseus-store/src/wal.rs:42",
        "127.0.0.1:7433 x86_64-unknown-linux-musl",
        "1,000,000.5 $ % & * @ # ~ ` ^ | \\ < >",
        "«quotes» „low“ ‘single’ — dash – en … ellipsis ¿qué?",
        "\u{a0}nbsp\u{2003}em\u{3000}ideographic\u{2028}line",
        "Mixed CASE WiTh 123abc ABC123",
        "search_query: what port does the web ui use?",
    ];

    fn torture() -> Vec<String> {
        let mut t: Vec<String> = TORTURE.iter().map(|s| s.to_string()).collect();
        // Just under, at, and over the longest word.
        for n in [99, 100, 101, 250] {
            t.push(format!("a {} b", "x".repeat(n)));
        }
        t.push("word ".repeat(700));
        t
    }

    fn hugging_face(ours: &WordPiece, theirs: &tokenizers::Tokenizer, texts: &[String]) {
        for text in texts {
            let want = theirs
                .encode(text.as_str(), true)
                .unwrap()
                .get_ids()
                .to_vec();
            let (got, cut) = ours.encode(text, usize::MAX);
            assert!(!cut);
            assert_eq!(got, want, "{text:?}");
        }
    }

    #[test]
    fn the_tokenizer_matches_hugging_face_id_for_id() {
        let json = crate::vtests::tiny_tokenizer_json().to_string();
        let ours = WordPiece::from_json(json.as_bytes()).unwrap();
        let theirs: tokenizers::Tokenizer = json.parse().unwrap();
        hugging_face(&ours, &theirs, &torture());
    }

    /// The real tokenizer against `tokenizers`, over the torture texts and,
    /// with `THESEUS_INDEX_DIR` set to an index directory, every chunk it
    /// holds as the index embeds it. The live check runs it
    /// (`--run-ignored only`).
    #[test]
    #[ignore = "reads ~/.cache/theseus/models (the live check runs it)"]
    fn the_real_tokenizer_matches_hugging_face_id_for_id() {
        let spec = crate::embedder::ModelSpec::nomic_v1_5();
        let home = std::env::var("HOME").unwrap();
        let path = spec.tokenizer_path(&std::path::Path::new(&home).join(".cache/theseus/models"));
        let ours = WordPiece::from_json(&std::fs::read(&path).unwrap()).unwrap();
        let theirs = tokenizers::Tokenizer::from_file(&path).unwrap();
        let mut texts = torture();
        if let Ok(dir) = std::env::var("THESEUS_INDEX_DIR") {
            let (index, fields) =
                crate::engine::open_or_create(&std::path::Path::new(&dir).join("bm25")).unwrap();
            let engine = crate::engine::Engine::new(index, fields).unwrap();
            let chunks = engine.dump().unwrap();
            eprintln!("{} chunks from {dir}", chunks.len());
            texts.extend(
                chunks
                    .into_iter()
                    .map(|c| format!("search_document: {}", c.text)),
            );
        }
        hugging_face(&ours, &theirs, &texts);
        eprintln!("{} texts tokenized alike", texts.len());
    }

    fn tiny() -> WordPiece {
        WordPiece::from_vocab(&[
            "[PAD]", "[UNK]", "[CLS]", "[SEP]", "[MASK]", "the", "cat", "sat", "on", "mat", "un",
            "##aff", "##able", ".", ",", "!", "cafe", "a", "##b", "中",
        ])
        .unwrap()
    }

    #[test]
    fn words_split_on_space_and_isolate_punctuation() {
        let w: Vec<&str> = words("hey friend!  how are you?!?").collect();
        assert_eq!(
            w,
            ["hey", "friend", "!", "how", "are", "you", "?", "!", "?"]
        );
    }

    #[test]
    fn normalizing_strips_accents_cleans_and_pads_cjk() {
        assert_eq!(normalize("Café\tNOW\u{0}\u{200b}"), "cafe now");
        assert_eq!(normalize("a中b"), "a 中 b");
    }

    #[test]
    fn encode_is_greedy_longest_first_and_cuts_at_the_cap() {
        let t = tiny();
        // [CLS] un ##aff ##able [SEP]
        assert_eq!(t.encode("unaffable", 512), (vec![2, 10, 11, 12, 3], false));
        // A word with no full cover is one [UNK]; specials written in the text are matched.
        assert_eq!(
            t.encode("unxyz [MASK] Cat.", 512).0,
            vec![2, 1, 4, 6, 13, 3]
        );
        let (ids, cut) = t.encode("the cat sat on the mat", 5);
        assert_eq!((ids, cut), (vec![2, 5, 6, 7, 3], true));
        assert_eq!(t.encode("", 512), (vec![2, 3], false));
    }
}
