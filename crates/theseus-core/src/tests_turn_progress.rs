//! The progress check and the turn's clock (theseus-2uy56, theseus-yct32),
//! through whole cores on the fake model with the judge off: a turn whose
//! commands keep failing past half its deadline is told to replan, past 0.85
//! of it is told to stop and report and ends after one more call; a turn
//! whose commands pass past the same time is told nothing; the time line is
//! appended history, so the cached prefix stays byte for byte between loops.
//!
//! The deadline is set the way `turn.submit` sets it, with its start in the
//! past, so the turn begins at the share the test names: no clock waits.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_protocol::{SessionKind, TurnSubmitResult, Usage};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::policy::Posture;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::rpc::{Core, Parts};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::clock_step::set_deadline;
use crate::turn::progress_step::PROGRESS_STOP;
use crate::turn::TurnRequest;
use crate::Config;

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(script: impl FnOnce(&Path) -> Vec<Scripted>) -> Rig {
    rig_with(script, |_| {})
}

fn rig_with(script: impl FnOnce(&Path) -> Vec<Scripted>, tune: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root: PathBuf = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    cfg.judge.enabled = false;
    tune(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&root)));
    let core = Core::build(Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

impl Rig {
    /// One turn in a fresh session whose deadline is `secs` long and began
    /// `ago` before the turn: `None` for no deadline.
    async fn turn(&self, deadline: Option<(u64, u64)>) -> (String, TurnSubmitResult) {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&rec.session_id, &rec).unwrap();
        let sid = rec.session_id.clone();
        // The start `turn.submit` gives both the deadline and the request.
        let start = deadline.map(|(secs, ago)| {
            let start = Instant::now() - Duration::from_secs(ago);
            set_deadline(&sid, start, Some(secs));
            start
        });
        let res = self.run(rec, start).await;
        (sid, res)
    }

    /// One turn of `rec`'s session whose request arrived at `arrived`.
    async fn run(&self, rec: SessionRecord, arrived: Option<Instant>) -> TurnSubmitResult {
        let sid = rec.session_id.clone();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                surface: None,
                session: rec,
                input: Some("Build the tide tables and make the check pass.".into()),
                target,
                sink: EventSink::new(self.core.bus.clone(), &sid, None),
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived,
                reply_to: None,
                answer_at: None,
            })
            .await
            .unwrap()
    }

    fn rows(&self, sid: &str) -> Vec<serde_json::Value> {
        let rows: Vec<(u64, LedgerRow)> = self.core.store.ledger_tail(5000).unwrap();
        rows.into_iter()
            .filter(|(_, r)| r.kind == "turn.progress" && r.session_id.as_deref() == Some(sid))
            .map(|(_, r)| r.data)
            .collect()
    }

    fn requests(&self) -> Vec<ProviderRequest> {
        self.fake.requests()
    }
}

/// A request's messages as text.
fn messages(r: &ProviderRequest) -> String {
    serde_json::to_string(&r.messages).unwrap()
}

/// A command in `root` that exits with `code`, as loop `id`'s one call.
fn command(root: &Path, id: &str, code: u8) -> Scripted {
    let input =
        json!({"argv": ["sh", "-c", format!("exit {code}")], "cwd": root.display().to_string()});
    Scripted::tools("Running the check.", &[(id, "proc_run", input)])
}

const STEER: &str = "This is not a stop: if work is left, keep going";
const STOP: &str = "Stop here: in your next answer make no tool calls";

/// Failing loops past half the deadline: the steer once, in the request
/// after the third failing loop, with its row; then the work goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_turn_past_half_its_deadline_is_told_to_replan_once() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 1),
            command(root, "c2", 1),
            command(root, "c3", 1),
            command(root, "c4", 1),
            command(root, "c5", 1),
            Scripted::text("It still fails; here is what is left."),
        ]
    });
    // 600 s, 360 used: 60%.
    let (sid, res) = r.turn(Some((600, 360))).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 6, "the steer stops nothing");
    assert_eq!(res.loops, 6);
    for (i, req) in reqs.iter().enumerate() {
        let steered = messages(req).contains(STEER);
        assert_eq!(
            steered,
            i >= 3,
            "request {i}: the steer follows the third failing loop"
        );
    }
    assert_eq!(messages(&reqs[5]).matches(STEER).count(), 1, "once a turn");
    assert!(
        messages(&reqs[3]).contains("no check has passed in its last 3 loops (3 failed)"),
        "{}",
        messages(&reqs[3])
    );
    let rows = r.rows(&sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["decision"], "steer");
    assert_eq!(rows[0]["loop"], 2);
    assert_eq!(rows[0]["since_pass"], 3);
    assert_eq!(rows[0]["failing"], 3);
    assert!(rows[0]["time_share"].as_f64().unwrap() >= 0.5);
    assert_eq!(rows[0]["deadline_ms"], 600_000);
}

