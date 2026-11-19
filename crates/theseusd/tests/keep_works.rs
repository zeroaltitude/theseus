//! Keeping a new secret from the card, the way a person does it (issue 29):
//! a real daemon, a turn that puts a keep card, the real `theseus` program at
//! a pseudo-terminal typing the value at its hidden prompt, and the stand-in
//! for the vault's program holding the value.

mod common;

use std::path::PathBuf;

use common::model::FakeModel;
use common::{table, Served};
use serde_json::{json, Value};

const ASK: &str = "keep a key for the lantern provider";
/// Invented, and never in any output.
const VALUE: &str = "INVENTED-KEEP-VALUE-2290";

struct Rig {
    s: Served,
    cli: PathBuf,
    vault: tempfile::TempDir,
    _model: FakeModel,
}

impl Rig {
    fn start() -> Self {
        let model = FakeModel::start(|prompt| match prompt.contains(ASK) {
            true => vec![(
                "vault_keep",
                json!({"vault": "Harbor", "title": "Lantern new", "field": "credential",
                       "purpose": "the lantern provider's key"}),
            )],
            false => vec![],
        });
        let vault = tempfile::tempdir().unwrap();
        common::vault::install(vault.path());
        let path = format!(
            "{}:{}",
            vault.path().join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let base = model.base.clone();
        // The model's keys are files holding a stand-in: the vault's program is the daemon's keeping alone.
        let key = vault.path().join("model-key");
        std::fs::write(&key, "test-secret-value-0000").unwrap();
        std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        let s = Served::start_with(
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
                table(t, "policy").insert("enforcement".into(), "notify".into());
            },
            move |cmd| {
                cmd.env("PATH", &path);
            },
        );
        let cli = PathBuf::from(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus");
        assert!(cli.exists(), "no {}: build the CLI", cli.display());
        Self {
            s,
            cli,
            vault,
            _model: model,
        }
    }

    /// A turn in a new session, and what it ended as.
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

    fn cards(&self) -> Vec<Value> {
        self.s.call("confirm.list", Value::Null).unwrap()["confirms"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn argv(&self, args: &[&str]) -> Vec<String> {
        let mut argv = vec![
            self.cli.display().to_string(),
            "--socket".into(),
            self.s.path("sock").display().to_string(),
        ];
        argv.extend(args.iter().map(|a| a.to_string()));
        argv
    }

    /// `theseus <args>` at a terminal, `steps` typed as its prompts appear.
    fn at_a_terminal(&self, args: &[&str], steps: &[(&str, &str)]) -> (i32, String) {
        let xdg = self.s.path("xdg-state").display().to_string();
        common::terminal::typed_at_a_terminal(
            &self.argv(args),
            &[("XDG_STATE_HOME", &xdg)],
            steps,
            8,
        )
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
}

/// The reproduction of 2026-11-18: what the program prints, and returns, with
/// and without an Approve pressed first.
#[test]
fn reproduce_what_the_terminal_command_does_on_a_waiting_card() {
    let r = Rig::start();
    let res = r.turn(ASK);
    let mut report = format!("turn: {res}\n");
    let cards = r.cards();
    report += &format!("cards waiting: {}\n", cards.len());
    for c in &cards {
        report += &format!(
            "  card {} tool {} keep-marker {} needs_proof {:?} proof {:?} may_answer {:?}\n  input keys {:?}\n",
            c["correlation_id"],
            c["tool"],
            c["input"]["keep"].is_object(),
            c["needs_proof"],
            c["proof"],
            c["may_answer"],
            c["input"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()),
        );
    }
    let (code, out) = r.at_a_terminal(&["vault", "keep"], &[]);
    report += &format!("--- no press first: exit {code}\n{out}\n");
    if let Some(q) = cards.first().and_then(|c| c["correlation_id"].as_str()) {
        let (code, out) = r.at_a_terminal(&["answer", q, "approve", "--no-wait"], &[]);
        report += &format!("--- Approve pressed (theseus answer): exit {code}\n{out}\n");
        let (code, out) = r.at_a_terminal(&["vault", "keep"], &[]);
        report += &format!("--- after the press: exit {code}\n{out}\n");
    }
    // Enter already waiting at each prompt, as a pasted or doubled newline leaves it.
    let (code, out) = r.at_a_terminal(
        &["vault", "keep"],
        &[("authenticator app: ", ""), ("nothing is shown): ", "")],
    );
    report += &format!("--- Enter typed at each prompt: exit {code}\n{out}\n");
    // A job's marker in the environment.
    let mut argv = vec!["/usr/bin/env".into(), "THESEUS_SESSION=ses_x".to_string()];
    argv.extend(r.argv(&["vault", "keep"]));
    let (code, out) = common::terminal::typed_at_a_terminal(&argv, &[], &[], 3);
    report += &format!("--- THESEUS_SESSION set (the terminal strips it, env puts it back): exit {code}\n{out}\n");
    // No terminal on standard input.
    let o = std::process::Command::new(&r.cli)
        .args(["--socket"])
        .arg(r.s.path("sock"))
        .args(["vault", "keep"])
        .env_remove("THESEUS_SESSION")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    report += &format!(
        "--- stdin is /dev/null: exit {:?}\n{}{}\n",
        o.status.code(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    // The value typed, with a code, on a daemon with no authenticator enrolled.
    let (code, out) = r.at_a_terminal(
        &["vault", "keep"],
        &[("authenticator app: ", "123456"), ("nothing is shown): ", VALUE)],
    );
    report += &format!("--- code and value typed: exit {code}\n{out}\n");
    assert!(!out.contains(VALUE), "the value was echoed");
    report += &format!("op argv: {:?}\n", common::vault::argv(r.vault.path()));
    report += &format!("vault.keep rows: {:?}\n", r.ledger("vault.keep"));
    report += &format!("daemon log tail:\n{}\n", common::tail(&r.s.log(), 30));
    panic!("REPRODUCTION\n{report}");
}
