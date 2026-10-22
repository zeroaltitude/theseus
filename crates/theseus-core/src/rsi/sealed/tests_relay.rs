//! The model relay's tests (theseus-pw1q.21.5): a stand-in provider behind
//! it, and a candidate that asks it for a model as a job in L1 would. The
//! relay serves one model with the daemon's key, reserves each call before
//! it forwards, refuses at the lines and while self-improvement is off or
//! halted, and keeps usage lines alone.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::rsi::{SelfHaltParams, SelfMode, SelfResumeParams};
use theseus_protocol::Usage;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::relay::Relay;
use crate::approval::Answerer;
use crate::rpc::{Core, Parts};
use crate::secrets::{Secret, SecretBoard};
use crate::Config;

/// The key the daemon holds; it must reach the provider and nothing else.
pub(super) const KEY: &str = "sk-test-not-a-key";

/// What the stand-in provider answers.
#[derive(Clone, Copy)]
pub(super) enum Answers {
    /// A whole answer with this usage.
    Whole { input: u64, output: u64 },
    /// A stream that stops after its first event.
    Cut,
    /// A whole answer (1000 in, 200 out) given once `HELD` is notified.
    Held,
}

/// Releases the answer an `Answers::Held` provider is holding.
pub(super) static HELD: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// A request the stand-in provider saw.
pub(super) struct Seen {
    pub key: Option<String>,
    pub body: Vec<u8>,
}

/// A stand-in Messages API on loopback.
pub(super) struct Upstream {
    pub url: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

pub(super) async fn upstream(answers: Answers) -> Upstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head, body) = loop {
                    let n = conn.read(&mut chunk).await.unwrap_or(0);
                    buf.extend_from_slice(&chunk[..n]);
                    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                        if n == 0 {
                            return;
                        }
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + length || n == 0 {
                        break (head, buf[end + 4..].to_vec());
                    }
                };
                let key = head
                    .lines()
                    .find_map(|l| l.strip_prefix("x-api-key:"))
                    .map(|v| v.trim().to_string());
                log.lock().unwrap().push(Seen { key, body });
                let event = |kind: &str, data: Value| format!("event: {kind}\ndata: {data}\n\n");
                if matches!(answers, Answers::Held) {
                    HELD.notified().await;
                }
                let (input, output) = match answers {
                    Answers::Whole { input, output } => (input, output),
                    _ => (1000, 200),
                };
                let events = match answers {
                    Answers::Whole { .. } | Answers::Held => [
                        event("message_start", json!({"type": "message_start", "message": {"id": "msg_1", "model": "stand-in", "role": "assistant", "content": [], "usage": {"input_tokens": input}}})),
                        event("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})),
                        event("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "HELLO"}})),
                        event("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
                        event("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": output}})),
                        event("message_stop", json!({"type": "message_stop"})),
                    ]
                    .concat(),
                    Answers::Cut => event("message_start", json!({"type": "message_start", "message": {"usage": {"input_tokens": 50}}})),
                };
                let _ = conn
                    .write_all(
                        format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{events}").as_bytes(),
                    )
                    .await;
            });
        }
    });
    Upstream { url, seen }
}

pub(super) struct Rig {
    pub core: Arc<Core>,
    pub up: Upstream,
    pub profile: String,
    pub model: String,
    pub state: PathBuf,
    pub sealed: PathBuf,
    pub _dir: tempfile::TempDir,
}

/// A core that acts, released, with the provider at the stand-in and a
/// sealed dir, `edit` applied to its config.
pub(super) async fn rig(answers: Answers, edit: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let (state, work, sealed) = (base.join("state"), base.join("work"), base.join("sealed"));
    for d in [&state, &work, &sealed] {
        std::fs::create_dir_all(d).unwrap();
    }
    let up = upstream(answers).await;
    // The provider's address is resolved when the document is read, so it
    // goes in the document.
    let mut doc: toml::Table = Config::EXAMPLE_TOML.parse().unwrap();
    let table = |doc: &mut toml::Table, k: &str| {
        doc.entry(k)
            .or_insert_with(|| toml::Table::new().into())
            .as_table_mut()
            .unwrap()
            .clone()
    };
    let (mut model, mut server) = (table(&mut doc, "model"), table(&mut doc, "server"));
    model.insert("api_base".into(), up.url.clone().into());
    server.insert(
        "state_dir".into(),
        state.to_string_lossy().into_owned().into(),
    );
    doc.insert("model".into(), model.into());
    doc.insert("server".into(), server.into());
    let mut cfg = Config::parse(&doc.to_string()).unwrap().0;
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.self_improve.mode = SelfMode::Act;
    cfg.self_improve.sealed.dir = Some(sealed.to_string_lossy().into_owned());
    edit(&mut cfg);
    cfg.validate().unwrap();
    let profile = cfg.model.live.clone();
    let model = cfg.profile(&profile).unwrap().model.clone();
    let name = cfg.model.api_key_secret.clone();
    let secrets = SecretBoard::new([name.clone()], std::time::Instant::now());
    secrets.publish([(name, Ok(Secret::new(KEY.into())))].into(), "fake");
    let store = crate::store::Store::open(&state.join("store")).unwrap();
    let core = Core::build(Parts {
        secrets,
        ..Parts::for_tests(
            cfg,
            Arc::new(crate::provider::FakeProvider::default()),
            store,
        )
    })
    .unwrap();
    core.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    Rig {
        core,
        up,
        profile,
        model,
        state,
        sealed,
        _dir: dir,
    }
}

