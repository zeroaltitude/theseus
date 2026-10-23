use serde_json::json;
use theseus_protocol::ConfirmRequest;

use super::*;

fn gloss(state: &Path, why: Option<&str>) -> Gloss {
    Gloss {
        state: state.to_path_buf(),
        who: "Theseus's self-repair: calls from harness fail".into(),
        own_task: Some("tsk-01a1593f".into()),
        model_why: why.map(str::to_string),
    }
}

fn read(path: &Path) -> ConfirmRequest {
    ConfirmRequest {
        correlation_id: "act_0000feed".into(),
        session_id: "ses_01a1593f784974cf92a4a4666c968b67".into(),
        tool: "fs.read".into(),
        input: json!({"path": path}),
        reason:
            "fs.read — approve (enforcement = approve; its file is under /var/lib/theseus/state, fa"
                .into(),
        ..Default::default()
    }
}

/// An approval says who, what, why, the risk and what Yes and No do, in
/// words: an earlier run's report is "its own report from the 05:12 run",
/// never an elided path or a raw id.
#[test]
fn an_approval_says_who_what_why_risk_and_yes_no() {
    let state = tempfile::tempdir().unwrap();
    let state = state.path().canonicalize().unwrap();
    std::fs::create_dir_all(state.join("self/reports")).unwrap();
    let report = state.join("self/reports/self-20261020-tsk-01a1593f784974cf92a4a4666c968b67.md");
    std::fs::write(&report, "r\n").unwrap();
    let p = plain(
        &read(&report),
        &gloss(&state, Some("I want to see what the last run concluded.")),
    );
    assert_eq!(p.who, "Theseus's self-repair: calls from harness fail");
    assert!(
        p.what.starts_with("read its own report from the "),
        "{}",
        p.what
    );
    assert!(p.what.ends_with(" run"), "{}", p.what);
    assert_eq!(p.why, "I want to see what the last run concluded.");
    assert_eq!(p.risk, "read-only");
    assert_eq!(p.yes, "runs this one call.");
    assert!(p.no.starts_with("refuses it"), "{}", p.no);
    let all = format!("{p:?}");
    for raw in ["tsk-", "ses_", "…", "enforcement", "act_0000feed"] {
        assert!(!all.contains(raw), "{raw} in {all}");
    }
    // Another run's report is told apart from its own.
    let other = state.join("self/reports/self-20261020-tsk-0f00ba5e.md");
    std::fs::write(&other, "r\n").unwrap();
    let said = what_of(&read(&other), &gloss(&state, None));
    assert!(
        said.starts_with("read another self-repair run's report"),
        "{said}"
    );
}

/// The places a model looks at in a guess are named for what they hold.
#[test]
fn a_path_is_named_for_what_it_holds() {
    let state = Path::new("/home/x/.theseus");
    let g = gloss(state, None);
    for (path, has) in [
        (
            "/home/x/.theseus/store/blobs",
            "own store (conversation content)",
        ),
        ("/home/x/.theseus", "whole state folder"),
        ("/home/x/.theseus/self/gates/g.sh", "gate script or log"),
        (
            "/home/x/.theseus/self/trees/sails/src/a.rs",
            "`src/a.rs` in its working tree",
        ),
        ("/home/x/.ssh/id_ed25519", "your SSH keys"),
        (
            "/home/x/projects/app/src/main.rs",
            "`/home/x/projects/app/src/main.rs`",
        ),
    ] {
        let said = object_of(path, &g);
        assert!(said.contains(has), "{path}: {said}");
    }
    // A long path keeps its end.
    let long = format!("/home/x/{}/effort.rs", "deep/".repeat(40));
    let said = object_of(&long, &g);
    assert!(
        said.ends_with("/effort.rs`") && said.contains('…'),
        "{said}"
    );
}

/// The badge is the worst the call can do: a read is read-only, an edit
/// writes, a command runs a program, a fetch is the network, and the floor
/// touches secrets.
#[test]
fn the_risk_badge_follows_the_call() {
    let mk = |tool: &str, input| ConfirmRequest {
        tool: tool.into(),
        input,
        ..Default::default()
    };
    for (tool, input, want) in [
        ("fs.read", json!({"path": "a"}), "read-only"),
        ("fs.grep", json!({"path": "a", "pattern": "b"}), "read-only"),
        ("fs.edit", json!({"path": "a"}), "writes"),
        ("proc.run", json!({"argv": ["ls"]}), "runs a program"),
        ("http.fetch", json!({"url": "https://x"}), "network"),
        (
            "fs.read",
            json!({"path": "/home/x/.aws/credentials"}),
            "touches secrets",
        ),
    ] {
        assert_eq!(risk_of(&mk(tool, input)), want, "{tool}");
    }
    let mut floor = mk("fs.read", json!({"path": "a"}));
    floor.floor = true;
    assert_eq!(risk_of(&floor), "touches secrets");
}

