//! The hygiene of what the read keeps (§2.5): which values may be a constant,
//! the repository's key, and the shape of a day's record.

use super::*;
use crate::node::{Body, Node};

#[test]
fn secret_shapes_are_found_by_prefix_and_by_look() {
    let aws = format!("{}{}", "AKIA", "IOSFODNN7EXAMPL3");
    let sk = format!("{}{}", "sk-", "proj-q8Zr3VnL0aXw9TgB5mYc2HdK");
    let jwt = format!("{}{}", "eyJ", "hbGciOiJIUzI1NiJ9.e30.abc");
    for s in [&aws, &sk, &jwt] {
        assert!(secret_shaped(s), "{s}");
    }
    assert!(
        secret_shaped("Zk3j9QpLx7Vn2Rt8Wm4Yc6Hb1Fs5Dg0A"),
        "a long mixed run"
    );
    for ok in [
        "main",
        "feat/job-3",
        "crates/theseus-core/src/lib.rs",
        "0123456789abcdef0123456789abcdef01234567",
        "origin",
        "--no-ff",
        "harbord",
    ] {
        assert!(!secret_shaped(ok), "{ok}");
    }
}

#[test]
fn a_person_like_string_is_not_a_constant() {
    for p in ["jane@example.org", "Jane Doe", "Commit by Mary Smith"] {
        assert!(person_like(p), "{p}");
        assert!(!constant_ok(p, ""), "{p}");
    }
    assert!(!person_like("git@example.org:team/repo.git"));
    assert!(!person_like("fix the flaky test"));
}

#[test]
fn a_constant_is_short_and_has_no_absolute_path_outside_the_repository() {
    assert!(constant_ok("harbord", ""));
    assert!(constant_ok("crates/**.rs", "crates/**.rs"));
    assert!(!constant_ok("/home/someone/secret/notes.txt", "$ABS"));
    assert!(!constant_ok(&"x".repeat(65), ""));
    assert!(!constant_ok("two\nlines", ""));
    assert!(!constant_ok("", ""));
}

#[test]
fn a_repository_is_keyed_by_its_path_and_its_origins_path() {
    let mk = |url: &str| {
        let d = std::env::temp_dir().join(format!("theseus-scope-{}", crate::new_id("s")));
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("crates/a")).unwrap();
        std::fs::write(
            d.join(".git/config"),
            format!("[remote \"origin\"]\n\turl = {url}\n"),
        )
        .unwrap();
        d
    };
    let a = mk("git@example.org:harbor/harbord.git");
    let (k1, root) = scope_of(&a.display().to_string());
    assert!(k1.starts_with("r:") && root.is_some());
    // A subdirectory is the same repository.
    assert_eq!(scope_of(&a.join("crates/a").display().to_string()).0, k1);
    // Another checkout, even of the same origin, is another key.
    let b = mk("git@example.org:harbor/harbord.git");
    assert_ne!(scope_of(&b.display().to_string()).0, k1);
    // A directory in no repository is its own scope.
    let loose = std::env::temp_dir().join(format!("theseus-loose-{}", crate::new_id("l")));
    std::fs::create_dir_all(&loose).unwrap();
    assert!(scope_of(&loose.display().to_string()).0.starts_with("d:"));
    // The key holds neither the path nor the host.
    assert!(!k1.contains("harbor") && !k1.contains("example"));
}

#[test]
fn a_days_list_round_trips_and_holds_tokens_only() {
    let mut l = crate::rsi::mine::fixture::Ledger::new();
    l.owners_example(crate::rsi::mine::fixture::REPO);
    let day = Day {
        date: "2026-10-10".into(),
        runs: l.runs.clone(),
    };
    let json = serde_json::to_string(&day).unwrap();
    assert_eq!(serde_json::from_str::<Day>(&json).unwrap(), day);
    assert!(
        !json.contains("feat/job") && !json.contains("git merge"),
        "ids and digests only"
    );
}

