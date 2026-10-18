//! The sealed store's own tests (theseus-pw1q.5): `put` returns no bytes, the
//! scores match a known seeded set, and a canary planted in an item never
//! comes out of the scorer, its errors or its logs.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;

/// Planted in items; it must never appear in anything the scorer gives.
const CANARY: &str = "zebra-protocol-7f3a9c-canary";

fn item(task: &str, check: &str, canary: Option<&str>) -> Vec<u8> {
    let mut v = json!({"v": 1, "task": task, "check": check});
    if let Some(c) = canary {
        v["canary"] = json!(c);
    }
    serde_json::to_vec(&v).unwrap()
}

/// A candidate that answers from a table; a task not in it fails to run.
struct Table(BTreeMap<String, String>);

impl Runner for Table {
    fn answer(&self, task: &Task<'_>) -> Result<Answer, RunFailed> {
        self.0
            .get(task.text())
            .map(|r| Answer {
                reply: r.clone(),
                cost_usd: 0.25,
            })
            .ok_or(RunFailed)
    }
}

fn store() -> (tempfile::TempDir, Sealed) {
    let d = tempfile::tempdir().unwrap();
    let s = Sealed::new(d.path().join("sealed"));
    (d, s)
}

/// Six items in two families, and a candidate that gets four right, one
/// wrong, and fails to run on one.
fn seeded(s: &Sealed) -> Table {
    let mut table = BTreeMap::new();
    let rows = [
        (
            "fact",
            "What port does the tide table listen on?",
            "reply has \"7433\"",
            Some("7433"),
        ),
        (
            "fact",
            "Name the store's index engine in one word.",
            "reply has word \"redb\"",
            None,
        ),
        (
            "fact",
            "Which engine was refused for the index?",
            "reply has \"fjall\" | \"sled\"",
            None,
        ),
        (
            "decision",
            "Should the harness retry a refused request?",
            "reply has \"once\"\nreply lacks \"never\"",
            None,
        ),
        (
            "decision",
            "Where does a spent reservation settle?",
            "reply has \"at what it spent\"",
            None,
        ),
        (
            "decision",
            "How is a halted switch released?",
            "reply has \"the owner\" or reply has \"owner's resume\"",
            None,
        ),
    ];
    // The answers: 3 right in the order given for items 0, 1, 3, 5; item 2
    // answers wrongly; item 4 has no answer (the run fails).
    let answers = [
        Some("It listens on 7433."),
        Some("The index is REDB."),
        Some("It used lmdb."),
        Some("Retry once, never twice... no wait: once."),
        None,
        Some("Only the owner's resume counts."),
    ];
    for ((fam, task, check, _), ans) in rows.iter().zip(answers) {
        s.put("core", fam, &item(task, check, None)).unwrap();
        if let Some(a) = ans {
            table.insert((*task).to_string(), a.to_string());
        }
    }
    Table(table)
}

#[test]
fn the_scores_match_a_known_seeded_set() {
    let (_d, s) = store();
    let table = seeded(&s);
    let scores = s.score("core", &table).unwrap();
    // Item 3 says "never", so its `lacks` line fails: 3 of 6 pass.
    assert_eq!(
        (scores.items, scores.passed, scores.failed, scores.errors),
        (6, 3, 3, 1)
    );
    assert!((scores.rate - 0.5).abs() < 1e-9);
    assert!(
        (scores.cost_usd - 1.25).abs() < 1e-9,
        "five runs at 25 cents"
    );
    let f = &scores.families;
    assert_eq!(f.keys().collect::<Vec<_>>(), ["decision", "fact"]);
    assert_eq!(
        (f["fact"].items, f["fact"].passed, f["fact"].failed),
        (3, 2, 1)
    );
    assert_eq!(
        (
            f["decision"].items,
            f["decision"].passed,
            f["decision"].failed
        ),
        (3, 1, 2)
    );
    assert_eq!(f["decision"].errors, 1);
    assert!((f["fact"].rate - 2.0 / 3.0).abs() < 1e-9);
    assert_eq!(scores.failures.len(), 3);
    // The failure ids are opaque, stable and distinct.
    assert!(scores
        .failures
        .iter()
        .all(|id| id.len() == 18 && id.starts_with("f_")));
    let again = s.score("core", &table).unwrap();
    assert_eq!(scores, again, "a score of the same set is the same");
    // Keyed: the same items under another key name their failures otherwise.
    let (_d2, other) = store();
    seeded(&other);
    let theirs = other.score("core", &table).unwrap();
    assert_eq!(theirs.passed, scores.passed);
    assert_ne!(theirs.failures, scores.failures, "the id is a keyed hash");
    assert_eq!(s.counts(), BTreeMap::from([("core".to_string(), 6)]));
}

