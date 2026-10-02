//! The index tender's supervisor (roadmap row 51; M6 §2.2). Its lifecycle
//! runs on tokio's paused clock with a stand-in for the operating system: the
//! tender starts with its arguments, a kill restarts it after its backoff (1 s,
//! doubling to 60 s, and 1 s again after a healthy run), an exec's tender is
//! taken over with no second start, and a stop sends SIGTERM and waits for
//! nothing. Health and `index.query` ask a stand-in tender on a real socket,
//! under their deadlines.

use std::ffi::OsString;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::index::{IndexQueryParams, IndexStatus};
use theseus_protocol::{error_code, Id, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::Instant;

use crate::config::IndexConfig;
use crate::ledger::LedgerRow;
use crate::tender::{
    exit_words, next_backoff, IndexTender, Os, BACKOFF_FIRST, BACKOFF_MAX, HEALTHY_RUN,
    HEALTH_DEADLINE,
};

/// The operating system, as the supervisor sees it: each start recorded with
/// its time on the paused clock, each SIGTERM recorded, and a tender an exec
/// kept, when one is set, with its arguments when they are.
#[derive(Default)]
struct FakeOs {
    kept: Mutex<Option<u32>>,
    kept_args: Mutex<Option<Vec<String>>>,
    spawns: Mutex<Vec<(u32, Instant, Vec<String>)>>,
    terminated: Mutex<Vec<u32>>,
    /// Starts that fail before one succeeds.
    fail: Mutex<u32>,
}

impl Os for FakeOs {
    fn running(&self) -> Option<u32> {
        *self.kept.lock().unwrap()
    }

    fn spawn(&self, _program: &Path, args: &[OsString]) -> io::Result<u32> {
        let mut fail = self.fail.lock().unwrap();
        if *fail > 0 {
            *fail -= 1;
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let mut spawns = self.spawns.lock().unwrap();
        let pid = 1001 + spawns.len() as u32;
        let args = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        spawns.push((pid, Instant::now(), args));
        Ok(pid)
    }

    fn terminate(&self, pid: u32) {
        self.terminated.lock().unwrap().push(pid);
    }

    fn args_of(&self, pid: u32) -> Option<Vec<OsString>> {
        let args = if *self.kept.lock().unwrap() == Some(pid) {
            self.kept_args.lock().unwrap().clone()?
        } else {
            let s = self.spawns.lock().unwrap();
            s.iter().find(|(p, _, _)| *p == pid)?.2.clone()
        };
        Some(args.into_iter().map(OsString::from).collect())
    }
}

impl FakeOs {
    fn starts(&self) -> usize {
        self.spawns.lock().unwrap().len()
    }

    fn last(&self) -> (u32, Instant) {
        let s = self.spawns.lock().unwrap();
        let (pid, at, _) = s.last().expect("a start");
        (*pid, *at)
    }
}

/// A supervisor of the store at `<state>/store`, with its rows kept.
fn supervisor(
    cfg: IndexConfig,
    state: &Path,
    os: Arc<FakeOs>,
) -> (Arc<IndexTender>, Arc<Mutex<Vec<Value>>>) {
    let t = Arc::new(IndexTender::new(
        cfg,
        &state.join("store"),
        Some(PathBuf::from("/opt/theseus/theseus-index")),
        os,
    ));
    let rows = Arc::new(Mutex::new(Vec::new()));
    let kept = rows.clone();
    t.set_ledger(Arc::new(move |row: LedgerRow| {
        assert_eq!(
            row.kind, "index.tender",
            "every fact of the tender is its row"
        );
        kept.lock().unwrap().push(row.data)
    }));
    (t, rows)
}

/// Let the supervisor's task run until it waits on its inbox or its timer.
async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

fn events(rows: &Mutex<Vec<Value>>) -> Vec<String> {
    rows.lock()
        .unwrap()
        .iter()
        .map(|r| r["event"].as_str().unwrap_or("?").to_string())
        .collect()
}

fn code(c: i32) -> Option<ExitStatus> {
    Some(ExitStatus::from_raw(c << 8))
}

fn signal(s: i32) -> Option<ExitStatus> {
    Some(ExitStatus::from_raw(s))
}

/// 1 s after the first exit, doubling while it keeps failing, never past a
/// minute; and 1 s again after a run of a minute.
#[test]
fn the_backoff_doubles_to_a_minute_and_a_healthy_run_resets_it() {
    let quick = Duration::from_millis(10);
    let mut last = None;
    let mut seen = Vec::new();
    for _ in 0..9 {
        let w = next_backoff(last, quick);
        seen.push(w.as_secs());
        last = Some(w);
    }
    assert_eq!(seen, [1, 2, 4, 8, 16, 32, 60, 60, 60]);
    assert_eq!(next_backoff(Some(BACKOFF_MAX), HEALTHY_RUN), BACKOFF_FIRST);
    let almost = HEALTHY_RUN - Duration::from_millis(1);
    assert_eq!(
        next_backoff(Some(Duration::from_secs(8)), almost),
        Duration::from_secs(16)
    );
    assert_eq!(
        exit_words(code(3).unwrap()),
        "held (exit 3: another tender holds the index)"
    );
    assert_eq!(exit_words(code(1).unwrap()), "exit 1");
    assert_eq!(exit_words(signal(9).unwrap()), "signal 9");
}

/// It starts with its arguments; a kill restarts it 1 s later, quick exits
/// wait 2 s and 4 s, and after a run of a minute the wait is 1 s again. Each
/// start and exit is a row.
#[tokio::test(start_paused = true)]
async fn the_tender_starts_with_its_arguments_and_a_kill_restarts_it_with_backoff() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    let (t, rows) = supervisor(IndexConfig::default(), dir.path(), os.clone());
    assert_eq!(t.status().unwrap().state, "pending");
    let task = tokio::spawn(t.clone().run());
    settle().await;
    assert_eq!(os.starts(), 1);
    let args = os.spawns.lock().unwrap()[0].2.clone();
    let store = dir.path().join("store").display().to_string();
    let index = dir.path().join("index").display().to_string();
    let me = std::process::id().to_string();
    let models = crate::config::expand("~/.cache/theseus/models")
        .display()
        .to_string();
    assert_eq!(
        args,
        [
            "serve",
            "--store",
            &store,
            "--index",
            &index,
            "--parent",
            &me,
            "--weights-dir",
            &models,
            "--threads",
            "1",
            "--idle-unload-mins",
            "10"
        ]
    );
    let (first, _) = os.last();
    let st = t.status().unwrap();
    assert_eq!(
        (st.state.as_str(), st.pid, st.restarts, st.adopted),
        ("running", Some(first), 0, false)
    );
    assert_eq!(st.binary.as_deref(), Some("/opt/theseus/theseus-index"));

    // Killed: the reaper's sweep reports it, and the next waits 1 s.
    let killed = Instant::now();
    t.exited(first, signal(9));
    settle().await;
    let st = t.status().unwrap();
    assert_eq!(
        (
            st.state.as_str(),
            st.pid,
            st.last_exit.as_deref(),
            st.backoff_ms
        ),
        ("backoff", None, Some("signal 9"), 1000)
    );
    tokio::time::sleep(Duration::from_millis(999)).await;
    assert_eq!(os.starts(), 1, "nothing starts before its backoff ends");
    tokio::time::sleep(Duration::from_millis(2)).await;
    assert_eq!(os.starts(), 2);
    assert_eq!(os.last().1 - killed, BACKOFF_FIRST);

    // Quick exits: 2 s, then 4 s. An exit of an older tender is history.
    for want in [2, 4] {
        let (pid, _) = os.last();
        t.exited(first, signal(9));
        let gone = Instant::now();
        t.exited(pid, code(1));
        settle().await;
        assert_eq!(t.status().unwrap().backoff_ms, want * 1000);
        tokio::time::sleep(Duration::from_secs(want) + Duration::from_millis(1)).await;
        assert_eq!(os.last().1 - gone, Duration::from_secs(want));
    }
    assert_eq!(os.starts(), 4);
    assert_eq!(t.status().unwrap().restarts, 3);

    // A run of a minute was healthy: the next exit waits 1 s again.
    tokio::time::sleep(HEALTHY_RUN).await;
    let (pid, _) = os.last();
    let gone = Instant::now();
    t.exited(pid, code(3));
    settle().await;
    let st = t.status().unwrap();
    assert_eq!(st.backoff_ms, 1000);
    assert_eq!(
        st.last_exit.as_deref(),
        Some("held (exit 3: another tender holds the index)")
    );
    tokio::time::sleep(Duration::from_millis(1001)).await;
    assert_eq!(os.last().1 - gone, BACKOFF_FIRST);
    assert_eq!(
        events(&rows),
        [
            "started", "exited", "started", "exited", "started", "exited", "started", "exited",
            "started"
        ]
    );
    task.abort();
}

/// After an exec restart the tender this daemon's last image started is
/// still its child: the supervisor takes it over and starts no second. When
/// that one exits, the next starts after its backoff, as a restart.
#[tokio::test(start_paused = true)]
async fn an_exec_restart_takes_over_the_running_tender_and_starts_no_second() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    *os.kept.lock().unwrap() = Some(4242);
    let (t, rows) = supervisor(IndexConfig::default(), dir.path(), os.clone());
    let task = tokio::spawn(t.clone().run());
    settle().await;
    tokio::time::sleep(BACKOFF_MAX * 2).await;
    assert_eq!(os.starts(), 0, "no second tender");
    let st = t.status().unwrap();
    assert_eq!(
        (st.state.as_str(), st.pid, st.adopted, st.restarts),
        ("running", Some(4242), true, 0)
    );
    t.exited(4242, signal(9));
    settle().await;
    tokio::time::sleep(Duration::from_millis(1001)).await;
    assert_eq!(os.starts(), 1);
    let st = t.status().unwrap();
    assert_eq!(
        (st.pid, st.adopted, st.restarts),
        (Some(os.last().0), false, 1)
    );
    assert_eq!(events(&rows), ["adopted", "exited", "started"]);
    task.abort();
}