/// A Messages request for `model` whose one message is `text`.
pub(super) fn request(model: &str, text: &str) -> Value {
    json!({"model": model, "max_tokens": 1024, "stream": true,
           "messages": [{"role": "user", "content": text}]})
}

/// What a candidate in L1 does: connect, send the request in the proxy's
/// absolute form, read the answer to its end. The status and the body.
pub(super) fn ask(relay: &Arc<Relay>, body: &Value) -> (u16, String) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    let (server, _) = l.accept().unwrap();
    relay.serve_connection(server);
    let body = body.to_string();
    write!(
        c,
        "POST http://x.relay.invalid/v1/messages HTTP/1.1\r\nhost: x.relay.invalid\r\n\
         x-api-key: relay\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    let status = out.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap();
    (
        status,
        out.split_once("\r\n\r\n")
            .map_or("", |(_, b)| b)
            .to_string(),
    )
}

async fn ask_async(relay: &Arc<Relay>, body: Value) -> (u16, String) {
    let relay = relay.clone();
    tokio::task::spawn_blocking(move || ask(&relay, &body))
        .await
        .unwrap()
}

fn new_relay(r: &Rig) -> Arc<Relay> {
    Relay::new(&r.core, &r.profile, "self/tide", "t.relay.invalid").unwrap()
}

fn untouched(r: &Rig) -> bool {
    r.up.seen.lock().unwrap().is_empty()
}

#[tokio::test(flavor = "current_thread")]
async fn it_forwards_the_one_model_with_the_daemons_key_and_meters_the_usage() {
    let r = rig(
        Answers::Whole {
            input: 1000,
            output: 200,
        },
        |_| {},
    )
    .await;
    let relay = new_relay(&r);
    relay.begin(1.0);
    let sent = request(&r.model, "Recite the tide table");
    let (status, body) = ask_async(&relay, sent.clone()).await;
    assert_eq!(status, 200);
    assert!(body.contains("HELLO"), "{body}");
    assert!(!body.contains(KEY), "the key never comes back");
    {
        let seen = r.up.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen[0].key.as_deref(),
            Some(KEY),
            "the daemon's key, not the candidate's"
        );
        assert_eq!(
            seen[0].body,
            sent.to_string().as_bytes(),
            "the body goes on as it came"
        );
    }
    let m = relay.finish();
    let usage = Usage {
        input_tokens: 1000,
        output_tokens: 200,
        ..Usage::default()
    };
    let expected = r.core.catalog.get(&r.model).unwrap().cost_usd(&usage);
    assert!((m.usd - expected).abs() < 1e-9, "{} vs {expected}", m.usd);
    assert_eq!((m.calls, m.refused), (1, false));
    assert_eq!(relay.spent_usd(), m.usd, "the run's meter is the item's");
    let view = r.core.runner.self_budget.view().unwrap();
    assert!(
        (view.day_spent_usd - expected).abs() < 1e-9,
        "settled to the meter, not left at the hold"
    );
    let log = relay.log();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (log[0].usage.input_tokens, log[0].usage.output_tokens),
        (1000, 200)
    );
    assert!(
        !log[0].to_string().contains("tide"),
        "a usage line holds no body"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn it_refuses_every_other_model_and_every_other_path() {
    let r = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |_| {},
    )
    .await;
    let relay = new_relay(&r);
    relay.begin(1.0);
    let (status, body) = ask_async(&relay, request("claude-haiku-5-5", "x")).await;
    assert_eq!(status, 403);
    assert!(
        !body.contains("haiku"),
        "a refusal is a fixed sentence: {body}"
    );
    assert!(untouched(&r), "the provider was never asked");
    let mut get = request(&r.model, "x");
    get["max_tokens"] = Value::Null;
    assert_eq!(ask_async(&relay, get).await.0, 400);
    assert!(untouched(&r));
    assert_eq!(relay.finish().usd, 0.0);
}

