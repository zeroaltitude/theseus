//! One journey across every surface on a real daemon (theseus-ypnxt.8, theseus-h9tu1): the owner
//! starts a task in the Discord DM and another from the CLI; both appear, ask, are answered from a
//! surface other than the one that shows them, are joined, paused, end, and are found again.
//!
//! The daemon is this build's. The model is the scripted `theseus-sim fake-model` (a child), Discord
//! the in-process stand-in (its REST and its gateway), the CLI the real `theseus`, and a bare
//! protocol client watches the work board for how fast a change arrives. Every time is taken from
//! one clock, the rig's.
//!
//! What this file does not run: the herdr steps (a tab per task, `blocked` while one asks, marked
//! done at the end). The scripted herdr is a model inside `crates/theseus/tests/herdr_up.rs`, and a
//! real herdr (a scratch one, with its own socket and config) ran them on 2026-10-18 (the notes of
//! theseus-h9tu1). The chat's `/task new` is typed into the real TUI on a terminal of a private tmux
//! server (`a_task_started_in_the_chat_has_a_short_id_and_a_title`).
//!
//! The steps another row builds are ignored tests that assert that row's contract and name the
//! row that removes the ignore (CLOUD_TASK.md, contract C8).

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{table, Served};
use serde_json::{json, Value};
use theseus_sim::fake_discord::{
    FakeDiscord, FakeThread, Guild, Msg, Pressed, Typed, BOT_ID, DEFAULT_GUILD,
};

const ANA: u64 = 900_000_000_000_000_101;
const LAB: u64 = 900_000_000_000_000_010;
/// Ana's DM with the bot: the stand-in's channel for a DM is the user's id plus one.
const DM: u64 = ANA + 1;
const KEY_VAR: &str = "THESEUS_JOURNEY_TEST_KEY";
const QUAY: &str = "Survey the north quay.";
const GULLS: &str = "Count the gulls";
const PAUSE: &str = "\u{23f8}\u{fe0f}";
const RESUME: &str = "\u{25b6}\u{fe0f}";

/// The scripted model. The DM's request starts the quay task, which asks which pace, then runs a job
/// that waits for the `join` gate (a working task the owner joins), then counts. The join's reply
/// runs a job that waits for the `pause` gate (a working task the owner pauses). The CLI's task asks
/// once and ends. A task's prompt quotes the message that started it, and the stand-in matches on a
/// turn's last plain text, so the DM's own turn is the one that begins `[Discord DM`. The gates are
/// files the test creates: a wait for one is in the daemon's job, not in a timer, so a slow machine
/// moves no window.
fn rules(gates: &std::path::Path) -> Value {
    let ask = |q: &str| {
        json!({"calls": [{"name": "owner_ask", "input": {"question": q, "default": 1, "options":
            [{"label": "Careful"}, {"label": "Quick"}, {"label": "Skip it"}]}}]})
    };
    let wait_for = |gate: &str| {
        let script = format!(
            "while [ ! -e {}/{gate} ]; do sleep 0.05; done",
            gates.display()
        );
        json!({"calls": [{"name": "proc_run", "input": {"argv": ["sh", "-c", script],
            "timeout_secs": 600}}]})
    };
    json!([
        {"when": "[Discord DM", "steps": [
            {"calls": [{"name": "task_create", "input": {"brief": QUAY,
                "notify_done": true, "arrangement": {"pieces": [
                    {"quote": "Please start a quay task to survey the north quay.", "role": "objective"}]}}}]},
            {"text": "Started the quay survey."}]},
        {"when": "Skip berth nine", "steps": [
            wait_for("pause"),
            {"text": "Noted: berth nine is skipped. Quay surveyed: four berths."}]},
        {"when": "Please start a quay task", "steps": [
            ask("Which pace for the quay survey?"),
            wait_for("join"),
            {"text": "Berths counted."}]},
        {"when": GULLS, "steps": [
            ask("Which gulls first?"),
            {"text": "Gulls counted: nine."}]}
    ])
}

/// `theseus-sim fake-model` as a child, killed when dropped. It writes each request it gets to
/// `<dir>/requests`.
struct Model {
    child: Child,
    base: String,
}

impl Model {
    fn start(dir: &std::path::Path) -> Self {
        let sim = PathBuf::from(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus-sim");
        let rules_path = dir.join("rules.json");
        std::fs::create_dir_all(dir.join("gates")).unwrap();
        std::fs::write(&rules_path, rules(&dir.join("gates")).to_string()).unwrap();
        let mut child = Command::new(sim)
            .args(["fake-model", "--addr", "127.0.0.1:0", "--dump"])
            .arg(dir.join("requests"))
            .arg("--rules")
            .arg(&rules_path)
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawning theseus-sim fake-model (build -p theseus-sim first)");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let base = line
            .trim()
            .strip_prefix("fake model on ")
            .unwrap_or_else(|| panic!("fake-model said {line:?}"))
            .to_string();
        Self { child, base }
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// ana owns the guild; `#lab` is private to her.
fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA])
}