/// After a restart in place onto a note that changed `[index]`, the kept
/// tender has the old settings: it gets SIGTERM, and the next starts with the
/// new ones after its backoff. One whose settings match is kept.
#[tokio::test(start_paused = true)]
async fn a_restart_onto_changed_settings_restarts_the_kept_tender() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    *os.kept.lock().unwrap() = Some(4242);
    let cfg = IndexConfig {
        threads: 2,
        ..IndexConfig::default()
    };
    let (t, rows) = supervisor(cfg, dir.path(), os.clone());
    let mut old: Vec<String> = t
        .args()
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let at = old.iter().position(|a| a == "--threads").unwrap();
    old[at + 1] = "1".into();
    *os.kept_args.lock().unwrap() = Some(old);
    let task = tokio::spawn(t.clone().run());
    settle().await;
    assert_eq!(*os.terminated.lock().unwrap(), [4242]);
    t.exited(4242, signal(libc::SIGTERM));
    settle().await;
    tokio::time::sleep(Duration::from_millis(1001)).await;
    assert_eq!(os.starts(), 1);
    let args = os.spawns.lock().unwrap()[0].2.clone();
    let at = args.iter().position(|a| a == "--threads").unwrap();
    assert_eq!(args[at + 1], "2");
    assert_eq!(
        events(&rows),
        ["adopted", "settings_changed", "exited", "started"]
    );
    task.abort();

    // The same settings: kept, and nothing is signalled.
    let os = Arc::new(FakeOs::default());
    *os.kept.lock().unwrap() = Some(4243);
    let (t, _) = supervisor(IndexConfig::default(), dir.path(), os.clone());
    let same = t
        .args()
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    *os.kept_args.lock().unwrap() = Some(same);
    let task = tokio::spawn(t.clone().run());
    settle().await;
    tokio::time::sleep(BACKOFF_MAX).await;
    assert!(os.terminated.lock().unwrap().is_empty());
    assert_eq!(os.starts(), 0);
    assert_eq!(t.status().unwrap().pid, Some(4243));
    task.abort();
}