#[test]
fn put_returns_an_opaque_id_and_no_bytes() {
    let (_d, s) = store();
    let bytes = item(
        "Say the secret handshake word.",
        "reply has \"swordfish\"",
        Some(CANARY),
    );
    let id = s.put("core", "fact", &bytes).unwrap();
    assert_eq!(id.as_str().len(), 16);
    assert!(id.as_str().bytes().all(|b| b.is_ascii_hexdigit()));
    for shown in [id.to_string(), format!("{id:?}")] {
        assert!(
            !shown.contains("handshake") && !shown.contains("swordfish") && !shown.contains(CANARY)
        );
    }
    assert_eq!(
        s.put("core", "fact", &bytes).unwrap(),
        id,
        "the same bytes, the same id"
    );
    assert_eq!(s.counts()["core"], 1, "and one item");
    let other = s
        .put(
            "core",
            "fact",
            &item("A second task.", "reply has \"x\"", None),
        )
        .unwrap();
    assert_ne!(other, id);
    // Written 0600 in a 0700 dir; nothing in the API reads one back.
    let file = s.dir.join("core").join("fact").join(format!("{id}.item"));
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&s.dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    // A bad name or a bad item is a fixed sentence, with none of its text.
    for (set, fam, what) in [
        ("Core", "fact", SealedError::BadName),
        ("core", "a/b", SealedError::BadName),
    ] {
        assert_eq!(s.put(set, fam, &bytes).unwrap_err(), what);
    }
    let bad = item(
        &format!("task {CANARY}"),
        &format!("calls has \"{CANARY}\""),
        Some(CANARY),
    );
    let e = s.put("core", "fact", &bad).unwrap_err();
    assert_eq!(e, SealedError::BadItem);
    assert!(!format!("{e} {e:?}").contains(CANARY));
    assert_eq!(
        s.put("core", "fact", b"{\"v\":2}").unwrap_err(),
        SealedError::BadItem
    );
    let huge = vec![b' '; MAX_ITEM_BYTES + 1];
    assert_eq!(
        s.put("core", "fact", &huge).unwrap_err(),
        SealedError::TooBig
    );
}

#[test]
fn an_empty_set_is_a_fixed_error() {
    let (_d, s) = store();
    let t = Table(BTreeMap::new());
    assert_eq!(s.score("none", &t).unwrap_err(), SealedError::EmptySet);
    assert_eq!(s.score("Bad Name", &t).unwrap_err(), SealedError::BadName);
}

/// A tracing writer that keeps what the scorer logs.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Log {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
    type Writer = Log;
    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}