/// Failing loops past 0.85 of the deadline: the stop, and the next request is
/// the turn's last, whatever it answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_turn_near_its_deadline_is_told_to_stop_and_its_next_call_is_its_last() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 1),
            command(root, "c2", 1),
            command(root, "c3", 1),
            // It calls a tool anyway: the call runs, and the turn ends.
            command(root, "c4", 1),
            Scripted::text("A fifth answer no turn asks for."),
        ]
    });
    // 600 s, 540 used: 90%.
    let (sid, res) = r.turn(Some((600, 540))).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 4, "the stop's request is the last");
    assert_eq!((res.loops, res.stop_reason.as_str()), (4, PROGRESS_STOP));
    assert!(messages(&reqs[3]).contains(STOP), "{}", messages(&reqs[3]));
    assert!(!messages(&reqs[3]).contains(STEER), "a stop, not a steer");
    let rows = r.rows(&sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["decision"], "stop");
}

/// A stop answered with the report ends the turn there, and the stop nudge
/// and the end note never add a loop to it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_turn_that_reports_ends_with_its_report() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 1),
            command(root, "c2", 1),
            command(root, "c3", 1),
            Scripted::text("Done: the parser. Not done: the check, which still fails."),
            Scripted::text("An answer no turn asks for."),
        ]
    });
    let (_, res) = r.turn(Some((600, 540))).await;
    assert_eq!(r.requests().len(), 4);
    assert_eq!((res.loops, res.stop_reason.as_str()), (4, PROGRESS_STOP));
    assert!(res.output.contains("Not done: the check"), "{}", res.output);
}

/// Commands that pass, past the same time: no note and no row, and the turn
/// runs every loop it asks for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_whose_checks_pass_is_told_nothing_at_the_same_time() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 0),
            command(root, "c2", 0),
            command(root, "c3", 0),
            command(root, "c4", 0),
            command(root, "c5", 0),
            Scripted::text("Built, and the check passes."),
        ]
    });
    let (sid, res) = r.turn(Some((600, 540))).await;
    let reqs = r.requests();
    assert_eq!((reqs.len(), res.loops), (6, 6));
    assert_eq!(res.stop_reason, "no_tool_calls");
    for req in &reqs {
        let m = messages(req);
        assert!(!m.contains(STEER) && !m.contains(STOP), "{m}");
    }
    assert!(r.rows(&sid).is_empty());
}

/// The time line: in the first request, after the input, and in the
/// messages, never the system or the tools, which stay byte for byte from
/// loop to loop; a turn with no deadline has none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_time_line_is_history_and_the_cached_prefix_holds() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 0),
            Scripted::text("Done."),
            command(root, "c2", 0),
            Scripted::text("Done."),
        ]
    });
    let (_, _) = r.turn(Some((900, 0))).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 2);
    let line = "[The harness: 14 minutes left of this turn's 15.]";
    assert!(messages(&reqs[0]).contains(line), "{}", messages(&reqs[0]));
    assert_eq!(
        messages(&reqs[1])
            .matches("minutes left of this turn's")
            .count(),
        1,
        "one line a minute, never one a loop"
    );
    for req in &reqs {
        let fixed = serde_json::to_string(&(&req.system, &req.tools)).unwrap();
        assert!(
            !fixed.contains("minutes left"),
            "the line is outside the prefix"
        );
    }
    assert_eq!(
        serde_json::to_string(&reqs[0].system).unwrap(),
        serde_json::to_string(&reqs[1].system).unwrap()
    );
    assert_eq!(
        serde_json::to_string(&reqs[0].tools).unwrap(),
        serde_json::to_string(&reqs[1].tools).unwrap()
    );
    // The first request's messages are the second's first messages, byte
    // for byte: only appended to.
    let (a, b) = (&reqs[0].messages, &reqs[1].messages);
    assert!(a.len() < b.len());
    assert_eq!(
        serde_json::to_string(&a[..a.len() - 1]).unwrap(),
        serde_json::to_string(&b[..a.len() - 1]).unwrap()
    );
    // The CPU fact sits in the session's system block, the same every loop.
    let system = serde_json::to_string(&reqs[0].system).unwrap();
    assert!(system.contains("CPUs available to commands: "), "{system}");
    // No deadline: no line.
    let (_, _) = r.turn(None).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 4);
    assert!(!messages(&reqs[2]).contains("left of this turn"));
    assert!(!messages(&reqs[3]).contains("left of this turn"));
}

