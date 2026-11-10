//! A stop's jobs, with the real `theseusd` (review 2's S2, theseus-bzq):
//! `execution.stop` ends a session's jobs together, every SIGTERM first, one
//! shared grace, then SIGKILL for the stragglers, and waits on the runtime's
//! timer, so no worker is held while it waits. The daemon runs on one worker
//! thread (`TOKIO_WORKER_THREADS=1`): a stop that held its worker would hold
//! every other request with it.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A job that outlives SIGTERM: its shell traps it and carries on (a
/// `sleep` the signal ends is followed by the next), so only SIGKILL ends
/// it. The trap takes the signal's time in the shell itself, with no fork
/// (bash's `$EPOCHREALTIME`, in microseconds), ignores SIGTERM from then
/// on, and writes the time to `job-<i>.term.tmp`, renamed `job-<i>.term`
/// once whole (theseus-y0lm). Its wrapper's stop SIGTERMs every process it
/// meets in the tree through the grace, one born after the first signal
/// too: the trap's `date` once was, and its file was left empty. The `mv`
/// now inherits the ignored signal. It writes its pid to `job-<i>.pid`,
/// and ends by itself within 30 s should a failing test leave it.
fn stubborn(i: usize) -> Value {
    json!({
        "argv": ["bash", "-c", format!(
            "trap 't=$EPOCHREALTIME; trap \"\" TERM; printf \"%s\\n\" \"${{t/[.,]/}}\" > job-{i}.term.tmp && mv job-{i}.term.tmp job-{i}.term' TERM; echo $$ > job-{i}.pid; n=0; while [ $n -lt 600 ]; do sleep 0.05; n=$((n+1)); done"
        )],
        "timeout_secs": 60,
    })
}

/// A job that reports and leaves something running (theseus-7kha): its
/// command double-forks a `setsid` grandchild, writes its pid, and exits at
/// once, so the command completes and its result settles while the
/// grandchild runs on. The wrapper is a child subreaper (theseus-6qy), so
/// the orphan stays under it and the wrapper lingers for it, marked in the
/// spool. This is what `proc.run` leaves behind.
///
/// Its own bound is 120 s, far longer than any wait the tests below make
/// (`GONE_WITHIN`, 10 s), so a leftover that goes is one the stop ended and
/// never one that timed out while a test watched: the first draft of this
/// test bounded it at 30 s and `a_stop_ends_what_proc_run_left_running`
/// passed its "is it gone" wait on main, 31 s in, on the self-timeout alone.
/// It also ends as soon as the rig's directory goes, so a failing test
/// leaves nothing running.
fn leftover() -> Value {
    json!({
        "argv": ["bash", "-c",
            "( setsid bash -c 'echo $$ > leftover.pid; n=0; \
               while [ $n -lt 1200 ] && [ -d \"$1\" ]; do sleep 0.1; n=$((n+1)); done' \
               bash \"$PWD\" > /dev/null 2>&1 < /dev/null & ); \
             echo left-something-running"],
        "timeout_secs": 60,
    })
}

/// How long a stop gets to end a leftover before a test calls it kept. The
/// stop waits on its own verdicts within one `STOP_GRACE` (2 s), so this is
/// generous; it is bounded well under `leftover`'s own 120 s so the two can
/// never be confused.
const GONE_WITHIN: Duration = Duration::from_secs(10);

