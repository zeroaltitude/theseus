//! The scratch runner's tests (theseus-pw1q.21.5): a set scored with the
//! stand-in model behind the relay, the cost from the relay's meter, the
//! runner's directory gone after, no task text in the store or the logs, and
//! the view a job would get. The jobs run through a stand-in launcher, since
//! L1 needs a non-root daemon with a job cgroup; the live run is the
//! maintainer's (CLOUD_REPORT.md).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_kernel::job::L1;
use theseus_kernel::probe_run::ProbeRun;

use super::relay::Relay;
use super::runner::{runner_view, Candidate, Job, Launch, BINARIES, DAEMON_ENV};
use super::tests_relay::{ask, request, rig, Answers, Rig, KEY};
use crate::rpc::Core;

const CANARY: &str = "saffron-meridian-4c1e-canary";

/// The candidate, in a stand-in: it asks the relay for the task as its
/// provider client would, and prints the reply as `ask --json` does, with a
/// cost of its own that nothing may believe. A task with `CRASH` in it exits
/// 1.
struct Pretend {
    relay: Arc<Relay>,
    model: String,
    jobs: Arc<Mutex<Vec<Job>>>,
    during: Box<dyn Fn() + Send + Sync>,
}

impl Launch for Pretend {
    fn run(&self, job: &Job) -> ProbeRun {
        self.jobs.lock().unwrap().push(job.clone());
        (self.during)();
        let task = job.argv.last().unwrap();
        let run = |code, stdout: String| ProbeRun {
            code: Some(code),
            stdout,
            stderr: String::new(),
            failed: None,
        };
        if task.contains("CRASH") {
            return run(1, String::new());
        }
        let (status, body) = ask(&self.relay, &request(&self.model, task));
        if status != 200 {
            return run(1, String::new());
        }
        let reply = if body.contains("HELLO") { "HELLO" } else { "" };
        run(0, json!({"output": reply, "cost_usd": 0.0}).to_string())
    }
}

/// A directory holding a stand-in build.
fn build() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap();
    for b in BINARIES {
        std::fs::write(dir.join(b), format!("#!/bin/sh\n# stand-in {b}\n")).unwrap();
    }
    (d, dir)
}

fn candidate(r: &Rig, bins: &Path) -> Candidate {
    Candidate {
        branch: "self/tide".into(),
        bins: bins.to_path_buf(),
        profile: r.profile.clone(),
    }
}

fn put(r: &Rig, family: &str, task: &str, check: &str) {
    let item = json!({"v": 1, "task": task, "check": check});
    r.core
        .sealed_put("tides", family, item.to_string().as_bytes(), "reviewer-a")
        .unwrap();
}

/// What a scored run leaves to look at.
struct Ran {
    scores: super::Scores,
    relay: Arc<Relay>,
    jobs: Arc<Mutex<Vec<Job>>>,
}

async fn score(r: &Rig, bins: &Path, during: impl Fn() + Send + Sync + 'static) -> Ran {
    let slot: Arc<Mutex<Option<Arc<Relay>>>> = Arc::default();
    let jobs: Arc<Mutex<Vec<Job>>> = Arc::default();
    let (s, j, model) = (slot.clone(), jobs.clone(), r.model.clone());
    let scores = r
        .core
        .sealed_score_with("tides", candidate(r, bins), "abc123", move |_, relay| {
            *s.lock().unwrap() = Some(relay.clone());
            Box::new(Pretend {
                relay: relay.clone(),
                model,
                jobs: j,
                during: Box::new(during),
            })
        })
        .await
        .unwrap();
    let relay = slot.lock().unwrap().take().unwrap();
    Ran {
        scores,
        relay,
        jobs,
    }
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(5000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == kind)
        .map(|(_, row)| row.data)
        .collect()
}

