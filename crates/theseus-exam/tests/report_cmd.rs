//! `theseus-exam report` on records some other model answered: it exits
//! non-zero, says which, and writes nothing.

use std::process::Command;

use theseus_exam::drive::Record;

fn record(models: &[&str]) -> Record {
    Record {
        item: "fact-1".into(),
        family: "fact".into(),
        arm: "none".into(),
        run: 1,
        pass: Some(true),
        profile: Some("profile-x".into()),
        profile_model: Some("model-a".into()),
        models: models.iter().map(|m| (*m).to_string()).collect(),
        ..Default::default()
    }
}

fn report(runs: &std::path::Path, out: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_theseus-exam"))
        .arg("report")
        .arg("--runs")
        .arg(runs)
        .arg("--out")
        .arg(out)
        .output()
        .unwrap()
}

#[test]
fn the_report_command_writes_nothing_for_records_another_model_answered() {
    let d = tempfile::tempdir().unwrap();
    let runs = d.path().join("runs.jsonl");
    let out = d.path().join("report.md");
    let line = |r: &Record| serde_json::to_string(r).unwrap();
    std::fs::write(&runs, format!("{}\n", line(&record(&["model-b"])))).unwrap();
    let o = report(&runs, &out);
    assert!(!o.status.success(), "{o:?}");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("model-a") && err.contains("model-b"), "{err}");
    assert!(!out.exists(), "nothing is written");

    std::fs::write(&runs, format!("{}\n", line(&record(&["model-a"])))).unwrap();
    let o = report(&runs, &out);
    assert!(o.status.success(), "{o:?}");
    assert!(out.exists());
}
