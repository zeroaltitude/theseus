//! Keeping a new secret from the card, the way a person does it (issue 29):
//! a real daemon, a turn that puts a keep card, the real `theseus` program at
//! a pseudo-terminal typing the value at its hidden prompt, and the stand-in
//! for the vault's program holding the value. The first run of this feature on
//! the owner's machine failed at the terminal command, after tests that drove
//! the daemon's requests one at a time and never the program.
//!
//! Every value here is invented, and each test that gives one searches for it
//! where it must not be: what the program printed, the daemon's log, every
//! ledger row and every file under the state directory.

mod common;

use std::path::PathBuf;

use common::model::FakeModel;
use common::{table, Served};
use serde_json::{json, Value};

const ASK: &str = "keep a key for the lantern provider";
/// Invented, and never in any output.
const VALUE: &str = "INVENTED-KEEP-VALUE-2290";
const CODE_PROMPT: &str = "Code from your authenticator app: ";
const VALUE_PROMPT: &str = "nothing is shown): ";

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
        let cli = common::cli_bin();
        assert!(cli.exists(), "no {}: build the CLI", cli.display());
        Self {
            s,
            cli,
            vault,
            _model: model,
        }
    }

    /// A turn in a new session.
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

    /// The turn asks to keep the key, and the card it put.
    fn card(&self) -> Value {
        self.turn(ASK);
        let mut cards = self.cards();
        assert_eq!(
            cards.len(),
            1,
            "one card waits: {cards:?}\n{}",
            self.s.log()
        );
        cards.remove(0)
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

    /// `theseus <args>` at a terminal, `steps` typed as its prompts appear and
    /// `prefill` already waiting on the terminal.
    fn at_a_terminal(&self, args: &[&str], steps: &[(&str, &str)], prefill: &str) -> (i32, String) {
        let xdg = self.s.path("xdg-state").display().to_string();
        let secret = self.vault.path().join("pairing").display().to_string();
        common::terminal::typed_at_a_terminal(
            &self.argv(args),
            &[("XDG_STATE_HOME", &xdg), ("PTY_SECRET_FILE", &secret)],
            steps,
            prefill,
        )
    }

    /// The owner pairs an authenticator app, and ends the window that pairing
    /// opens, so the next keep asks for a code.
    fn pair(&self) {
        let (code, out) = self.at_a_terminal(
            &["owner", "proof", "enroll"],
            &[(CODE_PROMPT, "{code:-1}")],
            "",
        );
        assert_eq!(code, 0, "{out}\n{}", self.s.log());
        assert!(out.contains("Paired"), "{out}");
        let (code, out) = self.at_a_terminal(&["owner", "proof", "lock"], &[], "");
        assert_eq!(code, 0, "{out}");
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.s.call("ledger.tail", json!({"n": 5000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }

    /// The `vault.keep` rows, by outcome.
    fn keep_rows(&self, outcome: &str) -> Vec<Value> {
        self.ledger("vault.keep")
            .into_iter()
            .filter(|r| r["outcome"] == outcome)
            .collect()
    }

    /// The value is nowhere it must not be: not in what the program printed,
    /// the daemon's log, any ledger row, or any file of the state directory.
    fn value_nowhere(&self, printed: &str) {
        assert!(!printed.contains(VALUE), "in what the program printed");
        assert!(!self.s.log().contains(VALUE), "in the daemon's log");
        let t = self.s.call("ledger.tail", json!({"n": 20000})).unwrap();
        assert!(!t.to_string().contains(VALUE), "in a ledger row");
        let mut stack = vec![self.s.path("state")];
        while let Some(p) = stack.pop() {
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                stack.extend(std::fs::read_dir(&p).unwrap().flatten().map(|e| e.path()));
            } else if let Ok(bytes) = std::fs::read(&p) {
                assert!(
                    !bytes.windows(VALUE.len()).any(|w| w == VALUE.as_bytes()),
                    "in {}",
                    p.display()
                );
            }
        }
        assert!(
            common::vault::argv(self.vault.path())
                .iter()
                .all(|l| !l.contains(VALUE)),
            "on a command line of the vault's program"
        );
    }
}

/// The card, the real program at a terminal with a stray newline already
/// waiting, the value in the stand-in vault, the card settled, the rows
/// written, and the value nowhere else. The path a person takes.
#[test]
fn a_keep_goes_from_the_card_through_the_real_program_into_the_vault() {
    let r = Rig::start();
    r.pair();
    let card = r.card();
    // The daemon states what the card takes: only a decline can be pressed, and the first
    // words say where the value is typed.
    let options: Vec<&str> = card["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap())
        .collect();
    assert_eq!(options, ["decline"], "{card}");
    assert!(
        card["reason"]
            .as_str()
            .unwrap()
            .starts_with("Type the value in your own terminal: run `theseus vault keep`"),
        "{card}"
    );
    let q = card["correlation_id"].as_str().unwrap().to_string();

    let (code, out) = r.at_a_terminal(
        &["vault", "keep"],
        &[(VALUE_PROMPT, VALUE)],
        "\n",
    );
    assert_eq!(code, 0, "{out}\n{}", r.s.log());
    // The newline that was already waiting was thrown away: it was not taken for an empty code or
    // an empty value (which would have been asked for once more).
    assert!(!out.contains("Nothing was typed"), "{out}");
    // Nothing that reads as a result comes before the daemon says it: the heading, then the end.
    let heading = out.find("To keep a new item \"Lantern new\"").expect(&out);
    let kept = out.find("Kept in the vault \"Harbor\"").expect(&out);
    assert!(heading < kept, "{out}");
    assert!(!out[..kept].contains("Kept"), "{out}");

    assert_eq!(
        common::vault::stored(r.vault.path())
            .as_deref()
            .map(str::trim),
        Some(VALUE),
        "the value is in the vault's program"
    );
    assert!(r.cards().is_empty(), "the card is settled");
    let rows = r.keep_rows("kept");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["question"], q.as_str());
    assert!(
        rows[0]["pid"].is_u64(),
        "the client's process is in the row: {rows:?}"
    );
    r.value_nowhere(&out);
}

/// Nothing waits, and the terminal says why in the daemon's words, each time:
/// none asked, a name that matches no card, a card that was declined. It exits
/// non-zero and leaves a row each time.
#[test]
fn the_terminal_says_why_nothing_waits_and_leaves_a_row() {
    let r = Rig::start();
    let (code, out) = r.at_a_terminal(&["vault", "keep"], &[], "");
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("No secret is waiting to be kept, and Theseus has no recent record of asking"),
        "{out}"
    );
    let card = r.card();
    let q = card["correlation_id"].as_str().unwrap().to_string();
    let (code, out) = r.at_a_terminal(&["answer", &q, "decline", "--no-wait"], &[], "");
    assert_eq!(code, 0, "{out}");
    let (code, out) = r.at_a_terminal(&["vault", "keep"], &[], "");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("was declined"), "{out}");
    assert!(out.contains("Ask Theseus to keep it again"), "{out}");
    let (code, out) = r.at_a_terminal(&["vault", "keep", "zzzzzz"], &[], "");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("No keep card ends in \"zzzzzz\""), "{out}");
    let listed = r.keep_rows("listed");
    let why: Vec<&str> = listed.iter().map(|l| l["why"].as_str().unwrap()).collect();
    assert_eq!(why, ["none_asked", "declined", "none_named"], "{listed:?}");
    assert!(listed.iter().all(|l| l["pid"].is_u64() && l["cards"] == 0));
}