/// A command in `root` that prints about 2.7 KB (a tool result of the size
/// the bench's median call writes to the cache) after sleeping `secs`.
fn bulky(root: &Path, id: &str, secs: u64) -> Scripted {
    let script = format!("sleep {secs}; head -c 2700 /dev/zero | tr '\\0' x");
    let input = json!({"argv": ["sh", "-c", script], "cwd": root.display().to_string()});
    Scripted::tools("Running the check.", &[(id, "proc_run", input)])
}

/// A request's messages as one serialized string per content block.
fn blocks(r: &ProviderRequest) -> Vec<String> {
    r.messages
        .iter()
        .flat_map(|m| {
            let role = m["role"].as_str().unwrap_or_default().to_string();
            m["content"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |b| format!("{role}:{b}"))
        })
        .collect()
}

/// The bytes a request adds to the one before it.
fn growth(prev: &ProviderRequest, next: &ProviderRequest) -> usize {
    let size = |r: &ProviderRequest| blocks(r).iter().map(String::len).sum::<usize>();
    size(next) - size(prev)
}

/// theseus-tfl5p. The bench read a call that follows a clock line as writing
/// twice the cache of other calls (1,412 tokens against 674). The line is
/// appended history, so the next request must be the last one plus what is
/// new, block for block, and the line must be all the clock adds: a call that
/// follows one writes within 20% of a call that does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_after_a_mid_turn_clock_line_writes_what_other_calls_write() {
    let r = rig(|root| {
        vec![
            bulky(root, "c1", 5),
            bulky(root, "c2", 0),
            bulky(root, "c3", 0),
            Scripted::text("Done."),
        ]
    });
    // 15 minutes, 56 s used: the first loop sees 14 minutes left, and the
    // 5 s command carries the turn across a whole-minute boundary.
    let (_, _) = r.turn(Some((900, 56))).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 4);
    let lines = |i: usize| {
        messages(&reqs[i])
            .matches("minutes left of this turn's")
            .count()
    };
    assert_eq!(
        (lines(0), lines(1), lines(2), lines(3)),
        (1, 2, 2, 2),
        "a second line after the slow command, none after the fast ones"
    );
    // Every request keeps the one before it, block for block.
    for pair in reqs.windows(2) {
        let (a, b) = (blocks(&pair[0]), blocks(&pair[1]));
        assert!(
            a.len() <= b.len() && a[..] == b[..a.len()],
            "{a:#?}\n{b:#?}"
        );
        assert_eq!(
            serde_json::to_string(&(&pair[0].system, &pair[0].tools)).unwrap(),
            serde_json::to_string(&(&pair[1].system, &pair[1].tools)).unwrap(),
            "the clock never touches the cached prefix"
        );
    }
    let after_clock = growth(&reqs[0], &reqs[1]);
    let plain = growth(&reqs[1], &reqs[2]);
    assert!(
        (after_clock as f64) <= plain as f64 * 1.2,
        "the call after the clock line adds {after_clock} bytes, another adds {plain}"
    );
}

/// The turn keeps its sense of time (theseus-tfl5p must not remove the
/// clock): one that begins near its deadline reads how near, and one with
/// minutes to spare reads them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_near_its_deadline_still_reads_the_time_it_has_left() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 0),
            Scripted::text("Done."),
            command(root, "c2", 0),
            Scripted::text("Done."),
        ]
    });
    // 10 minutes, 570 s used: 30 s left.
    let (_, _) = r.turn(Some((600, 570))).await;
    let reqs = r.requests();
    assert!(
        messages(&reqs[0]).contains("[The harness: less than a minute left of this turn's 10.]"),
        "{}",
        messages(&reqs[0])
    );
    // 10 minutes, 335 s used: 4 minutes 25 s left.
    let (_, _) = r.turn(Some((600, 335))).await;
    let reqs = r.requests();
    assert!(
        messages(&reqs[2]).contains("[The harness: 4 minutes left of this turn's 10.]"),
        "{}",
        messages(&reqs[2])
    );
}

