//! A turn's narrower default `fs.read` window (theseus-tdkcn): with
//! `ToolCtx::default_read_lines` set, a read with neither offset nor limit
//! returns that many lines, with the footer unchanged; a named limit always
//! wins, and a context that sets none keeps the tool's 2,000.

use serde_json::json;

use crate::{fs::Read, Tool, ToolCtx};

fn read(ctx: &ToolCtx, input: serde_json::Value) -> String {
    Read.run(&input, ctx).map_err(|e| e.message).unwrap().text
}

fn setup(n: usize) -> (tempfile::TempDir, ToolCtx) {
    let d = tempfile::tempdir().unwrap();
    let body: String = (1..=n).map(|i| format!("row {i}\n")).collect();
    std::fs::write(d.path().join("big.rs"), body).unwrap();
    let ctx = ToolCtx::for_tests(d.path());
    (d, ctx)
}

fn shown(text: &str) -> usize {
    text.lines().filter(|l| l.contains("\trow ")).count()
}

#[test]
fn a_narrowed_default_reads_four_hundred_lines_and_says_how_to_read_on() {
    let (_d, mut ctx) = setup(1000);
    ctx.default_read_lines = 400;
    let text = read(&ctx, json!({"path": "big.rs"}));
    assert_eq!(shown(&text), 400);
    assert!(
        text.contains("[showing lines 1-400 of 1000; pass offset=401 to read on]"),
        "{text}"
    );
}

#[test]
fn the_base_context_reads_the_whole_thousand() {
    let (_d, ctx) = setup(1000);
    let text = read(&ctx, json!({"path": "big.rs"}));
    assert_eq!(shown(&text), 1000);
    assert!(!text.contains("[showing lines"));
}

#[test]
fn an_explicit_limit_wins_under_either_context() {
    let (_d, mut ctx) = setup(1000);
    for narrowed in [0, 400] {
        ctx.default_read_lines = narrowed;
        let text = read(&ctx, json!({"path": "big.rs", "limit": 900}));
        assert_eq!(shown(&text), 900, "default {narrowed}");
        assert!(text.contains("[showing lines 1-900 of 1000; pass offset=901 to read on]"));
    }
}

/// An offset names a window of the model's own: the tool's default holds.
#[test]
fn an_offset_alone_keeps_the_tools_own_default() {
    let (_d, mut ctx) = setup(1000);
    ctx.default_read_lines = 400;
    let text = read(&ctx, json!({"path": "big.rs", "offset": 101}));
    assert_eq!(shown(&text), 900);
}
