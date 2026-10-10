//! The CLI asks for a page, or for one, never for every session
//! (theseus-7bee): the real binary against a scripted daemon that records
//! each request's params, so a call that goes back to a bare list fails here.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
const S: &str = "ses_q7f3k2";

fn session(id: &str) -> Value {
    json!({"session_id": id, "kind": "conversation", "label": null, "title": "Tidy the notes",
           "created_at_unix_ms": 1_759_300_000_000u64, "last_active_ms": 1_759_300_900_000u64,
           "turns": 3})
}

fn execution(id: &str, session: &str) -> Value {
    json!({"execution_id": id, "session_id": session, "kind": "conversation", "state": "waiting",
           "turns": 3, "interrupted": 0, "outstanding": 0, "queued_results": 0,
           "budget": {"limit_usd": 100.0, "spent_usd": 0.25, "reserved_usd": 0.0,
                      "held_unknown_usd": 0.0, "available_usd": 99.75},
           "wake": null, "ended_reason": null,
           "created_at_ms": 1_759_300_000_000u64, "updated_at_ms": 1_759_300_900_000u64})
}

/// What a command printed, and each request it made as `(method, params)`.
struct Ran {
    stdout: String,
    stderr: String,
    requests: Vec<(String, Value)>,
}

/// Run `theseus ARGS` against a daemon that answers each request, by its
/// method, with the next of that method's `answers` (the last one again when
/// they run out), and records it.
fn run(args: &[&str], answers: Vec<(&'static str, Vec<Value>)>) -> Ran {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut reader = BufReader::new(&s);
        let mut seen = Vec::new();
        let mut used = std::collections::HashMap::<String, usize>::new();
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let req: Value = serde_json::from_str(&line).unwrap();
            line.clear();
            let m = req["method"].as_str().unwrap().to_string();
            seen.push((m.clone(), req["params"].clone()));
            let result = match answers.iter().find(|(name, _)| *name == m) {
                Some((_, rs)) => {
                    let i = used.entry(m.clone()).or_default();
                    let r = rs[(*i).min(rs.len() - 1)].clone();
                    *i += 1;
                    r
                }
                // What the command records after its output (theseus-yus0).
                None if m == "session.wait" => {
                    json!({"reached": "timeout", "already": false, "confirms": []})
                }
                None => json!({}),
            };
            let a = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
            if (&s).write_all(format!("{a}\n").as_bytes()).is_err() {
                break;
            }
        }
        seen
    });
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .args(args)
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env("TZ", "<-07>7")
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env_remove("HERDR_ENV")
        .env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_SOCKET_PATH")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let requests = daemon.join().unwrap();
    Ran {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        requests,
    }
}

fn params_of(r: &Ran, method: &str) -> Vec<Value> {
    r.requests
        .iter()
        .filter(|(m, _)| m == method)
        .map(|(_, p)| p.clone())
        .collect()
}

fn history_of(id: &str) -> Value {
    json!({"session": session(id), "nodes": [], "pending_confirms": []})
}

#[test]
fn history_without_a_session_asks_for_the_newest_one_and_its_newest_200_nodes() {
    let r = run(
        &["history"],
        vec![
            (
                "session.list",
                vec![json!({"sessions": [session(S)], "older": 99})],
            ),
            ("session.history", vec![history_of(S)]),
        ],
    );
    assert_eq!(
        params_of(&r, "session.list"),
        vec![json!({"n": 1})],
        "{}",
        r.stderr
    );
    let h = params_of(&r, "session.history");
    assert_eq!(h.len(), 1, "{}", r.stderr);
    assert_eq!(h[0]["n"], 200);
    assert_eq!(h[0]["session_id"], S);
}

#[test]
fn history_n_overrides_the_default_page() {
    let r = run(
        &["history", S, "-n", "7"],
        vec![("session.history", vec![history_of(S)])],
    );
    assert_eq!(params_of(&r, "session.history")[0]["n"], 7, "{}", r.stderr);
    assert!(params_of(&r, "session.list").is_empty());
}