fn six(r: &Rig) {
    for (i, check) in ["HELLO", "HELLO", "NOPE"].iter().enumerate() {
        put(
            r,
            "fact",
            &format!("Fact question {i}?"),
            &format!("reply has \"{check}\""),
        );
    }
    for i in 0..3 {
        put(
            r,
            "route",
            &format!("Route question {i}?"),
            "reply has \"HELLO\"",
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_set_of_two_families_is_answered_and_scored_at_the_relays_cost() {
    let r = rig(
        Answers::Whole {
            input: 1000,
            output: 200,
        },
        |_| {},
    )
    .await;
    six(&r);
    let (_d, bins) = build();
    let ran = score(&r, &bins, || {}).await;
    let s = &ran.scores;
    assert_eq!((s.items, s.passed, s.failed, s.errors), (6, 5, 1, 0));
    assert_eq!(s.families["fact"].items, 3);
    assert_eq!(
        (s.families["fact"].passed, s.families["fact"].failed),
        (2, 1)
    );
    assert_eq!(
        (s.families["route"].passed, s.families["route"].rate),
        (3, 1.0)
    );
    assert_eq!(s.failures.len(), 1);
    // The cost is what the relay metered; the candidate said 0.
    let one = r
        .core
        .catalog
        .get(&r.model)
        .unwrap()
        .cost_usd(&theseus_protocol::Usage {
            input_tokens: 1000,
            output_tokens: 200,
            ..Default::default()
        });
    assert!(
        s.cost_usd > 0.0 && (s.cost_usd - 6.0 * one).abs() < 1e-9,
        "{} vs {}",
        s.cost_usd,
        6.0 * one
    );
    assert!((ran.relay.spent_usd() - s.cost_usd).abs() < 1e-9);
    assert_eq!(r.up.seen.lock().unwrap().len(), 6);
    // One row for (set, branch, head), with the metered cost and no text.
    let scored = rows(&r.core, "self.sealed.scored");
    assert_eq!(scored.len(), 1);
    assert_eq!(scored[0]["set"], "tides");
    assert_eq!(scored[0]["branch"], "self/tide");
    assert_eq!(scored[0]["head"], "abc123");
    assert_eq!(scored[0]["passed"], 5);
    assert!((scored[0]["cost_usd"].as_f64().unwrap() - s.cost_usd).abs() < 1e-9);
    assert!(
        !scored[0].to_string().contains("question"),
        "no item's text in the row"
    );
    // The jobs were handed the daemon's two variables and nothing of ours.
    let jobs = ran.jobs.lock().unwrap();
    for j in jobs.iter() {
        let mut names: Vec<&str> = j.env.iter().map(|(k, _)| k.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "SEALED_RELAY_KEY",
                "THESEUS_CONFIG",
                "THESEUS_STATE_DIR",
                "TOKIO_WORKER_THREADS"
            ]
        );
        assert!(
            j.env.iter().all(|(_, v)| !v.contains(KEY)),
            "the real key is not in the job"
        );
        assert_eq!(j.deadline_ms, r.core.cfg.route_bench.whole_seconds * 1000);
    }
    assert_eq!(DAEMON_ENV, ["THESEUS_CONFIG", "THESEUS_STATE_DIR"]);
}

#[tokio::test(flavor = "current_thread")]
async fn the_scratch_dir_is_gone_after() {
    let r = rig(
        Answers::Whole {
            input: 10,
            output: 10,
        },
        |_| {},
    )
    .await;
    put(&r, "fact", "Which tide?", "reply has \"HELLO\"");
    let (_d, bins) = build();
    let root = r.state.join("self").join("runner").join("self-tide");
    let during = {
        let (root, bins) = (root.clone(), bins.clone());
        move || {
            // While it runs: the build copied read-only with its digests, and
            // the config beside it.
            for b in BINARIES {
                let copy = root.join("bin").join(b);
                let mode = std::fs::metadata(&copy).unwrap().permissions();
                assert!(mode.readonly(), "{b} is read-only");
                assert_eq!(
                    std::fs::read(&copy).unwrap(),
                    std::fs::read(bins.join(b)).unwrap()
                );
            }
            assert!(root.join("cfg").join("theseus.toml").is_file());
            assert!(std::fs::metadata(&root).unwrap().permissions().readonly());
        }
    };
    let ran = score(&r, &bins, during).await;
    assert_eq!(ran.scores.passed, 1);
    assert!(!root.exists(), "the runner's directory is removed with it");
    assert!(!r
        .state
        .join("self")
        .join("runner")
        .join("self-tide")
        .exists());
}

#[tokio::test(flavor = "current_thread")]
async fn the_copies_are_digested_and_read_only() {
    let r = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |_| {},
    )
    .await;
    let (_d, bins) = build();
    let (core, c) = (r.core.clone(), candidate(&r, &bins));
    let digests = tokio::task::spawn_blocking(move || {
        let runner = super::runner::ScratchRunner::with_launch(&core, &c, |_, relay| {
            Box::new(Pretend {
                relay: relay.clone(),
                model: String::new(),
                jobs: Arc::default(),
                during: Box::new(|| {}),
            })
        })
        .unwrap();
        runner.digests().to_vec()
    })
    .await
    .unwrap();
    let names: Vec<&str> = digests.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, BINARIES);
    for (name, sum) in &digests {
        assert_eq!(
            *sum,
            hex::encode(Sha256::digest(std::fs::read(bins.join(name)).unwrap()))
        );
    }
    assert!(
        !r.state
            .join("self")
            .join("runner")
            .join("self-tide")
            .exists(),
        "dropped: gone"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn no_task_text_reaches_the_store_or_the_logs() {
    use std::io::Write;
    #[derive(Clone, Default)]
    struct Log(Arc<Mutex<Vec<u8>>>);
    impl Write for Log {
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
    let log = Log::default();
    let sub = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);

    let r = rig(
        Answers::Whole {
            input: 10,
            output: 10,
        },
        |_| {},
    )
    .await;
    put(
        &r,
        "fact",
        &format!("Recite {CANARY} please"),
        "reply has \"HELLO\"",
    );
    put(
        &r,
        "fact",
        &format!("CRASH on {CANARY}"),
        "reply has \"HELLO\"",
    );
    let (_d, bins) = build();
    let ran = score(&r, &bins, || {}).await;
    assert_eq!((ran.scores.passed, ran.scores.errors), (1, 1));
    // The provider was shown the task (it must be), and that is all.
    assert!(r
        .up
        .seen
        .lock()
        .unwrap()
        .iter()
        .any(|s| String::from_utf8_lossy(&s.body).contains(CANARY)));
    // The relay's log, the daemon's tracing, the ledger, and every file under
    // the state dir.
    for line in ran.relay.log() {
        assert!(!line.to_string().contains(CANARY));
    }
    let traced = String::from_utf8_lossy(&log.0.lock().unwrap()).into_owned();
    assert!(
        traced.contains("sealed relay:"),
        "the usage line is logged: {traced}"
    );
    assert!(!traced.contains(CANARY), "{traced}");
    let ledger = serde_json::to_string(
        &r.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(5000)
            .unwrap(),
    )
    .unwrap();
    assert!(!ledger.contains(CANARY));
    assert!(
        !tree_holds(&r.state, CANARY.as_bytes()),
        "no file of the production state holds it"
    );
}

/// Whether any file under `dir` holds `needle`.
fn tree_holds(dir: &Path, needle: &[u8]) -> bool {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let p = e.path();
            if p.is_dir() {
                tree_holds(&p, needle)
            } else {
                std::fs::read(&p).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle))
            }
        })
}

