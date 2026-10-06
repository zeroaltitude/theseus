//! The exam over the real recall pipeline, end to end (step 34b's wire-in,
//! row 55): a small exam written into a store, then `arms::run` with every
//! arm, which starts this workspace's `theseusd` once per daemon arm
//! (`none`, `bm25`, `baseline`), each on its own copy of the store in
//! `[memory] mode = "live"` with its own index tender, over two runs on fresh
//! daemons, against a stand-in model on 127.0.0.1 that answers from a recall
//! note when its request has one, and otherwise says it does not know.
//!
//! What it proves:
//! - the arms differ because each daemon runs the arm its config names:
//!   `none` fails what `bm25` and `baseline` pass through recall, and every
//!   recall row names its daemon's arm and asks for its sources (`bm25` never
//!   vectors; `baseline` asks for them, and this tender, without its model's
//!   files, says so in `skipped`);
//! - the oracle's note is, byte for byte, the `Recall` the `baseline` daemon
//!   rendered for the same gold, and sits where it sits, after the task;
//! - no daemon, and no tender, is left running;
//! - the replay rebuilds the baseline daemon's live turns from their rows,
//!   and recomputes every arm over them as of each turn.
//!
//! The daemon is the `theseusd` beside this test's binary (a workspace test
//! run builds it, and `theseus-index` beside it), or `THESEUS_EXAM_THESEUSD`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_exam::arms::{self, ArmsPlan};
use theseus_exam::drive::{self, Arm};
use theseus_exam::fixture;
use theseus_exam::item::Exam;

/// Two facts, each in a session of its own, in words the other never uses.
const SMALL: &str = r#"
version = "exam-arms"
utc_offset_min = -420

[[item]]
id = "fact-1"
family = "fact"
task = "Which port does the plover dashboard listen on? Just the number."
gold = ["a.1"]
check = '''
reply has word "7519"
'''
[[item.session]]
key = "a"
place = "discord DM"
[[item.session.node]]
at = "2026-09-14 10:02"
who = "zeroaltitude"
text = "The plover dashboard moved off its old port: it listens on 7519 now."

[[item]]
id = "fact-2"
family = "fact"
task = "When does the kestrel tower start its nightly backup? Just the time."
gold = ["a.1"]
check = '''
reply has "03:40"
'''
[[item.session]]
key = "a"
place = "discord DM"
[[item.session.node]]
at = "2026-08-02 21:15"
who = "zeroaltitude"
text = "The kestrel tower's nightly backup starts at 03:40 since the disk swap."
"#;

/// A fake `op`: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

/// What the stand-in says when its request carries no recall note.
const UNKNOWN: &str = "I have no record of that.";

fn theseusd() -> PathBuf {
    if let Some(p) = std::env::var_os("THESEUS_EXAM_THESEUSD") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().unwrap();
    let p = exe
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("theseusd");
    assert!(
        p.is_file(),
        "no theseusd at {}: build it first, or set THESEUS_EXAM_THESEUSD",
        p.display()
    );
    assert!(
        p.with_file_name("theseus-index").is_file(),
        "no theseus-index beside {}: build it first",
        p.display()
    );
    p
}

/// The stand-in model: the Messages API, streamed, one response a
/// connection. A request whose last user message holds a recall note is
/// answered with the note; any other with [`UNKNOWN`]. It keeps every
/// request's last user message, as its text blocks.
struct Model {
    base: String,
    seen: Arc<Mutex<Vec<Vec<String>>>>,
}

fn start_model() -> Model {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let keep = seen.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let keep = keep.clone();
            std::thread::spawn(move || {
                let _ = answer(s, &keep);
            });
        }
    });
    Model { base, seen }
}