#[test]
fn watch_interactive_without_a_session_asks_for_one_page_of_one() {
    let r = run(
        &["watch", "--interactive"],
        vec![
            ("session.list", vec![json!({"sessions": [session(S)]})]),
            (
                "session.wait",
                vec![
                    json!({"reached": "timeout", "already": false, "confirms": [],
                            "view": null}),
                ],
            ),
        ],
    );
    let lists = params_of(&r, "session.list");
    assert_eq!(lists.first(), Some(&json!({"n": 1})), "{}", r.stderr);
}

#[test]
fn sessions_prints_the_newest_page_and_names_how_to_see_more() {
    let r = run(
        &["sessions"],
        vec![(
            "session.list",
            vec![json!({"sessions": [session(S)], "older": 1_759_200_000_000u64})],
        )],
    );
    assert_eq!(
        params_of(&r, "session.list"),
        vec![json!({"n": 50})],
        "{}",
        r.stderr
    );
    assert!(r.stdout.contains(S), "{}", r.stdout);
    assert!(
        r.stdout.contains("theseus sessions --before 1759200000000") && r.stdout.contains("--all"),
        "the footer names the next page and the whole list: {}",
        r.stdout
    );
}

#[test]
fn the_last_page_has_no_footer() {
    let r = run(
        &["sessions"],
        vec![("session.list", vec![json!({"sessions": [session(S)]})])],
    );
    assert!(r.stdout.contains(S));
    assert!(!r.stdout.contains("older sessions"), "{}", r.stdout);
}

#[test]
fn sessions_before_and_n_page_back_and_all_asks_for_every_session() {
    let list = || vec![("session.list", vec![json!({"sessions": [session(S)]})])];
    let r = run(&["sessions", "--before", "77", "-n", "3"], list());
    assert_eq!(
        params_of(&r, "session.list"),
        vec![json!({"n": 3, "before": 77})],
        "{}",
        r.stderr
    );
    let r = run(&["sessions", "list", "--before", "5"], list());
    assert_eq!(
        params_of(&r, "session.list"),
        vec![json!({"n": 50, "before": 5})],
        "{}",
        r.stderr
    );
    let r = run(&["sessions", "--all"], list());
    let all = params_of(&r, "session.list");
    assert_eq!(all.len(), 1, "{}", r.stderr);
    assert!(
        all[0].is_null() || all[0].as_object().is_some_and(|o| o.is_empty()),
        "--all keeps the bare list: {}",
        all[0]
    );
    assert!(r.stdout.contains(S));
}

#[test]
fn executions_pages_the_same_way() {
    let page = |older: Option<u64>| {
        let mut v = json!({"executions": [execution("exe_q7f3k2", S)]});
        if let Some(o) = older {
            v["older"] = json!(o);
        }
        vec![("execution.list", vec![v])]
    };
    let r = run(&["executions"], page(Some(42)));
    assert_eq!(
        params_of(&r, "execution.list"),
        vec![json!({"n": 50})],
        "{}",
        r.stderr
    );
    assert!(
        r.stdout.contains("theseus executions --before 42"),
        "{}",
        r.stdout
    );
    let r = run(&["executions", "--before", "42"], page(None));
    assert_eq!(
        params_of(&r, "execution.list"),
        vec![json!({"n": 50, "before": 42})]
    );
    assert!(!r.stdout.contains("older executions"));
}

/// A name that is the end of an id is looked for among every execution, read
/// a page at a time: the cursor of each page is the next one's `before`.
#[test]
fn a_name_that_needs_the_executions_reads_them_page_by_page() {
    let first = json!({"executions": [execution("exe_aaaa11", "ses_aaaa11")], "older": 9});
    let last = json!({"executions": [execution("exe_q7f3k2", S)]});
    let r = run(
        &["stop", "f3k2"],
        vec![
            ("execution.list", vec![first, last]),
            ("execution.stop", vec![json!({})]),
        ],
    );
    assert_eq!(
        params_of(&r, "execution.list"),
        vec![json!({"n": 1000}), json!({"n": 1000, "before": 9})],
        "{}",
        r.stderr
    );
    assert_eq!(
        params_of(&r, "execution.stop")[0]["execution_id"],
        "exe_q7f3k2"
    );
}