/// A command that passes beside a read that fails, every loop: the check is
/// the command's, so the turn is a passing one and is told nothing
/// (theseus-rmjqr).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_passing_command_beside_a_failed_read_is_a_pass() {
    let both = |root: &Path, n: u32| {
        let run = json!({"argv": ["sh", "-c", "exit 0"], "cwd": root.display().to_string()});
        let read = json!({"path": root.join("missing.txt").display().to_string()});
        Scripted::tools(
            "Checking, and reading.",
            &[
                (&format!("c{n}"), "proc_run", run),
                (&format!("r{n}"), "fs_read", read),
            ],
        )
    };
    let r = rig(|root| {
        let mut script: Vec<Scripted> = (1..=4).map(|n| both(root, n)).collect();
        script.push(Scripted::text("Done."));
        script
    });
    // 600 s, 360 used: 60%.
    let (sid, res) = r.turn(Some((600, 360))).await;
    assert_eq!(res.loops, 5);
    for req in r.requests() {
        let m = messages(&req);
        assert!(!m.contains(STEER) && !m.contains(STOP), "{m}");
        assert!(!m.contains("harness:progress"), "{m}");
    }
    assert!(r.rows(&sid).is_empty());
}

/// A deadline is the turn's whose submit set it: a later turn of the session,
/// which arrived on its own (a nudge, a wake), has none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_turn_of_the_session_does_not_inherit_its_deadline() {
    let r = rig(|_| vec![Scripted::text("First."), Scripted::text("Second.")]);
    let (sid, _) = r.turn(Some((900, 0))).await;
    assert!(messages(&r.requests()[0]).contains("minutes left of this turn's 15"));
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    r.run(rec, Some(Instant::now())).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 2);
    // The session's history holds the first turn's line, and no second one.
    assert_eq!(
        messages(&reqs[1])
            .matches("minutes left of this turn's")
            .count(),
        1
    );
}

/// A call billed about $0.47 on the template's live profile.
fn billed(then: Scripted) -> Scripted {
    Scripted::Billed {
        usage: Usage {
            input_tokens: 2_000,
            output_tokens: 45_000,
            ..Usage::default()
        },
        then: Box::new(then),
    }
}

/// The 2026-10-24 shape (theseus-ikr75): the suite passes, one file is
/// written, then six loops of reads of what was read, with the turn past half
/// its $6 spend limit before they end.
fn hunt(root: &Path) -> Vec<Scripted> {
    let file = root.join("tides.txt").display().to_string();
    let mut script = vec![
        billed(command(root, "c1", 0)),
        billed(Scripted::tools(
            "Writing.",
            &[("w1", "fs_write", json!({"path": file, "content": "high\n"}))],
        )),
    ];
    for n in 1..=6 {
        script.push(billed(Scripted::tools(
            "Reading.",
            &[(&format!("r{n}"), "fs_read", json!({ "path": file }))],
        )));
    }
    script.push(Scripted::text("Found it, and fixed it."));
    script
}

fn limit(c: &mut Config, mode: crate::config::SpendLimitMode) {
    c.kernel.spend_limit_usd = 6.0;
    c.kernel.spend_limit_mode = mode;
}

/// A guarded limit (`ask`) and no deadline: the turn is past half its spend
/// with no check in six loops, and it is told nothing and goes on until the
/// model ends it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spend_alone_does_not_steer_a_guarded_turn_with_no_deadline() {
    let r = rig_with(hunt, |c| limit(c, crate::config::SpendLimitMode::Ask));
    let (sid, res) = r.turn(None).await;
    assert!(res.loops >= 9, "{}", res.loops);
    assert_eq!(res.stop_reason, "no_tool_calls");
    for req in r.requests() {
        let m = messages(&req);
        assert!(!m.contains(STEER) && !m.contains("harness:progress"), "{m}");
    }
    assert!(r.rows(&sid).is_empty());
    // The turn was past half its limit, so the silence is the rule's.
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    assert!(rec.cost_usd >= 3.0, "spent ${}", rec.cost_usd);
}