/// A stop sends SIGTERM and waits for nothing: the supervisor ends with no
/// time passing, and an exit after it starts nothing.
#[tokio::test(start_paused = true)]
async fn a_stop_sends_sigterm_and_waits_for_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    let (t, rows) = supervisor(IndexConfig::default(), dir.path(), os.clone());
    let task = tokio::spawn(t.clone().run());
    settle().await;
    let (pid, _) = os.last();
    let t0 = Instant::now();
    t.stop(false);
    settle().await;
    assert!(task.is_finished(), "the supervisor ends at once");
    assert_eq!(Instant::now(), t0, "no time passed");
    assert_eq!(*os.terminated.lock().unwrap(), [pid]);
    t.exited(pid, signal(libc::SIGTERM));
    tokio::time::sleep(BACKOFF_MAX * 2).await;
    assert_eq!(os.starts(), 1, "nothing starts after a stop");
    assert_eq!(t.status().unwrap().state, "stopped");
    assert_eq!(events(&rows), ["started"], "a stop writes no row");
    // A second stop does nothing.
    t.stop(false);
    assert_eq!(os.terminated.lock().unwrap().len(), 1);
}

/// A restart in place onto the vault's changed note keeps the tender for the
/// next image: no signal.
#[tokio::test(start_paused = true)]
async fn a_restart_in_place_keeps_the_tender() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    let (t, _) = supervisor(IndexConfig::default(), dir.path(), os.clone());
    let task = tokio::spawn(t.clone().run());
    settle().await;
    t.stop(true);
    settle().await;
    assert!(task.is_finished());
    assert!(os.terminated.lock().unwrap().is_empty());
    let st = t.status().unwrap();
    assert_eq!(st.state, "stopped");
    assert!(st.why.unwrap().contains("next image takes the tender over"));
}

