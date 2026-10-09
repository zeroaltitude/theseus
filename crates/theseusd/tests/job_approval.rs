//! A Theseus job cannot answer for the operator from its own shell
//! (theseus-zmgb), with the real `theseusd`, its real job wrappers, and the
//! real `theseus` CLI. A stand-in for the Messages API asks for the tool
//! calls. Every job here runs under `notify`, as the owner's posture has it, so
//! nothing but the answer itself is in the way:
//! - a job's `theseus confirm`, and its double-forked grandchild's, before and
//!   after the job's main process has exited, are refused by the CLI, since
//!   each carries the job's `THESEUS_SESSION`, and nothing reaches the
//!   daemon; the wrapper lingers for the grandchild; the operator's own
//!   answer counts, and the call runs;
//! - a job that kills its own wrapper leaves its grandchild to the daemon,
//!   which adopts and reaps it (theseus-z4b), and that orphan's answer is
//!   refused the same way;
//! - a job may tighten a tool, and the CLI refuses its undo of a tightening;
//! - a cancel still kills what it killed before.
//!
//! The marker is a speed bump, not a boundary: a job can strip its
//! environment, and L1, whose view has no route to the daemon, is the
//! boundary. The CLI is the `theseus` binary beside `theseusd`, which `cargo
//! nextest run --workspace` builds for the CLI's own tests. The operator's
//! commands run with the marker removed, so a test run inside a Theseus job
//! still has an operator.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_kernel::job::{ANSWER_WAIT, STOP_GRACE};

/// What the stand-in model asks for: the tool calls for each prompt, by its
/// start. Set once the rig's paths are known.
type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    script: Script,
    /// Processes a test started outside the daemon's reach: killed, with
    /// their process groups, when the rig goes, so a test that fails leaves
    /// nothing running.
    leftovers: Mutex<Vec<u32>>,
    _model: FakeModel,
}