/// Whether `pid` goes within `GONE_WITHIN`. Not `Fixture::wait`, whose 40 s
/// patience is longer than a leftover should ever need and which panics
/// rather than answering: a test here must be able to assert that a process
/// is still running as much as that it is gone.
fn gone_soon(pid: u32) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < GONE_WITHIN {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !alive(pid)
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
        Self::with_floor(false)
    }

    /// `new`, with `proc.run` on the approval floor, so a call waits for the
    /// operator and the action it dispatches carries a `confirm`
    /// (theseus-7kha): that is the "approved" half of an approved detached
    /// task, and the only thing a stop keeps a leftover for.
    fn on_the_floor() -> Self {
        Self::with_floor(true)
    }

    fn with_floor(floor: bool) -> Self {
        let model = FakeModel::start(|prompt| {
            if prompt.contains("Start the three builds") {
                (0..3).map(|i| ("proc_run", stubborn(i))).collect()
            } else if prompt.contains("Leave something running in the background") {
                let mut job = leftover();
                job["background"] = json!(true);
                vec![("proc_run", job)]
            } else if prompt.contains("Leave something running") {
                vec![("proc_run", leftover())]
            } else {
                vec![]
            }
        });
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
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        if floor {
            // The inherited posture stays `notify`; the per-tool exception
            // is what puts `proc.run` alone on the floor (§3.9).
            let mut tools = toml::Table::new();
            tools.insert("proc.run".into(), "approve".into());
            table(&mut t, "policy").insert("tools".into(), tools.into());
        }
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, _model: model }
    }

    /// The daemon, on one runtime worker.
    fn spawn(&self) -> Daemon {
        let log = std::fs::File::create(self.path("theseusd.log")).unwrap();
        let d = Daemon::spawn(
            std::process::Command::new(env!("CARGO_BIN_EXE_theseusd"))
                .arg("--config")
                .arg(self.path("config.toml"))
                .arg("--state-dir")
                .arg(self.path("state"))
                .arg("--socket")
                .arg(self.path("sock"))
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        self.path("bin").display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("TOKIO_WORKER_THREADS", "1")
                .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
                .env_remove("THESEUS_OP_TOKEN_FILE")
                .env_remove("THESEUS_CONFIG")
                .env_remove("THESEUS_STATE_DIR")
                .env_remove("THESEUS_SOCKET")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        self.wait("the socket", || self.call("health", Value::Null).ok());
        d
    }

    /// What a stop left, for a job whose SIGTERM is not on record: each
    /// job's files, the jobs' rows in `action.list` (`cancel`, the verdict),
    /// the stop's answer and how long it took, and the daemon's log lines
    /// for the stop.
    fn stop_seen(&self, eid: &str, took: Duration, answer: &Value) -> String {
        let mut out = vec![format!("the stop took {took:?} and answered {answer}")];
        let mut files: Vec<_> = std::fs::read_dir(self.path("projects"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("job-"))
            })
            .collect();
        files.sort();
        for p in files {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            out.push(format!("{}: {:?}", p.display(), text));
        }
        match self.call("action.list", json!({"execution_id": eid})) {
            Ok(a) => {
                for a in a["actions"].as_array().into_iter().flatten() {
                    out.push(format!("action: {a}"));
                }
            }
            Err(e) => out.push(format!("action.list failed: {e}")),
        }
        let log = self.log();
        let stop = log.lines().filter(|l| {
            ["stop", "cancel", "job", "wrapper", "verdict"]
                .iter()
                .any(|w| l.contains(w))
        });
        out.extend(stop.map(|l| format!("log: {l}")));
        out.join("\n")
    }

    /// A pid the job wrote to `file` in the projects dir.
    fn pid(&self, file: &str) -> Option<u32> {
        std::fs::read_to_string(self.path("projects").join(file))
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

/// Running or sleeping: neither gone nor a zombie.
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s[s.rfind(')')? + 2..].chars().next())
        .is_some_and(|c| c != 'Z' && c != 'X')
}