#[tokio::test(flavor = "current_thread")]
async fn it_refuses_while_self_improvement_is_off_or_halted() {
    let r = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |_| {},
    )
    .await;
    let relay = new_relay(&r);
    relay.begin(1.0);
    r.core
        .self_halt(
            &SelfHaltParams {
                why: Some("test".into()),
                ..SelfHaltParams::default()
            },
            &Answerer::from("the CLI"),
        )
        .unwrap();
    assert_eq!(ask_async(&relay, request(&r.model, "x")).await.0, 403);
    assert!(untouched(&r), "halted: nothing forwarded");
    assert!(relay.finish().refused, "and the item fails");

    let off = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |c| c.self_improve.mode = SelfMode::Off,
    )
    .await;
    let relay = new_relay(&off);
    relay.begin(1.0);
    assert_eq!(ask_async(&relay, request(&off.model, "x")).await.0, 403);
    assert!(untouched(&off), "off: nothing forwarded");
}

#[tokio::test(flavor = "current_thread")]
async fn it_stops_at_the_items_line_and_at_the_budgets() {
    let r = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |_| {},
    )
    .await;
    let relay = new_relay(&r);
    // A line under the call's worst case.
    relay.begin(0.00001);
    assert_eq!(ask_async(&relay, request(&r.model, "x")).await.0, 403);
    let m = relay.finish();
    assert!(m.refused && m.usd == 0.0 && untouched(&r), "{m:?}");

    // The self budget's branch line.
    let tight = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |c| {
            c.self_improve.budget.branch_usd = 0.0001;
        },
    )
    .await;
    let relay = new_relay(&tight);
    relay.begin(1.0);
    assert_eq!(ask_async(&relay, request(&tight.model, "x")).await.0, 403);
    assert!(relay.finish().refused);
    assert!(untouched(&tight));
    assert_eq!(
        tight.core.runner.self_budget.view().unwrap().day_spent_usd,
        0.0,
        "nothing was held"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_call_whose_answer_never_ends_settles_at_its_worst_case() {
    let r = rig(Answers::Cut, |_| {}).await;
    let relay = new_relay(&r);
    relay.begin(1.0);
    let sent = request(&r.model, "x");
    assert_eq!(ask_async(&relay, sent.clone()).await.0, 200);
    let worst = r
        .core
        .catalog
        .get(&r.model)
        .unwrap()
        .reserve_micros(1024, sent.to_string().len().div_ceil(3) as u64);
    let m = relay.finish();
    assert!(
        (m.usd - worst as f64 / 1e6).abs() < 1e-9,
        "{} vs {worst} micros",
        m.usd
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_call_that_outlives_its_item_is_not_charged_to_the_next() {
    let r = rig(Answers::Held, |_| {}).await;
    let relay = new_relay(&r);
    relay.begin(1.0);
    let late = tokio::task::spawn_blocking({
        let (relay, model) = (relay.clone(), r.model.clone());
        move || ask(&relay, &request(&model, "slow"))
    });
    // The candidate's deadline passes with its call still open.
    while r.up.seen.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    let first = relay.finish();
    assert_eq!((first.usd, first.calls), (0.0, 0));
    relay.begin(1.0);
    HELD.notify_one();
    assert_eq!(late.await.unwrap().0, 200);
    let second = relay.finish();
    assert_eq!(
        (second.usd, second.calls),
        (0.0, 0),
        "the late call is not this item's"
    );
    assert!(relay.spent_usd() > 0.0, "but the run's meter has it");
}

/// The relay sends past the data floor's chokepoint, so a profile of
/// restricted origin gets no relay at all (theseus-82jhp): the example's
/// `glm` (GLM on z.ai, China origin) is refused when the relay is made, and
/// the profile under test, of US origin, still is served.
#[tokio::test(flavor = "current_thread")]
async fn a_profile_of_restricted_origin_gets_no_relay() {
    let r = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |_| {},
    )
    .await;
    let origin = r.core.cfg.profile_origin("glm", &r.core.catalog);
    assert!(origin.restricted(), "{origin:?}");
    let refused = Relay::new(&r.core, "glm", "self/tide", "t.relay.invalid");
    let said = format!("{:#}", refused.err().expect("refused"));
    assert!(said.contains("restricted origin"), "{said}");
    assert!(untouched(&r), "the provider was never asked");
    assert_eq!(new_relay(&r).model(), r.model);
}