/// Its sibling: the same turn under a limit that only notifies, with a
/// deadline at half its time, steers once, in the passing case's words.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_turn_under_notify_with_a_deadline_is_steered_once() {
    let r = rig_with(hunt, |c| limit(c, crate::config::SpendLimitMode::Notify));
    // 600 s, 300 used: 50%.
    let (sid, res) = r.turn(Some((600, 300))).await;
    assert!(res.loops >= 9, "{}", res.loops);
    assert_eq!(res.stop_reason, "no_tool_calls");
    let reqs = r.requests();
    let last = messages(reqs.last().unwrap());
    assert_eq!(last.matches(STEER).count(), 1, "once a turn");
    assert!(!last.contains("smallest step"), "its checks pass: {last}");
    let rows = r.rows(&sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["decision"], "steer");
}

/// Spend past half under a limit that only notifies, but no deadline: told
/// nothing either.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spend_alone_does_not_steer_a_turn_with_no_deadline_under_notify() {
    let r = rig_with(hunt, |c| limit(c, crate::config::SpendLimitMode::Notify));
    let (sid, res) = r.turn(None).await;
    assert!(res.loops >= 9, "{}", res.loops);
    for req in r.requests() {
        assert!(!messages(&req).contains(STEER));
    }
    assert!(r.rows(&sid).is_empty());
}

/// The deadline's stop is the only thing that enforces a turn's deadline, and
/// it counts loops with no passing check as the base did, reading or not
/// (theseus-ikr75 join fix 1). A turn that only reads, past 0.85 of its
/// deadline, is stopped after its third loop, as a turn with no check always
/// was; its steer stays quiet at 60% (the test below).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_only_reads_is_still_stopped_at_its_deadline() {
    let r = rig(|root| {
        let mut script: Vec<Scripted> = (1..=6)
            .map(|n| {
                let file = root.join(format!("tides{n}.txt"));
                std::fs::write(&file, "high\n").unwrap();
                Scripted::tools(
                    "Reading.",
                    &[(
                        &format!("r{n}"),
                        "fs_read",
                        json!({ "path": file.display().to_string() }),
                    )],
                )
            })
            .collect();
        script.push(Scripted::text("Read it all."));
        script
    });
    // 600 s, 540 used: 90%.
    let (sid, res) = r.turn(Some((600, 540))).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 4, "the stop's request is the last");
    assert_eq!((res.loops, res.stop_reason.as_str()), (4, PROGRESS_STOP));
    assert!(messages(&reqs[3]).contains(STOP), "{}", messages(&reqs[3]));
    assert!(
        !messages(&reqs[2]).contains(STOP),
        "not before the third loop"
    );
    let rows = r.rows(&sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["decision"], "stop");
    // The steer's count held at 0 (reading is progress); the stop's did not.
    assert_eq!(rows[0]["since_pass"], 0);
    assert_eq!(rows[0]["since_check"], 3);
    assert!(
        messages(&reqs[3]).contains("no check has run in its last 3 loops"),
        "{}",
        messages(&reqs[3])
    );
}

/// The same after the turn's first edit, where a read of a new file is
/// progress for the steer: it is still no check, and the stop still fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_keeps_opening_new_files_after_an_edit_is_still_stopped() {
    let r = rig(|root| {
        let mut script = vec![Scripted::tools(
            "Writing.",
            &[(
                "w1",
                "fs_write",
                json!({"path": root.join("notes.txt").display().to_string(), "content": "high\n"}),
            )],
        )];
        for n in 1..=6 {
            let file = root.join(format!("tides{n}.txt"));
            std::fs::write(&file, "high\n").unwrap();
            script.push(Scripted::tools(
                "Reading.",
                &[(
                    &format!("r{n}"),
                    "fs_read",
                    json!({ "path": file.display().to_string() }),
                )],
            ));
        }
        script.push(Scripted::text("Read it all."));
        script
    });
    // 600 s, 540 used: 90%.
    let (sid, res) = r.turn(Some((600, 540))).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 4, "the stop's request is the last");
    assert_eq!((res.loops, res.stop_reason.as_str()), (4, PROGRESS_STOP));
    assert!(messages(&reqs[3]).contains(STOP), "{}", messages(&reqs[3]));
    let rows = r.rows(&sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["decision"], "stop");
    assert_eq!(rows[0]["since_pass"], 0);
    assert_eq!(rows[0]["since_check"], 3);
}