/// Review 2's S2 (theseus-bzq): three jobs that ignore SIGTERM, then a stop.
/// It settles within one grace (2 s) and a margin, not three, every job's
/// command is gone after it, and while it waits, `health` answers at once on
/// the daemon's one worker.
#[test]
fn a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker() {
    let r = Rig::new();
    let _daemon = r.spawn();
    let res = r
        .call(
            "turn.submit",
            json!({"input": "Start the three builds", "author": "test", "attachments": []}),
        )
        .unwrap();
    let eid = res["execution_id"].as_str().unwrap().to_string();
    let jobs: Vec<u32> = r.wait("the three jobs running", || {
        let pids: Vec<u32> = (0..3)
            .filter_map(|i| r.pid(&format!("job-{i}.pid")))
            .collect();
        let e = r.call("execution.list", Value::Null).ok()?;
        let e = e["executions"]
            .as_array()?
            .iter()
            .find(|e| e["execution_id"] == eid.as_str())?
            .clone();
        (pids.len() == 3 && e["state"] == "waiting" && e["outstanding"] == 3).then_some(pids)
    });

    let stop = {
        let sock = r.path("sock");
        let eid = eid.clone();
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let s = UnixStream::connect(sock).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
            let req = json!({"jsonrpc": "2.0", "id": 1, "method": "execution.stop",
                "params": {"execution_id": eid, "author": "test"}});
            (&s).write_all(format!("{req}\n").as_bytes()).unwrap();
            let line = BufReader::new(&s).lines().next().unwrap().unwrap();
            (t0.elapsed(), serde_json::from_str::<Value>(&line).unwrap())
        })
    };
    // While the stop waits out its grace, health answers at once.
    std::thread::sleep(Duration::from_millis(200));
    let mut answers = vec![];
    while !stop.is_finished() {
        let t0 = Instant::now();
        r.call("health", Value::Null).unwrap();
        answers.push(t0.elapsed());
        std::thread::sleep(Duration::from_millis(100));
    }
    let (took, answer) = stop.join().unwrap();
    let result = &answer["result"];
    assert_eq!(result["stopped"], true, "{answer}");
    assert_eq!(result["stopped_actions"].as_array().unwrap().len(), 3);
    assert!(
        took >= Duration::from_secs(2),
        "the grace was waited out: {took:?}"
    );
    // One grace, not three, by order rather than by a stopwatch
    // (theseus-3dsz): every job had its SIGTERM before any job's grace was
    // out. Stopped one after another, each next job would be signalled only
    // after the last one's grace (2 s), however fast the machine; stopped
    // together, they are signalled at once, however slow it is. Each time
    // is the one its trap took as it began, with no fork, so a trap slow to
    // write still proves the order; a job with no time on record is the
    // failure, printed with what tells its cause apart (theseus-y0lm).
    let terms: Vec<u128> = (0..3)
        .map(|i| {
            let t = std::fs::read_to_string(r.path("projects").join(format!("job-{i}.term")))
                .unwrap_or_default();
            t.lines()
                .next()
                .and_then(|l| l.trim().parse().ok())
                .unwrap_or_else(|| {
                    panic!(
                        "job {i} had no SIGTERM\n{}",
                        r.stop_seen(&eid, took, &answer)
                    )
                })
        })
        .collect();
    let spread =
        Duration::from_micros((terms.iter().max().unwrap() - terms.iter().min().unwrap()) as u64);
    assert!(
        spread < Duration::from_secs(2),
        "one grace, not three: the jobs' SIGTERMs came {spread:?} apart"
    );
    assert!(
        answers.len() >= 5,
        "health was asked during the stop: {answers:?}"
    );
    // A health that waited on the stop would answer only once the grace
    // was out: its first answer, asked 200 ms in, at about 1.8 s, and the
    // stop finished by then, so one answer in all. The bound is well under
    // that and well over a scheduler's stall under load (theseus-3dsz: one
    // answer took 880 ms at a load of 25 on 16 cores).
    let slowest = answers.iter().max().unwrap();
    assert!(
        *slowest < Duration::from_millis(1500),
        "health waited on the stop: {answers:?}"
    );
    for pid in &jobs {
        assert!(!alive(*pid), "job {pid} is gone");
    }
    let actions = r.call("action.list", json!({"execution_id": eid})).unwrap();
    let stopped: Vec<&Value> = actions["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["tool"] == "proc.run")
        .collect();
    assert_eq!(stopped.len(), 3, "{actions}");
    for a in stopped {
        assert_eq!(a["state"], "cancelled", "{a}");
        assert_eq!(a["cancel"], "termination_verified", "{a}");
    }
}

