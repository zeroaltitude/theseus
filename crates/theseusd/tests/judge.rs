//! The judge in a real daemon (M5 23a): `loop.v1` in shadow, with the real
//! `theseusd`, a stand-in for the Messages API, and the fake Jev.
//! - a start with the judge enabled and Jev out of reach answers health at
//!   once, and builds nothing of the judge before a judgment;
//! - a turn is judged once, after it, and recorded with its cost, which
//!   health's judge block counts; the model's request is the same bytes with
//!   the judge on and off;
//! - a `kill -9` while a judgment is out leaves today's block reserved past
//!   what was settled: the next start books that rest at its first
//!   judgment, never before.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode};

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
    fn new(jev: Option<&str>) -> Self {
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
            // loop.v1 alone: the inbound point's packs (25a, 25e's route.v1)
            // are judged in theseus-core's tests_inbound and tests_route, and
            // content.v2 in its tests_content (theseus-9ymz0). These tests count
            // the calls Jev is made, so a pack that asks about every item
            // entering a session would be counting its own.
            let packs = table(j, "packs");
            for p in [
                "classify.v4",
                "role.v1",
                "split.v1",
                "route.v8",
                "directive.v1",
                "onto.v1",
                "content.v2",
            ] {
                let mut off = toml::Table::new();
                off.insert("rung".into(), "off".into());
                packs.insert(p.into(), off.into());
            }
        }
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

    fn judge(&self) -> Value {
        self.call("health", Value::Null).unwrap()["judge"].clone()
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

/// Enabled, with Jev out of reach: nothing of the judge is built on the
/// start path. After serving, its client is built to warm Jev's connections
/// (theseus-ddbi; `judge/warm.rs`), and a warm-up that reaches nothing
/// moves no breaker and makes no call. The ladder's first read, after
/// serving, adopts the three packs the owner put live, once (26a), and
/// health then says so.
#[test]
fn a_start_with_the_judge_on_builds_nothing_of_it() {
    let rig = Rig::new(Some("http://127.0.0.1:9"));
    let d = rig.spawn();
    let j = rig.wait("the ladder's first read and the warm-up's client", || {
        let j = rig.judge();
        (j["packs"]
            .to_string()
            .contains("(owner: decision of 2026-10-04)")
            && j["breaker"] != "idle")
            .then_some(j)
    });
    assert_eq!(j["enabled"], true, "{j}");
    assert_eq!(j["breaker"], "closed", "{j}");
    assert_eq!(
        j["packs"],
        json!([
            // Live at a turn's stop while `[judge] stop_nudge` is on (theseus-fh0u).
            "loop.v1: live",
            "security.v3: live (owner: decision of 2026-10-04)",
            "classify.v4: off",
            "role.v1: off",
            "split.v1: off",
            "route.v8: off (the config's ceiling; on the ladder: live (owner: the owner's decision of 2026-10-23))",
            "directive.v1: off",
            "onto.v1: off",
            // The sweep's merge and split packs, live from their landing (theseus-nzzz4).
            "same_thing.v1: live",
            "two_things.v1: live",
            "same_kind.v1: live",
            "name_it.v1: live",
            "continue.v1: shadow",
            "categorize.v1: shadow",
            "rerank.v1: live (owner: decision of 2026-10-04)",
            "recall_cross.v1: live",
            "families.v1: live",
            "memory.v1: shadow",
            "attribution.v1: shadow",
            "citation.v1: shadow",
            "compare.v1: shadow",
            "heard.v1: live",
            "people.v1: live",
            "people_seen.v1: live",
            // Off in this rig: these tests count the calls Jev is made, and a
            // pack that asks about every item entering a session would be
            // counting its own (theseus-9ymz0).
            "content.v2: off"
        ])
    );
    assert_eq!(rig.rows("pack.mode").len(), 3, "one adoption row each");
    assert!(rig.rows("judge.call").is_empty());
    rig.stop(d);
}

/// `text` with the time a turn's prompt leads with (`[Terminal · Wed
/// 2026-10-21 07:13 -07:00] …`, written by `surface::lead`) replaced by
/// `<now>`: two turns a minute apart are otherwise the same request.
fn unstamped(text: &str) -> String {
    const HEAD: &str = "[Terminal · ";
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(HEAD) {
        let (head, tail) = rest.split_at(i + HEAD.len());
        out.push_str(head);
        match tail.find(']') {
            Some(j) => {
                out.push_str("<now>");
                rest = &tail[j..];
            }
            None => {
                rest = tail;
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The stamp is all `unstamped` takes out: a minute or a day apart compares
/// equal, one other byte does not.
#[test]
fn the_time_a_prompt_leads_with_is_named_and_nothing_else() {
    let at = |s: &str| format!(r#"{{"text":"[Terminal · {s}] Say done."}}"#);
    let (a, b) = (
        at("Wed 2026-10-21 07:13 -07:00"),
        at("Wed 2026-10-21 07:14 -07:00"),
    );
    assert_eq!(unstamped(&a), unstamped(&b));
    let (late, early) = (
        at("Wed 2026-10-21 23:59 -07:00"),
        at("Thu 2026-10-22 00:00 -07:00"),
    );
    assert_eq!(unstamped(&late), unstamped(&early));
    assert_ne!(unstamped(&a), unstamped(&a.replace("Say done", "Say dune")));
    assert_ne!(unstamped(&a), unstamped(&a.replace("Terminal", "Termina1")));
}

/// One turn is judged once, after it, with its cost in health's block; the
/// model was asked the same bytes as with the judge off.
#[test]
fn a_turn_is_judged_after_it_and_asks_the_model_the_same_bytes() {
    let jev = FakeJev::start().unwrap();
    let on = Rig::new(Some(&jev.base()));
    let d = on.spawn();
    let res = on.ask("Say done.");
    assert_eq!(res["stop_reason"], "no_tool_calls", "{res}");
    let rows = on.wait("the judgment", || {
        let r = on.rows("judge.call");
        (!r.is_empty()).then_some(r)
    });
    assert_eq!(rows.len(), 1);
    let d0 = &rows[0]["data"];
    assert_eq!(d0["pack"], "loop.v1");
    assert_eq!(d0["outcome"]["outcome"], "answered", "{d0}");
    assert_eq!(rows[0]["session_id"], res["session_id"]);
    let cost = d0["cost_micros"].as_u64().unwrap();
    assert!(cost > 0);
    let j = on.wait("health's count", || {
        let j = on.judge();
        (j["calls_today"] == 1).then_some(j)
    });
    assert_eq!(
        j["spend_today_usd"].as_f64(),
        Some(cost as f64 / 1e6),
        "{j}"
    );
    assert_eq!(jev.seen().len(), 1);
    on.stop(d);

    let off = Rig::new(None);
    let d = off.spawn();
    off.ask("Say done.");
    assert_eq!(off.judge()["enabled"], false);
    off.stop(d);
    // Each rig's own temp dir and its turn's minute are all that differ, and are named.
    let bytes = |r: &Rig| {
        let raw = r.model.raw_requests();
        assert_eq!(raw.len(), 1);
        let text = String::from_utf8_lossy(&raw[0]);
        unstamped(&text.replace(&r.dir.path().display().to_string(), "<dir>"))
    };
    assert_eq!(
        bytes(&on),
        bytes(&off),
        "the model's request, with the judge on and off"
    );
}

/// A `kill -9` while a judgment is out: the next start reads today's block
/// (health shows what was settled) and books its rest at its first
/// judgment, with one `judge.block_booked` row, never before.
#[test]
fn a_kill_mid_block_books_the_rest_at_the_next_starts_first_judgment() {
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Slow(Duration::from_secs(4)));
    let rig = Rig::new(Some(&jev.base()));
    let d = rig.spawn();
    rig.ask("Say done.");
    rig.wait("the call to Jev", || (jev.connections() == 1).then_some(()));
    drop(d); // SIGKILL, then reaped
    jev.set_mode(FakeMode::Up);
    let d = rig.spawn();
    let j = rig.judge();
    assert_eq!(
        j["spend_today_usd"], 0.0,
        "nothing settled before the kill: {j}"
    );
    assert!(
        rig.rows("judge.block_booked").is_empty(),
        "nothing booked before a judgment"
    );
    rig.ask("Say done again.");
    let booked = rig.wait("the booked rest", || {
        let r = rig.rows("judge.block_booked");
        (!r.is_empty()).then_some(r)
    });
    assert_eq!(booked.len(), 1);
    assert_eq!(booked[0]["data"]["booked_micros"], 10_000, "{booked:?}");
    rig.wait("the second judgment", || {
        (rig.rows("judge.call").len() == 1).then_some(())
    });
    let spent = rig.judge()["spend_today_usd"].as_f64().unwrap();
    assert!(spent > 0.01, "the block's rest, then the call: {spent}");
    rig.stop(d);
}

/// The ladder (M5 26a) on a real daemon, over its socket as the CLI sends:
/// the owner's forced canary is a row and health's line; a security
/// promotion is a card listed with every question, whose approval writes
/// the mode; and after a restart with `max_mode = "shadow"`, health shows
/// every pack in shadow and the restart wrote no row.
#[test]
fn the_ladder_moves_over_the_socket_and_max_mode_caps_it_after_a_restart() {
    let rig = Rig::new(Some("http://127.0.0.1:9"));
    let d = rig.spawn();
    let r = rig
        .call(
            "pack.promote",
            json!({"pack": "loop.v1", "to": "canary", "share": 1.0}),
        )
        .unwrap();
    assert_eq!(r["row"]["forced"], true, "{r}");
    let line = |rig: &Rig, pack: &str| -> String {
        rig.judge()["packs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .find(|l| l.starts_with(pack))
            .unwrap()
            .to_string()
    };
    assert_eq!(
        line(&rig, "loop.v1"),
        "loop.v1: canary 1.0 (owner: forced by the owner)"
    );
    let card = rig
        .call("pack.promote", json!({"pack": "security.v1", "to": "live"}))
        .unwrap();
    let q = card["question"].as_str().unwrap().to_string();
    let listed = rig.call("confirm.list", Value::Null).unwrap();
    assert!(listed.to_string().contains(&q), "{listed}");
    // security.v1 is wired nowhere, so health has no line for it: its rows are the proof.
    let moved = |rig: &Rig| {
        rig.rows("pack.mode")
            .iter()
            .filter(|r| r.to_string().contains("security.v1"))
            .count()
    };
    assert_eq!(moved(&rig), 0, "no mode yet");
    rig.call(
        "action.confirm",
        json!({"correlation_id": q, "approve": true}),
    )
    .unwrap();
    assert_eq!(moved(&rig), 1, "the approved card wrote its mode");
    let rows = rig.rows("pack.mode").len();
    assert_eq!(
        rows, 5,
        "the three live packs' adoptions, the canary, and the approved card"
    );
    rig.stop(d);
    let path = rig.path("config.toml");
    let mut t: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    t["judge"]
        .as_table_mut()
        .unwrap()
        .insert("max_mode".into(), "shadow".into());
    std::fs::write(&path, toml::to_string(&t).unwrap()).unwrap();
    let d = rig.spawn();
    let lines = rig.wait("the ladder's first read", || {
        let p = rig.judge()["packs"].clone();
        p.to_string().contains("the config's ceiling").then_some(p)
    });
    for l in lines.as_array().unwrap().iter().filter_map(Value::as_str) {
        assert!(
            l.contains(": shadow") || l.contains(": off"),
            "{l}: every pack capped"
        );
    }
    assert!(
        lines.to_string().contains("on the ladder: canary 1.0"),
        "{lines}"
    );
    assert_eq!(rig.rows("pack.mode").len(), rows, "a restart writes none");
    rig.stop(d);
}
