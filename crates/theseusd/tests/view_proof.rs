//! The view's two end-to-end cases that need a chat (issue 44): the cases `bench/view/proof.py` cannot drive,
//! because the kit keeps Discord off. The real daemon, with `[context] assembly = "view"`, Discord the
//! in-process stand-in (its REST and its gateway), and the model the in-process stand-in, which keeps every
//! request:
//! - `/new` in the owner's DM: the next turn's request holds nothing said before the mark, and the memory
//!   tool still reads the conversation from before it;
//! - a shared channel with a second person in it: a turn there holds nothing the owner said in his DM.
//!
//! Every word is the test's own. A marker string stands for what must not be sent, and each test first shows
//! the marker IS sent where it belongs, so its absence later is the rule and not a request that holds
//! nothing.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::{table, Served};
use serde_json::{json, Value};
use theseus_sim::fake_discord::{FakeDiscord, Guild, Typed, DEFAULT_GUILD};

const ANA: u64 = 900_000_000_000_000_201;
const BEN: u64 = 900_000_000_000_000_202;
const HARBOUR: u64 = 900_000_000_000_000_020;
const KEY_VAR: &str = "THESEUS_VIEW_PROOF_TEST_KEY";

const BEFORE: &str = "BUOYCODE-BEFORE-NEW-4471";
const PRIVATE: &str = "OSPREY-PRIVATE-HARBOUR-9902";

struct Rig {
    s: Served,
    fake: Arc<FakeDiscord>,
    model: FakeModel,
}

/// ana owns the guild; `#harbour` is a channel every member views, ben among them.
fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .channel(HARBOUR, "harbour")
}

fn bindings() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{HARBOUR}\"\nname = \"harbour\"\nusers = [\"{ANA}\", \"{BEN}\"]\n\
         mention_only = false\nprivate = false\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

