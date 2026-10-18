//! "Since you last looked" against a real daemon (theseus-qkoi3), the live
//! check: a client looks at the board, goes away, and a week of work happens
//! while it is gone: three small tasks report, a slow one is cancelled, a call
//! asks and is answered, and a timer fires. It comes back and `work.digest`
//! from the position it left tells every one of them, once, with the same words
//! `theseus digest --since-seen` prints; a second read from that digest's end
//! tells nothing. `THESEUS_RECORD_DIGEST=<file>` writes the answer, as the
//! cockpit's recorded fixture.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;

use common::model::FakeModel;
use serde_json::{json, Value};

/// What the stand-in model calls for a prompt that contains a phrase.
fn script(prompt: &str) -> Vec<(&'static str, Value)> {
    if prompt.contains("Run the slow job.") {
        return vec![("proc_run", json!({"argv": ["sleep", "30"]}))];
    }
    if prompt.starts_with("[Task ") && prompt.contains("Count to three.") {
        return vec![];
    }
    let calls: [(&str, Vec<(&'static str, Value)>); 4] = [
        (
            "write the note",
            vec![("fs_write", json!({"path": "note.txt", "content": "kept\n"}))],
        ),
        (
            "start a small task",
            vec![(
                "task_create",
                json!({"brief": "Count to three.", "arrangement":
                       {"pieces": [{"quote": "Please start a small task.", "role": "objective"}]}}),
            )],
        ),
        (
            "set a timer",
            vec![("wake_at", json!({"after": "1s", "note": "tick-tock"}))],
        ),
        (
            "start a slow task",
            vec![(
                "task_create",
                json!({"brief": "Run the slow job.", "arrangement":
                       {"pieces": [{"quote": "Please start a slow task.", "role": "objective"}]}}),
            )],
        ),
    ];
    calls
        .into_iter()
        .find(|(p, _)| prompt.contains(p))
        .map(|(_, c)| c)
        .unwrap_or_default()
}

struct Rig {
    dir: tempfile::TempDir,
    _model: FakeModel,
}

impl Fixture for Rig {
    fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Rig {
    fn new() -> Self {
        let model = FakeModel::start(script);
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects", "state"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path("bin/op"), FAKE_OP).unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        table(&mut t, "policy").insert("enforcement".into(), "open".into());
        table(&mut t, "policy")
            .entry("tools")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
            .insert("fs.write".into(), "approve".into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, _model: model }
    }

    fn ask(&self, sid: &str, prompt: &str) -> Value {
        self.call(
            "turn.submit",
            json!({"session_id": sid, "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    fn open(&self) -> String {
        self.call("session.open", json!({})).unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// The real `theseus` binary beside the daemon's, on this daemon.
    fn cli(&self, args: &[&str]) -> Option<std::process::Output> {
        let bin = PathBuf::from(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus");
        bin.exists().then(|| {
            std::process::Command::new(bin)
                .arg("--socket")
                .arg(self.path("sock"))
                .args(args)
                .env_remove("THESEUS_SESSION")
                .env("XDG_STATE_HOME", self.path("xdg"))
                .env("TZ", "America/Phoenix")
                .output()
                .expect("running theseus")
        })
    }
}

impl Rig {
    /// `work.digest` from `since`.
    fn digest(&self, since: u64) -> Value {
        self.call("work.digest", json!({"since": since})).unwrap()
    }

    /// The lines of a digest as (kind, count).
    fn kinds(d: &Value) -> Vec<(String, u64)> {
        d["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["kind"].as_str().unwrap().to_string(),
                    l["count"].as_u64().unwrap_or(1),
                )
            })
            .collect()
    }
}

impl Rig {
    /// A week of work: three small tasks report, a slow one is cancelled, a call
    /// asks and is answered, and a timer fires.
    fn while_away(&self) {
        let r = self;
        // It is away. Three small tasks report, each under a conversation of its own.
        for _ in 0..3 {
            let c = r.open();
            r.ask(&c, "Please start a small task.");
        }
        // A slow task is cancelled.
        let c = r.open();
        r.ask(&c, "Please start a slow task.");
        let task = r.wait("the slow task", || {
            let t = r.call("task.list", json!({"session_id": c})).ok()?;
            let t = t["tasks"].as_array()?.first()?.clone();
            t["task_id"].as_str().map(str::to_string)
        });
        r.call("task.cancel", json!({"task": task})).unwrap();
        // A call asks, and is answered.
        let c = r.open();
        r.ask(&c, "Please write the note.");
        let q = r.wait("the question", || {
            let s = r.call("work.watch", json!({"limit": 50})).ok()?;
            let _ = r.call("work.unwatch", json!({}));
            s["views"]
                .as_array()?
                .iter()
                .find(|v| v["kind"] == "question")
                .and_then(|v| v["question"]["id"].as_str().map(str::to_string))
        });
        r.call(
            "work.answer",
            json!({"question_id": q, "option": "approve"}),
        )
        .unwrap();
        // A timer fires.
        let c = r.open();
        r.ask(&c, "Please set a timer.");
    }
}

#[test]
fn what_happened_while_a_client_was_away_is_told_once_from_where_it_left() {
    let r = Rig::new();
    let d1 = r.spawn();
    // The client looks at the board: it keeps where.
    let left = r.call("work.watch", json!({"limit": 10})).unwrap()["position"]
        .as_u64()
        .unwrap();
    let _ = r.call("work.unwatch", json!({}));
    assert!(r.digest(left)["lines"].as_array().unwrap().is_empty());

    r.while_away();

    // Back: every one of them, told once.
    let d = r.wait("the whole digest", || {
        let d = r.digest(left);
        let k = Rig::kinds(&d);
        let n = |kind: &str| -> u64 { k.iter().filter(|(x, _)| x == kind).map(|(_, c)| c).sum() };
        (n("reported") == 3 && n("cancelled") == 1 && n("answered") == 1 && n("woke") >= 1)
            .then_some(d)
    });
    assert_eq!(d["from"].as_u64(), Some(left));
    assert_eq!(
        d["from_ledger"],
        json!(false),
        "the ring reaches back to the look"
    );
    let words: Vec<&str> = d["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["words"].as_str().unwrap())
        .collect();
    eprintln!("the digest:\n{}", words.join("\n"));
    assert_eq!(
        words
            .iter()
            .filter(|w| w.ends_with("Count to three. reported"))
            .count(),
        3,
        "{words:?}"
    );
    assert!(
        words
            .iter()
            .any(|w| w.contains("Run the slow job.") && w.contains("cancelled")),
        "{words:?}"
    );
    assert!(
        words
            .iter()
            .any(|w| w.starts_with("✓ you answered: fs.write")),
        "{words:?}"
    );

    // The daemon restarts: its board is seeded past the look, so the same window is read
    // from the ledger, and the same things happened, each once.
    drop(d1);
    let _d2 = r.spawn();
    let from_ledger = r.digest(left);
    assert_eq!(from_ledger["from_ledger"], json!(true), "{from_ledger}");
    let n = |d: &Value, kind: &str| -> u64 {
        Rig::kinds(d)
            .iter()
            .filter(|(x, _)| x == kind)
            .map(|(_, c)| c)
            .sum()
    };
    for kind in ["reported", "cancelled", "answered"] {
        assert_eq!(n(&from_ledger, kind), n(&d, kind), "{kind}");
    }
    assert_eq!(
        n(&from_ledger, "woke"),
        1,
        "one timer, one line: {from_ledger}"
    );

    // The real CLI says the same, and a second read from where the first ended tells nothing.
    if let Some(out) = r.cli(&["digest", "--since", "10m"]) {
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            text.starts_with("◆ 3 reported, 1 answered by you, 1 cancelled"),
            "{text}"
        );
        assert!(text.contains("📋 Count to three. reported"), "{text}");
    }
    let to = d["to"].as_u64().unwrap();
    assert!(r.digest(to)["lines"].as_array().unwrap().is_empty());

    if let Ok(path) = std::env::var("THESEUS_RECORD_DIGEST") {
        std::fs::write(path, serde_json::to_string_pretty(&d).unwrap()).unwrap();
    }
}

/// What the lines of a digest add up to, by kind.
fn count(d: &Value, kind: &str) -> u64 {
    Rig::kinds(d)
        .iter()
        .filter(|(x, _)| x == kind)
        .map(|(_, c)| c)
        .sum()
}

/// The daemon stops between a client being told and the next one's arrival,
/// once by a kill and once by a clean stop: from the end of what was told, the
/// restarted daemon tells nothing, and from where the client left it tells the
/// same things, each once, from the ledger (theseus-qkoi3: the timer's line
/// was told at the due scan's position by the ring and again at its
/// `wake.fired` row's by the ledger).
#[test]
fn a_stop_between_telling_and_the_next_arrival_tells_nothing_twice() {
    let r = Rig::new();
    let d1 = r.spawn();
    let left = r.call("work.watch", json!({"limit": 10})).unwrap()["position"]
        .as_u64()
        .unwrap();
    let _ = r.call("work.unwatch", json!({}));
    r.while_away();
    let told = r.wait("the whole digest", || {
        let d = r.digest(left);
        (count(&d, "reported") == 3
            && count(&d, "cancelled") == 1
            && count(&d, "answered") == 1
            && count(&d, "woke") == 1)
            .then_some(d)
    });
    assert_eq!(told["from_ledger"], json!(false));
    let to = told["to"].as_u64().unwrap();

    // Killed: the next client finds a board seeded past its position.
    drop(d1);
    let d2 = r.spawn();
    let after = r.digest(to);
    assert!(after["lines"].as_array().unwrap().is_empty(), "{after}");
    let again = r.digest(left);
    assert_eq!(again["from_ledger"], json!(true), "{again}");
    for kind in ["reported", "cancelled", "answered", "woke"] {
        assert_eq!(count(&again, kind), count(&told, kind), "{kind}: {again}");
    }

    // Stopped cleanly, and again.
    let _ = r.call("shutdown", Value::Null);
    drop(d2);
    let _d3 = r.spawn();
    assert!(r.digest(to)["lines"].as_array().unwrap().is_empty());
    let again = r.digest(left);
    for kind in ["reported", "cancelled", "answered", "woke"] {
        assert_eq!(count(&again, kind), count(&told, kind), "{kind}: {again}");
    }
}