/// theseus-7kha: a `/stop` ends what `proc.run` left running, not only what a
/// terminal left. The job's command reports and settles; the grandchild it
/// double-forked runs on under the wrapper, which lingers for it and marks
/// itself in the spool. Before this row a stop reached neither: the job is
/// not in `to_kill` (its action settled), and `stop_backends` looks for a
/// wrapper with `spool.read_pid`, which a lingering wrapper has removed at
/// its report (`Spool::wrapper_lives`), so the loop fell through every arm
/// and did nothing. After it the stop asks the lingering wrapper to stop its
/// whole tree, and says what it ended.
#[test]
fn a_stop_ends_what_proc_run_left_running() {
    let r = Rig::new();
    let _daemon = r.spawn();
    let res = r
        .call(
            "turn.submit",
            json!({"input": "Leave something running", "author": "test", "attachments": []}),
        )
        .unwrap();
    let eid = res["execution_id"].as_str().unwrap().to_string();
    // The command has reported and its wrapper lingers for the grandchild.
    let left = r.wait("the leftover and its lingering wrapper", || {
        let pid = r.pid("leftover.pid")?;
        let marked = std::fs::read_dir(r.path("state/spool/lingering"))
            .ok()?
            .flatten()
            .count();
        (marked > 0 && alive(pid)).then_some(pid)
    });
    assert!(alive(left), "the leftover was not running before the stop");

    let answer = r
        .call(
            "execution.stop",
            json!({"execution_id": eid, "author": "test"}),
        )
        .unwrap();

    // Part 1: it is ended, promptly and because of the stop.
    assert!(
        gone_soon(left),
        "the leftover {left} outlived the stop: {answer}\n{}",
        r.log_tail()
    );
    // Part 3: the stop says so.
    let ended = answer["left_ended"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        ended.len(),
        1,
        "the stop did not say what it ended: {answer}\n{}",
        r.log_tail()
    );
    assert!(
        answer["left_kept"].as_array().is_none_or(|k| k.is_empty()),
        "nothing was approved, so nothing should be kept: {answer}"
    );
}