impl Rig {
    fn new(calls: impl Fn(&str) -> Vec<(&'static str, Value)> + Send + Sync + 'static) -> Self {
        let fake = FakeDiscord::start_with_gateway();
        fake.set_guild(guild());
        let gateway = fake.gateway().unwrap().addr.clone();
        let model = FakeModel::start(calls);
        let (base, rest) = (model.base.clone(), fake.addr.clone());
        let s = Served::start_with(
            |dir| {
                std::fs::create_dir_all(dir.join("state")).unwrap();
                std::fs::write(dir.join("state/bindings.toml"), bindings()).unwrap();
            },
            move |t| {
                table(t, "model").insert("api_base".into(), base.clone().into());
                for (_, p) in table(t, "providers").iter_mut() {
                    p.as_table_mut()
                        .unwrap()
                        .insert("api_base".into(), base.clone().into());
                }
                for (_, v) in table(t, "secrets").iter_mut() {
                    *v = format!("env:{KEY_VAR}").into();
                }
                let d = table(t, "discord");
                d.insert("enabled".into(), true.into());
                d.insert("rest_proxy".into(), rest.into());
                d.insert("gateway_proxy".into(), format!("ws://{gateway}").into());
                d.insert("edit_interval_ms".into(), 250.into());
                d.insert("ping_window_secs".into(), 0.into());
                d.insert("task_home".into(), "".into());
                table(t, "places").insert("owner".into(), vec![format!("discord:{ANA}")].into());
                table(t, "context").insert("assembly".into(), "view".into());
            },
            |cmd| {
                cmd.env(KEY_VAR, "tv-view-proof-test-2c41");
            },
        );
        Self { s, fake, model }
    }

    /// Type as `user`, once the binding has connected to the gateway; `channel` None is the user's DM.
    fn say(&self, user: (u64, &str), channel: Option<u64>, text: &str) {
        wait("the binding on the gateway", 30, || {
            self.fake
                .say(&Typed {
                    user: user.0,
                    name: user.1,
                    channel,
                    content: text,
                    file: None,
                })
                .is_ok()
                .then_some(())
        });
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.s
            .call("ledger.tail", json!({"n": 500, "kind": kind}))
            .ok()
            .and_then(|v| v["rows"].as_array().cloned())
            .unwrap_or_default()
    }

    /// Say `text` and wait until a turn it opened has ended; the model's requests that turn made.
    fn turn(&self, user: (u64, &str), channel: Option<u64>, text: &str) -> Vec<Value> {
        let ended = self.rows("turn.ended").len();
        self.say(user, channel, text);
        wait(text, 60, || {
            (self.rows("turn.ended").len() > ended).then_some(())
        });
        self.requests_of(text)
    }

    /// Every request whose last user text holds `text` (the turn's own, its later calls included).
    fn requests_of(&self, text: &str) -> Vec<Value> {
        self.model
            .requests()
            .into_iter()
            .filter(|r| opening(r).contains(text))
            .collect()
    }

    /// The `context.compiled` rows' strategies and the `context.view_fallback` rows' reasons, for the log.
    fn strategies(&self) -> String {
        let s: Vec<String> = self
            .rows("context.compiled")
            .iter()
            .map(|r| r["data"]["strategy"].as_str().unwrap_or("?").to_string())
            .collect();
        let f: Vec<String> = self
            .rows("context.view_fallback")
            .iter()
            .map(|r| r["data"]["why"].as_str().unwrap_or("?").to_string())
            .collect();
        format!("compiled {s:?}, hand-backs {f:?}")
    }
}

/// The text of a request's last user message that carries no tool result: what opened its turn.
fn opening(req: &Value) -> String {
    let msgs = req["messages"].as_array().cloned().unwrap_or_default();
    for m in msgs.iter().rev() {
        if m["role"] != "user" {
            continue;
        }
        match &m["content"] {
            Value::String(s) => return s.clone(),
            Value::Array(blocks) => {
                if blocks.iter().any(|b| b["type"] == "tool_result") {
                    continue;
                }
                return blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            _ => return String::new(),
        }
    }
    String::new()
}

/// Whether `text` is anywhere in the request (its system, its tools and its messages).
fn holds(req: &Value, text: &str) -> bool {
    req.to_string().contains(text)
}

/// Where `text` sits in a request, for a failure's message: the part (system block or message, by index and
/// role) and the characters around it. Every word in a request here is the test's own.
fn where_held(req: &Value, text: &str) -> String {
    let mut parts: Vec<(String, String)> = Vec::new();
    for (i, b) in req["system"].as_array().into_iter().flatten().enumerate() {
        parts.push((format!("system[{i}]"), b.to_string()));
    }
    for (i, m) in req["messages"].as_array().into_iter().flatten().enumerate() {
        parts.push((format!("messages[{i}] {}", m["role"]), m.to_string()));
    }
    parts
        .iter()
        .filter_map(|(name, body)| {
            let at = body.find(text)?;
            let from = body[..at]
                .char_indices()
                .rev()
                .nth(600)
                .map_or(0, |(i, _)| i);
            let to = body[at..]
                .char_indices()
                .nth(200)
                .map_or(body.len(), |(i, _)| at + i);
            Some(format!("{name}: …{}…", &body[from..to]))
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The text of every tool result the request carries.
fn tool_results(req: &Value) -> String {
    let mut out = String::new();
    for m in req["messages"].as_array().into_iter().flatten() {
        for b in m["content"].as_array().into_iter().flatten() {
            if b["type"] == "tool_result" {
                out.push_str(&b["content"].to_string());
            }
        }
    }
    out
}

fn wait<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let t0 = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "no {what} within {secs} s"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// After `/new` in the owner's DM, the next turn's request holds nothing said before the mark; the memory
/// tool, asked for the conversation from before it, still reads it.
///
/// Ignored, as a finding, not a pass: on today's code the memory half holds, and the request after `/new`
/// still carries the owner's own earlier words there, laid as his past (slot E, "the owner's own earlier
/// words"), as the design's decision 7 rules. This test states the stricter rule it was asked to; which rule
/// holds is the owner's call, and the test is not bent to either.
#[test]
#[ignore = "finding: decision 7 keeps the owner's own words before /new as his past (slot E)"]
fn after_new_the_next_turn_holds_nothing_from_before_and_memory_still_answers() {
    let earlier: Arc<Mutex<String>> = Arc::default();
    let asked = earlier.clone();
    let r = Rig::new(move |prompt| {
        if prompt.contains("Look up the earlier buoy talk") {
            let sid = asked.lock().unwrap().clone();
            return vec![("memory_lookup", json!({"session": sid}))];
        }
        vec![]
    });
    let ana = (ANA, "ana");
    let first = r.turn(ana, None, &format!("The buoy code is {BEFORE}."));
    assert!(!first.is_empty(), "the first turn reached the model");
    let second = r.turn(ana, None, "Please note the tide too.");
    assert!(
        second.iter().any(|q| holds(q, BEFORE)),
        "before /new, the next turn holds the earlier message ({})",
        r.strategies()
    );
    let before_new = r.rows("turn.started");
    let sid = before_new.last().unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    *earlier.lock().unwrap() = sid;
    // `/new` is a control word: the binding answers it itself, and no turn runs.
    let bot_said = || {
        r.fake
            .messages(ANA + 1)
            .iter()
            .filter(|m| m.author != ANA.to_string())
            .count()
    };
    let said = bot_said();
    r.say(ana, None, "/new");
    wait("the answer to /new", 30, || {
        (bot_said() > said).then_some(())
    });
    let after = r.turn(ana, None, "What should we chart next?");
    assert!(!after.is_empty(), "the turn after /new reached the model");
    // The memory turn runs before the rule on `after` is asserted, so a failure of one does not hide the other.
    let looked = r.turn(ana, None, "Look up the earlier buoy talk.");
    let read = looked.iter().map(tool_results).collect::<String>();
    assert!(
        read.contains(BEFORE),
        "the memory tool reads the conversation from before /new: {read}"
    );
    for q in &after {
        assert!(
            !holds(q, BEFORE),
            "a request after /new holds a message from before it ({}); where: {}",
            r.strategies(),
            where_held(q, BEFORE)
        );
    }
    eprintln!("view proof /new: {}", r.strategies());
}

/// In a shared channel with a second person in it, a turn's request holds nothing the owner said in his
/// own DM.
#[test]
fn a_shared_place_with_a_second_person_holds_nothing_of_the_owners_dm() {
    let r = Rig::new(|_| vec![]);
    let ana = (ANA, "ana");
    r.turn(ana, None, &format!("Keep this between us: {PRIVATE}."));
    let dm = r.turn(ana, None, "What did I just tell you?");
    assert!(
        dm.iter().any(|q| holds(q, PRIVATE)),
        "in the DM, the next turn holds the owner's earlier message ({})",
        r.strategies()
    );
    // Ben speaks first, so he is present; whether his message gets a turn is the binding's, not this test's.
    let started = r.rows("turn.started").len();
    r.say(
        (BEN, "ben"),
        Some(HARBOUR),
        "Morning all, the tide is turning.",
    );
    let t0 = Instant::now();
    while r.rows("turn.started").len() == started && t0.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    wait("every turn ended", 60, || {
        (r.rows("turn.started").len() <= r.rows("turn.ended").len() + r.rows("turn.failed").len())
            .then_some(())
    });
    let shared = r.turn(
        ana,
        Some(HARBOUR),
        "What is the plan for the harbour today?",
    );
    assert!(
        !shared.is_empty(),
        "the shared channel's turn reached the model"
    );
    for q in &shared {
        assert!(
            !holds(q, PRIVATE),
            "a request in the shared channel holds the owner's DM ({})",
            r.strategies()
        );
    }
    eprintln!("view proof shared place: {}", r.strategies());
}