fn bindings() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\nprivate = true\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The factor `THESEUS_JOURNEY_SLOWDOWN` (default 1) puts on every time bound, so a run on a starved
/// machine can show the order of things held while their latency did not. The bounds are the
/// brief's; the multiplier is only for finding out which kind of failure a loaded run is.
fn slack(ms: u64) -> u64 {
    let k: u64 = std::env::var("THESEUS_JOURNEY_SLOWDOWN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    ms * k
}

/// How long `f` took to become true, polled every 5 ms; it fails the test past `limit_ms`.
fn within(what: &str, limit_ms: u64, mut f: impl FnMut() -> bool) -> u64 {
    let limit_ms = slack(limit_ms);
    let t0 = Instant::now();
    loop {
        if f() {
            return t0.elapsed().as_millis() as u64;
        }
        assert!(
            t0.elapsed() < Duration::from_millis(limit_ms),
            "no {what} within {limit_ms} ms"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A bare protocol client on `work.watch`: when each `work.changed` was committed (its `at_ms`) and
/// when this client read it, both on the machine's clock.
struct Watcher {
    seen: Arc<Mutex<Vec<(u64, u64)>>>,
}

impl Watcher {
    fn start(sock: PathBuf) -> Self {
        let seen: Arc<Mutex<Vec<(u64, u64)>>> = Arc::default();
        let log = seen.clone();
        let mut s = UnixStream::connect(&sock).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "work.watch",
            "params": {"interactive": false}});
        s.write_all(format!("{req}\n").as_bytes()).unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(&mut s).lines() {
                let Ok(line) = line else { return };
                let at = now_ms();
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if v["method"] == "work.changed" {
                    if let Some(c) = v["params"]["at_ms"].as_u64() {
                        log.lock().unwrap().push((c, at));
                    }
                }
            }
        });
        Self { seen }
    }

    /// The receipts' delays in ms, sorted.
    fn delays(&self) -> Vec<u64> {
        let mut d: Vec<u64> = self
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|(c, r)| r.saturating_sub(*c))
            .collect();
        d.sort_unstable();
        d
    }
}

struct Rig {
    s: Served,
    fake: Arc<FakeDiscord>,
    clock: Instant,
    scratch: tempfile::TempDir,
    _model: Model,
    /// Each step's name and the rig's clock when it ended, in ms.
    timeline: Mutex<Vec<(String, u64)>>,
}

impl Rig {
    fn new() -> Self {
        let fake = FakeDiscord::start_with_gateway();
        fake.set_guild(guild());
        let gateway = fake.gateway().unwrap().addr.clone();
        let scratch = tempfile::tempdir().unwrap();
        let model = Model::start(scratch.path());
        let (base, rest) = (model.base.clone(), fake.addr.clone());
        let s = Served::start_with(
            |dir| {
                std::fs::create_dir_all(dir.join("state")).unwrap();
                std::fs::write(dir.join("state/bindings.toml"), bindings()).unwrap();
            },
            move |t| {
                table(t, "model").insert("api_base".into(), base.clone().into());
                for (_, p) in table(t, "providers").iter_mut() {
                    p.as_table_mut()
                        .unwrap()
                        .insert("api_base".into(), base.clone().into());
                }
                for (_, v) in table(t, "secrets").iter_mut() {
                    *v = format!("env:{KEY_VAR}").into();
                }
                let d = table(t, "discord");
                d.insert("enabled".into(), true.into());
                d.insert("rest_proxy".into(), rest.into());
                d.insert("gateway_proxy".into(), format!("ws://{gateway}").into());
                d.insert("edit_interval_ms".into(), 250.into());
                d.insert("ping_window_secs".into(), 0.into());
                d.insert("forum".into(), true.into());
                d.insert("task_channels".into(), true.into());
                d.insert("live_board".into(), true.into());
                table(t, "places").insert("owner".into(), vec![format!("discord:{ANA}")].into());
            },
            |cmd| {
                cmd.env(KEY_VAR, "tv-journey-test-6e1a");
            },
        );
        Self {
            s,
            fake,
            clock: Instant::now(),
            scratch,
            _model: model,
            timeline: Mutex::default(),
        }
    }

    /// Note that step `name` ended now.
    fn mark(&self, name: &str) {
        let t = self.clock.elapsed().as_millis() as u64;
        eprintln!("[{t:>6} ms] {name}");
        self.timeline.lock().unwrap().push((name.into(), t));
    }

    fn cli(&self, args: &[&str]) -> (i32, String, String) {
        self.cli_in(args, "")
    }

