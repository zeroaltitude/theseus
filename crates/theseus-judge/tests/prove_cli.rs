//! `theseus-judge prove` end to end: the binary on the fixtures.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_theseus-judge");
const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/prove");

fn run(args: &[&str]) -> (bool, String, String) {
    let o = Command::new(BIN).args(args).output().unwrap();
    (
        o.status.success(),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

#[test]
fn markdown_goes_to_stdout_by_default_and_a_small_file_is_insufficient() {
    let (ok, out, _) = run(&["prove", &format!("{DIR}/small.jsonl")]);
    assert!(ok);
    assert!(out.contains("## Verdict: insufficient"), "{out}");
    assert!(out.contains("labeled tasks: 4 of 30"), "{out}");
}

#[test]
fn json_and_markdown_files_are_written_and_the_minimum_is_a_flag() {
    let dir = std::env::temp_dir().join(format!("theseus-judge-prove-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (json, md) = (dir.join("r.json"), dir.join("r.md"));
    let input = format!("{DIR}/canary_wins.jsonl");
    let (ok, out, err) = run(&[
        "prove",
        &input,
        "--json",
        json.to_str().unwrap(),
        "--markdown",
        md.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    assert!(out.is_empty(), "no stdout when both outputs are files");
    let j: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(j["verdict"]["kind"], "canary_better");
    assert_eq!(j["canary"]["completions_per_usd"]["value"], 1.5);
    assert!(std::fs::read_to_string(&md)
        .unwrap()
        .contains("## Verdict: canary_better"));
    // Raising the minimum past the cohorts turns the same data insufficient.
    let (ok, out, _) = run(&["prove", &input, "--min-tasks", "41"]);
    assert!(ok && out.contains("## Verdict: insufficient"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_input_and_flags_exit_one_with_the_reason() {
    for args in [
        &["prove"][..],
        &["prove", "/nonexistent.jsonl"],
        &["frob"],
        &["prove", "x", "--bogus"],
    ] {
        let (ok, out, err) = run(args);
        assert!(!ok && out.is_empty(), "{args:?}");
        assert!(err.starts_with("theseus-judge: "), "{err}");
    }
}