/// No authenticator app is enrolled, and a keep card takes no code (issue 54):
/// nothing is said of one, no code is asked, and the value is asked for and
/// kept.
#[test]
fn no_authenticator_is_needed_for_a_keep_card() {
    let r = Rig::start();
    r.card();
    let (code, out) = r.at_a_terminal(&["vault", "keep"], &[(VALUE_PROMPT, VALUE)], "");
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("none is enrolled yet"), "{out}");
    assert!(!out.contains(CODE_PROMPT), "{out}");
    assert_eq!(r.keep_rows("kept").len(), 1);
    r.value_nowhere(&out);
}

/// A vault the account may not write in gets no card, and the owner is told
/// what they can do; the terminal's own form says it before any prompt too.
#[test]
fn a_vault_the_account_cannot_write_to_gets_no_card() {
    let r = Rig::start();
    std::fs::write(r.vault.path().join("op.readonly"), "").unwrap();
    r.turn(ASK);
    assert!(r.cards().is_empty(), "no card is put: {:?}", r.cards());
    let (code, out) = r.at_a_terminal(
        &[
            "vault",
            "keep",
            "--vault",
            "Harbor",
            "--item",
            "Lantern new",
            "--field",
            "credential",
        ],
        &[],
        "",
    );
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("vault account may not make a new item"),
        "{out}"
    );
    assert!(out.contains("Give the account write access"), "{out}");
    assert!(!out.contains(VALUE_PROMPT), "{out}");
    assert_eq!(r.keep_rows("stopped")[0]["why"], "not_writable");
}

