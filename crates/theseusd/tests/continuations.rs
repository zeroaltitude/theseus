//! A continuation runs on the profile its session's last turn ran on, never
//! on the live one (theseus-kol), with the real `theseusd` and a stand-in for
//! the Messages API. The template's live profile is `sonnet`
//! (claude-sonnet-5-5); every turn here is asked on `glm` (an unrestricted
//! model here, theseus-a1wvq.3, on provider `zai`), whose endpoint is the same stand-in, which keeps every
//! request, so each continuation's model is read from what it was asked.
//! - a job that outlives `proc_sync_secs`: its late result's turn;
//! - a SIGKILL while a job runs inside its turn: the restart's continuation,
//!   then the late result's;
//! - a wake the turn set: the wake's turn.

mod common;

use common::rpc::call;
use common::{table, Fixture, FAKE_OP};

use std::path::{Path, PathBuf};

use common::model::FakeModel;
use serde_json::{json, Value};

const GLM: &str = common::UNRESTRICTED_SECOND;

struct Rig {
    dir: tempfile::TempDir,
    model: FakeModel,
}

impl Fixture for Rig {
    fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Rig {
    /// `Run the job` gets a `proc.run` that marks its start in the projects
    /// dir and sleeps `job_secs`; `Wake me soon` gets a wake two seconds
    /// ahead; anything else gets text.
    fn new(proc_sync_secs: u64, job_secs: u64) -> Self {
        let model = FakeModel::start(move |prompt| {
            if prompt.contains("Run the job") {
                let script = format!("touch started; sleep {job_secs}; echo the job is done");
                vec![("proc_run", json!({"argv": ["sh", "-c", script]}))]
            } else if prompt.contains("Wake me soon") {
                vec![("wake_at", json!({"after": "2s", "note": "check the job"}))]
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
        // `zai` is only the second provider here (theseus-a1wvq.3).
        common::unrestricted_glm(&mut t);
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), (proc_sync_secs as i64).into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, model }
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.call("ledger.tail", json!({"n": 500, "kind": kind}))
            .unwrap()["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// Wait until the stand-in has been asked about the job's late result,
    /// and every turn started has ended but the `cut` ones a kill ended.
    fn late_result_answered(&self, cut: usize) {
        self.wait("the late result's turn", || {
            let asked = self.model.requests().iter().any(late_text);
            let started = self.rows("turn.started").len();
            (asked && started > cut && self.rows("turn.ended").len() + cut == started).then_some(())
        });
    }

    /// Every model call went to `glm`, and every turn ran on it, the
    /// continuations included (`n_continuations` of them, at least).
    fn all_on_glm(&self, n_continuations: usize) {
        let asked: Vec<String> = self
            .model
            .requests()
            .iter()
            .map(|r| r["model"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(!asked.is_empty());
        assert!(
            asked.iter().all(|m| m == GLM),
            "every call goes to {GLM}, never the live profile's model: {asked:?}"
        );
        let started = self.rows("turn.started");
        let continuations = started
            .iter()
            .filter(|r| r["data"]["continuation"] == true)
            .count();
        assert!(
            continuations >= n_continuations,
            "{continuations} continuations: {started:?}"
        );
        for r in &started {
            assert_eq!(r["data"]["profile"], "glm", "{r}");
            assert_eq!(r["data"]["model"], GLM, "{r}");
        }
    }
}

/// A turn asked on `glm`, on a new session.
fn ask_glm(sock: &Path, prompt: &str) -> Result<Value, Value> {
    call(
        sock,
        "turn.submit",
        json!({"input": prompt, "profile": "glm", "author": "test", "attachments": []}),
    )
}

/// Whether a request's last message is a job's late result.
fn late_text(req: &Value) -> bool {
    req["messages"]
        .as_array()
        .and_then(|m| m.last())
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| {
            c.iter().any(|b| {
                b["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("Background result"))
            })
        })
}

/// A job that outlives `proc_sync_secs` is answered `background`, and its
/// late result's turn runs on the profile the turn that started it ran on.
#[test]
fn a_late_results_turn_runs_on_the_profile_that_started_the_job() {
    let r = Rig::new(1, 3);
    let _daemon = r.spawn();
    let first = ask_glm(&r.path("sock"), "Run the job").unwrap();
    assert_eq!(first["profile"], "glm", "{first}");
    // A socket daemon's result names nothing left for later: its late
    // results come back on their own (theseus-mqxk).
    assert!(first.get("later").is_none(), "{first}");
    r.late_result_answered(0);
    r.all_on_glm(1);
}

/// A SIGKILL while the job runs inside its turn: the restarted daemon's
/// continuation (the restart's placeholder for the call) and the late
/// result's turn both run on `glm`, and the late result is answered.
#[test]
fn a_restarts_continuation_runs_on_the_profile_of_the_turn_it_cut() {
    let r = Rig::new(30, 4);
    let daemon = r.spawn();
    let sock = r.path("sock");
    let asking = std::thread::spawn(move || ask_glm(&sock, "Run the job"));
    r.wait("the job's start", || {
        r.path("projects/started").exists().then_some(())
    });
    drop(daemon);
    // The connection closes with the daemon, unanswered.
    assert!(asking.join().unwrap().is_err());
    let _daemon = r.spawn();
    r.late_result_answered(1);
    r.all_on_glm(2);
    let placeholder = r.model.requests().iter().any(|q| {
        q["messages"]
            .to_string()
            .contains("the harness restarted meanwhile")
    });
    assert!(placeholder, "the restart's placeholder was read");
    assert_eq!(r.rows("execution.interrupted").len(), 1);
}

/// A wake the turn set: its turn runs on the profile the session's last turn
/// ran on, not the live one.
#[test]
fn a_wakes_turn_runs_on_the_profile_of_the_turn_that_set_it() {
    let r = Rig::new(1, 1);
    let _daemon = r.spawn();
    let first = ask_glm(&r.path("sock"), "Wake me soon").unwrap();
    assert_eq!(first["output"], "Done.", "{first}");
    r.wait("the wake's turn", || {
        let fired = r.rows("wake.fired").len();
        let started = r.rows("turn.started").len();
        (fired == 1 && started == 2 && r.rows("turn.ended").len() == 2).then_some(())
    });
    r.all_on_glm(1);
    // The socket daemon's words for the wake are as they were, and its
    // result names nothing left (theseus-mqxk).
    assert!(first.get("later").is_none(), "{first}");
    let said = r.model.requests().iter().any(|q| {
        let m = q["messages"].to_string();
        m.contains("This conversation gets a turn then") && !m.contains("will not fire")
    });
    assert!(said, "the wake's answer changed");
}
