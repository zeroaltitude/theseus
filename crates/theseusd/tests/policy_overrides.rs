//! The owner's own `[policy]` overrides, driven through a real daemon and
//! the core's turn (theseus-m9fdg.5). The pieces are tested alone in the
//! core (`policy.rs`, `tests_notices.rs`); this is their composition on
//! the wire: a daemon at a posture, an owner who lists
//! `allow_argv = [["echo"]]` and `notify_argv = [["echo", "loud"]]`, and a
//! model that runs three programs, one a turn.
//!
//! Faked: the model (`FakeModel`, a stand-in Messages API). Real: the
//! daemon process, its config loader, the gate, the kernel, the job
//! wrapper and the ledger.

mod common;

use common::model::FakeModel;
use common::{table, Served};
use serde_json::{json, Value};

/// A TOML list of argv prefixes.
fn argvs(list: &[&[&str]]) -> toml::Value {
    toml::Value::Array(
        list.iter()
            .map(|a| {
                toml::Value::Array(a.iter().map(|s| toml::Value::String((*s).into())).collect())
            })
            .collect(),
    )
}

struct Rig {
    s: Served,
    _model: FakeModel,
    _keys: tempfile::TempDir,
}

impl Rig {
    /// `enforcement` is the daemon's posture; `allow` lists `allow_argv`.
    fn start(enforcement: &str, allow: bool) -> Self {
        let model = FakeModel::start(|prompt| {
            let argv: Vec<&str> = match prompt.split_whitespace().next() {
                Some("quiet") => vec!["echo", "quiet-ran"],
                Some("loud") => vec!["echo", "loud", "loud-ran"],
                Some("other") => vec!["printf", "other-ran"],
                _ => return vec![],
            };
            vec![("proc_run", json!({"argv": argv}))]
        });
        let base = model.base.clone();
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
                policy.insert("enforcement".into(), enforcement.into());
                policy.insert("notify_argv".into(), argvs(&[&["echo", "loud"]]));
                if allow {
                    policy.insert("allow_argv".into(), argvs(&[&["echo"]]));
                }
                table(t, "tools").insert("proc_sync_secs".into(), 10.into());
            },
        );
        Self {
            s,
            _model: model,
            _keys: keys,
        }
    }

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

/// At `open`, the owner's `notify_argv` entry makes `echo loud` run and
/// say so (a `tool.notified` row naming the entry), while `echo quiet` and
/// `printf` run with no notice: the override changes only what it names.
#[test]
fn a_notify_entry_runs_the_program_and_tells_the_owner_at_an_open_posture() {
    let r = Rig::start("open", false);
    let ran = |res: &Value| res["stop_reason"] == "no_tool_calls" && res["tool_calls"] == 1;
    let (q, l, o) = (r.turn("quiet"), r.turn("loud"), r.turn("other"));
    for res in [&q, &l, &o] {
        assert!(ran(res), "ran unasked: {res}\n{}", r.s.log());
    }
    let notified = r.ledger("tool.notified");
    // The positive count: exactly the one call the override names.
    assert_eq!(notified.len(), 1, "{notified:?}");
    assert_eq!(notified[0]["tool"], "proc.run");
    assert_eq!(
        notified[0]["input"]["argv"],
        json!(["echo", "loud", "loud-ran"])
    );
    let why = notified[0].to_string();
    assert!(
        why.contains("echo loud"),
        "the notice names the entry: {why}"
    );
    assert!(r.ledger("tool.confirm_requested").is_empty());
}

/// At `approve`, `allow_argv = [["echo"]]` runs `echo quiet` unasked, and an
/// `echo` that also matches `notify_argv` is read first and falls to the
/// posture, so it asks, naming the entry: allow and notify compose as the
/// policy's header says (a notify match is no allow). `printf` asks.
#[test]
fn an_allow_entry_runs_unasked_but_a_notify_match_beside_it_still_asks_at_approve() {
    let r = Rig::start("approve", true);
    let q = r.turn("quiet");
    assert_eq!(q["stop_reason"], "no_tool_calls", "{q}");
    assert_eq!(q["tool_calls"], 1);
    let l = r.turn("loud");
    let o = r.turn("other");
    assert_eq!(l["stop_reason"], "awaiting_confirm", "{l}");
    assert_eq!(o["stop_reason"], "awaiting_confirm", "{o}");
    let asked = r.ledger("tool.confirm_requested");
    assert_eq!(asked.len(), 2, "{asked:?}");
    let reasons: Vec<String> = asked.iter().map(|a| a["reason"].to_string()).collect();
    assert!(
        reasons[0].contains("notify list entry `echo loud`"),
        "{reasons:?}"
    );
    assert!(!reasons[1].contains("notify list"), "{reasons:?}");
}