    fn cli_in(&self, args: &[&str], stdin: &str) -> (i32, String, String) {
        let bin = PathBuf::from(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus");
        let mut child = Command::new(bin)
            .arg("--socket")
            .arg(self.s.path("sock"))
            .args(args)
            .env("XDG_STATE_HOME", self.s.path("xdg-state"))
            .env("COLUMNS", "200")
            .env("NO_COLOR", "1")
            .env_remove("THESEUS_SESSION")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// The tasks `task.list` holds.
    fn tasks(&self) -> Vec<Value> {
        let l = self.s.call("task.list", json!({})).unwrap();
        l["tasks"].as_array().cloned().unwrap_or_default()
    }

    fn task(&self, title: &str) -> Option<Value> {
        self.tasks().into_iter().find(|t| t["title"] == title)
    }

    /// The short id of the task titled `title`, once it exists.
    fn short(&self, title: &str) -> String {
        within(&format!("task {title}"), 10_000, || {
            self.task(title).is_some()
        });
        self.task(title).unwrap()["short"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// The work board's view of the task with this short id.
    fn view(&self, short: &str) -> Value {
        let w = self
            .s
            .call("work.watch", json!({"interactive": false}))
            .unwrap();
        w["views"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["kind"] == "task" && v["id"].as_str().is_some_and(|i| i.ends_with(short)))
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// Open a gate a job of the script waits at.
    fn open_gate(&self, name: &str) {
        std::fs::write(self.scratch.path().join("gates").join(name), "").unwrap();
    }

    fn bot_messages(&self) -> Vec<Msg> {
        self.fake
            .all_messages()
            .into_iter()
            .filter(|m| m.author == BOT_ID.to_string())
            .collect()
    }

    /// The bot's message whose first text holds `needle`, as it reads now.
    fn latest(&self, needle: &str) -> Option<Msg> {
        self.bot_messages()
            .into_iter()
            .find(|m| m.versions[0].contains(needle))
    }

    /// The pinned task board in the DM, as it reads now.
    fn board(&self) -> String {
        self.fake
            .messages(DM)
            .into_iter()
            .find(|m| m.pinned)
            .and_then(|m| m.versions.last().cloned())
            .unwrap_or_default()
    }

    /// The forum post of the task with this short id.
    fn post(&self, short: &str) -> Option<FakeThread> {
        self.fake
            .threads()
            .into_iter()
            .find(|t| t.name.contains(short))
    }

    /// A post's card, its first message, as it reads now.
    fn card(&self, post: &FakeThread) -> String {
        self.fake
            .messages(post.id)
            .first()
            .and_then(|m| m.versions.last().cloned())
            .unwrap_or_default()
    }

    /// The card of a question, by a word in it.
    fn question_card(&self, word: &str) -> Option<Msg> {
        self.bot_messages()
            .into_iter()
            .find(|m| m.versions[0].contains('❓') && m.versions[0].contains(word))
    }

    /// Type as ana, once the binding has connected to the gateway.
    fn say(&self, channel: Option<u64>, text: &str) {
        within("the binding on the gateway", 20_000, || {
            self.fake
                .say(&Typed {
                    user: ANA,
                    name: "ana",
                    channel,
                    content: text,
                    file: None,
                })
                .is_ok()
        });
    }

    /// The question id `theseus tasks` prints under the task with this short id.
    fn answer_id(&self, short: &str) -> Option<String> {
        let (_, out, _) = self.cli(&["tasks"]);
        let mut lines = out.lines().skip_while(|l| !l.contains(short));
        lines.next()?;
        lines
            .next()?
            .split("theseus answer ")
            .nth(1)?
            .split_whitespace()
            .next()
            .map(String::from)
    }

    /// The board's row of a task: its line, with the heading of the section it is under.
    fn board_row(&self, short: &str) -> Option<(String, String)> {
        let mut heading = String::new();
        for line in self.board().lines() {
            if line.starts_with("**<") && line.ends_with("**") {
                heading = line.trim_matches('*').to_string();
            }
            if line.contains(short) {
                return Some((heading, line.to_string()));
            }
        }
        None
    }

    /// The heading of the board section a task's row is under.
    fn board_section(&self, short: &str) -> Option<String> {
        let mut heading = None;
        for line in self.board().lines() {
            if line.starts_with("**<") && line.ends_with("**") {
                heading = Some(line.trim_matches('*').to_string());
            }
            if line.contains(short) {
                return heading;
            }
        }
        None
    }
}

fn started_short(out: &str) -> String {
    out.split_whitespace().nth(1).unwrap().to_string()
}

fn p95(sorted: &[u64]) -> u64 {
    sorted
        .get((sorted.len() * 95).div_ceil(100).saturating_sub(1))
        .copied()
        .unwrap_or(0)
}

/// What kind of interrupt a bot message is, by its first words.
fn kind_of(text: &str) -> &'static str {
    if text.contains('❓') {
        "question"
    } else if text.contains("📋 **Task") {
        "finish"
    } else if text.contains('❌') && text.contains("Task") {
        "failure"
    } else {
        "chat"
    }
}

/// The journey: the rig, the two tasks, and each step's measurements (ms, by name) for the report.
struct Journey {
    r: Rig,
    watcher: Watcher,
    quay: String,
    gulls: String,
    post: Option<FakeThread>,
    ms: std::collections::BTreeMap<&'static str, u64>,
    /// What the surfaces showed at the steps, for the script's report.
    captures: std::collections::BTreeMap<&'static str, String>,
}

impl Journey {
    fn post(&self) -> &FakeThread {
        self.post.as_ref().expect("step 2 found the post")
    }

    /// Step 1, start: the DM's request starts the quay task; the CLI starts the gulls task.
    fn start() -> Self {
        let r = Rig::new();
        let watcher = Watcher::start(r.s.path("sock"));
        r.say(None, "Please start a quay task to survey the north quay.");
        // The CLI's task goes to the owner's Home, which is the DM's conversation once it exists.
        let quay = r.short(QUAY);
        let (code, out, err) = r.cli(&["task", "start", GULLS]);
        assert_eq!(code, 0, "step 1: theseus task start: {err}");
        let gulls = started_short(&out);
        for s in [&quay, &gulls] {
            assert_eq!(s.len(), 6, "step 1: a short id of six characters: {s}");
        }
        r.mark("1 start");
        Self {
            r,
            watcher,
            quay,
            gulls,
            post: None,
            ms: Default::default(),
            captures: Default::default(),
        }
    }

    /// Step 2, appear: each task is in `theseus tasks`, and the quay task has its forum post, its card
    /// and its row on the pinned board, each within 2 s (2.5 s for the board, which is edited at
    /// most once in `BOARD_EVERY`, 2 s, so a row can follow an edit by just over that).
    fn appear(&mut self) {
        let (r, quay, gulls) = (&self.r, &self.quay, &self.gulls);
        let cli = within("both tasks in theseus tasks", 2000, || {
            let (_, out, _) = r.cli(&["tasks"]);
            out.contains(quay) && out.contains(gulls)
        });
        let post = within("the quay task's forum post", 2000, || {
            r.post(quay).is_some()
        });
        let card = within("its card", 2000, || {
            r.post(quay).is_some_and(|p| r.card(&p).contains(quay))
        });
        let board = within("its row on the pinned board", 2500, || {
            r.board().contains(quay)
        });
        let gulls_post = within("the gulls task's forum post", 2000, || {
            r.post(gulls).is_some()
        });
        let gulls_board = within("its row on the pinned board", 2500, || {
            r.board().contains(gulls)
        });
        self.ms
            .extend([("gulls_post", gulls_post), ("gulls_board", gulls_board)]);
        self.post = r.post(quay);
        self.ms.extend([
            ("cli", cli),
            ("post", post),
            ("card", card),
            ("board", board),
        ]);
        r.mark("2 appear");
    }

    /// Step 3, ask: both tasks have asked. The question is on the Discord card with its buttons, within
    /// 2 s of being asked, and on `theseus tasks` with its `theseus answer ID` line.
    fn ask(&mut self) {
        let r = &self.r;
        let quay_ms = within("the quay question on Discord", 8000, || {
            r.question_card("Which pace").is_some()
        });
        let gulls_ms = within("the gulls question on Discord", 8000, || {
            r.question_card("Which gulls").is_some()
        });
        for (word, short) in [("Which pace", &self.quay), ("Which gulls", &self.gulls)] {
            let card = r.question_card(word).unwrap();
            let labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
            assert_eq!(
                labels,
                ["Careful", "Quick", "Skip it", "Something else"],
                "step 3: the card of {short} offers the three options and one more"
            );
            let asked = r.view(short)["question"]["asked_at_ms"].as_u64().unwrap();
            let late = card.created_ms.saturating_sub(asked);
            assert!(
                late <= slack(2000),
                "step 3: the card of {short} came {late} ms after the question"
            );
            let id = r.answer_id(short).unwrap_or_else(|| {
                panic!("step 3: theseus tasks shows no `theseus answer` under {short}")
            });
            assert!(id.len() >= 4, "step 3: a question id: {id}");
        }
        within("the board's Needs you", 4000, || {
            r.board_section(&self.quay)
                .is_some_and(|s| s.contains("Needs you"))
        });
        // The status line is paced to one write in 10 s, and both tasks ask within a second of
        // starting, so it goes from nothing to "needs you" (a task's moment of "running" is too
        // short to catch).
        let presence = within("presence reading needs you", 15_000, || {
            r.fake.gateway_state().is_some_and(|g| {
                g.presences
                    .iter()
                    .any(|(_, p)| p.to_string().contains("needs you"))
            })
        });
        self.ms.extend([
            ("quay_card", quay_ms),
            ("gulls_card", gulls_ms),
            ("presence", presence),
        ]);
        let (_, tasks, _) = r.cli(&["tasks"]);
        let card = r.question_card("Which pace").unwrap();
        self.captures.extend([
            ("theseus tasks, both asking", tasks),
            (
                "the quay question's card",
                card.versions.join("\n--- edited to ---\n"),
            ),
            ("the pinned board, both asking", r.board()),
        ]);
        r.mark("3 ask");
    }

    /// Step 4, answer elsewhere, cleared everywhere. The quay question is answered from the CLI: within
    /// 1 s the Discord card shows it answered, and the board row leaves "needs you" (the board is
    /// edited at most every 2 s, so it gets 2.5 s). 5. Join, while the quay task works through its
    /// 2.5 s window: the owner types in its post. 4 (again). The gulls question, answered by a
    /// Discord press, is cleared from the CLI.
    fn answer_and_join(&mut self) {
        let r = &self.r;
        let qid = r.answer_id(&self.quay).unwrap();
        let (code, out, err) = r.cli(&["answer", &qid, "2", "--no-wait"]);
        assert_eq!(code, 0, "step 4: theseus answer: {out}{err}");
        let card = within("the card to show the CLI's answer", 1000, || {
            r.latest("Which pace")
                .is_some_and(|m| m.versions.last().unwrap().contains("by the CLI"))
        });
        assert!(
            r.question_card("Which pace").unwrap().buttons.is_empty(),
            "step 4: the answered card has no buttons"
        );
        within(
            "step 5: the task mid-turn, where the owner joins it",
            1000,
            || r.task(QUAY).unwrap()["state"] == "running",
        );
        r.say(Some(self.post().id), "Skip berth nine.");
        let board = within("the board row to leave needs you", 2500, || {
            r.board_section(&self.quay)
                .is_none_or(|s| !s.contains("Needs you"))
        });
        // The gulls question, answered by a Discord press, is cleared from the CLI.
        let gid = r.answer_id(&self.gulls).unwrap();
        let gulls_q = r.question_card("Which gulls").unwrap();
        r.fake
            .press(&Pressed {
                message: &gulls_q.id,
                button: "Quick",
                user: ANA,
                name: "ana",
            })
            .unwrap();
        let cli = within("the CLI to show the press", 1000, || {
            let (_, out, _) = r.cli(&["tasks"]);
            !out.contains(&format!("theseus answer {gid}"))
        });
        let press = within("the gulls card to show the press", 1000, || {
            r.latest("Which gulls")
                .is_some_and(|m| m.versions.last().unwrap().contains("by discord:ana"))
        });
        // The core steers a message into a running task at the task's next loop, so the join waits
        // in the quay task, whose job holds at the gate, and is in its history once the job ends. It
        // is the task's own next loop and not a turn queued behind it: the report carrying the
        // join's answer (step 7) is the proof.
        assert!(
            r.latest("Berths counted").is_none(),
            "step 5: the task has not gone on without the owner's word"
        );
        let released = Instant::now();
        r.open_gate("join");
        let session = r.task(QUAY).unwrap()["task_id"]
            .as_str()
            .unwrap()
            .to_string();
        let join = within("the join in the task's history", 2000, || {
            let h =
                r.s.call("session.history", json!({"session_id": session}))
                    .unwrap();
            h.to_string().contains("Skip berth nine")
        });
        within("a model request carrying the join", 2000, || {
            std::fs::read_dir(r.scratch.path().join("requests"))
                .unwrap()
                .flatten()
                .any(|f| {
                    std::fs::read_to_string(f.path()).is_ok_and(|t| t.contains("Skip berth nine"))
                })
        });
        let request = released.elapsed().as_millis() as u64;
        self.ms.extend([
            ("card_cleared", card),
            ("board_cleared", board),
            ("join", join),
            ("join_request", request),
            ("cli_cleared", cli),
            ("press_card", press),
        ]);
        r.mark("4 answer elsewhere");
        r.mark("5 join");
    }

    /// Step 6, pause and resume: a pause reaction on the card pauses the task, a play reaction resumes it.
    fn pause_and_resume(&mut self) {
        let (r, quay) = (&self.r, &self.quay);
        let post = self.post().id;
        let first = r.fake.messages(post)[0].id.clone();
        r.fake.react(ANA, post, &first, PAUSE).unwrap();
        let pause = within("the view to read paused", 1000, || {
            r.view(quay)["paused"] == true
        });
        within("the card to say paused", 4000, || {
            r.card(&r.post(quay).unwrap()).contains("paused")
        });
        r.fake.react(ANA, post, &first, RESUME).unwrap();
        let resume = within("the view to read running again", 1000, || {
            r.view(quay)["paused"] != true
        });
        r.open_gate("pause");
        self.ms.extend([("pause", pause), ("resume", resume)]);
        r.mark("6 pause and resume");
    }

    /// Step 7, end: both tasks report. The DM gets "📋 Task … finished · title" with the report, and the
    /// forum post ends with its done name and a tag other than its first.
    fn end(&mut self) {
        let (r, quay, gulls) = (&self.r, &self.quay, &self.gulls);
        let end = within("both tasks finished", 25_000, || {
            r.latest(&format!("Task `{quay}` finished")).is_some()
                && r.latest(&format!("Task `{gulls}` finished")).is_some()
        });
        let report = r.latest(&format!("Task `{quay}` finished")).unwrap();
        let quay_text = report.versions[0].clone();
        assert_eq!(report.channel, DM.to_string(), "step 7: reported to the DM");
        assert!(
            report.versions[0].contains(&format!("finished** · {QUAY}"))
                && report.versions[0].contains("Quay surveyed: four berths."),
            "step 7: the report, with the join's answer: {}",
            report.versions[0]
        );
        let report = r.latest(&format!("Task `{gulls}` finished")).unwrap();
        let post_card = r.card(&r.post(quay).unwrap());
        self.captures.extend([
            ("the quay task's report in the DM", quay_text),
            (
                "the gulls task's report in the DM",
                report.versions[0].clone(),
            ),
            ("the quay task's forum card at the end", post_card),
        ]);
        assert!(
            report.versions[0].contains("Gulls counted: nine."),
            "step 7: the report: {}",
            report.versions[0]
        );
        within("the post's final name and tag", 4000, || {
            let p = r.post(quay).unwrap();
            p.name.starts_with('✅') && p.tag_sets.len() >= 2 && p.tags != p.tag_sets[0]
        });
        self.ms.insert("end", end);
        r.mark("7 end");
    }

    /// Step 8, digest: `theseus digest` lists both finished tasks. 9. Find: with nothing open,
    /// `theseus tasks` shows nothing open (that it also points at the ended tasks is the ignored
    /// test below).
    fn digest_and_find(&mut self) {
        let r = &self.r;
        let (code, digest, err) = r.cli(&["digest"]);
        assert_eq!(code, 0, "step 8: {err}");
        for title in [QUAY, GULLS] {
            assert!(
                digest.contains(title) && digest.contains("reported"),
                "step 8: the digest names {title}:\n{digest}"
            );
        }
        self.captures.insert("theseus digest", digest);
        r.mark("8 digest");
        let (_, none, _) = r.cli(&["tasks"]);
        assert!(
            !none.contains("needs you") && !none.contains("working"),
            "step 9: nothing open:\n{none}"
        );
        self.captures.insert("theseus tasks, nothing open", none);
        r.mark("9 find");
    }

    /// h9tu1 3, never more. The board, the cards, the pointers and the forum post are created
    /// silent. Only a question, a failure and a flagged finish ring among the task paths' creates.
    /// The chat's own replies (the DM's turn, and what a task says in its post) ring by the owner's
    /// choice: counted apart, never judged. The spacing of the rings in one conversation is
    /// recorded, not judged: `ping_window_secs` (the 10 s rule) is off by default and the recipe
    /// leaves it off, and when on it can hold a question silent behind a chat reply
    /// (CLOUD_REPORT.md). Returns the rings' counts and the closest two in one conversation.
    fn interrupts(&self) -> Value {
        let all = self.r.bot_messages();
        for m in &all {
            let t = &m.versions[0];
            if t.starts_with("📋 **Tasks**")
                || t.starts_with('🧵')
                || t.starts_with("-# ")
                || m.id == self.post().id.to_string()
            {
                assert!(m.silent(), "h9tu1 3: created silent: {t}");
            }
        }
        let rings: Vec<&Msg> = all.iter().filter(|m| !m.silent()).collect();
        let count = |k: &str| {
            rings
                .iter()
                .filter(|m| kind_of(&m.versions[0]) == k)
                .count()
        };
        let (questions, finishes, failures) =
            (count("question"), count("finish"), count("failure"));
        assert_eq!(failures, 0, "h9tu1 3: no task failed");
        assert_eq!(
            (questions, finishes),
            (2, 2),
            "h9tu1 3: a ring for each question and finish"
        );
        let chat: Vec<String> = rings
            .iter()
            .filter(|m| kind_of(&m.versions[0]) == "chat")
            .map(|m| {
                format!(
                    "{}: {}",
                    m.channel,
                    m.versions[0].chars().take(40).collect::<String>()
                )
            })
            .collect();
        let mut by_channel: std::collections::BTreeMap<&str, Vec<u64>> = Default::default();
        for m in rings.iter().filter(|m| kind_of(&m.versions[0]) != "chat") {
            by_channel
                .entry(m.channel.as_str())
                .or_default()
                .push(m.created_ms);
        }
        let mut closest = u64::MAX;
        for at in by_channel.values_mut() {
            at.sort_unstable();
            closest = at.windows(2).map(|w| w[1] - w[0]).fold(closest, u64::min);
        }
        json!({"questions": questions, "finishes": finishes, "closest_ms": closest, "chat": chat})
    }

    /// h9tu1 6, still fast: a plain turn stays within its frames while the run is on, and a change
    /// reaches a watching client in `p95_limit_ms` at p95 (10 ms in the real-time test, which the
    /// script runs; the gate's run only holds the change to not waiting on a poll).
    fn fast(&self, p95_limit_ms: u64) -> Value {
        let r = &self.r;
        let sid = r.s.call("session.open", json!({})).unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let turn = |text: &str| {
            r.s.call(
                "turn.submit",
                json!({"session_id": sid, "input": text, "author": "test", "attachments": []}),
            )
            .unwrap()
        };
        turn("first");
        let plain = turn("a plain turn");
        let frames = plain["trace"]["attrs"]["frames"].as_u64().unwrap();
        assert!(frames <= 4, "h9tu1 6: a plain turn wrote {frames} frames");
        let delays = self.watcher.delays();
        assert!(delays.len() >= 20, "h9tu1 6: {} changes seen", delays.len());
        let p = p95(&delays);
        assert!(
            p <= slack(p95_limit_ms),
            "h9tu1 6: p95 {p} ms of {delays:?}"
        );
        json!({"frames": frames, "p95_ms": p, "changes": delays.len(), "max_ms": delays.last()})
    }
}

impl Journey {
    /// Steps 1 to 9 and the two measured checks, in order.
    fn run(p95_limit_ms: u64) -> (Self, Value, Value) {
        let mut j = Self::start();
        j.appear();
        j.ask();
        j.answer_and_join();
        j.pause_and_resume();
        j.end();
        j.digest_and_find();
        let interrupts = j.interrupts();
        let fast = j.fast(p95_limit_ms);
        eprintln!(
            "measured (ms): {:?}\ninterrupts: {interrupts}\nfast: {fast}",
            j.ms
        );
        (j, interrupts, fast)
    }

    /// `THESEUS_JOURNEY_REPORT` names a file to write each step's time and the measurements to, as
    /// JSON, for the script's report.
    fn report(&self, interrupts: &Value, fast: &Value) {
        let Ok(path) = std::env::var("THESEUS_JOURNEY_REPORT") else {
            return;
        };
        let steps: Vec<Value> = self
            .r
            .timeline
            .lock()
            .unwrap()
            .iter()
            .map(|(n, t)| json!({"step": n, "at_ms": t}))
            .collect();
        let report = json!({"steps": steps, "ms": self.ms, "interrupts": interrupts, "fast": fast,
            "captures": self.captures});
        std::fs::write(path, report.to_string()).unwrap();
    }
}

/// The journey, step by step. Each step's assertions are named after it.
#[test]
fn the_owner_starts_asks_answers_joins_pauses_ends_and_finds_two_tasks() {
    let (j, interrupts, fast) = Journey::run(100);
    j.report(&interrupts, &fast);
}

/// The journey again, with the change latency held to 10 ms at p95: a measure of real time, which a
/// loaded machine does not meet (the same client reads each change on a starved thread).
#[test]
#[ignore = "real time: scripts/acceptance/journey.sh"]
fn a_change_reaches_a_watching_client_in_ten_ms_at_p95() {
    let (j, interrupts, fast) = Journey::run(10);
    j.report(&interrupts, &fast);
}

/// The gulls task asked, answered, ended: the smallest run that has an ended task.
fn finish_the_gulls(r: &Rig) {
    let (_, out, _) = r.cli(&["task", "start", GULLS]);
    let gulls = started_short(&out);
    within("the gulls question", 10_000, || {
        r.answer_id(&gulls).is_some()
    });
    let id = r.answer_id(&gulls).unwrap();
    r.cli(&["answer", &id, "2", "--no-wait"]);
    within("the gulls task ended", 20_000, || {
        r.task(GULLS).is_some_and(|t| t["state"] == "complete")
    });
}

/// Step 9: with nothing open, `theseus tasks` still points at the ended tasks, and `--ended` lists
/// and searches them (task-history).
#[test]
fn tasks_with_nothing_open_points_at_the_ended_tasks() {
    let r = Rig::new();
    finish_the_gulls(&r);
    let (_, none, _) = r.cli(&["tasks"]);
    assert!(none.contains("theseus tasks --ended"), "{none}");
    let (_, all, _) = r.cli(&["tasks", "--ended"]);
    assert!(all.contains(GULLS), "{all}");
    let (_, found, _) = r.cli(&["tasks", "--ended", "gulls"]);
    assert!(found.contains(GULLS), "{found}");
    let (_, missed, _) = r.cli(&["tasks", "--ended", "quay"]);
    assert!(!missed.contains(GULLS), "{missed}");
}

/// Step 8: the digest names each task with its report line (task-history).
#[test]
fn the_digest_names_each_task_with_its_report_line() {
    let r = Rig::new();
    finish_the_gulls(&r);
    let (_, digest, _) = r.cli(&["digest"]);
    assert!(
        digest
            .lines()
            .any(|l| l.contains(GULLS) && l.contains("Gulls counted: nine.")),
        "{digest}"
    );
}

/// Steps 2 and 3: the one progress line on the board row and in `theseus tasks`
/// (`$S of $L …`, contract C1; progress-words).
#[test]
fn the_progress_line_is_on_the_board_row_and_in_tasks() {
    let r = Rig::new();
    r.say(None, "Please start a quay task to survey the north quay.");
    let quay = r.short(QUAY);
    within("the quay question", 8000, || {
        r.question_card("Which pace").is_some()
    });
    // A row of a task that asks is its question (the board's Needs you, the CLI's `asks: …`);
    // once answered, the task works, and its rows carry the line.
    let qid = r.answer_id(&quay).unwrap();
    r.cli(&["answer", &qid, "2", "--no-wait"]);
    // The row, not its section's heading, carries the line (the join's wait looked for the cost
    // in the heading, which never has it).
    within("the line on the board's working row", 8000, || {
        r.board_row(&quay)
            .is_some_and(|(h, row)| !h.contains("Needs you") && row.contains(" of $25"))
    });
    let (_, tasks, _) = r.cli(&["tasks"]);
    let row = tasks.lines().find(|l| l.contains(&quay)).unwrap();
    assert!(row.contains(" of $25"), "{row}");
}

/// Step 6: `paused` first in the one line, on the board and the card (progress-words), and `theseus
/// task pause|resume` (task-history).
#[test]
fn a_paused_task_says_paused_first_and_the_cli_pauses_and_resumes_it() {
    let r = Rig::new();
    r.say(None, "Please start a quay task to survey the north quay.");
    let quay = r.short(QUAY);
    within("the quay question", 8000, || {
        r.question_card("Which pace").is_some()
    });
    let (code, out, err) = r.cli(&["task", "pause", &quay]);
    assert_eq!(code, 0, "{out}{err}");
    within("paused", 1000, || r.view(&quay)["paused"] == true);
    let post = r.post(&quay).unwrap();
    within("the card's paused line", 4000, || {
        let card = r.card(&post);
        card.contains("paused") && card.contains("$0.03 of $25")
    });
    let (code, out, err) = r.cli(&["task", "resume", &quay]);
    assert_eq!(code, 0, "{out}{err}");
    within("running again", 1000, || r.view(&quay)["paused"] != true);
}

/// Step 7: the report carries a link to the task's post (discord-tasks).
#[test]
fn the_report_links_the_tasks_post() {
    let r = Rig::new();
    r.say(None, "Please start a quay task to survey the north quay.");
    let quay = r.short(QUAY);
    within("the quay question", 8000, || {
        r.question_card("Which pace").is_some()
    });
    let post = r.post(&quay).unwrap();
    let qid = r.answer_id(&quay).unwrap();
    r.cli(&["answer", &qid, "2", "--no-wait"]);
    r.open_gate("join");
    within("the report", 30_000, || {
        r.latest(&format!("Task `{quay}` finished")).is_some()
    });
    let msg = r.latest(&format!("Task `{quay}` finished")).unwrap();
    assert!(
        msg.versions[0].contains(&format!("<#{}>", post.id)),
        "{}",
        msg.versions[0]
    );
}

/// A private tmux server for a real terminal: the chat's TUI draws only on a tty. Killed with it.
struct Tmux {
    name: String,
}

impl Tmux {
    /// None when there is no tmux to run a terminal in.
    fn start(cmd: &[&str], env: &[(&str, &str)]) -> Option<Self> {
        let name = format!("jtui{}-{}", std::process::id(), UNIQ.fetch_add(1, Ordering::Relaxed));
        let mut shell = String::from("env -u THESEUS_SESSION TERM=xterm-256color");
        for (k, v) in env {
            shell.push_str(&format!(" {k}={v}"));
        }
        for c in cmd {
            shell.push_str(&format!(" '{c}'"));
        }
        let ok = Command::new("tmux")
            .args(["-L", &name, "-f", "/dev/null", "new-session", "-d", "-s", "chat"])
            .args(["-x", "140", "-y", "40", &shell])
            .status()
            .is_ok_and(|s| s.success());
        ok.then_some(Self { name })
    }

    fn keys(&self, args: &[&str]) {
        let _ = Command::new("tmux")
            .args(["-L", &self.name, "send-keys", "-t", "chat"])
            .args(args)
            .status();
    }

    fn screen(&self) -> String {
        let out = Command::new("tmux")
            .args(["-L", &self.name, "capture-pane", "-p", "-t", "chat"])
            .output();
        out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.name, "kill-server"])
            .status();
    }
}

static UNIQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A task started with `/task new …` in the chat (chat-start-pause): typed into the real
/// `theseus chat` (the TUI) on a terminal of a private tmux server, against the rig's daemon. The
/// task exists under its title with a short id, and the chat says both. A host with no tmux or no
/// `theseus-tui` beside the CLI has no terminal to run it in, and says so.
#[test]
fn a_task_started_in_the_chat_has_a_short_id_and_a_title() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus");
    if !bin.with_file_name("theseus-tui").exists() {
        eprintln!("skipped: no theseus-tui beside {}", bin.display());
        return;
    }
    let r = Rig::new();
    let sock = r.s.path("sock");
    let state = r.s.path("xdg-state");
    let Some(tmux) = Tmux::start(
        &[&bin.to_string_lossy(), "--socket", &sock.to_string_lossy(), "chat"],
        &[("XDG_STATE_HOME", &state.to_string_lossy())],
    ) else {
        eprintln!("skipped: no tmux to give the chat a terminal");
        return;
    };
    // The chat opens its conversation, then takes the command; ask again while it says it is not open.
    let mut sent = None::<Instant>;
    within("the task started from the chat", 30_000, || {
        if r.task("Count the terns").is_some() {
            return true;
        }
        let waited = sent.is_none_or(|t| t.elapsed() > Duration::from_secs(3));
        if waited && !tmux.screen().contains("started Count the terns") {
            tmux.keys(&["C-u"]);
            tmux.keys(&["-l", "/task new Count the terns"]);
            tmux.keys(&["Enter"]);
            sent = Some(Instant::now());
        }
        std::thread::sleep(Duration::from_millis(250));
        false
    });
    let t = r.task("Count the terns").expect("the task exists");
    let short = t["short"].as_str().unwrap().to_string();
    within("the chat says what it started", 10_000, || {
        let screen = tmux.screen();
        screen.contains("started Count the terns") && screen.contains(&short)
    });
}
