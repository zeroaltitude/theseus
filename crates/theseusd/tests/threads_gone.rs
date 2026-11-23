//! Threads are gone (2026-11-22), against a real daemon: a message that names a thread (`#slug`)
//! runs in the place it was written, like any other message, and its words are in that place's
//! next request; `theseus ask --thread SLUG` runs here, says so in one line, and prints no
//! thread's title after the answer.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;

use common::model::FakeModel;
use serde_json::{json, Value};

const SAID: &str = "#harbour-log the tide gauge reads four metres at the north wall";

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
    fn new() -> Self {
        let model = FakeModel::start(|_| vec![]);
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
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, model }
    }

    fn ask(&self, sid: &str, prompt: &str) -> Value {
        self.call(
            "turn.submit",
            json!({"session_id": sid, "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    /// The model's requests whose text holds `words`.
    fn requests_holding(&self, words: &str) -> Vec<Value> {
        (self.model.requests().into_iter())
            .filter(|q| q.to_string().contains(words))
            .collect()
    }
}

/// A message written with a thread's address runs in its own session, whatever thread it names
/// (one made with that slug included), and the next turn there carries its words.
#[test]
fn a_message_with_a_thread_address_runs_in_its_own_place() {
    let r = Rig::new();
    let _d = r.spawn();
    // A thread of that slug, where threads can still be made: the address is still ignored.
    let _ = r.call(
        "thread.new",
        json!({"slug": "harbour-log", "title": "Harbour log"}),
    );
    let sid = r.call("session.open", json!({})).unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let first = r.ask(&sid, SAID);
    assert_eq!(first["session_id"], sid.as_str(), "it ran here: {first}");
    assert!(first["thread"].is_null(), "no thread: {first}");
    let next = r.ask(&sid, "What did I just say about the gauge?");
    assert_eq!(next["session_id"], sid.as_str());
    let asked = r.requests_holding("What did I just say about the gauge?");
    assert!(!asked.is_empty(), "the next turn reached the model");
    assert!(
        asked.iter().any(|q| q.to_string().contains(SAID)),
        "the message's words, address and all, are in this place's next request"
    );
}

/// `theseus ask` with a thread named, by `--thread` or by the shell, `env` set on it.
fn ask_naming_a_thread(r: &Rig, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let bin = common::cli_bin();
    assert!(
        bin.exists(),
        "the theseus binary is built beside the daemon's"
    );
    std::process::Command::new(bin)
        .arg("--socket")
        .arg(r.path("sock"))
        .args(["ask", "--no-stream"])
        .args(args)
        .env_remove("THESEUS_SESSION")
        .env_remove("THESEUS_THREAD")
        .envs(env.iter().copied())
        .output()
        .expect("running theseus")
}

/// `theseus ask --thread SLUG` runs here, and says so in one line.
#[test]
fn ask_with_a_thread_runs_here_and_says_so() {
    let r = Rig::new();
    let _d = r.spawn();
    let o = ask_naming_a_thread(
        &r,
        &[
            "--thread",
            "harbour-log",
            "hello from the shell about the north wall",
        ],
        &[],
    );
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    assert!(err.contains("threads are gone; this ran here"), "{err}");
    assert!(
        !r.requests_holding("hello from the shell about the north wall")
            .is_empty(),
        "the turn ran and reached the model"
    );
}

/// The thread an ask names and ignores is never printed as where the answer was (issue 44): no
/// reply tag with its title or slug after the answer, by `--thread` or by `THESEUS_THREAD`.
#[test]
fn an_ignored_thread_address_prints_no_thread_title() {
    let r = Rig::new();
    let _d = r.spawn();
    for (args, env) in [
        (
            vec!["--thread", "harbour-log", "first word from the north wall"],
            vec![],
        ),
        (
            vec!["second word from the north wall"],
            vec![("THESEUS_THREAD", "harbour-log")],
        ),
    ] {
        let o = ask_naming_a_thread(&r, &args, &env);
        let (out, err) = (
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr),
        );
        assert!(o.status.success(), "{err}");
        assert!(err.contains("threads are gone; this ran here"), "{err}");
        for text in [&out, &err] {
            assert!(!text.contains("→ harbour-log"), "a thread's tag: {text}");
            assert!(!text.contains("Harbour log"), "a thread's title: {text}");
        }
    }
}