/// A stop during the backoff starts nothing after it.
#[tokio::test(start_paused = true)]
async fn a_stop_during_the_backoff_starts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    let (t, _) = supervisor(IndexConfig::default(), dir.path(), os.clone());
    let task = tokio::spawn(t.clone().run());
    settle().await;
    t.exited(os.last().0, code(1));
    settle().await;
    t.stop(false);
    settle().await;
    assert!(task.is_finished());
    tokio::time::sleep(BACKOFF_MAX * 2).await;
    assert_eq!(os.starts(), 1);
    assert!(
        os.terminated.lock().unwrap().is_empty(),
        "nothing ran to signal"
    );
}

/// A start that fails is tried again after its backoff: 1 s, then 2 s.
#[tokio::test(start_paused = true)]
async fn a_start_that_fails_is_tried_again_after_its_backoff() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    *os.fail.lock().unwrap() = 2;
    let (t, rows) = supervisor(IndexConfig::default(), dir.path(), os.clone());
    let t0 = Instant::now();
    let task = tokio::spawn(t.clone().run());
    settle().await;
    assert_eq!(os.starts(), 0);
    let st = t.status().unwrap();
    assert_eq!(st.state, "backoff");
    assert!(
        st.why.as_deref().unwrap().starts_with("it would not start"),
        "{st:?}"
    );
    tokio::time::sleep(Duration::from_millis(3001)).await;
    assert_eq!(os.starts(), 1);
    assert_eq!(os.last().1 - t0, Duration::from_secs(3));
    assert_eq!(t.status().unwrap().restarts, 0, "the first that ran");
    assert_eq!(events(&rows), ["failed", "failed", "started"]);
    task.abort();
}

/// With `[index] enabled = false` nothing starts, health says `off`, and a
/// tender an exec kept is stopped.
#[tokio::test(start_paused = true)]
async fn off_starts_nothing_and_stops_a_kept_tender() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    *os.kept.lock().unwrap() = Some(77);
    let cfg = IndexConfig {
        enabled: false,
        ..IndexConfig::default()
    };
    let (t, _) = supervisor(cfg, dir.path(), os.clone());
    t.clone().run().await;
    assert_eq!(os.starts(), 0);
    assert_eq!(*os.terminated.lock().unwrap(), [77]);
    assert!(t.status().is_none());
    let h = t.health(HEALTH_DEADLINE).await;
    assert_eq!(h.state, "off");
    let q = t.query(&IndexQueryParams::new("anything")).await;
    assert_eq!(q.unwrap_err().0, error_code::DISABLED);
}

/// Without its binary beside the daemon's, the tender is `absent`, says
/// where it looked, and is never tried again.
#[tokio::test]
async fn no_binary_is_absent_and_never_tried_again() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    let t = Arc::new(IndexTender::new(
        IndexConfig::default(),
        &dir.path().join("store"),
        None,
        os.clone(),
    ));
    t.clone().run().await;
    assert_eq!(os.starts(), 0);
    let st = t.status().unwrap();
    assert_eq!(st.state, "absent");
    let why = st.why.unwrap();
    assert!(
        why.starts_with("no theseus-index beside theseusd, at "),
        "{why}"
    );
    let h = t.health(HEALTH_DEADLINE).await;
    assert_eq!(h.state, "down");
    assert_eq!(h.why.as_deref(), Some(why.as_str()));
}

