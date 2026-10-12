//! Earned autonomy with the real `theseusd` and the real `theseus` CLI
//! (theseus-uttu): the owner approves the same command three times, forces
//! the card, accepts it, and the command then runs with one notice and no
//! question while a command off the prefix still asks; a revoke makes it ask
//! again. A job's own `policy earned accept` is refused by the CLI. A
//! stand-in for the Messages API asks for the tool calls. The live check
//! the maintainer runs by hand is this scenario with the owner's keys.

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

type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    script: Script,
    model: FakeModel,
}

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
        for d in ["bin", "projects"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        let cli = theseusd.with_file_name("theseus");
        assert!(
            cli.exists(),
            "no {} to run: build the CLI (cargo build -p theseus)",
            cli.display()
        );
        std::fs::copy(&cli, path("projects/theseus")).unwrap();
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
        table(&mut t, "policy").insert("enforcement".into(), "approve".into());
        table(table(&mut t, "policy"), "earned").insert("min_approvals".into(), 3.into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), 5.into());
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
            model,
        };
        r.until("the secrets", |h| h["secrets"]["state"] == "ready");
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn asks(&self, prompt: &str, calls: Vec<(&'static str, Value)>) {
        self.script.lock().unwrap().push((prompt.into(), calls));
    }

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

    fn wait(&self, what: &str, f: impl Fn() -> bool) {
        let t0 = Instant::now();
        while !f() {
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
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

    fn turn(&self, prompt: &str) -> Value {
        let s = self.call("session.open", json!({"label": prompt})).unwrap();
        self.call(
            "turn.submit",
            json!({"session_id": s["session_id"], "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    /// The operator's own `theseus`, run by the test: no job marker.
    fn cli(&self, args: &[&str]) -> (bool, String) {
        let o = Command::new(self.path("projects/theseus"))
            .arg("--socket")
            .arg(self.path("projects/sock"))
            .args(args)
            .env("XDG_STATE_HOME", self.path("xdg-state"))
            .env_remove("THESEUS_SESSION")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        (o.status.success(), text)
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

    fn log(&self) -> String {
        let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }

    /// A turn that runs `echo hello <word>`, the owner's approval from the
    /// CLI, and the turn's end (the model's second request).
    fn approved_run(&self, word: &str) {
        let prompt = format!("run {word}");
        self.asks(
            &prompt,
            vec![("proc_run", json!({"argv": ["echo", "hello", word]}))],
        );
        let before = self.model.requests().len();
        let res = self.turn(&prompt);
        let corr = res["awaiting_confirm"]
            .as_str()
            .unwrap_or_else(|| panic!("approve asks: {res}\n{}", self.log()))
            .to_string();
        let (ok, out) = self.cli(&["confirm", "--approve", &corr, "--no-wait"]);
        assert!(ok, "{out}");
        self.wait("the turn's end", || {
            self.model.requests().len() >= before + 2
        });
    }
}

/// The group's id in the card's text: `ern_` and eight hex characters.
fn id_in(text: &str) -> String {
    let at = text
        .find("ern_")
        .unwrap_or_else(|| panic!("no group id in:\n{text}"));
    text[at..at + 12].to_string()
}

#[test]
fn three_approvals_the_card_a_yes_and_the_command_runs_with_a_notice_until_it_is_revoked() {
    let r = Rig::start();
    for w in ["a", "b", "c"] {
        r.approved_run(w);
    }
    assert_eq!(r.ledger("action.confirmed").len(), 3);

    // The card, forced: one candidate, with its record.
    let (ok, card) = r.cli(&["policy", "earned", "--propose-now"]);
    assert!(ok, "{card}\n{}", r.log());
    assert!(card.contains("Posted a card"), "{card}");
    assert!(card.contains("echo hello"), "{card}");
    assert!(card.contains("earned: 3 approvals"), "{card}");
    let id = id_in(&card);
    assert_eq!(r.ledger("policy.earned_proposed").len(), 1);
    let (ok, shown) = r.cli(&["policy", "earned", "show", &id]);
    assert!(
        ok && shown.contains("3 approvals") && shown.contains("e.g. echo hello"),
        "{shown}"
    );

    // A job's own accept is refused by the CLI, and nothing reaches the daemon.
    let refused = Command::new(r.path("projects/theseus"))
        .arg("--socket")
        .arg(r.path("projects/sock"))
        .args(["policy", "earned", "accept", &id])
        .env("THESEUS_SESSION", "ses_0000aa1b2c3")
        .env("XDG_STATE_HOME", r.path("xdg-state"))
        .output()
        .unwrap();
    assert!(!refused.status.success());
    let text = String::from_utf8_lossy(&refused.stderr).to_string()
        + &String::from_utf8_lossy(&refused.stdout);
    assert!(
        text.contains("theseus policy earned accept refused"),
        "{text}"
    );
    assert!(r.ledger("policy.earned_granted").is_empty());

    // The owner's yes.
    let (ok, yes) = r.cli(&["policy", "earned", "accept", &id]);
    assert!(
        ok && yes.contains("runs with a notice instead of asking"),
        "{yes}"
    );
    let granted = r.ledger("policy.earned_granted");
    assert_eq!(
        (granted.len(), granted[0]["approvals"].clone()),
        (1, json!(3))
    );
    let (_, listed) = r.cli(&["policy"]);
    assert!(listed.contains(&format!("earned grant: {id}")), "{listed}");

    // The same command again: no question, one notice. Another command asks.
    r.asks(
        "run d",
        vec![("proc_run", json!({"argv": ["echo", "hello", "d"]}))],
    );
    let res = r.turn("run d");
    assert!(
        res["awaiting_confirm"].is_null(),
        "the grant runs it: {res}\n{}",
        r.log()
    );
    let notices = r.ledger("tool.notified");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0]["setting"], format!("earned grant {id}"));
    r.asks(
        "other",
        vec![("proc_run", json!({"argv": ["echo", "goodbye"]}))],
    );
    assert!(
        r.turn("other")["awaiting_confirm"].is_string(),
        "off the prefix it asks"
    );

    // The number reads, and the grant would have saved the three questions.
    let (ok, number) = r.cli(&["policy", "earned"]);
    assert!(
        ok && number.contains("Interrupts per active hour"),
        "{number}"
    );
    assert!(number.contains("would have saved 3"), "{number}");

    // A revoke: it asks again.
    let (ok, out) = r.cli(&["policy", "revoke", &id]);
    assert!(ok && out.contains("asks first again"), "{out}");
    r.asks(
        "run e",
        vec![("proc_run", json!({"argv": ["echo", "hello", "e"]}))],
    );
    assert!(
        r.turn("run e")["awaiting_confirm"].is_string(),
        "revoked: it asks"
    );
    let (_, listed) = r.cli(&["policy"]);
    assert!(!listed.contains("earned grant:"), "{listed}");
}
