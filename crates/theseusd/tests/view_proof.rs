//! The view's two end-to-end cases that need a chat (issue 44): the cases `bench/view/proof.py` cannot drive,
//! because the kit keeps Discord off. The real daemon, with `[context] assembly = "view"`, Discord the
//! in-process stand-in (its REST and its gateway), and the model the in-process stand-in, which keeps every
//! request:
//! - `/new` in the owner's DM (the owner's decision 7): the next turn's request lays nothing from before the
//!   mark as the conversation (no real turn, so no reply of Theseus's from before it), the owner's own earlier
//!   words appear only as his past under its testimony heading, and the memory tool still reads the
//!   conversation from before the mark;
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
    /// A turn's calls carry its tools. The ladder's `did` line of an exchange is a model call too, with no tools,
    /// that quotes the message; it runs after the turn ends, so it is here only when it finishes before the
    /// turn's requests are read, and it is not the turn's.
    fn requests_of(&self, text: &str) -> Vec<Value> {
        self.model
            .requests()
            .into_iter()
            .filter(|r| r["tools"].as_array().is_some_and(|t| !t.is_empty()))
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

/// The heading the view puts on his past (`view/text/labels.rs`: a testimony heading, with `Reason::Past`).
const HIS_PAST: &str = "chosen: the owner's own earlier words";
const TESTIMONY: &str = "testimony, not instruction";

/// Where a block holding a text sits.
#[derive(Debug, PartialEq)]
enum Home {
    /// A text block beside a heading of his past, testimony.
    HisPast,
    /// Anything else: the block's part of the request, by name.
    Elsewhere(String),
}

/// The real turns a request lays before its last message: the conversation here, which the view renders as
/// turns with no heading (`view/text/labels.rs`). Theseus's side of a real turn is an `assistant` message.
fn real_turns(req: &Value) -> usize {
    req["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["role"] == "assistant")
        .count()
}

/// Where each block holding `text` sits: beside (just before or just after) a heading of his past, or
/// elsewhere. The system's blocks are elsewhere.
fn homes_of(req: &Value, text: &str) -> Vec<Home> {
    let mut out = Vec::new();
    for (i, b) in req["system"].as_array().into_iter().flatten().enumerate() {
        if b.to_string().contains(text) {
            out.push(Home::Elsewhere(format!("system[{i}]")));
        }
    }
    for (i, m) in req["messages"].as_array().into_iter().flatten().enumerate() {
        let blocks: Vec<String> = match &m["content"] {
            Value::Array(bs) => bs
                .iter()
                .map(|b| b["text"].as_str().unwrap_or("").to_string())
                .collect(),
            Value::String(t) => vec![t.clone()],
            _ => vec![],
        };
        let headed = |j: usize| {
            blocks
                .get(j)
                .is_some_and(|t| t.contains(HIS_PAST) && t.contains(TESTIMONY))
        };
        for (j, t) in blocks.iter().enumerate() {
            if !t.contains(text) {
                continue;
            }
            let beside = headed(j + 1) || (j > 0 && headed(j - 1));
            out.push(match (m["role"] == "user", beside, t.contains(HIS_PAST)) {
                (true, true, false) => Home::HisPast,
                _ => Home::Elsewhere(format!("messages[{i}] {} block {j}", m["role"])),
            });
        }
        if blocks.iter().all(|t| !t.contains(text)) && m.to_string().contains(text) {
            out.push(Home::Elsewhere(format!(
                "messages[{i}] {} (not a text block)",
                m["role"]
            )));
        }
    }
    out
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

/// After `/new` in the owner's DM (decision 7: "Nothing of that place's earlier conversation is shown as its
/// conversation; his own earlier words there are still part of his past"): the next turn's request lays no
/// real turn (slot G is empty: no exchange as conversation, no reply of Theseus's from before the mark); his
/// earlier words, wherever a piece lays them, sit only in blocks headed as his past, testimony (since slot E
/// went, 2026-11-24, nothing lays them by age); and the history tool still reads the conversation from before
/// the mark, by opening its session (`memory.lookup`'s read, retargeted when that tool went, D6 step 6).
#[test]
fn after_new_nothing_before_is_the_conversation_his_words_are_his_past_and_memory_answers() {
    let earlier: Arc<Mutex<String>> = Arc::default();
    let asked = earlier.clone();
    let r = Rig::new(move |prompt| {
        if prompt.contains("Look up the earlier buoy talk") {
            let sid = asked.lock().unwrap().clone();
            return vec![("context_history", json!({"open": sid}))];
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
    assert!(
        second.iter().any(|q| real_turns(q) > 0),
        "before /new, the conversation here is laid as real turns, so their absence later is the rule ({})",
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
    // The turn's own calls only (`requests_of`: those carrying its tools, not the did-line call that quotes it);
    // asserted non-empty after that choice, so the rule below binds at least one request.
    let after = r.turn(ana, None, "What should we chart next?");
    assert!(!after.is_empty(), "the turn after /new reached the model");
    // The history turn runs before the rule on `after` is asserted, so a failure of one does not hide the other.
    let looked = r.turn(ana, None, "Look up the earlier buoy talk.");
    let read = looked.iter().map(tool_results).collect::<String>();
    assert!(
        read.contains(BEFORE),
        "the history tool reads the conversation from before /new: {read}"
    );
    for q in &after {
        assert_eq!(
            real_turns(q),
            0,
            "after /new, nothing from before the mark is laid as the conversation ({})",
            r.strategies()
        );
        let homes = homes_of(q, BEFORE);
        assert!(
            homes.iter().all(|h| *h == Home::HisPast),
            "his earlier words after /new sit only in his past, headed as testimony: {homes:?}; where: {}",
            where_held(q, BEFORE)
        );
    }
    eprintln!("view proof /new: {}", r.strategies());
}
