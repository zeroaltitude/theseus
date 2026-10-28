//! Capture in a real daemon (theseus-slvl.5): `directive.v1`, live, with the
//! real `theseusd`, a stand-in for the Messages API, and the fake Jev with
//! scripted verdicts. The owner types "no tables with me" in the CLI's
//! socket (a CLI surface: the owner's own words):
//! - a sure preference is kept as a standing record with a notice (its rows),
//!   and the model's next request carries it in the system's second block;
//! - an undo (`standing.revoke`) ends it and labels the judgment
//!   `wrong capture`;
//! - said again, with the record live, writes `standing.repeated`, no second
//!   record, and `standing.list` counts it for the week;
//! - an order is asked, never kept; the answer keeps it;
//! - `[standing] capture = false` asks Jev nothing.
//!
//! The capture is judged beside the turn and written off its path.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted};

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
    /// The template on the stand-in model, the judge on (at `jev`, or off).
    fn new(jev: Option<&str>, capture: bool) -> Self {
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
        if let Some(base) = jev {
            // These tests count Jev's calls: recall (on by default,
            // theseus-is8t) brings its own judgments.
            table(&mut t, "memory").insert("mode".into(), "off".into());
            let j = table(&mut t, "judge");
            j.insert("enabled".into(), true.into());
            j.insert("api_base".into(), base.into());
            j.insert("connect_secs".into(), 1.into());
            j.insert("total_secs".into(), 5.into());
            // directive.v1 alone at the inbound point: its batch with the
            // other inbound packs is theseus-core's tests_capture.
            let packs = table(j, "packs");
            for p in ["classify.v4", "role.v1", "route.v9"] {
                let mut off = toml::Table::new();
                off.insert("rung".into(), "off".into());
                packs.insert(p.into(), off.into());
            }
        }
        table(&mut t, "standing").insert("capture".into(), capture.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, model }
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s; the daemon's log ends:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.call("ledger.tail", json!({"n": 500, "kind": kind}))
            .unwrap()["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn ask(&self, prompt: &str) -> Value {
        self.call(
            "turn.submit",
            json!({"input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    fn stop(&self, mut d: Daemon) {
        let _ = self.call("shutdown", Value::Null);
        self.wait("the daemon's exit", || d.try_wait());
    }
}

fn hears(jev: &FakeJev, kind: &str, confidence: f64) {
    jev.script(
        "directive.v1/kind",
        Scripted::Choice {
            option: kind.into(),
            confidence,
        },
    );
    jev.script(
        "directive.v1/scope",
        Scripted::Choice {
            option: "everywhere".into(),
            confidence: 0.9,
        },
    );
}

impl Rig {
    fn standing(&self) -> Value {
        self.call("standing.list", json!({"all": true})).unwrap()
    }

    fn live(&self) -> Vec<Value> {
        self.standing()["records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["state"] == "live")
            .cloned()
            .collect()
    }
}

/// Kept with a notice, undone, said again and counted.
#[test]
fn a_preference_is_kept_undone_and_counted_when_said_again() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.93);
    let r = Rig::new(Some(&jev.base()), true);
    let d = r.spawn();
    // Said once: kept.
    let res = r.ask("no tables with me");
    assert_eq!(res["stop_reason"], "no_tool_calls", "{res}");
    let live = r.wait("the kept record", || {
        let l = r.live();
        (!l.is_empty()).then_some(l)
    });
    assert_eq!(live.len(), 1, "{live:?}");
    let id = live[0]["id"].as_str().unwrap().to_string();
    assert_eq!(live[0]["text"], "no tables with me");
    assert_eq!(live[0]["kind"], "preference");
    assert_eq!(live[0]["scope"], json!({"kind": "everywhere"}));
    assert_eq!(live[0]["captured_by"], "capture");
    assert!(live[0]["source"]
        .as_str()
        .unwrap()
        .starts_with("capture:msg_"));
    let cap = r.rows("standing.captured");
    assert_eq!(cap.len(), 1, "{cap:?}");
    assert_eq!(cap[0]["data"]["record"], id.as_str());
    // The next turn's request to the model carries it in the system: a
    // plain question is nothing to keep.
    hears(&jev, "none", 0.95);
    r.ask("What is 2 + 2?");
    let carried = r.model.requests().iter().any(|q| {
        q["system"]
            .to_string()
            .contains(&format!("- {id}: no tables with me"))
    });
    assert!(carried, "the next turn's system lacks the record");
    // Said again, with it live: counted, never a second record.
    hears(&jev, "preference", 0.9);
    r.wait("a repeat is counted", || {
        r.ask("I said no tables with me");
        (!r.rows("standing.repeated").is_empty()).then_some(())
    });
    assert_eq!(r.live().len(), 1, "no second record");
    assert!(
        r.live()[0]["repeats"].as_u64().unwrap() >= 1,
        "{:?}",
        r.live()
    );
    // Undone: the record ends, and the judgment is labeled wrong.
    let ended = r.call("standing.revoke", json!({"id": id})).unwrap();
    assert_eq!(ended["ended"], id.as_str());
    assert!(r.live().is_empty());
    let labels = r.rows("judge.label");
    assert_eq!(labels.len(), 1, "{labels:?}");
    assert_eq!(labels[0]["data"]["label"], "wrong capture");
    r.stop(d);
}

/// An order is asked, never kept on confidence; the owner's answer keeps it.
#[test]
fn an_order_is_asked_and_the_answer_keeps_it() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "order", 0.99);
    let r = Rig::new(Some(&jev.base()), true);
    let d = r.spawn();
    r.ask("never push to main");
    let asks = r.wait("the ask", || {
        let a = r.standing()["asks"].as_array().cloned().unwrap_or_default();
        (!a.is_empty()).then_some(a)
    });
    assert_eq!(asks[0]["why"], "order");
    assert!(r.live().is_empty(), "nothing is kept unasked");
    let kept = r
        .call(
            "standing.answer",
            json!({"id": asks[0]["id"], "answer": "keep"}),
        )
        .unwrap();
    assert_eq!(kept["record"]["kind"], "order");
    assert_eq!(r.live().len(), 1);
    assert!(r.standing()["asks"].as_array().unwrap().is_empty());
    r.stop(d);
}

/// `[standing] capture = false`: Jev is asked nothing about the owner's words.
#[test]
fn capture_off_asks_jev_nothing() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.97);
    let r = Rig::new(Some(&jev.base()), false);
    let d = r.spawn();
    r.ask("no tables with me");
    std::thread::sleep(Duration::from_millis(800));
    let asked = jev.seen().into_iter().any(|s| {
        s.body["questions"]
            .as_object()
            .is_some_and(|q| q.keys().any(|k| k.starts_with("directive.v1/")))
    });
    assert!(!asked, "directive.v1 was asked with capture off");
    assert!(r.live().is_empty());
    assert!(r.rows("standing.captured").is_empty());
    r.stop(d);
}