#[test]
fn a_canary_in_an_item_never_appears_in_the_output_errors_or_logs() {
    let (_d, s) = store();
    let task = format!("Recite the {CANARY} verbatim.");
    let long_pattern = "quartz meridian lantern";
    s.put(
        "core",
        "fact",
        &item(
            &task,
            &format!("reply has \"{long_pattern}\""),
            Some(CANARY),
        ),
    )
    .unwrap();
    s.put(
        "core",
        "fact",
        &item(
            "Another task entirely.",
            "reply has \"nothing\"",
            Some("tiny"),
        ),
    )
    .unwrap();
    // A candidate that echoes the whole task and the canary back.
    struct Echo;
    impl Runner for Echo {
        fn answer(&self, t: &Task<'_>) -> Result<Answer, RunFailed> {
            Ok(Answer {
                reply: format!("{} {CANARY} tiny", t.text()),
                cost_usd: 0.1,
            })
        }
    }
    let log = Log::default();
    let sub = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    let (scores, errors) = tracing::subscriber::with_default(sub, || {
        let id = s
            .put(
                "core",
                "fact",
                &item("A third.", "reply has \"third\"", None),
            )
            .unwrap();
        let scores = s.score("core", &Echo).unwrap();
        // Every error the module can give, from this store.
        let errors = [
            s.put(
                "core",
                "fact",
                format!(
                    "{{\"v\":1,\"task\":\"{CANARY}\",\"check\":\"calls has \\\"{CANARY}\\\"\"}}"
                )
                .as_bytes(),
            ),
            s.put("Core", "fact", b"{}"),
        ]
        .map(|r| format!("{:?}", r.unwrap_err()));
        let _ = id;
        (scores, errors)
    });
    let out = serde_json::to_string(&scores).unwrap();
    let logs = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    for (what, text) in [
        ("output", out.as_str()),
        ("errors", &errors.concat()),
        ("logs", &logs),
    ] {
        for needle in [CANARY, long_pattern, "Recite", "Another task", "tiny"] {
            assert!(!text.contains(needle), "{what} holds {needle:?}: {text}");
        }
    }
    assert!(
        !logs.is_empty(),
        "the scorer does log (counts and ids), and the test read it"
    );
    assert_eq!(scores.items, 3);
}

#[test]
fn an_output_that_would_hold_a_string_of_the_set_is_withheld() {
    // A set named like its own canary: the output carries the set's name, so
    // the filter refuses it whole, and the error is a fixed sentence.
    let (_d, s) = store();
    let set = "canary-lantern-set";
    s.put(set, "fact", &item("What?", "reply has \"x\"", Some(set)))
        .unwrap();
    struct Right;
    impl Runner for Right {
        fn answer(&self, _: &Task<'_>) -> Result<Answer, RunFailed> {
            Ok(Answer {
                reply: "x".into(),
                cost_usd: 0.0,
            })
        }
    }
    assert_eq!(s.score(set, &Right).unwrap_err(), SealedError::Withheld);
}

#[test]
fn the_config_names_the_guard_and_the_store() {
    let mut cfg = crate::Config::example();
    assert!(
        guard_of(&cfg).is_none() && open(&cfg).is_none(),
        "unset: no sealed set"
    );
    let d = tempfile::tempdir().unwrap();
    cfg.self_improve.sealed_dir = Some(d.path().join("s").to_string_lossy().into_owned());
    assert!(!guard_of(&cfg).unwrap().is_none());
    assert!(open(&cfg).is_some());
}

/// The strings of the first tuple `name = (` .. `)` in the keel guard's source.
fn guard_tuple(src: &str, name: &str) -> Vec<String> {
    let after = src
        .split_once(&format!("\n{name} = ("))
        .unwrap_or_else(|| panic!("scripts/keel-guard.py has no {name}"))
        .1;
    let body = after.split_once("\n)").map_or(after, |(b, _)| b);
    let body = body.split_once(')').map_or(body, |(b, _)| b);
    body.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

#[test]
fn the_keel_guard_holds_every_keel_path() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = std::fs::read_to_string(root.join("scripts/keel-guard.py")).unwrap();
    let files = guard_tuple(&src, "KEEL_FILES");
    let prefixes = guard_tuple(&src, "KEEL_PREFIXES");
    assert!(!files.is_empty() && !prefixes.is_empty(), "{files:?}");
    let missing: Vec<&str> = KEEL_PATHS
        .iter()
        .copied()
        .filter(|p| {
            if p.ends_with('/') {
                !prefixes.iter().any(|k| k == p)
            } else {
                !files.iter().any(|k| k == p)
            }
        })
        .collect();
    assert!(
        missing.is_empty(),
        "scripts/keel-guard.py's KEEL_FILES / KEEL_PREFIXES lack: {missing:?}"
    );
}
