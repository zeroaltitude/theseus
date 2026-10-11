//! The planted-secret corpus through the scrubber (theseus-oyrt): every
//! shape in every encoding, alone and all in one output, comes through as
//! none of its bytes; and the clean corpus comes through unchanged, each
//! false positive counted. theseusd's `tests/planted_secrets.rs` sends the
//! same corpus through a daemon end to end.

use super::corpus::{clean, planted, secrets};
use super::Scrubber;

const SEED: u64 = 0x5EC2_E75A;

/// Each planted text that let any piece of its secret through, with the
/// piece, and the stand-ins each put in.
fn leaks(s: &Scrubber, text: &str, carries: &[String]) -> Vec<String> {
    let (out, _) = s.scrub(text);
    carries
        .iter()
        .filter(|c| out.contains(c.as_str()))
        .cloned()
        .collect()
}

#[test]
fn every_planted_secret_is_withheld_in_every_encoding() {
    let s = Scrubber::default();
    let planted = planted(SEED);
    let shapes: std::collections::BTreeSet<&str> = secrets(SEED).iter().map(|p| p.shape).collect();
    let mut failed = Vec::new();
    for p in &planted {
        assert!(
            !p.carries.is_empty(),
            "{} {}: nothing to look for",
            p.shape,
            p.encoding
        );
        let (out, n) = s.scrub(&p.text);
        let leaked = leaks(&s, &p.text, &p.carries);
        if n == 0 || !leaked.is_empty() || !out.contains("[redacted:") {
            failed.push(format!(
                "{} / {}: n={n} leaked {leaked:?}\n{out}",
                p.shape, p.encoding
            ));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} planted texts leaked:\n{}",
        failed.len(),
        planted.len(),
        failed.join("\n")
    );
    // All in one output, as a tool prints a file holding them.
    let whole: String = planted.iter().map(|p| p.text.as_str()).collect();
    let (out, _) = s.scrub(&whole);
    let leaked: Vec<&String> = planted
        .iter()
        .flat_map(|p| &p.carries)
        .filter(|c| out.contains(c.as_str()))
        .collect();
    assert!(leaked.is_empty(), "in one output: {leaked:?}");
    eprintln!(
        "corpus: {} shapes ({} secrets), {} planted texts, {} bytes, all withheld",
        shapes.len(),
        secrets(SEED).len(),
        planted.len(),
        whole.len()
    );
}

#[test]
fn the_clean_corpus_comes_through_unchanged() {
    let s = Scrubber::default();
    let mut positives = Vec::new();
    let mut bytes = 0;
    for (what, text) in clean(SEED) {
        bytes += text.len();
        let (out, n) = s.scrub(&text);
        if n > 0 || out != text {
            positives.push(format!("{what}: {n}\n{out}"));
        }
    }
    eprintln!(
        "clean corpus: {bytes} bytes, {} false positives",
        positives.len()
    );
    assert!(
        positives.is_empty(),
        "false positives:\n{}",
        positives.join("\n")
    );
}
