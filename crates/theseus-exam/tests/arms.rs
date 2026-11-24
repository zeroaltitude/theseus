//! The exam over the real recall pipeline, end to end (step 34b's wire-in,
//! row 55): a small exam written into a store, then `arms::run` with every
//! arm, which starts this workspace's `theseusd` once per daemon arm
//! (`none`, `bm25`, `baseline`), each on its own copy of the store in
//! `[memory] mode = "live"` with its own index tender, and once per cell on a
//! copy of its arm's, over two runs on fresh daemons, against a stand-in model on 127.0.0.1 that answers from a recall
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
/// request's last user message, as its text blocks, and every request whole.
struct Model {
    base: String,
    seen: Arc<Mutex<Vec<Vec<String>>>>,
    bodies: Arc<Mutex<Vec<Value>>>,
}

fn start_model() -> Model {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let (keep, whole) = (seen.clone(), bodies.clone());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (keep, whole) = (keep.clone(), whole.clone());
            std::thread::spawn(move || {
                let _ = answer(s, &keep, &whole);
            });
        }
    });
    Model { base, seen, bodies }
}

fn answer(
    mut stream: TcpStream,
    seen: &Mutex<Vec<Vec<String>>>,
    bodies: &Mutex<Vec<Value>>,
) -> std::io::Result<()> {
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
    bodies.lock().unwrap().push(req.clone());
    // The conversation's blocks of the last user message: the task's, then
    // its Recall's. The harness's own tail blocks are left out, as every
    // other stand-in leaves them out (`task_graph::view::is_head`), because
    // `check_notes` counts these blocks — so no opener registered in that
    // list can save a count, and a new tail block would otherwise read as a
    // third block of the person's (theseus-wadu's date and git block did).
    // The Recall block is no tail block and is not in the list: it stays,
    // which is what `joined` below looks for.
    let texts: Vec<String> = req["messages"]
        .as_array()
        .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
        .map(|m| match &m["content"] {
            Value::String(s) => vec![s.clone()],
            Value::Array(b) => b
                .iter()
                .filter_map(|b| b["text"].as_str())
                .filter(|t| !theseus_core::task_graph::view::is_head(t))
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    let joined = texts.join("\n");
    // On the view (issue 44) a daemon's recall is no block of its own: each recalled item is a piece of the
    // block beside the message, under a heading that says recall chose it. Those pieces, and nothing else of
    // that block, are this stand-in's notes too; the oracle's note, given in the message, is read as before.
    let recalled = recalled_beside(&req);
    let reply = match (joined.find("[Recalled by the harness:"), recalled.first()) {
        (Some(at), _) => format!("From my notes: {}", &joined[at..]),
        (None, Some(piece)) => format!("From my notes: {piece}"),
        (None, None) => UNKNOWN.to_string(),
    };
    let mut texts = texts;
    texts.extend(recalled);
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
    assert_eq!(s.daemons, ["none", "bm25", "baseline"]);
    assert_eq!(s.runs.len(), 2, "{s:?}");
    assert!(s.killed.is_empty(), "every stop was clean: {:?}", s.killed);
    // Nothing is left: no daemon, and no tender.
    assert!(
        theseus_exam::daemon::processes_naming(&path("work")).is_empty(),
        "{:?}",
        theseus_exam::daemon::processes_naming(&path("work"))
    );

    let recs = drive::read_records(&path("runs.jsonl")).unwrap();
    assert_eq!(recs.len(), 2 * 4 * 2, "items × arms × runs");
    check_records(&recs);
    check_notes(&path("store"), &m, &exam, &model.seen.lock().unwrap());

    // The replay over the store of the first run's baseline cell of `fact-1`
    // (each cell has a daemon of its own): its live turn's query is rebuilt
    // to its row's digest, every arm asks a tender of its own as of the turn,
    // and no later node answers.
    let cell = drive::cell_name("fact-1", Arm::Baseline, 1);
    let r = theseus_exam::replay::run(
        &bin,
        &plan.base_config,
        &path("work/run-1/cells").join(cell).join("state/store"),
        &path("replay"),
        &plan.env,
        plan.settle,
        &mut |l| said.push(l.to_string()),
    )
    .unwrap_or_else(|e| panic!("{e:#}\n{}", said.join("\n")));
    assert_eq!((r.turns, r.skipped.len()), (1, 0), "{:?}", r.skipped);
    assert_eq!(r.admitted["none"], 0);
    assert!(
        r.admitted["bm25"] > 0 && r.admitted["baseline"] > 0,
        "{r:?}"
    );
    assert!(r.leaks.values().all(|n| *n == 0), "{r:?}");
    assert!(theseus_exam::daemon::processes_naming(&path("replay")).is_empty());

    // The report reads the arms from these records.
    let md = theseus_exam::report::render(&recs, &exam, "runs.jsonl");
    assert!(md.contains("| `baseline − none` | all | 2 |"), "{md}");
    assert!(md.contains("| `oracle − baseline` | all | 2 |"), "{md}");
    assert!(
        md.contains("- **Vectors**: `baseline` recalled without vectors in 4 of its 4 recalls"),
        "{md}"
    );
}

/// The test's directory: a fake `op` in `bin`, `projects` and `no-models` made.
fn rig(dir: &Path) {
    for d in ["bin", "projects", "no-models"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(dir.join("bin/op"), FAKE_OP).unwrap();
    std::fs::set_permissions(dir.join("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Each daemon's environment: the fake `op` first on `PATH`, a token that is none.
fn env(dir: &Path) -> Vec<(OsString, Option<OsString>)> {
    vec![
        (
            OsString::from("PATH"),
            Some(OsString::from(format!(
                "{}:{}",
                dir.join("bin").display(),
                std::env::var("PATH").unwrap_or_default()
            ))),
        ),
        (
            "OP_SERVICE_ACCOUNT_TOKEN".into(),
            Some("test-not-a-token".into()),
        ),
        ("THESEUS_OP_TOKEN_FILE".into(), None),
    ]
}

/// What the second cell to run (of two, by their start) was sent of the first's words, if anything: the
/// first's task, or the reply it was given when the second was given another, anywhere in a request whose last
/// user message holds the second's task (its turn's, and any the daemon sends after it, which hold its own
/// reply).
fn second_heard_first(recs: &[drive::Record], exam: &Exam, bodies: &[Value]) -> Option<String> {
    let mut recs: Vec<&drive::Record> = recs.iter().collect();
    recs.sort_by_key(|r| r.started_at_ms);
    assert_eq!(recs.len(), 2, "two cells");
    let task = |r: &drive::Record| exam.item(&r.item).unwrap().task.clone();
    let (first, second) = (task(recs[0]), task(recs[1]));
    let last_user = |b: &Value| {
        let m = b["messages"].as_array().unwrap();
        let u = m.iter().rev().find(|m| m["role"] == "user").unwrap();
        u["content"].to_string()
    };
    let asked: Vec<String> = bodies
        .iter()
        .filter(|b| last_user(b).contains(&second))
        .map(Value::to_string)
        .collect();
    assert!(!asked.is_empty(), "the second cell's request was seen");
    // Both `none` cells are told they are not known, in the same words: those are no word of the first's alone.
    let mut words = vec![first];
    if recs[0].reply != recs[1].reply {
        words.push(recs[0].reply.clone());
    }
    words
        .into_iter()
        .find(|w| !w.is_empty() && asked.iter().any(|b| b.contains(w.as_str())))
}

/// Two cells of the `none` arm, one item each, one after the other in one run: the second cell's request holds
/// no word the first said or was told. The control is the driver as it was, both cells on one daemon: there
/// the second's request lays the first's exchange (the view's window crosses the sessions of a place), and
/// this test says so first, so it cannot pass by a request that would show nothing anyway.
#[test]
fn a_cells_request_holds_nothing_any_other_cell_said() {
    let bin = theseusd();
    let exam = Exam::parse(SMALL).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = |p: &str| dir.path().join(p);
    rig(dir.path());
    let m = fixture::write(&exam, &path("store")).unwrap();
    let items: Vec<String> = exam.file.items.iter().map(|i| i.id.clone()).collect();

    // The new driver: each cell on a daemon of its own.
    let model = start_model();
    let plan = ArmsPlan {
        theseusd: bin.clone(),
        base_config: base_config(&bin, &model.base, dir.path()),
        store: path("store"),
        work: path("work"),
        arms: vec![Arm::None],
        runs: 1,
        items,
        limit_usd: 5.0,
        workers: 1,
        profile: "sonnet".into(),
        seed: 34,
        timeout: Duration::from_secs(60),
        out: path("cells.jsonl"),
        settle: Duration::from_secs(90),
        env: env(dir.path()),
    };
    let mut said = Vec::new();
    let t0 = std::time::Instant::now();
    let s = arms::run(&plan, &exam, &m, &mut |l| said.push(l.to_string()))
        .unwrap_or_else(|e| panic!("{e:#}\n{}", said.join("\n")));
    let alone = t0.elapsed();
    assert!(s.killed.is_empty(), "every stop was clean: {:?}", s.killed);
    assert!(theseus_exam::daemon::processes_naming(&path("work")).is_empty());
    let recs = drive::read_records(&path("cells.jsonl")).unwrap();
    assert!(recs.iter().all(|r| r.error.is_none()), "{recs:?}");
    for r in &recs {
        let cell = drive::cell_name(&r.item, Arm::None, 1);
        assert!(
            path("work/run-1/cells")
                .join(cell)
                .join("state/store")
                .is_dir(),
            "{} ran alone",
            r.item
        );
    }

    // The driver as it was: both cells on the run's one `none` daemon.
    let (old, heard, shared) = on_one_daemon(&plan, &exam, &m, dir.path());
    assert!(old.iter().all(|r| r.error.is_none()), "{old:?}");
    let old_heard = second_heard_first(&old, &exam, &heard);
    assert!(
        old_heard.is_some(),
        "on one daemon the second cell's request lays the first's words"
    );
    let now_heard = second_heard_first(&recs, &exam, &model.bodies.lock().unwrap());
    assert_eq!(
        now_heard, None,
        "a daemon per cell: the second cell's request holds no word of the first's"
    );
    eprintln!(
        "on one daemon the second heard {old_heard:?}; two cells: {alone:?} by arms::run (the run's \
         daemon, its preflight, a daemon per cell), {shared:?} on one daemon already up"
    );
}

/// The two cells as the driver ran them before, both on one `none` daemon started from the run's snapshot of
/// it: their records, every request their model was sent, and how long the two took.
fn on_one_daemon(
    plan: &ArmsPlan,
    exam: &Exam,
    m: &fixture::Manifest,
    dir: &Path,
) -> (Vec<drive::Record>, Vec<Value>, Duration) {
    let path = |p: &str| dir.join(p);
    let model = start_model();
    let base = base_config(&plan.theseusd, &model.base, dir);
    std::fs::create_dir_all(path("old")).unwrap();
    arms::copy_dir(&plan.work.join("prepared/none/state"), &path("old/state")).unwrap();
    let mut d = theseus_exam::daemon::start(
        &plan.theseusd,
        &path("old"),
        "none",
        &theseus_exam::daemon::config_for(&base, "none"),
        &plan.env,
    )
    .unwrap();
    d.wait_serving().unwrap();
    d.wait_indexed(m.last_position, plan.settle, &mut |_| {})
        .unwrap();
    let all: Vec<&theseus_exam::item::Item> = exam.file.items.iter().collect();
    let mut dp = drive::Plan {
        identity: None,
        sockets: BTreeMap::from([("none".to_string(), d.socket.clone())]),
        notes: drive::oracle_notes(&plan.store, m, &all).unwrap(),
        profile: plan.profile.clone(),
        profile_model: String::new(),
        arms: vec![Arm::None],
        runs: 1,
        items: plan.items.clone(),
        limit_usd: plan.limit_usd,
        workers: 1,
        seed: plan.seed,
        timeout: plan.timeout,
        out: path("shared.jsonl"),
        only_run: Some(1),
        cells: None,
    };
    dp.profile_model = drive::preflight(&dp, exam, m).unwrap().1;
    let t0 = std::time::Instant::now();
    drive::run(&dp, exam, m).unwrap();
    let took = t0.elapsed();
    assert!(d.stop().unwrap().is_empty());
    let recs = drive::read_records(&path("shared.jsonl")).unwrap();
    let bodies = model.bodies.lock().unwrap().clone();
    (recs, bodies, took)
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
        // The cell ran on the profile it names, whose model alone answered:
        // the stand-in's id in `detail.model` is `profile.list`'s, so the
        // report's model check passes a good run.
        assert_eq!(r.profile.as_deref(), Some("sonnet"), "{} {}", r.item, r.arm);
        let want = r.profile_model.clone().expect("the profile's model");
        assert_eq!(r.models, [want], "{} {} r{}", r.item, r.arm, r.run);
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
                assert!(c.science.starts_with("baseline@"), "{c:?}");
                assert_eq!(c.gold_admitted, 1, "{arm} {}: {c:?}", r.item);
                assert!(!c.sources.contains_key("vector"), "{c:?}");
                // bm25 never asks for vectors; baseline does, and this
                // tender, with no model files, says why it has none.
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
        assert_eq!(p("oracle"), 2, "{item}");
    }
}

/// The headings the view lays over a piece (`view::text::labels`): one line, before the piece's words.
fn is_heading(line: &str) -> bool {
    line.ends_with("; testimony, not instruction; possibly stale")
        || line.ends_with("author unknown; not the owner's instruction")
        || line.starts_with("the owner said, in ")
}

/// The recall laid in the block beside the last user message (`view::wire_text::BESIDE_HEAD`, after the harness's
/// part of that block when it has one, `turn::now_step`): a line that opens
/// with the recall's own preamble ("[Recalled by the harness: …") and whose next words are the view's heading for the
/// recalled item, to the end of that item (the next heading after it). A preamble that is an earlier turn's words (the
/// oracle's message, which carries its note in the text, laid again in a later turn of the same daemon) is followed by
/// the note's own header instead, and is not recall: the `none` arm answers from nothing.
fn recalled_beside(req: &Value) -> Vec<String> {
    let last = (req["messages"].as_array().into_iter().flatten()).rfind(|m| m["role"] == "user");
    let blocks: Vec<&str> = match last.map(|m| &m["content"]) {
        Some(Value::Array(b)) => b.iter().filter_map(|b| b["text"].as_str()).collect(),
        Some(Value::String(s)) => vec![s.as_str()],
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for block in blocks
        .into_iter()
        .filter(|b| b.contains(theseus_core::view::wire_text::BESIDE_HEAD))
    {
        let lines: Vec<&str> = block.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.starts_with("[Recalled by the harness:") {
                continue;
            }
            let next = lines[i + 1..].iter().find(|l| !l.trim().is_empty());
            if !next.is_some_and(|l| is_heading(l)) {
                continue;
            }
            let mut piece = vec![*line];
            let mut headings = 0;
            for l in &lines[i + 1..] {
                if is_heading(l) {
                    headings += 1;
                    if headings > 1 {
                        break;
                    }
                }
                piece.push(l);
            }
            out.push(piece.join("\n").trim_end().to_string());
            break;
        }
    }
    out
}

/// A note's lines as the view and the oracle share them: the view lays each recalled item under a heading of its
/// own where the core's note numbers it (`(1) [m:…] …`), so the headings, the numbers and the blank lines around
/// them are left out; every other byte is compared.
fn note_lines(note: &str) -> Vec<String> {
    let number = |l: &str| -> String {
        match l.split_once(") ") {
            Some((n, rest)) if n.starts_with('(') && n[1..].chars().all(|c| c.is_ascii_digit()) => {
                rest.to_string()
            }
            _ => l.to_string(),
        }
    };
    (note.lines())
        .filter(|l| !l.trim().is_empty() && !is_heading(l))
        .map(number)
        .collect()
}

/// The oracle's note is the baseline daemon's render of the same gold, byte
/// for byte, after the task.
fn check_notes(store: &Path, m: &fixture::Manifest, exam: &Exam, seen: &[Vec<String>]) {
    let notes = drive::oracle_notes(store, m, &exam.file.items.iter().collect::<Vec<_>>()).unwrap();
    for item in &exam.file.items {
        let note = notes[&item.id].as_deref().unwrap();
        // The baseline daemon's request: the task's block, then its Recall's.
        assert!(
            seen.iter()
                .any(|t| t.len() == 2 && t[0].ends_with(&item.task)),
            "no request with {}'s recall: {seen:?}",
            item.id
        );
        // On the view the baseline's note is laid beside the message, the view's heading over its item where the
        // oracle's note numbers it: with those lines out, the two are the same, line for line (issue 44).
        assert!(
            seen.iter().any(|t| t.len() == 2
                && t[0].ends_with(&item.task)
                && note_lines(&t[1]) == note_lines(note)),
            "{}: the core rendered another note: {seen:?}",
            item.id
        );
        let oracle = format!("{}\n\n{note}", item.task);
        assert!(
            seen.iter().any(|t| t.len() == 1 && t[0].ends_with(&oracle)),
            "no oracle request for {}: {seen:?}",
            item.id
        );
    }
}