impl Drop for Rig {
    fn drop(&mut self) {
        for pid in self.leftovers.lock().unwrap().drain(..) {
            for target in [format!("-{pid}"), pid.to_string()] {
                let _ = Command::new("kill")
                    .args(["-9", "--", &target])
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }
}

/// Where a job writes what it saw. It is inside the workspace root, as every
/// path the jobs name is, so that each `proc.run` runs under `notify`.
const OUT: &str = "projects/out";

impl Rig {
    fn start() -> Self {
        let script = Script::default();
        let asks = script.clone();
        let model = FakeModel::start(move |prompt| {
            asks.lock()
                .unwrap()
                .iter()
                .find(|(p, _)| prompt.starts_with(p.as_str()))
                .map(|(_, calls)| calls.clone())
                .unwrap_or_default()
        });
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects/bin", OUT] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        let cli = theseusd.with_file_name("theseus");
        assert!(
            cli.exists(),
            "no {} to run: build the CLI (cargo build -p theseus)",
            cli.display()
        );
        std::fs::copy(&cli, path("projects/bin/theseus")).unwrap();
        let op = path("bin/op");
        std::fs::write(
            &op,
            "#!/bin/sh\n\
             case \"$1\" in\n\
             \x20 inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n\
             \x20 read) printf '%s' test-secret-value-0000 ;;\n\
             \x20 *) exit 1 ;;\n\
             esac\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
            t.entry(key)
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
        }
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        table(table(&mut t, "policy"), "tools").insert("fs.write".into(), "approve".into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            Command::new(&theseusd)
                .arg("--config")
                .arg(path("config.toml"))
                .arg("--state-dir")
                .arg(path("state"))
                .arg("--socket")
                .arg(path("projects/sock"))
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        path("bin").display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
                .env_remove("THESEUS_OP_TOKEN_FILE")
                .env_remove("THESEUS_CONFIG")
                .env_remove("THESEUS_STATE_DIR")
                .env_remove("THESEUS_SOCKET")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        let mut r = Self {
            dir,
            daemon,
            script,
            leftovers: Mutex::default(),
            _model: model,
        };
        r.until("the secrets", |h| h["secrets"]["state"] == "ready");
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn out(&self, file: &str) -> PathBuf {
        self.path(OUT).join(file)
    }

    /// The model answers a prompt that starts with `prompt` with `calls`.
    fn asks(&self, prompt: &str, calls: Vec<(&'static str, Value)>) {
        self.script.lock().unwrap().push((prompt.into(), calls));
    }

    /// Ask `health` until `ok` says so.
    fn until(&mut self, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            if let Ok(h) = self.call("health", Value::Null) {
                if ok(&h) {
                    return h;
                }
            }
            if let Some(status) = self.daemon.try_wait() {
                panic!("theseusd exited ({status}) before {what}:\n{}", self.log());
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// What a job wrote to `file`, once it is complete (`<file>.done`).
    fn read_done(&self, file: &str) -> String {
        self.wait(file, || {
            self.out(&format!("{file}.done"))
                .exists()
                .then(|| std::fs::read_to_string(self.out(file)).unwrap())
        })
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s =
            UnixStream::connect(self.path("projects/sock")).map_err(|e| json!(e.to_string()))?;
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (&s).write_all(format!("{req}\n").as_bytes())
            .map_err(|e| json!(e.to_string()))?;
        for line in BufReader::new(&s).lines() {
            let v: Value = serde_json::from_str(&line.map_err(|e| json!(e.to_string()))?)
                .map_err(|e| json!(e.to_string()))?;
            if v["id"] == 1 {
                return match v.get("error") {
                    Some(e) if !e.is_null() => Err(e.clone()),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
        Err(json!("the connection closed"))
    }

    /// A turn in a new session: its result, once it parks or ends.
    fn turn(&self, prompt: &str) -> Value {
        let s = self.call("session.open", json!({"label": prompt})).unwrap();
        self.call(
            "turn.submit",
            json!({"session_id": s["session_id"], "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    /// The CLI, run by the test: the operator's own process.
    /// The operator's own `theseus`, from outside every job: no marker.
    fn cli(&self, args: &[&str]) -> std::process::Output {
        Command::new(self.path("projects/bin/theseus"))
            .arg("--socket")
            .arg(self.path("projects/sock"))
            .args(args)
            // The CLI's seen file is the test's own, never the machine's (theseus-yus0).
            .env("XDG_STATE_HOME", self.path("xdg-state"))
            .env_remove("THESEUS_SESSION")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.call("ledger.tail", json!({"n": 2000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }

    /// `proc.run` of `sh -c script`, whose `$1` is the CLI, `$2` the socket,
    /// `$3` where it writes what it saw, and `$4…` the rest.
    fn job(&self, script: &str, rest: &[&str]) -> (&'static str, Value) {
        let mut argv = vec![
            "sh".to_string(),
            "-c".into(),
            script.into(),
            "job".into(),
            self.path("projects/bin/theseus").display().to_string(),
            self.path("projects/sock").display().to_string(),
            self.path(OUT).display().to_string(),
        ];
        argv.extend(rest.iter().map(|s| s.to_string()));
        ("proc_run", json!({"argv": argv, "timeout_secs": 120}))
    }

    /// Kill `pid`, and its process group, when the rig goes.
    fn owns(&self, pid: u32) {
        self.leftovers.lock().unwrap().push(pid);
    }

    /// The pid a job wrote to `file`.
    fn pid(&self, file: &str) -> u32 {
        self.wait(file, || {
            std::fs::read_to_string(self.out(file))
                .ok()
                .filter(|s| s.ends_with('\n'))
                .and_then(|s| s.trim().parse().ok())
        })
    }

    fn log(&self) -> String {
        let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }
}

/// How a job's script starts: `$T` is the CLI, `$S` the socket, `$O` where
/// it writes, and `$id` the call waiting for the operator, as `theseus
/// confirm` lists it.
const FIND_WAITING: &str = r#"T="$1"; S="$2"; O="$3"
i=0; id=""
while [ -z "$id" ] && [ "$i" -lt 1000 ]; do
  id=$("$T" --socket "$S" --json confirm 2>/dev/null | grep -o 'act_[0-9a-f]*' | head -n 1)
  [ -n "$id" ] || sleep 0.02
  i=$((i + 1))
done
"#;

/// `theseus confirm --approve "$id"` into `$O/<name>`, its output then
/// `exit=<status>`, then `$O/<name>.done`.
fn approve_into(name: &str) -> String {
    format!(
        "\"$T\" --socket \"$S\" confirm --approve \"$id\" --no-wait > \"$O/{name}\" 2>&1; \
         echo \"exit=$?\" >> \"$O/{name}\"; touch \"$O/{name}.done\"\n"
    )
}

/// `/proc/<pid>/stat`: the state letter and the parent's pid.
fn state(pid: u32) -> Option<(char, u32)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut f = s[s.rfind(')')? + 2..].split(' ');
    Some((f.next()?.chars().next()?, f.next()?.parse().ok()?))
}

fn alive(pid: u32) -> bool {
    state(pid).is_some_and(|(s, _)| s != 'Z' && s != 'X')
}

/// The refusal a job's process gets from the CLI (theseus-zmgb): `theseus
/// confirm` prints it, sends nothing, and exits 1.
fn assert_refused(out: &str, what: &str) {
    assert!(
        out.contains(
            "theseus confirm refused: it is the operator's to run, and this shell is a Theseus \
             job's (THESEUS_SESSION=ses_"
        ) && out.contains("Run it from your own shell."),
        "{what}: {out}"
    );
    assert!(out.trim_end().ends_with("exit=1"), "{what}: {out}");
}

/// The job's own action: its correlation id, the name its spooled output
/// and its wrapper's arguments carry.
fn job_of(r: &Rig, res: &Value) -> Value {
    let exec = res["execution_id"].as_str().unwrap();
    let actions = r
        .call("action.list", json!({"execution_id": exec}))
        .unwrap();
    actions["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["tool"] == "proc.run")
        .unwrap()
        .clone()
}

/// A job runs `theseus confirm --approve <id>` for the call its own session
/// waits on: the CLI refuses it, with the reason, sends nothing, and exits 1.
/// So does its double-forked grandchild's (`setsid sh -c '… theseus confirm
/// …' &`), once while the job's main process lives, and again after it has
/// exited, when the grandchild has been reparented to the lingering wrapper.
/// Health counts the wrapper that lingers. The operator's own answer counts,
/// and the call runs. When the grandchild ends, the wrapper exits.
#[test]
fn a_job_cannot_answer_its_own_sessions_approval_nor_can_its_grandchild() {
    let mut r = Rig::start();
    let script = format!(
        "{FIND_WAITING}echo \"$PPID\" > \"$O/wrapper.pid\"\n{}cat \"$O/job.out\"\n\
         setsid sh -c '\n\
         T=\"$1\"; S=\"$2\"; O=\"$3\"; id=\"$4\"; main=\"$5\"\n\
         {}\
         while [ -e \"/proc/$main\" ]; do sleep 0.02; done\n\
         cut -d\" \" -f4 /proc/$$/stat > \"$O/grandchild.ppid\"\n\
         {}\
         while [ ! -e \"$O/release\" ] && [ -d \"$O\" ]; do sleep 0.02; done\n\
         ' grandchild \"$T\" \"$S\" \"$O\" \"$id\" \"$$\" > /dev/null 2>&1 < /dev/null &\n\
         while [ ! -e \"$O/g1.done\" ] && [ -d \"$O\" ]; do sleep 0.02; done\n\
         echo \"the job's main process exits\"\n",
        approve_into("job.out"),
        approve_into("g1"),
        approve_into("g2"),
    );
    r.asks(
        "answer your own approval",
        vec![
            r.job(&script, &[]),
            (
                "fs_write",
                json!({"path": "written.txt", "content": "approved\n"}),
            ),
        ],
    );
    let res = r.turn("answer your own approval");
    let corr = res["awaiting_confirm"]
        .as_str()
        .unwrap_or_else(|| panic!("the write waits: {res}\n{}", r.log()))
        .to_string();

    // The job, and its grandchild twice: each refused, and the call waits.
    assert_refused(&r.read_done("job.out"), "the job");
    assert_refused(
        &r.read_done("g1"),
        "the grandchild, while the job's main process lives",
    );
    assert_refused(&r.read_done("g2"), "the grandchild, after it has exited");
    let wrapper = r.pid("wrapper.pid");
    r.owns(wrapper);
    assert_eq!(
        r.pid("grandchild.ppid"),
        wrapper,
        "the orphaned grandchild was reparented to the wrapper, not to init"
    );
    assert!(
        r.ledger("approval.refused").is_empty(),
        "nothing reached the daemon"
    );
    let job = job_of(&r, &res)["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    let confirms = r.call("confirm.list", Value::Null).unwrap();
    assert_eq!(confirms["confirms"][0]["correlation_id"], corr.as_str());
    assert!(!r.path("projects/written.txt").exists());
    // The job's own output, which its model reads, says why.
    let result = std::fs::read_to_string(r.path(&format!("state/spool/results/{job}.out")))
        .unwrap_or_default();
    assert!(result.contains("theseus confirm refused"), "{result}");
    // The wrapper lingers for the grandchild, and health says so.
    let h = r.until("the lingering wrapper", |h| {
        h["kernel"]["lingering_wrappers"] == 1
    });
    assert_eq!(h["kernel"]["lingering_wrappers"], 1);
    assert!(alive(wrapper));
    // The daemon's children say the same: the wrapper is its child, and
    // lingers (theseus-z4b).
    assert_eq!(
        (
            &h["children"]["wrappers_lingering"],
            &h["children"]["wrappers_running"]
        ),
        (&json!(1), &json!(0)),
        "{}",
        h["children"]
    );

    // The operator's own answer counts.
    let ok = r.cli(&["confirm", "--approve", &corr, "--no-wait"]);
    let said = String::from_utf8_lossy(&ok.stdout);
    assert!(
        ok.status.success(),
        "{said}{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(said.contains(&format!("approved {corr}")), "{said}");
    let written = r.wait("the approved write", || {
        std::fs::read_to_string(r.path("projects/written.txt")).ok()
    });
    assert_eq!(written, "approved\n");

    // The grandchild ends; the wrapper exits, and the daemon reaps it.
    std::fs::write(r.out("release"), "").unwrap();
    r.wait("the wrapper to exit", || (!alive(wrapper)).then_some(()));
    r.until("no lingering wrapper", |h| {
        h["kernel"]["lingering_wrappers"] == 0
            && h["children"]["wrappers_lingering"] == 0
            && h["children"]["reaped_wrappers"] == 1
            && h["children"]["zombies"] == 0
    });
}

/// A job that kills its own wrapper (`kill -9 $PPID`) leaves what it started
/// to the daemon, which is a child subreaper (theseus-z4b). Its double-forked
/// grandchild is reparented to `theseusd`, not to init, and its `theseus
/// confirm --approve` for the call its session waits on is refused by the
/// CLI, as the job's was: it still carries the job's marker. Nothing moves,
/// and the operator's own answer still counts. The
/// daemon reaps the wrapper the job killed, the job's main process when it
/// exits, and the grandchild when it ends.
#[test]
fn a_job_that_kills_its_wrapper_leaves_an_orphan_that_cannot_answer() {
    let mut r = Rig::start();
    let daemon = r.daemon.id();
    // The grandchild waits until its parent, the job's main process, has
    // exited, and it has been reparented past the wrapper the job killed.
    let script = format!(
        "{FIND_WAITING}echo \"$PPID\" > \"$O/wrapper.pid\"\n\
         setsid sh -c '\n\
         T=\"$1\"; S=\"$2\"; O=\"$3\"; id=\"$4\"; main=\"$5\"; w=\"$6\"\n\
         echo $$ > \"$O/orphan.pid\"\n\
         while [ -d \"$O\" ]; do\n\
         \x20 p=$(cut -d\" \" -f4 /proc/$$/stat)\n\
         \x20 [ \"$p\" != \"$main\" ] && [ \"$p\" != \"$w\" ] && break\n\
         \x20 sleep 0.02\n\
         done\n\
         echo \"$p\" > \"$O/orphan.ppid\"\n\
         {}\
         while [ ! -e \"$O/release\" ] && [ -d \"$O\" ]; do sleep 0.02; done\n\
         ' orphan \"$T\" \"$S\" \"$O\" \"$id\" \"$$\" \"$PPID\" > /dev/null 2>&1 < /dev/null &\n\
         kill -9 \"$PPID\"\n\
         echo \"the job killed its wrapper\"\n",
        approve_into("orphan"),
    );
    r.asks(
        "kill your own wrapper",
        vec![
            r.job(&script, &[]),
            (
                "fs_write",
                json!({"path": "written.txt", "content": "approved\n"}),
            ),
        ],
    );
    let res = r.turn("kill your own wrapper");
    let corr = res["awaiting_confirm"]
        .as_str()
        .unwrap_or_else(|| panic!("the write waits: {res}\n{}", r.log()))
        .to_string();

    // The orphan's answer is refused by the CLI.
    assert_refused(&r.read_done("orphan"), "the orphan");
    let wrapper = r.pid("wrapper.pid");
    let orphan = r.pid("orphan.pid");
    r.owns(orphan);
    assert_eq!(
        r.pid("orphan.ppid"),
        daemon,
        "the orphan was reparented to theseusd, not to init"
    );
    assert!(!alive(wrapper), "the job killed its wrapper");
    // A security event (theseus-6uo): the wrapper died by a signal before it
    // reported, and no cancel killed it. It is ledgered, and the job's action
    // is unknown at once, long before its deadline.
    let lost = r.wait("the lost wrapper's row", || {
        Some(r.ledger("job.wrapper_lost")).filter(|l| !l.is_empty())
    });
    let job = job_of(&r, &res);
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert_eq!(
        (
            &lost[0]["correlation_id"],
            &lost[0]["pid"],
            &lost[0]["signal"]
        ),
        (&job["correlation_id"], &json!(wrapper), &json!(9)),
        "{lost:?}"
    );
    assert_eq!(job["state"], "outcome_unknown", "{job}");
    assert!(r.ledger("approval.refused").is_empty());
    let confirms = r.call("confirm.list", Value::Null).unwrap();
    assert_eq!(confirms["confirms"][0]["correlation_id"], corr.as_str());
    assert!(!r.path("projects/written.txt").exists());
    // The daemon holds the orphan, and has reaped the wrapper the job killed
    // and the job's main process.
    let h = r.until("the adopted orphan", |h| {
        let c = &h["children"];
        c["orphans"] == 1 && c["reaped_wrappers"] == 1 && c["reaped_orphans"] == 1
    });
    assert_eq!(
        (&h["children"]["subreaper"], &h["children"]["zombies"]),
        (&json!(true), &json!(0))
    );

    let ok = r.cli(&["confirm", "--approve", &corr, "--no-wait"]);
    let out = String::from_utf8_lossy(&ok.stdout);
    assert!(
        ok.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(out.contains(&format!("approved {corr}")), "{out}");
    let written = r.wait("the approved write", || {
        std::fs::read_to_string(r.path("projects/written.txt")).ok()
    });
    assert_eq!(written, "approved\n");

    // The orphan ends, and the daemon reaps it.
    std::fs::write(r.out("release"), "").unwrap();
    r.wait("the orphan to exit", || (!alive(orphan)).then_some(()));
    r.until("the orphan reaped", |h| {
        let c = &h["children"];
        c["orphans"] == 0 && c["zombies"] == 0 && c["reaped_orphans"] == 2
    });
}

/// A job may make a tool ask first ("should have asked"), which only makes
/// things stricter, but the CLI refuses its undo of a tightening, which
/// loosens. The operator's undo counts.
#[test]
fn a_job_can_tighten_a_tool_but_not_undo_a_tightening() {
    let r = Rig::start();
    let pressed = r.cli(&["policy", "tighten", "fs.edit"]);
    assert!(pressed.status.success(), "{pressed:?}");
    let script = r#"T="$1"; S="$2"; O="$3"
"$T" --socket "$S" policy tighten fs.patch > "$O/tighten" 2>&1; echo "exit=$?" >> "$O/tighten"; touch "$O/tighten.done"
"$T" --socket "$S" policy untighten fs.edit > "$O/untighten" 2>&1; echo "exit=$?" >> "$O/untighten"; touch "$O/untighten.done"
cat "$O/tighten" "$O/untighten"
"#;
    r.asks("change the policy", vec![r.job(script, &[])]);
    r.turn("change the policy");
    let tighten = r.read_done("tighten");
    assert!(tighten.trim_end().ends_with("exit=0"), "{tighten}");
    let untighten = r.read_done("untighten");
    assert!(
        untighten.contains("theseus policy untighten refused: it is the operator's to run"),
        "{untighten}"
    );
    assert!(untighten.trim_end().ends_with("exit=1"), "{untighten}");
    let h = r.call("health", Value::Null).unwrap();
    let tightened: Vec<&str> = h["tightenings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["tool"].as_str())
        .collect();
    assert_eq!(tightened, ["fs.edit", "fs.patch"]);
    assert!(r.ledger("approval.refused").is_empty());
    let undone = r.cli(&["policy", "untighten", "fs.edit"]);
    assert!(undone.status.success(), "{undone:?}");
}

/// A cancel ends the job's whole tree (M4 18a; theseus-hcc): the command,
/// what stayed in its group, and a descendant in its own session, which a
/// cancel by the group never reached. The wrapper stops them and says how:
/// the cancel's answer and the action carry its verdict, verified at once.
#[test]
fn a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too() {
    let r = Rig::start();
    let script = r#"O="$3"
echo "$PPID" > "$O/wrapper.pid"; echo "$$" > "$O/main.pid"
sleep 60 & echo "$!" > "$O/in-group.pid"
setsid sleep 60 > /dev/null 2>&1 < /dev/null & echo "$!" > "$O/own-session.pid"
wait
"#;
    r.asks("run a long job", vec![r.job(script, &[])]);
    let res = r.turn("run a long job");
    let exec = res["execution_id"].as_str().unwrap().to_string();
    let wrapper = r.pid("wrapper.pid");
    r.owns(wrapper);
    let main = r.pid("main.pid");
    let in_group = r.pid("in-group.pid");
    let own_session = r.pid("own-session.pid");
    r.owns(own_session);
    r.wait("setsid to exec sleep", || {
        std::fs::read_to_string(format!("/proc/{own_session}/comm"))
            .ok()
            .filter(|c| c.trim() == "sleep")
    });
    let t0 = Instant::now();
    let c = r
        .call("execution.cancel", json!({"execution_id": exec}))
        .unwrap();
    let took = t0.elapsed();
    assert_eq!(c["cancelled_actions"].as_array().unwrap().len(), 1, "{c}");
    for (what, pid) in [
        ("the wrapper", wrapper),
        ("the command", main),
        ("its child", in_group),
    ] {
        r.wait(&format!("{what} to be killed"), || {
            (!alive(pid)).then_some(())
        });
    }
    if alive(own_session) {
        let _ = Command::new("kill")
            .args(["-9", &own_session.to_string()])
            .status();
        panic!("the descendant in its own session survived the cancel: {c}");
    }
    let v = &c["verdicts"][0];
    assert_eq!(
        (
            v["verified_by"].as_str(),
            v["killed"].as_u64(),
            v["survivors"].as_u64()
        ),
        (Some("tree"), Some(3), Some(0)),
        "{c}"
    );
    let actions = r
        .call("action.list", json!({"execution_id": exec}))
        .unwrap();
    let job = actions["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["tool"] == "proc.run")
        .unwrap()
        .clone();
    assert_eq!(
        (job["state"].as_str(), job["cancel"].as_str()),
        (Some("cancelled"), Some("termination_verified")),
        "{job}"
    );
    // The stop's own time, from its signal to its verdict: every process
    // here dies on SIGTERM, in tens of ms even at nice 19 beside a busy loop
    // per core, so a stop near its grace (one that waited it out reads 1999
    // ms) is wrong. The round trip's wall time is the machine's load as well,
    // so it is held only at the daemon's own deadline, past which the cancel
    // reads uncertain (theseus-cs71).
    let ms = v["ms"].as_u64().unwrap();
    eprintln!("cancel: took {took:?}, the stop's ms {ms}");
    assert!(
        ms < STOP_GRACE.as_millis() as u64 / 2,
        "the tree emptied on SIGTERM, well within the grace: {ms} ms: {c}"
    );
    assert!(
        took < STOP_GRACE + ANSWER_WAIT,
        "the cancel answered within the daemon's wait for its wrapper: {took:?}"
    );
    // A cancel's own stop is expected: once the daemon has reaped the
    // wrapper, there is no `job.wrapper_lost` (theseus-6uo).
    let mut r = r;
    r.until("the killed wrapper reaped", |h| {
        h["children"]["reaped_wrappers"].as_u64() >= Some(1)
    });
    assert!(r.ledger("job.wrapper_lost").is_empty());
}