#[test]
fn a_window_adds_runs_and_extends_a_turn_that_went_on() {
    let mut l = crate::rsi::mine::fixture::Ledger::new();
    let r = l.run("s1", "2026-10-10", "r:x");
    l.model(r);
    l.argv(r, &["git", "status"], 0, "");
    let first = l.runs[0].clone();
    let mut more = first.clone();
    more.calls[0].pos += 100;
    let mut w = Window::default();
    let ch = w.add(Folded {
        runs: vec![first],
        vocab: l.vocab.clone(),
        cursor: 5,
        calls: 1,
    });
    assert_eq!(ch, ["2026-10-10"]);
    w.add(Folded {
        runs: vec![more],
        vocab: Vocab::default(),
        cursor: 9,
        calls: 1,
    });
    let run = &w.days["2026-10-10"].runs;
    assert_eq!(run.len(), 1, "one turn is one run");
    assert_eq!((run[0].calls.len(), run[0].loops.len()), (2, 2));
    assert_eq!(run[0].calls[1].loop_ix, 1, "its loops are its own");
}

#[test]
fn the_window_is_the_last_days() {
    let now = 20 * 86_400_000;
    let d = window_dates(now, 14);
    assert_eq!(d.len(), 14);
    assert_eq!(d[0], date_of(now));
    assert_eq!(d[13], date_of(now - 13 * 86_400_000));
}

/// A turn's nodes: a flow's own call, its steps (`<id>.<n>`, written as the
/// model's calls would be), and one call by hand.
fn flow_turn(store: &Store, steps: bool) {
    let (sid, tid) = ("s1", "t1");
    let mut a = Node::assistant(
        sid,
        tid,
        0,
        Body::AssistantMessage {
            blocks: vec![],
            model: "claude-test".into(),
            provider: "anthropic".into(),
            stop_reason: None,
            usage: theseus_protocol::Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    a.created_at_ms = 1_000;
    let mut recs = vec![a.record().unwrap()];
    let mut ids = vec![("flow:run_scripts", "F1"), ("proc.run", "H1")];
    if steps {
        ids.extend([("proc.run", "F1.1"), ("proc.run", "F1.2")]);
    }
    for (i, (tool, id)) in ids.into_iter().enumerate() {
        let mut call = Node::tool_call(
            sid,
            Some(tid),
            Some(0),
            Body::ToolCall {
                tool_use_id: id.into(),
                tool: tool.into(),
                wire_name: tool.replace(['.', ':'], "_"),
                input: serde_json::json!({"argv": ["git", "merge", "--no-ff", format!("feat/job-{i}")]}),
                assistant_node: a.id.clone(),
                correlation_id: Some(id.into()),
                gate: None,
            },
        );
        call.created_at_ms = 1_001 + i as u64;
        recs.push(call.record().unwrap());
    }
    store.append(&recs).unwrap();
}

/// A flow's own steps are no by-hand calls (theseus-pw1q.21.8): the miner
/// reads the same calls with the steps in the ledger as without them, and
/// still reads the call made by hand.
#[test]
fn a_flows_steps_are_not_by_hand_calls() {
    let read = |steps: bool| {
        let dir = std::env::temp_dir().join(format!("theseus-flow-steps-{}", crate::new_id("s")));
        let store = Store::open(&dir.join("store")).unwrap();
        flow_turn(&store, steps);
        let clean = |_: &str| true;
        let session = |_: &str| {
            Some(SessionInfo {
                dir: Some(dir.display().to_string()),
                place: "cli".into(),
            })
        };
        let env = Env {
            key: b"k".to_vec(),
            clean: &clean,
            session: &session,
        };
        fold(&store, &env, 0, None, 1 << 60, 1000).unwrap().calls
    };
    let without = read(false);
    assert!(without >= 1, "the call by hand is read: {without}");
    assert_eq!(read(true), without, "the steps add no call");
}
