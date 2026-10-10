//! `fs.read`'s own cap (theseus-v73m): a read is contiguous, at most
//! `FS_READ_MAX_CHARS` characters, and stops at a whole line with the footer
//! that names the next offset; never a head and a tail with a hole between.

use std::io::Write as _;

use serde_json::json;

use crate::fs_read_cap::FS_READ_MAX_CHARS;
use crate::{fs::Read, Tool, ToolCtx};

/// A file of `n` lines of 99 characters each, every line its number.
fn lines(d: &tempfile::TempDir, name: &str, n: usize) -> std::path::PathBuf {
    let p = d.path().join(name);
    let mut w = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
    for i in 1..=n {
        writeln!(w, "line {i:06} {}", "x".repeat(87)).unwrap();
    }
    p
}

fn read(d: &tempfile::TempDir, input: serde_json::Value) -> String {
    Read.run(&input, &ToolCtx::for_tests(d.path()))
        .map_err(|e| e.message)
        .unwrap()
        .text
}

/// The numbers of the rows a read shows, in order.
fn rows(text: &str) -> Vec<usize> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(n, _)| n.trim().parse().unwrap())
        .collect()
}

#[test]
fn a_60_kb_file_is_read_whole() {
    let d = tempfile::tempdir().unwrap();
    lines(&d, "mid.rs", 600);
    let text = read(&d, json!({"path": "mid.rs"}));
    assert_eq!(rows(&text), (1..=600).collect::<Vec<_>>());
    assert!(
        !text.contains("[showing"),
        "no footer: {}",
        &text[text.len() - 200..]
    );
    assert!(text.chars().count() > 60_000);
}

#[test]
fn a_300_kb_file_is_a_contiguous_window_with_its_footer() {
    let d = tempfile::tempdir().unwrap();
    lines(&d, "big.rs", 3_000);
    let text = read(&d, json!({"path": "big.rs"}));
    let shown = rows(&text);
    let last = *shown.last().unwrap();
    assert_eq!(shown, (1..=last).collect::<Vec<_>>(), "contiguous, from 1");
    assert!(
        text.chars().count() <= FS_READ_MAX_CHARS,
        "{}",
        text.chars().count()
    );
    assert!(last > 900, "the window fills its cap: {last}");
    assert!(
        text.ends_with(&format!(
            "\n[showing lines 1-{last} of 3000; pass offset={} to read on]\n",
            last + 1
        )),
        "{}",
        &text[text.len() - 200..]
    );
    // The footer's offset reads on, contiguous again.
    let next = read(&d, json!({"path": "big.rs", "offset": last + 1}));
    assert_eq!(rows(&next).first(), Some(&(last + 1)));
}

#[test]
fn a_window_of_a_file_over_the_size_cap_stops_at_the_cap_too() {
    let d = tempfile::tempdir().unwrap();
    let p = lines(&d, "huge.log", 3_000);
    let out = crate::fs_window::read_bounded(&p, 10, 100_000, 1 << 20, u64::MAX)
        .unwrap()
        .unwrap();
    let shown = rows(&out.text);
    let last = *shown.last().unwrap();
    assert_eq!(shown, (10..=last).collect::<Vec<_>>());
    assert!(out.text.chars().count() <= FS_READ_MAX_CHARS);
    assert!(
        out.text
            .contains(&format!("; pass offset={} to read on]", last + 1)),
        "{}",
        &out.text[out.text.len() - 200..]
    );
}