/// A stand-in tender on `<state>/index/sock`: `index.status` says `ready`;
/// `index.query` answers the query's text and `k` back, and refuses the text
/// `bad`. While `hang` is set, it reads a request and never answers.
fn stand_in(state: &Path, hang: Arc<AtomicBool>) {
    let dir = state.join("index");
    std::fs::create_dir_all(&dir).unwrap();
    let listener = tokio::net::UnixListener::bind(dir.join("sock")).unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((s, _)) = listener.accept().await else {
                return;
            };
            let hang = hang.clone();
            tokio::spawn(async move {
                let (r, mut w) = s.into_split();
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if hang.load(Ordering::SeqCst) {
                        std::future::pending::<()>().await;
                    }
                    let req: Request = serde_json::from_str(&line).unwrap();
                    let resp = match req.method.as_str() {
                        "index.status" => Response::ok(
                            req.id,
                            IndexStatus {
                                state: "ready".into(),
                                mode: "bm25_only".into(),
                                pid: 7,
                                documents: 3,
                                nodes: 2,
                                ..IndexStatus::default()
                            },
                        ),
                        "index.query" if req.params["text"] == "bad" => {
                            Response::err(req.id, error_code::INVALID_PARAMS, "k is past 100")
                        }
                        "index.query" => Response::ok(
                            req.id,
                            json!({"hits": [{"node_id": "nod_1", "chunk": 0, "session_id": "ses_1",
                                   "position": 4, "kind": "user_message", "origin": "cli",
                                   "time_ms": 1, "external": false, "text": req.params["text"],
                                   "sources": {"bm25": {"rank": 1, "score": 1.5}}, "fused": 0.016}],
                                   "indexed_through": req.params["k"], "lag": {"bytes": 0, "ms": 0},
                                   "timings": {"bm25_ms": 0.1, "entity_ms": 0.1, "fuse_ms": 0.0,
                                               "load_ms": 0.0, "total_ms": 0.3}}),
                        ),
                        _ => Response::err(Id::Num(0), error_code::METHOD_NOT_FOUND, "no"),
                    };
                    let mut out = serde_json::to_vec(&resp).unwrap();
                    out.push(b'\n');
                    if w.write_all(&out).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
}

/// Health and `index.query` ask the tender on its socket: health takes its
/// status, a query goes as it came and its answer comes back, and a query the
/// tender refuses keeps the tender's code.
#[tokio::test]
async fn health_and_query_ask_the_tender_on_its_socket() {
    let dir = tempfile::tempdir().unwrap();
    stand_in(dir.path(), Arc::default());
    let os = Arc::new(FakeOs::default());
    let (t, _) = supervisor(IndexConfig::default(), dir.path(), os);
    let h = t.health(HEALTH_DEADLINE).await;
    assert_eq!(h.state, "ready");
    assert_eq!(h.why, None);
    let s = h.status.unwrap();
    assert_eq!((s.pid, s.documents, s.nodes), (7, 3, 2));
    assert_eq!(h.tender.unwrap().state, "pending");
    let mut p = IndexQueryParams::new("port 7433");
    p.k = 5;
    let r = t.query(&p).await.unwrap();
    assert_eq!(r.indexed_through, 5);
    assert_eq!(r.hits[0].text, "port 7433");
    assert_eq!(r.hits[0].sources["bm25"].rank, 1);
    let (code, why) = t.query(&IndexQueryParams::new("bad")).await.unwrap_err();
    assert_eq!(code, error_code::INVALID_PARAMS);
    assert_eq!(why, "the index tender refused the query: k is past 100");
}

/// A running tender whose socket does not answer: health waits no longer
/// than its deadline, and says what it last heard, and how long ago. With
/// nothing heard yet, or no socket at all, it is `down`, and says why.
#[tokio::test]
async fn health_waits_no_longer_than_its_deadline_on_a_tender_that_does_not_answer() {
    let dir = tempfile::tempdir().unwrap();
    let os = Arc::new(FakeOs::default());
    let (t, _) = supervisor(IndexConfig::default(), dir.path(), os.clone());
    let h = t.health(HEALTH_DEADLINE).await;
    assert_eq!(h.state, "down");
    assert_eq!(h.why.as_deref(), Some("it starts once the daemon serves"));
    let task = tokio::spawn(t.clone().run());
    settle().await;
    assert_eq!(t.status().unwrap().state, "running");
    let h = t.health(HEALTH_DEADLINE).await;
    assert_eq!(h.state, "down");
    let why = h.why.unwrap();
    assert!(
        why.starts_with("its socket did not answer (connecting to "),
        "{why}"
    );

    let hang = Arc::new(AtomicBool::new(false));
    stand_in(dir.path(), hang.clone());
    assert_eq!(t.health(HEALTH_DEADLINE).await.state, "ready");
    hang.store(true, Ordering::SeqCst);
    let t0 = std::time::Instant::now();
    let h = t.health(HEALTH_DEADLINE).await;
    let took = t0.elapsed();
    assert!(
        took >= HEALTH_DEADLINE && took < Duration::from_secs(2),
        "{took:?}"
    );
    assert_eq!(h.state, "ready", "its last answer");
    let why = h.why.unwrap();
    assert!(
        why.starts_with("its socket did not answer (no answer within 100 ms): its status as of "),
        "{why}"
    );
    task.abort();
}