/// The model's reason is the last line it wrote before the call, one line.
#[test]
fn the_models_reason_is_its_last_line_before_the_call() {
    assert_eq!(
        reason_line("Let me look.\n\nI need the 05:12 report to see why it failed.\n").as_deref(),
        Some("I need the 05:12 report to see why it failed.")
    );
    assert_eq!(reason_line("  \n"), None);
}

/// The reason the real guard writes: its question inside the gate's
/// `— approve (…)`, as `Decision::at_least` words it.
fn guarded(alt: Option<&str>) -> ConfirmRequest {
    use crate::policy::{Decision, Posture};
    use crate::toolrun::misroute::Held;
    let held = Held {
        path: "~/projects/walkthrough/out/harbour-sum.md".into(),
        verb: "edits",
        thread: "Harbour log notes".into(),
        alt: alt.map(str::to_string),
    };
    let d = Decision::new(Posture::Notify, "notify".into()).at_least(
        Posture::Approve,
        &held.why(),
        "the thread misroute guard",
        "fs.write",
        "create out/harbour-sum.md",
    );
    ConfirmRequest {
        tool: "fs.write".into(),
        input: json!({"path": "~/projects/walkthrough/out/harbour-sum.md"}),
        reason: d.reason,
        ..Default::default()
    }
}

/// A write the thread guard held says it waits because it may be in the wrong
/// thread, and the guard's question names both threads and the move; it does
/// not say the tool "is set to ask" (the posture was notify) (theseus-zcn80).
#[test]
fn a_misroute_hold_card_names_the_guard_and_both_threads() {
    let state = Path::new("/home/x/.theseus");
    let p = plain(
        &guarded(Some("Docs word census")),
        &gloss(state, Some("I will write the summary.")),
    );
    assert_eq!(
        p.held,
        "it may belong in another thread, so it waits for you"
    );
    assert!(!p.held.contains("set to ask"), "{}", p.held);
    assert_eq!(
        p.why,
        "This edits ~/projects/walkthrough/out/harbour-sum.md, which isn't in the Harbour log \
         notes thread. Go on, or move to the Docs word census?",
        "the guard's question, not the model's line"
    );
    // With no second thread in mind it still offers the move.
    let p = plain(&guarded(None), &gloss(state, None));
    assert!(p.why.ends_with("Go on, or move it?"), "{}", p.why);
}

/// A hold by a rule with a clause of its own (an approve-list match) says it
/// in `why` when the model said nothing, in the words Discord's card shows;
/// the owner's own posture has no clause and leaves `why` as it was.
#[test]
fn an_approve_list_card_says_why() {
    let state = Path::new("/home/x/.theseus");
    let req = |reason: &str| ConfirmRequest {
        tool: "proc.run".into(),
        input: json!({"argv": ["gh", "pr", "view"]}),
        reason: reason.into(),
        ..Default::default()
    };
    let listed = req("run gh: proc.run — approve (gh is on the approve list (§3.9))");
    assert_eq!(
        rule_clause(&listed.reason).as_deref(),
        Some("gh is on the approve list")
    );
    let p = plain(&listed, &gloss(state, None));
    assert_eq!(p.why, "gh is on the approve list");
    assert_eq!(p.held, "gh is on the approve list");
    // The model's own line wins the `why`; the rule stays in `held`.
    let p = plain(&listed, &gloss(state, Some("I want the PR's status.")));
    assert_eq!(
        (p.why.as_str(), p.held.as_str()),
        ("I want the PR's status.", "gh is on the approve list")
    );
    // The owner's posture is not a clause.
    let own = req("run gh: proc.run — approve (enforcement = approve)");
    assert_eq!(rule_clause(&own.reason), None);
    let p = plain(&own, &gloss(state, None));
    assert_eq!(p.why, "");
    assert_eq!(p.held, "`proc.run` is set to ask before it runs");
}
