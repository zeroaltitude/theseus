//! What `theseus history` with a search asks the daemon (issue 44): each search flag reaches `history.read` as
//! its own parameter, and a page's printed continuation, run as it was printed, asks the same search from that
//! page's cursor. The page is headed by what was asked: the place named, or every place, and the label. Each
//! runs the real binary against a scripted daemon on a Unix socket that keeps each request's params. Kept apart
//! from golden.rs, which is at its line ceiling; its output golden is `history_search_reads_every_place`'s.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
/// The zone every golden's times are written in: UTC−7, all year.
const GOLDEN_TZ: &str = "<-07>7";

/// Run `theseus --socket <sock> ARGS` against a daemon that answers each request in order with the next of
/// `answers`, and closes after the last: the binary's stdout, and each request's params.
fn run(args: &[&str], answers: Vec<Value>) -> (String, Vec<Value>) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut reader = BufReader::new(&s);
        let mut asked = Vec::new();
        for result in answers {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let req: Value = serde_json::from_str(&line).unwrap();
            asked.push(req["params"].clone());
            let a = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
            (&s).write_all(format!("{a}\n").as_bytes()).unwrap();
        }
        asked
    });
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .args(args)
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env_remove("HERDR_ENV")
        .env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_SOCKET_PATH")
        .env("TZ", GOLDEN_TZ)
        .output()
        .unwrap();
    let asked = daemon.join().unwrap();
    (String::from_utf8_lossy(&out.stdout).into_owned(), asked)
}

#[test]
fn history_search_sends_each_flag_and_its_continuation_reads_on() {
    let page = json!({"place": "discord:dm:5551", "end": "more",
    "next": {"at_ms": 1_794_312_000_000u64, "turn_id": "turn_a1"},
    "lines": [
        {"handle": "turn_a1", "place": "discord:dm:5551", "at_ms": 1_794_312_000_000u64,
         "when": "2026-11-11 05:20", "who": "the owner", "words": "the tide tables",
         "reply_begins": "Here they are", "outside": false, "labels": "personal: on the message"}
    ]});
    let args = [
        "history",
        "--from",
        "2026-11-01",
        "--to",
        "2026-11-15",
        "--label",
        "personal",
        "--words",
        "tide",
        "--place",
        "discord:dm:5551",
    ];
    let (text, asked) = run(&args, vec![page.clone()]);
    let want = json!({"from": "2026-11-01", "to": "2026-11-15", "label": "personal", "words": "tide",
        "place": "discord:dm:5551"});
    let sent = &asked[0];
    for (k, v) in want.as_object().unwrap() {
        assert_eq!(&sent[k], v, "{k}: {sent}");
    }
    assert!(sent["before"].is_null(), "{sent}");
    assert!(
        text.contains("── 1 exchange of discord:dm:5551's history labelled personal, newest first"),
        "{text}"
    );
    let empty = json!({"place": "", "end": "span_start", "lines": []});
    let (when, asked) = run(&["history", "--when", "last week"], vec![empty]);
    assert_eq!(asked[0]["when"], "last week", "{when}");
    assert!(
        asked[0]["label"].is_null() && asked[0]["place"].is_null(),
        "{}",
        asked[0]
    );
    let unscoped = json!({"place": "", "end": "more", "lines": []});
    let (unscoped, _) = run(&["history", "--label", "personal"], vec![unscoped]);
    assert!(
        unscoped.contains("of every place's history labelled personal"),
        "{unscoped}"
    );
    // The continuation, as printed, run again.
    let printed = (text.lines())
        .find_map(|l| l.strip_prefix("── more before these: theseus "))
        .unwrap_or_else(|| panic!("a continuation: {text}"));
    let again: Vec<&str> = printed.split_whitespace().collect();
    let (_, asked) = run(&again, vec![page]);
    let sent = &asked[0];
    for (k, v) in want.as_object().unwrap() {
        assert_eq!(&sent[k], v, "{k} kept: {sent}");
    }
    assert_eq!(
        sent["before"],
        json!({"at_ms": 1_794_312_000_000u64, "turn_id": "turn_a1"}),
        "{sent}"
    );
}