/// Reading is progress until there is something to check: a turn that only
/// reads, past half its deadline, is told nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_has_not_edited_yet_is_not_steered_for_reading() {
    let r = rig(|root| {
        let file = root.join("tides.txt").display().to_string();
        let mut script: Vec<Scripted> = (1..=6)
            .map(|n| {
                Scripted::tools(
                    "Reading.",
                    &[(&format!("r{n}"), "fs_read", json!({ "path": file }))],
                )
            })
            .collect();
        script.push(Scripted::text("Read it all."));
        script
    });
    // 600 s, 360 used: 60%.
    let (sid, res) = r.turn(Some((600, 360))).await;
    assert_eq!((res.loops, res.stop_reason.as_str()), (7, "no_tool_calls"));
    for req in r.requests() {
        assert!(!messages(&req).contains(STEER));
    }
    assert!(r.rows(&sid).is_empty());
}

/// The first loop writes a file, the next starts `sleep` in the background.
fn build(root: &Path) -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "Writing.",
            &[(
                "w1",
                "fs_write",
                json!({"path": root.join("notes.txt").display().to_string(), "content": "high\n"}),
            )],
        ),
        Scripted::tools(
            "Building.",
            &[(
                "b1",
                "proc_run",
                json!({"argv": ["sleep", "20"], "background": true}),
            )],
        ),
    ]
}

/// A read of the file `build` wrote, as loop `id`'s one call.
fn reread(root: &Path, id: &str) -> Scripted {
    Scripted::tools(
        "Looking again.",
        &[(
            id,
            "fs_read",
            json!({"path": root.join("notes.txt").display().to_string()}),
        )],
    )
}

/// A build in the background is progress (theseus-duzha): a turn that
/// started one and then reads it, loop after loop, is told nothing at 60% of
/// its deadline and not stopped at 90%.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_waits_on_a_background_build_is_neither_steered_nor_stopped() {
    for used in [360, 540] {
        let r = rig(|root| {
            let mut script = build(root);
            for n in 1..=7 {
                script.push(Scripted::tools(
                    "Reading the build.",
                    &[(&format!("j{n}"), "job_read", json!({"job": "j1"}))],
                ));
            }
            script.push(Scripted::text("The build is still running."));
            script
        });
        let (sid, res) = r.turn(Some((600, used))).await;
        assert_eq!(
            (res.loops, res.stop_reason.as_str()),
            (10, "no_tool_calls"),
            "{used} s used"
        );
        for req in r.requests() {
            assert!(!messages(&req).contains(STEER), "{used} s used");
            assert!(!messages(&req).contains(STOP), "{used} s used");
        }
        assert!(r.rows(&sid).is_empty(), "{used} s used");
    }
}

/// The steer is still there: a build that runs does not quiet a turn that
/// is not looking at it, but re-reading a file, and the same job's loops
/// once the build is not what they touch count as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_running_build_does_not_quiet_loops_that_do_not_touch_it() {
    let r = rig(|root| {
        let mut script = build(root);
        for n in 1..=6 {
            script.push(reread(root, &format!("r{n}")));
        }
        script.push(Scripted::text("Nothing new."));
        script
    });
    // 600 s, 360 used: 60%.
    let (sid, res) = r.turn(Some((600, 360))).await;
    assert_eq!(res.loops, 9);
    let reqs = r.requests();
    assert!(messages(reqs.last().unwrap()).contains(STEER));
    let rows = r.rows(&sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["decision"], "steer");
    assert_eq!(rows[0]["since_pass"], 3);
}

/// And the same turn with no build at all is steered, and at 90% stopped, so
/// a build changes nothing but its own loops.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_with_no_build_and_no_progress_is_still_steered_and_stopped() {
    for (used, decision) in [(360, "steer"), (540, "stop")] {
        let r = rig(|root| {
            let mut script = vec![build(root).remove(0)];
            for n in 1..=6 {
                script.push(reread(root, &format!("r{n}")));
            }
            script.push(Scripted::text("Nothing new."));
            script
        });
        let (sid, res) = r.turn(Some((600, used))).await;
        let rows = r.rows(&sid);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0]["decision"], decision);
        assert_eq!(res.stop_reason == PROGRESS_STOP, decision == "stop");
    }
}

const HINT: &str = "call what you built once, the way the task spells it";
const DOUBT: &str = "your last message names something it did not check or run";

/// A file written, as loop `id`'s one call.
fn write(root: &Path, id: &str) -> Scripted {
    Scripted::tools(
        "Writing.",
        &[(
            id,
            "fs_write",
            json!({"path": root.join("tool.py").display().to_string(), "content": "x\n"}),
        )],
    )
}