fn answer(mut stream: TcpStream, seen: &Mutex<Vec<Vec<String>>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut r = BufReader::new(stream.try_clone()?);
    let mut len = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let texts: Vec<String> = req["messages"]
        .as_array()
        .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
        .map(|m| match &m["content"] {
            Value::String(s) => vec![s.clone()],
            Value::Array(b) => b
                .iter()
                .filter_map(|b| b["text"].as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    let joined = texts.join("\n");
    let reply = match joined.find("[Recalled by the harness:") {
        Some(at) => format!("From my notes: {}", &joined[at..]),
        None => UNKNOWN.to_string(),
    };
    seen.lock().unwrap().push(texts);
    let events = [
        json!({"type": "message_start", "message": {"id": "msg_exam", "type": "message", "role": "assistant", "model": "claude-sonnet-5-5", "content": [], "usage": {"input_tokens": 60, "output_tokens": 1}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": reply}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 12}}),
        json!({"type": "message_stop"}),
    ];
    let mut out = String::from(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
    );
    for e in events {
        out.push_str(&format!(
            "event: {}\ndata: {e}\n\n",
            e["type"].as_str().unwrap_or("")
        ));
    }
    stream.write_all(out.as_bytes())?;
    stream.flush()
}

/// The template, safe to serve in a test: every secret a reference to the
/// vault `Test` (the fake `op` answers), the model on the stand-in, the
/// index's model files in an empty directory, a recall deadline a loaded
/// machine meets, and one item a pack. `arms::run` sets `[memory]`'s mode and arm itself.
fn base_config(bin: &Path, model: &str, dir: &Path) -> toml::Table {
    let out = Command::new(bin).args(["example-config"]).output().unwrap();
    let mut t: toml::Table = String::from_utf8(out.stdout).unwrap().parse().unwrap();
    fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
        t.entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
    }
    table(&mut t, "model").insert("api_base".into(), model.into());
    for (_, p) in table(&mut t, "providers").iter_mut() {
        p.as_table_mut()
            .unwrap()
            .insert("api_base".into(), model.into());
    }
    let secrets = table(&mut t, "secrets");
    let names: Vec<String> = secrets
        .keys()
        .filter(|k| k.as_str() != "github_token")
        .cloned()
        .collect();
    secrets.clear();
    for n in names {
        secrets.insert(n.clone(), format!("op://Test/{n}/credential").into());
    }
    let index = table(&mut t, "index");
    index.insert("enabled".into(), true.into());
    index.insert(
        "weights_dir".into(),
        dir.join("no-models").display().to_string().into(),
    );
    // One item a pack, so each recall admits its best hit alone and its
    // note can be compared with the oracle's whole.
    let memory = table(&mut t, "memory");
    memory.insert("recall_deadline_ms".into(), 5000.into());
    memory.insert("recall_max_items".into(), 1.into());
    table(&mut t, "tools").insert(
        "projects_dir".into(),
        dir.join("projects").display().to_string().into(),
    );
    t
}

#[test]
fn the_exam_runs_each_arm_on_a_daemon_of_its_own() {
    let bin = theseusd();
    let exam = Exam::parse(SMALL).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = |p: &str| dir.path().join(p);
    for d in ["bin", "projects", "no-models"] {
        std::fs::create_dir_all(path(d)).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path("bin/op"), FAKE_OP).unwrap();
    std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let m = fixture::write(&exam, &path("store")).unwrap();
    let model = start_model();

    let plan = ArmsPlan {
        theseusd: bin.clone(),
        base_config: base_config(&bin, &model.base, dir.path()),
        store: path("store"),
        work: path("work"),
        arms: Arm::ALL.to_vec(),
        runs: 2,
        items: Vec::new(),
        limit_usd: 5.0,
        workers: 2,
        profile: "sonnet".into(),
        seed: 34,
        timeout: Duration::from_secs(60),
        out: path("runs.jsonl"),
        settle: Duration::from_secs(90),
        env: vec![
            (
                OsString::from("PATH"),
                Some(OsString::from(format!(
                    "{}:{}",
                    path("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ))),
            ),
            (
                "OP_SERVICE_ACCOUNT_TOKEN".into(),
                Some("test-not-a-token".into()),
            ),
            ("THESEUS_OP_TOKEN_FILE".into(), None),
        ],
    };
    let mut said = Vec::new();
    let s = arms::run(&plan, &exam, &m, &mut |l| said.push(l.to_string()))
        .unwrap_or_else(|e| panic!("{e:#}\n{}", said.join("\n")));
    assert_eq!(
        s.daemons,
        ["none", "bm25", "baseline", "+retention", "+activation"]
    );
    assert_eq!(s.runs.len(), 2, "{s:?}");
    assert!(s.killed.is_empty(), "every stop was clean: {:?}", s.killed);
    // Nothing is left: no daemon, and no tender.
    assert!(
        theseus_exam::daemon::processes_naming(&path("work")).is_empty(),
        "{:?}",
        theseus_exam::daemon::processes_naming(&path("work"))
    );

    let recs = drive::read_records(&path("runs.jsonl")).unwrap();
    assert_eq!(recs.len(), 2 * 6 * 2, "items × arms × runs");
    check_records(&recs);
    check_notes(&path("store"), &m, &exam, &model.seen.lock().unwrap());

    // The replay over the first run's baseline daemon's store: its two live
    // turns' queries are rebuilt to their rows' digests, every arm asks a
    // tender of its own as of each turn, and no later node answers.
    let r = theseus_exam::replay::run(
        &bin,
        &plan.base_config,
        &path("work/run-1/baseline/state/store"),
        &path("replay"),
        &plan.env,
        plan.settle,
        &mut |l| said.push(l.to_string()),
    )
    .unwrap_or_else(|e| panic!("{e:#}\n{}", said.join("\n")));
    assert_eq!((r.turns, r.skipped.len()), (2, 0), "{:?}", r.skipped);
    assert_eq!(r.admitted["none"], 0);
    assert!(
        r.admitted["bm25"] > 0 && r.admitted["baseline"] > 0,
        "{r:?}"
    );
    assert!(r.leaks.values().all(|n| *n == 0), "{r:?}");
    assert!(theseus_exam::daemon::processes_naming(&path("replay")).is_empty());

    // The report reads the five arms from these records.
    let md = theseus_exam::report::render(&recs, &exam, "runs.jsonl");
    assert!(md.contains("| `baseline − none` | all | 2 |"), "{md}");
    assert!(md.contains("| `oracle − baseline` | all | 2 |"), "{md}");
    assert!(md.contains("| `+retention − baseline` | all | 2 |"), "{md}");
    assert!(
        md.contains("- **Vectors**: `baseline` recalled without vectors in 4 of its 4 recalls"),
        "{md}"
    );
}

/// Every cell has its verdict; `none` and `oracle` asked the index nothing;
/// each recall row names its daemon's arm and sources and admitted the gold;
/// and `none` fails what `bm25` and `baseline` pass.
fn check_records(recs: &[drive::Record]) {
    let mut passes: BTreeMap<(String, String), u32> = BTreeMap::new();
    for r in recs {
        assert!(
            r.error.is_none(),
            "{} {} r{}: {:?}",
            r.item,
            r.arm,
            r.run,
            r.error
        );
        *passes.entry((r.arm.clone(), r.item.clone())).or_default() +=
            u32::from(r.pass == Some(true));
        match r.arm.as_str() {
            "none" | "oracle" => assert!(
                r.recall.is_empty(),
                "{} asks the index nothing: {:?}",
                r.arm,
                r.recall
            ),
            arm => {
                assert_eq!(r.recall.len(), 1, "{arm} {}: {:?}", r.item, r.recall);
                let c = &r.recall[0];
                assert_eq!(
                    (c.arm.as_str(), c.mode.as_str(), c.outcome.as_str()),
                    (arm, "live", "ran")
                );
                // `+retention` and `+activation` run their own sciences (32a,
                // 32b), baseline's in all but the rank or the spread.
                let science = if arm == "+retention" {
                    "retention@"
                } else if arm == "+activation" {
                    "activation@"
                } else {
                    "baseline@"
                };
                assert!(c.science.starts_with(science), "{c:?}");
                assert_eq!(c.gold_admitted, 1, "{arm} {}: {c:?}", r.item);
                assert!(!c.sources.contains_key("vector"), "{c:?}");
                // bm25 never asks for vectors; baseline, +retention and
                // +activation do, and this tender, with no model files, says
                // why it has none.
                assert_eq!(c.skipped.contains_key("vector"), arm != "bm25", "{c:?}");
            }
        }
    }
    for item in ["fact-1", "fact-2"] {
        let p = |arm: &str| {
            passes
                .get(&(arm.to_string(), item.to_string()))
                .copied()
                .unwrap_or(0)
        };
        assert_eq!(p("none"), 0, "{item}: none has no past to read");
        assert_eq!(p("bm25"), 2, "{item}");
        assert_eq!(p("baseline"), 2, "{item}: baseline passes what none fails");
        assert_eq!(p("+retention"), 2, "{item}");
        assert_eq!(p("+activation"), 2, "{item}");
        assert_eq!(p("oracle"), 2, "{item}");
    }
}

/// The oracle's note is the baseline daemon's render of the same gold, byte
/// for byte, after the task.
fn check_notes(store: &Path, m: &fixture::Manifest, exam: &Exam, seen: &[Vec<String>]) {
    let notes = drive::oracle_notes(store, m, &exam.file.items.iter().collect::<Vec<_>>()).unwrap();
    for item in &exam.file.items {
        let note = notes[&item.id].as_deref().unwrap();
        // The baseline daemon's request: the task's block, then its Recall's
        // (`+activation`'s may admit more, so any one of them).
        assert!(
            seen.iter().any(|t| t.len() == 2 && t[0] == item.task),
            "no request with {}'s recall: {seen:?}",
            item.id
        );
        assert!(
            seen.iter()
                .any(|t| t.len() == 2 && t[0] == item.task && t[1] == note),
            "{}: the core rendered another note: {seen:?}",
            item.id
        );
        let oracle = format!("{}\n\n{note}", item.task);
        assert!(
            seen.iter().any(|t| t.len() == 1 && t[0] == oracle),
            "no oracle request for {}: {seen:?}",
            item.id
        );
    }
}