#[tokio::test(flavor = "current_thread")]
async fn an_item_the_relay_stops_is_a_failed_run() {
    let r = rig(
        Answers::Whole {
            input: 10,
            output: 10,
        },
        |c| {
            c.self_improve.sealed.item_usd = 0.00001;
        },
    )
    .await;
    six(&r);
    let (_d, bins) = build();
    let ran = score(&r, &bins, || {}).await;
    assert_eq!(
        (ran.scores.passed, ran.scores.errors, ran.scores.items),
        (0, 6, 6)
    );
    assert_eq!(ran.scores.cost_usd, 0.0);
    assert!(
        r.up.seen.lock().unwrap().is_empty(),
        "nothing was forwarded"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn nothing_runs_while_self_improvement_is_off_or_halted() {
    for mode in [
        crate::config::self_improve::SelfMode::Off,
        crate::config::self_improve::SelfMode::Act,
    ] {
        let r = rig(
            Answers::Whole {
                input: 1,
                output: 1,
            },
            |c| c.self_improve.mode = mode,
        )
        .await;
        put(&r, "fact", "Which tide?", "reply has \"HELLO\"");
        if mode == crate::config::self_improve::SelfMode::Act {
            r.core
                .self_halt(
                    &Default::default(),
                    &crate::approval::Answerer::from("the CLI"),
                )
                .unwrap();
        }
        let (_d, bins) = build();
        let e = r
            .core
            .sealed_score_with("tides", candidate(&r, &bins), "abc123", |_, _| {
                unreachable!("no launch")
            })
            .await
            .unwrap_err();
        assert!(
            format!("{e:#}").contains("does not run while self-improvement is"),
            "{e:#}"
        );
        assert!(rows(&r.core, "self.sealed.scored").is_empty());
        assert!(
            !r.state.join("self").join("runner").exists(),
            "nothing was made"
        );
        assert!(r.up.seen.lock().unwrap().is_empty());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_runner_view_holds_neither_the_sealed_dir_nor_the_production_state() {
    let r = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |c| {
            c.sandbox.ro_paths = vec!["/opt".into()];
        },
    )
    .await;
    let (_d, bins) = build();
    let built: Arc<Mutex<Option<(L1, String)>>> = Arc::default();
    let b = built.clone();
    let _ = r
        .core
        .sealed_score_with(
            "none",
            candidate(&r, &bins),
            "abc123",
            move |view, relay| {
                *b.lock().unwrap() = Some((view.clone(), relay.entry()));
                Box::new(Pretend {
                    relay: relay.clone(),
                    model: String::new(),
                    jobs: Arc::default(),
                    during: Box::new(|| {}),
                })
            },
        )
        .await;
    // An empty set stops the score, after the view was built.
    let (view, entry) = built.lock().unwrap().take().unwrap();
    let root = r.state.join("self").join("runner").join("self-tide");
    assert!(view.workspace.is_empty(), "no workspace");
    assert_eq!(
        view.ro_paths,
        vec![root.clone()],
        "the runner's directory alone, not [sandbox] ro_paths"
    );
    assert!(
        view.ro_paths
            .iter()
            .all(|p| !p.starts_with(&r.sealed) && !r.sealed.starts_with(p)),
        "no bind holds the sealed dir"
    );
    assert!(
        !view
            .ro_paths
            .iter()
            .any(|p| r.state.starts_with(p) || (p.starts_with(&r.state) && *p != root)),
        "none of the production state"
    );
    assert!(
        view.hidden.contains(&r.sealed),
        "the sealed dir is covered whatever binds it"
    );
    assert!(
        view.hidden.iter().all(|h| !root.starts_with(h)),
        "and nothing covers the runner's own dir"
    );
    assert_eq!(
        view.egress,
        vec![entry],
        "the relay's name is the whole egress list"
    );
    assert!(view.egress_dns.is_none());
    // The pure builder, from any base.
    let base = L1 {
        hidden: vec![PathBuf::from("/srv/x"), PathBuf::from("/srv")],
        workspace: vec!["/w".into()],
        rw: vec!["/w".into()],
        ..L1::default()
    };
    let v = runner_view(&base, Path::new("/srv/run"), "n:80");
    assert_eq!(v.hidden, vec![PathBuf::from("/srv/x")]);
    assert!(v.workspace.is_empty());
    assert!(v.rw.is_empty(), "an item's job binds nothing read-write");
}

/// The maintainer's live check: this tree's own build, in a real L1, through
/// the relay to the stand-in model. It needs a daemon that is not root, with a
/// job cgroup (L1 refuses a root daemon's job), and a built `theseusd` and
/// `theseus`:
///
/// `THESEUS_SEALED_LIVE_BINS=$PWD/target/debug cargo nextest run -p theseus-core
/// --run-ignored only -E 'test(a_real_candidate_in_l1)'`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs L1 (a non-root daemon with a job cgroup) and a built theseusd and theseus"]
async fn a_real_candidate_in_l1_answers_through_the_relay() {
    let bins = PathBuf::from(
        std::env::var("THESEUS_SEALED_LIVE_BINS")
            .expect("the directory holding theseusd and theseus"),
    );
    let r = rig(
        Answers::Whole {
            input: 1000,
            output: 100,
        },
        |_| {},
    )
    .await;
    put(&r, "fact", "Say hello.", "reply has \"HELLO\"");
    let scores = r
        .core
        .sealed_score_branch("tides", candidate(&r, &bins), "live")
        .await
        .unwrap();
    assert_eq!((scores.passed, scores.errors), (1, 0), "{scores:?}");
    assert!(scores.cost_usd > 0.0, "the relay metered the calls");
    assert!(
        !r.up.seen.lock().unwrap().is_empty(),
        "the stand-in model was asked, through the relay"
    );
    assert!(!r
        .state
        .join("self")
        .join("runner")
        .join("self-tide")
        .exists());
}

/// The same real build, the generated config and the relay, with only L1's
/// namespaces left out: the job is a plain child whose proxy variable names a
/// loopback listener that hands each connection to the relay, as L1's egress
/// listener does. It shows the candidate's provider client reaches the relay
/// in the proxy's absolute form, and that its turn ends with the stand-in
/// model's reply. (Run it as the live check above.)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a built theseusd and theseus"]
async fn a_real_candidate_over_a_loopback_listener_answers_through_the_relay() {
    struct Plain {
        proxy: String,
        home: PathBuf,
    }
    impl Launch for Plain {
        fn run(&self, j: &Job) -> ProbeRun {
            let mut cmd = std::process::Command::new(&j.argv[0]);
            cmd.args(&j.argv[1..])
                .env_clear()
                .current_dir(&self.home)
                .env("HOME", &self.home);
            for (k, v) in &j.env {
                cmd.env(
                    k,
                    if k == "THESEUS_STATE_DIR" {
                        self.home.join("state").display().to_string()
                    } else {
                        v.clone()
                    },
                );
            }
            cmd.env("PATH", "/usr/bin:/bin")
                .env("HTTP_PROXY", &self.proxy)
                .env("http_proxy", &self.proxy);
            let out = cmd.output().unwrap();
            ProbeRun {
                code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                failed: None,
            }
        }
    }
    let bins = PathBuf::from(
        std::env::var("THESEUS_SEALED_LIVE_BINS")
            .expect("the directory holding theseusd and theseus"),
    );
    let r = rig(
        Answers::Whole {
            input: 1000,
            output: 100,
        },
        |_| {},
    )
    .await;
    put(&r, "fact", "Say hello.", "reply has \"HELLO\"");
    let home = tempfile::tempdir().unwrap();
    let h = home.path().to_path_buf();
    let scores = r
        .core
        .sealed_score_with("tides", candidate(&r, &bins), "live", move |_, relay| {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let proxy = format!("http://{}", listener.local_addr().unwrap());
            let relay = relay.clone();
            std::thread::spawn(move || {
                for conn in listener.incoming().flatten() {
                    relay.serve_connection(conn);
                }
            });
            Box::new(Plain { proxy, home: h })
        })
        .await
        .unwrap();
    assert_eq!((scores.passed, scores.errors), (1, 0), "{scores:?}");
    assert!(scores.cost_usd > 0.0, "the relay metered the calls");
    let seen = r.up.seen.lock().unwrap();
    assert!(
        !seen.is_empty(),
        "the stand-in model was asked, through the relay"
    );
    assert!(
        seen.iter().all(|s| s.key.as_deref() == Some(KEY)),
        "with the daemon's key, never the placeholder"
    );
}