/// The deliverable's hint rides the request's tail from the loop after the
/// turn's first file edit (theseus-eb4o4), and a turn that wrote none never
/// carries it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_wrote_a_file_is_told_to_call_it_as_the_task_spells_it() {
    let r = rig(|root| {
        vec![
            write(root, "w1"),
            command(root, "c1", 0),
            Scripted::text("Done."),
        ]
    });
    let (_, res) = r.turn(None).await;
    assert_eq!(res.loops, 3);
    let reqs = r.requests();
    assert!(!messages(&reqs[0]).contains(HINT), "no work yet");
    for req in &reqs[1..] {
        assert_eq!(messages(req).matches(HINT).count(), 1, "once a request");
    }
    // A chat turn that only answers carries none, and neither does one that
    // only ran commands: the tails of such turns are what they were.
    let r = rig(|_| vec![Scripted::text("Hello.")]);
    r.turn(None).await;
    assert!(!messages(&r.requests()[0]).contains(HINT));
    let r = rig(|root| {
        vec![
            command(root, "c1", 0),
            command(root, "c2", 0),
            Scripted::text("Done."),
        ]
    });
    r.turn(None).await;
    for req in r.requests() {
        assert!(!messages(&req).contains(HINT));
    }
}

/// A closing message that names a doubt, early in the turn, sends it back
/// once (theseus-rqu7j); the second such message ends it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_doubt_the_closing_message_names_sends_an_early_turn_back_once() {
    let r = rig(|root| {
        vec![
            write(root, "w1"),
            Scripted::text("Wrote tool.py, but the sandbox has no torch, so I never ran it."),
            Scripted::text("Still not run: torch is missing."),
            Scripted::text("Never reached."),
        ]
    });
    // 600 s, 120 used: 20%.
    let (sid, res) = r.turn(Some((600, 120))).await;
    assert_eq!((res.loops, res.stop_reason.as_str()), (3, "no_tool_calls"));
    let reqs = r.requests();
    assert_eq!(reqs.len(), 3);
    assert!(!messages(&reqs[1]).contains(DOUBT));
    assert_eq!(messages(&reqs[2]).matches(DOUBT).count(), 1);
    let rows = r.rows(&sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["decision"], "doubt");
    assert_eq!(rows[0]["loop"], 1);
    assert!(rows[0]["time_share"].as_f64().unwrap() < 0.4);
}

/// And not when the gate is shut: past 40% of the time, past 40% of the
/// spend, a clean closing message, or a turn that did no work.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_doubt_does_not_send_a_turn_back_past_the_share_or_without_work() {
    let doubt = "I did not check the date.";
    // 600 s, 300 used: 50%.
    let r = rig(|root| {
        vec![
            write(root, "w1"),
            Scripted::text(doubt),
            Scripted::text("x"),
        ]
    });
    let (sid, res) = r.turn(Some((600, 300))).await;
    assert_eq!(res.loops, 2, "past 40% of the time");
    assert!(r.rows(&sid).is_empty());
    // Under the time share, over the spend share ($0.47 of $1).
    let r = rig_with(
        |root| {
            vec![
                billed(write(root, "w1")),
                Scripted::text(doubt),
                Scripted::text("x"),
            ]
        },
        |c| {
            c.kernel.spend_limit_usd = 1.0;
            c.kernel.spend_limit_mode = crate::config::SpendLimitMode::Notify;
        },
    );
    let (sid, res) = r.turn(Some((600, 60))).await;
    assert_eq!(res.loops, 2, "past 40% of the spend");
    assert!(r.rows(&sid).is_empty());
    // A message that names nothing.
    let r = rig(|root| vec![write(root, "w1"), Scripted::text("Done; the check passes.")]);
    let (sid, res) = r.turn(Some((600, 60))).await;
    assert_eq!(res.loops, 2);
    assert!(r.rows(&sid).is_empty());
    // A turn that edited nothing and ran nothing.
    let r = rig(|_| vec![Scripted::text(doubt), Scripted::text("x")]);
    let (sid, res) = r.turn(Some((600, 60))).await;
    assert_eq!(res.loops, 1);
    assert!(r.rows(&sid).is_empty());
    for req in r.requests() {
        assert!(!messages(&req).contains(DOUBT));
    }
}
