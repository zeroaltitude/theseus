//! An answer from an agent's process is labelled as the agent's, and what
//! spends money is the owner's own (theseus-5rvss), with the real `theseusd`
//! and the real `theseus` CLI. The daemon reads who is behind a socket
//! connection from `/proc`: a controlling terminal and no agent host above it
//! is the owner's own terminal; no terminal, or a process under a program
//! `[policy] agent_processes` lists, is an agent's.
//!
//! The test daemon lists its own `example-agent` (`common::safe_note`), never
//! `claude`, so where the tests run (under an agent's shell, say) decides nothing. The agent's CLI is
//! a new session with no controlling terminal (`setsid`), so a test run from a
//! terminal still has none; the owner's is a child of a pty (`python3`'s
//! `os.forkpty`), which gives it one.

mod common;

use std::path::PathBuf;
use std::process::{Command, Stdio};

use common::model::FakeModel;
use common::{table, Served};
use serde_json::{json, Value};

/// The agent host the test daemon lists (`common::safe_note`).
const HOST: &str = "example-agent";

struct Rig {
    s: Served,
    cli: PathBuf,
    _model: FakeModel,
    _keys: tempfile::TempDir,
}

impl Rig {
    /// A daemon whose `fs.write` asks and whose spend limit is `limit_usd`:
    /// a tiny one makes the first model call a budget question.
    fn start(limit_usd: f64) -> Self {
        let model = FakeModel::start(|prompt| {
            if prompt.starts_with("write") {
                vec![(
                    "fs_write",
                    json!({"path": "written.txt", "content": "approved\n"}),
                )]
            } else {
                vec![]
            }
        });
        let base = model.base.clone();
        // Every secret is a file holding a stand-in value: no vault to ask.
        let keys = tempfile::tempdir().unwrap();
        let key = keys.path().join("key");
        std::fs::write(&key, "test-secret-value-0000").unwrap();
        std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        let s = Served::start(
            |_| {},
            move |t| {
                for (_, v) in table(t, "secrets").iter_mut() {
                    *v = format!("file:{}", key.display()).into();
                }
                table(t, "model").insert("api_base".into(), base.clone().into());
                for (_, p) in table(t, "providers").iter_mut() {
                    p.as_table_mut()
                        .unwrap()
                        .insert("api_base".into(), base.clone().into());
                }
                let policy = table(t, "policy");
                policy.insert("enforcement".into(), "notify".into());
                table(policy, "tools").insert("fs.write".into(), "approve".into());
                let kernel = table(t, "kernel");
                kernel.insert("spend_limit_usd".into(), limit_usd.into());
                kernel.insert("spend_limit_mode".into(), "ask".into());
            },
        );
        let cli = PathBuf::from(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus");
        assert!(
            cli.exists(),
            "no {} to run: build the CLI (cargo build -p theseus)",
            cli.display()
        );
        // A program named for an agent host: a copy of `sh`.
        std::fs::create_dir_all(s.path("bin")).unwrap();
        std::fs::copy("/bin/sh", s.path(&format!("bin/{HOST}"))).unwrap();
        Self {
            s,
            cli,
            _model: model,
            _keys: keys,
        }
    }

    /// A turn in a new session: its result, once it parks or ends.
    fn turn(&self, prompt: &str) -> Value {
        let s = self
            .s
            .call("session.open", json!({"label": prompt}))
            .unwrap();
        self.s
            .call(
                "turn.submit",
                json!({"session_id": s["session_id"], "input": prompt, "author": "test", "attachments": []}),
            )
            .unwrap()
    }

    fn cli_args(&self, args: &[&str]) -> Vec<String> {
        let mut argv = vec![
            self.cli.display().to_string(),
            "--socket".into(),
            self.s.path("sock").display().to_string(),
        ];
        argv.extend(args.iter().map(|a| a.to_string()));
        argv
    }

    fn command(&self, argv: &[String]) -> Command {
        let mut c = Command::new(&argv[0]);
        c.args(&argv[1..])
            .env("XDG_STATE_HOME", self.s.path("xdg-state"))
            .env_remove("THESEUS_SESSION")
            .stdin(Stdio::null());
        c
    }

    /// `theseus <args>` from a process with no controlling terminal: a new
    /// session, as a service's or a gateway's exec has. Its output and
    /// whether it succeeded.
    fn no_terminal(&self, args: &[&str]) -> (bool, String) {
        use std::os::unix::process::CommandExt;
        let mut c = self.command(&self.cli_args(args));
        // SAFETY: `setsid` takes nothing and is async-signal-safe.
        unsafe {
            c.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let out = c.output().unwrap();
        (out.status.success(), text(&out))
    }

    /// `argv` as the child of a pty: it has a controlling terminal, as the
    /// owner's own shell does. Whether it succeeded, and its output.
    fn on_a_terminal(&self, argv: &[String]) -> (bool, String) {
        let xdg = self.s.path("xdg-state").display().to_string();
        let (code, out) = common::terminal::on_a_terminal(argv, &[("XDG_STATE_HOME", &xdg)]);
        (code == 0, out)
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.s.call("ledger.tail", json!({"n": 2000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }

    fn open_questions(&self) -> Vec<String> {
        self.s.call("confirm.list", Value::Null).unwrap()["confirms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["correlation_id"].as_str().unwrap().to_string())
            .collect()
    }

    /// The digest's lines of what was answered, as worded.
    fn answered(&self) -> Vec<String> {
        let d = self.s.call("work.digest", json!({"since": 0})).unwrap();
        d["lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["kind"] == "answered")
            .map(|l| l["words"].as_str().unwrap().to_string())
            .collect()
    }
}

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A budget question, which spends money: a real CLI answer from a process
/// with no terminal is refused, with the words, and the question stays open;
/// the same answer from a terminal resets the spend.
#[test]
fn a_cli_answer_with_no_terminal_is_refused_for_a_spend_limit_and_a_terminal_resets_it() {
    let r = Rig::start(0.001);
    let res = r.turn("hello");
    assert_eq!(res["stop_reason"], "budget", "{res}\n{}", r.s.log());
    let q = res["awaiting_confirm"]
        .as_str()
        .expect("the question")
        .to_string();

    let (ok, out) = r.no_terminal(&["answer", &q, "reset", "--no-wait"]);
    assert!(!ok, "{out}");
    assert!(
        out.contains("this spends money or changes AWS, so only the owner answers it"),
        "{out}"
    );
    let refused = r.ledger("approval.refused");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["by"], "agent:no terminal");
    assert_eq!(
        r.open_questions(),
        std::slice::from_ref(&q),
        "the question stays open"
    );
    assert!(r.ledger("budget.reset").is_empty());

    let (ok, out) = r.on_a_terminal(&r.cli_args(&["answer", &q, "reset", "--no-wait"]));
    assert!(ok, "{out}\n{}", r.s.log());
    let reset = r.ledger("budget.reset");
    assert_eq!(reset.len(), 1, "{reset:?}");
    assert_eq!(reset[0]["by"], "the CLI");
}

/// An ordinary approval is an agent's to answer: it runs, the ledger and the
/// digest say an agent answered it, and name it by the program above it when
/// there is one, over the terminal it has.
#[test]
fn an_agents_cli_answer_to_an_ordinary_approval_runs_and_is_labelled_as_the_agents() {
    // From no terminal.
    let r = Rig::start(100.0);
    let res = r.turn("write it");
    let q = res["awaiting_confirm"]
        .as_str()
        .expect("the write waits")
        .to_string();
    let (ok, out) = r.no_terminal(&["answer", &q, "approve", "--no-wait"]);
    assert!(ok, "{out}\n{}", r.s.log());
    let rows = r.ledger("action.confirm_answered");
    assert_eq!(rows[0]["by"], "agent:no terminal", "{rows:?}");
    assert!(r.ledger("approval.refused").is_empty());
    // `--no-wait` returns once the answer is recorded; the resumed turn writes the file after it.
    let path = r.s.path("projects/written.txt");
    let t0 = std::time::Instant::now();
    while !std::fs::read_to_string(&path).is_ok_and(|s| !s.is_empty())
        && t0.elapsed() < std::time::Duration::from_secs(30)
    {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let written = std::fs::read_to_string(&path);
    assert_eq!(written.as_deref().ok(), Some("approved\n"), "{}", r.s.log());
    let said = r.answered();
    assert!(
        said.len() == 1 && said[0].starts_with("✓ answered by an agent (no terminal): fs.write"),
        "{said:?}"
    );

    // Under a listed agent host, on a terminal: the host names it.
    let r = Rig::start(100.0);
    let res = r.turn("write it");
    let q = res["awaiting_confirm"]
        .as_str()
        .expect("the write waits")
        .to_string();
    let script = r
        .cli_args(&["answer", &q, "approve", "--no-wait"])
        .join(" ");
    let host = r.s.path(&format!("bin/{HOST}")).display().to_string();
    let (ok, out) = r.on_a_terminal(&[host, "-c".into(), script]);
    assert!(ok, "{out}\n{}", r.s.log());
    let rows = r.ledger("action.confirm_answered");
    assert_eq!(rows[0]["by"], format!("agent:{HOST}"), "{rows:?}");

    // On a terminal and under no agent: the owner's own.
    let r = Rig::start(100.0);
    let res = r.turn("write it");
    let q = res["awaiting_confirm"]
        .as_str()
        .expect("the write waits")
        .to_string();
    let (ok, out) = r.on_a_terminal(&r.cli_args(&["answer", &q, "approve", "--no-wait"]));
    assert!(ok, "{out}\n{}", r.s.log());
    assert_eq!(r.ledger("action.confirm_answered")[0]["by"], "the CLI");
    let said = r.answered();
    assert!(
        said.len() == 1 && said[0].starts_with("✓ you answered: fs.write"),
        "{said:?}"
    );
}