/// theseus-7kha, the exemption: an **approved detached task** survives a
/// stop, and the stop says it kept it and on whose word. `proc.run` is on
/// the approval floor here, the operator approves the call, and it is a
/// `background: true` job, so both halves of "an approved detached task"
/// hold: a short id (`j1`, which only a job that went on in the background
/// gets) and a `confirm` bound to this call's own digest.
///
/// Its action is kept whole with its process: an approved detached task goes
/// into `stop_execution_keeping`'s `keep` list as well, or the kernel would
/// cancel the action of a job whose process the stop leaves running.
#[test]
fn an_approved_detached_job_survives_a_stop_and_the_stop_says_it_kept_it() {
    let r = Rig::on_the_floor();
    let _daemon = r.spawn();
    let res = r
        .call(
            "turn.submit",
            json!({"input": "Leave something running in the background",
                   "author": "test", "attachments": []}),
        )
        .unwrap();
    let eid = res["execution_id"].as_str().unwrap().to_string();
    // It waits for the operator, and the operator approves it.
    let corr = res["awaiting_confirm"]
        .as_str()
        .unwrap_or_else(|| panic!("the call waits for approval: {res}\n{}", r.log_tail()))
        .to_string();
    r.call(
        "action.confirm",
        json!({"correlation_id": corr, "approve": true}),
    )
    .unwrap();
    let left = r.wait(
        "the approved job's leftover and its lingering wrapper",
        || {
            let pid = r.pid("leftover.pid")?;
            let marked = std::fs::read_dir(r.path("state/spool/lingering"))
                .ok()?
                .flatten()
                .count();
            (marked > 0 && alive(pid)).then_some(pid)
        },
    );

    let answer = r
        .call(
            "execution.stop",
            json!({"execution_id": eid, "author": "test"}),
        )
        .unwrap();

    // Part 2: it is KEPT. `gone_soon` waits out the whole window and then
    // asks again, so this is not a race with the stop's own grace.
    assert!(
        !gone_soon(left),
        "the stop ended an approved detached job: {answer}\n{}",
        r.log_tail()
    );
    // Part 3: and the stop says so, naming the operator who approved it.
    let kept = answer["left_kept"].as_array().cloned().unwrap_or_default();
    assert_eq!(kept.len(), 1, "the stop did not say what it kept: {answer}");
    assert!(
        kept[0]["approved_by"]
            .as_str()
            .is_some_and(|w| !w.is_empty()),
        "the kept job does not name who approved it: {answer}"
    );
    assert!(
        answer["left_ended"].as_array().is_none_or(|e| e.is_empty()),
        "it ended the one job it should have kept: {answer}"
    );
    // Its action was kept with it: not told to stop, and no cancel on it.
    let actions = r.call("action.list", json!({"execution_id": eid})).unwrap();
    let job = actions["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["correlation_id"] == corr.as_str())
        .unwrap_or_else(|| panic!("the job's action: {actions}"));
    assert_ne!(job["state"], "cancelled", "{job}");
    assert!(
        job["cancel"].is_null(),
        "a cancel was put on the kept job: {job}"
    );
}

/// theseus-7kha: the approval alone does not keep a leftover. The same
/// operator approves a **foreground** `proc.run` on the same floor, and its
/// leftover is still ended -- approving `cargo build` is "run that", not
/// "keep building once I say stop", and the stop is the later word.
///
/// This is the discriminating half of the pair: it differs from
/// `an_approved_detached_job_survives_a_stop...` only in `background`, so no
/// heuristic on the command could part the two, and keying the exemption on
/// `Action::confirm` alone would have kept both -- which would also have let
/// one `[policy.tools] "proc.run" = "approve"` line quietly undo this row.
#[test]
fn an_approved_foreground_jobs_leftover_is_still_ended() {
    let r = Rig::on_the_floor();
    let _daemon = r.spawn();
    let res = r
        .call(
            "turn.submit",
            json!({"input": "Leave something running", "author": "test", "attachments": []}),
        )
        .unwrap();
    let eid = res["execution_id"].as_str().unwrap().to_string();
    let corr = res["awaiting_confirm"]
        .as_str()
        .unwrap_or_else(|| panic!("the call waits for approval: {res}\n{}", r.log_tail()))
        .to_string();
    r.call(
        "action.confirm",
        json!({"correlation_id": corr, "approve": true}),
    )
    .unwrap();
    let left = r.wait("the approved foreground job's leftover", || {
        let pid = r.pid("leftover.pid")?;
        let marked = std::fs::read_dir(r.path("state/spool/lingering"))
            .ok()?
            .flatten()
            .count();
        (marked > 0 && alive(pid)).then_some(pid)
    });

    let answer = r
        .call(
            "execution.stop",
            json!({"execution_id": eid, "author": "test"}),
        )
        .unwrap();

    assert!(
        gone_soon(left),
        "an approved FOREGROUND job's leftover outlived the stop: {answer}\n{}",
        r.log_tail()
    );
    assert_eq!(
        answer["left_ended"]
            .as_array()
            .map(|e| e.len())
            .unwrap_or_default(),
        1,
        "the stop did not say it ended it: {answer}"
    );
    assert!(
        answer["left_kept"].as_array().is_none_or(|k| k.is_empty()),
        "a foreground job was kept on its approval alone: {answer}"
    );
}