/// A run that ends at its first prompt says so in words, and the daemon's
/// record says which way it ended: nothing typed twice, or the end of input.
#[test]
fn a_run_that_ends_at_a_prompt_says_how_and_leaves_a_row() {
    let r = Rig::start();
    r.pair();
    let card = r.card();
    let (code, out) = r.at_a_terminal(
        &["vault", "keep"],
        &[(VALUE_PROMPT, ""), (VALUE_PROMPT, "")],
        "",
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("Nothing was typed: once more."), "{out}");
    assert!(
        out.contains("nothing was typed, twice, so nothing was kept"),
        "{out}"
    );
    let (code, out) = r.at_a_terminal(
        &["vault", "keep"],
        &[(VALUE_PROMPT, "<EOF>")],
        "",
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("the terminal ended (end of input)"), "{out}");
    let ended = r.keep_rows("client_ended");
    let why: Vec<&str> = ended.iter().map(|e| e["why"].as_str().unwrap()).collect();
    assert_eq!(why, ["no_value", "end_of_input"], "{ended:?}");
    assert!(ended.iter().all(|e| e["pid"].is_u64()), "{ended:?}");
    assert_eq!(r.cards().len(), 1, "the card still waits");
    assert_eq!(r.cards()[0]["correlation_id"], card["correlation_id"]);
    assert!(common::vault::stored(r.vault.path()).is_none());
}

/// A keep card takes no second proof (the owner, 2026-11-25: "the form to ask
/// for it, yes, the 2fa, no"): with an authenticator paired and its window
/// ended, the terminal asks for the value alone, it is kept, and no row says
/// a proof stopped it.
#[test]
fn a_keep_asks_no_code_even_with_an_authenticator_paired() {
    let r = Rig::start();
    r.pair();
    r.card();
    let (code, out) = r.at_a_terminal(&["vault", "keep"], &[(VALUE_PROMPT, VALUE)], "");
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains(CODE_PROMPT), "{out}");
    assert!(r.keep_rows("stopped").is_empty());
    assert_eq!(r.keep_rows("kept").len(), 1);
    assert!(r.cards().is_empty());
    assert_eq!(
        common::vault::stored(r.vault.path())
            .as_deref()
            .map(str::trim),
        Some(VALUE)
    );
    r.value_nowhere(&out);
}

/// Not a terminal, and a job's shell: refused in the daemon's words, before
/// any prompt, each with a row.
#[test]
fn a_pipe_and_a_jobs_shell_are_refused_by_the_daemon_with_a_row() {
    let r = Rig::start();
    r.card();
    // A terminal for the shell, and standard input from /dev/null for the program: a pipe in the
    // owner's own shell. The daemon refuses it in words, before any prompt.
    let piped = r.argv(&["vault", "keep"]).join(" ");
    let (code, text) = common::terminal::typed_at_a_terminal(
        &[
            "/bin/sh".into(),
            "-c".into(),
            format!("{piped} < /dev/null"),
        ],
        &[],
        &[],
        "",
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("standard input and standard error must both be that terminal"),
        "{text}"
    );
    let mut argv = vec!["/usr/bin/env".into(), "THESEUS_SESSION=ses_job".to_string()];
    argv.extend(r.argv(&["vault", "keep"]));
    let (code, out) = common::terminal::typed_at_a_terminal(&argv, &[], &[], "");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("this shell is a Theseus job's"), "{out}");
    let stopped = r.keep_rows("stopped");
    let why: Vec<&str> = stopped.iter().map(|s| s["why"].as_str().unwrap()).collect();
    assert_eq!(why, ["no_terminal", "not_owners_shell"], "{stopped:?}");
}
