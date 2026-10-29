//! An unaddressed message finds its thread (theseus-rha2), through the core:
//! the router's addressing and pre-router with no Jev; Jev's `thread`
//! question on the very request the inbound batch rides (one request, no
//! call added); every band of the decision; the question and its answer; the
//! timeout that leaves the router's guess; the misroute guard; and `thread.move`
//! with its one note, its one re-submission and the correction the next close
//! message follows.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::thread_route::{RouteHow, ThreadAnswerParams, ThreadMoveParams, ThreadTag};
use theseus_protocol::threads::{ThreadEditParams, ThreadNewParams, WorkingSet};
use theseus_protocol::{
    method, Id, Message, Request, SessionKind, TurnSubmitParams, TurnSubmitResult,
};
use tokio::io::{
    duplex, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

use super::Core;
use crate::approval::{Answerer, Client, Surface};
use crate::ledger::LedgerRow;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::tests_inbound::inbound_only;
use crate::tests_judge::{rig_on, texts, Rig};

mod title;

/// The owner, and a second place of theirs.
const OWNER: u64 = 314_159_265_358_979;
const OWNER2: u64 = 271_828_182_845_904;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn ws(refs: &[&str], repos: &[&str], roots: &[&str]) -> WorkingSet {
    let v = |x: &[&str]| x.iter().map(|s| (*s).to_string()).collect();
    WorkingSet {
        refs: v(refs),
        repos: v(repos),
        roots: v(roots),
        ..Default::default()
    }
}

/// A thread with a working set.
fn thread(c: &Core, slug: &str, title: &str, w: WorkingSet) {
    c.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(title.into()),
            goal: Some(format!("Finish the {title} work")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    c.thread_edit(
        &ThreadEditParams {
            slug: slug.into(),
            working_set: Some(w),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
}

/// The two threads most tests use.
fn two_threads(c: &Core) {
    thread(
        c,
        "pr-round",
        "pr round",
        ws(&["#412"], &["tidewatch"], &[]),
    );
    thread(
        c,
        "security-fix",
        "security fix",
        ws(&["#977"], &["vault"], &[]),
    );
}

/// A place's session: a DM when `place` is given.
fn home(c: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    c.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        c.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

fn session_of(c: &Core, slug: &str) -> String {
    c.threads()
        .get(slug)
        .and_then(|t| t.session_id.clone())
        .unwrap_or_else(|| panic!("the thread {slug} has no session yet"))
}

fn rows(c: &Core, kind: &str) -> Vec<LedgerRow> {
    c.store
        .ledger_tail::<LedgerRow>(5000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

fn texts_of(c: &Core, session: &str) -> Vec<String> {
    c.store
        .session_nodes(session)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            crate::node::Body::UserMessage { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// A connection from the CLI to the core, for requests that overlap.
struct Wire {
    w: WriteHalf<DuplexStream>,
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    next: u64,
    /// Notifications read while waiting for a response.
    heard: Vec<Value>,
}

impl Wire {
    fn open(core: &Arc<Core>) -> Self {
        Self::open_as(core, Surface::Cli)
    }

    fn open_as(core: &Arc<Core>, surface: Surface) -> Self {
        let (client, server) = duplex(1 << 20);
        let (sr, sw) = tokio::io::split(server);
        tokio::spawn(
            core.clone()
                .serve_connection(sr, sw, Client::new("test", surface)),
        );
        let (cr, cw) = tokio::io::split(client);
        Wire {
            w: cw,
            lines: BufReader::new(cr).lines(),
            next: 1,
            heard: Vec::new(),
        }
    }

    async fn send(&mut self, m: &str, params: impl serde::Serialize) -> u64 {
        let id = self.next;
        self.next += 1;
        let req = Request::new(Id::Num(id), m, params);
        let mut line = serde_json::to_string(&req).unwrap();
        line.push('\n');
        self.w.write_all(line.as_bytes()).await.unwrap();
        id
    }

    /// The next message, or none within the bound.
    async fn message(&mut self, within: Duration) -> Option<Message> {
        let l = tokio::time::timeout(within, self.lines.next_line())
            .await
            .ok()?
            .unwrap()?;
        Some(serde_json::from_str(&l).unwrap())
    }

    /// Read until the response to `id`, keeping the notifications.
    async fn response(&mut self, id: u64) -> Value {
        loop {
            match self
                .message(Duration::from_secs(60))
                .await
                .expect("a response")
            {
                Message::Response(r) if r.id == Id::Num(id) => {
                    return serde_json::to_value(r).unwrap()
                }
                Message::Notification(n) => self
                    .heard
                    .push(json!({"method": n.method, "params": n.params})),
                _ => {}
            }
        }
    }

    /// Read until a notification of `method`.
    async fn notice(&mut self, method: &str) -> Value {
        if let Some(i) = self.heard.iter().position(|n| n["method"] == method) {
            return self.heard.remove(i)["params"].clone();
        }
        loop {
            match self
                .message(Duration::from_secs(30))
                .await
                .expect("a notification")
            {
                Message::Notification(n) if n.method == method => return n.params,
                Message::Notification(n) => self
                    .heard
                    .push(json!({"method": n.method, "params": n.params})),
                _ => {}
            }
        }
    }
}

fn params(session: &str, input: &str, reply_to_turn: Option<&str>) -> TurnSubmitParams {
    TurnSubmitParams {
        dir: None,
        carried: false,
        prompt: None,
        session_id: Some(session.into()),
        input: input.into(),
        profile: None,
        provider: None,
        model: None,
        author: None,
        attachments: vec![],
        reply_to: None,
        opened_from: None,
        thread: None,
        speaking: None,
        in_reply_to: reply_to_turn.map(str::to_string),
        route_at_ms: None,
        deadline_secs: None,
        via: None,
        surface: None,
    }
}

/// One `turn.submit`, its result.
async fn submit(c: &Arc<Core>, session: &str, input: &str) -> TurnSubmitResult {
    submit_replying(c, session, input, None).await
}

async fn submit_replying(
    c: &Arc<Core>,
    session: &str,
    input: &str,
    to_turn: Option<&str>,
) -> TurnSubmitResult {
    let mut w = Wire::open(c);
    let id = w
        .send(method::TURN_SUBMIT, params(session, input, to_turn))
        .await;
    let r = w.response(id).await;
    serde_json::from_value(r["result"].clone()).unwrap_or_else(|_| panic!("a turn result in {r}"))
}

fn tag(r: &TurnSubmitResult) -> ThreadTag {
    r.thread
        .clone()
        .unwrap_or_else(|| panic!("no thread tag in {r:?}"))
}

/// A rig of the inbound batch alone (classify.v4 acting, role.v1), against a
/// fake Jev, with `n` model replies.
fn rig(jev: &FakeJev, n: usize) -> Rig {
    rig_on(texts(n), Some(jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        // The DM of user 42 is the owner's: a private place.
        c.places.owner = Some(vec![
            format!("discord:{OWNER}"),
            format!("discord:{OWNER2}"),
        ]);
    })
}

// ---------------------------------------------------------------- addressing

/// A message that names a thread's PR runs in that thread without waiting for
/// Jev, which is not asked about threads at all; its reply goes back to the
/// DM it came from; and the place's own session took no turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_that_names_a_threads_pr_runs_there_and_replies_in_the_dm() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let t0 = Instant::now();
    let res = submit(c, &dm, "what is failing in PR 412?").await;
    let ran = session_of(c, "pr-round");
    assert_eq!(res.session_id, ran);
    let t = tag(&res);
    assert_eq!(
        (t.how, t.slug.as_deref(), t.movable),
        (RouteHow::Quiet, Some("pr-round"), false)
    );
    assert_eq!(t.words().as_deref(), Some("\u{2192} pr round"));
    assert_eq!(
        c.outbox.target(&ran).as_deref(),
        Some(format!("discord:dm:{OWNER}").as_str()),
        "the thread's reply goes where the message came from"
    );
    assert_eq!(
        c.store
            .get_session::<SessionRecord>(&dm)
            .unwrap()
            .unwrap()
            .turns,
        0,
        "the place's own conversation took no turn"
    );
    let routed = rows(c, "thread.routed");
    assert_eq!(routed.len(), 1);
    assert_eq!(routed[0].data["to"], "pr-round");
    assert_eq!(routed[0].data["early"], false);
    assert_eq!(
        routed[0].turn_id.as_deref(),
        Some(res.turn_id.as_str()),
        "the row rides the turn's own frame"
    );
    let pre = routed[0].data["pre"].as_array().unwrap();
    let hits = pre.iter().find(|p| p["thread"] == "pr-round").unwrap();
    assert_eq!(hits["hits"], json!(["#412"]));
    // Quick: nothing waited on Jev (a decisive router is free).
    assert!(t0.elapsed() < Duration::from_secs(5));
    // The next message, right after the reply, with no entity: the thread is
    // the likelier, and Jev tells a fragment from a switch.
    thread_answer(&jev, "pr-round", 0.9);
    let res2 = submit(c, &dm, "and the tests too").await;
    assert_eq!(res2.session_id, ran);
    assert_eq!(tag(&res2).how, RouteHow::Quiet);
    assert_eq!(
        texts_of(c, &ran),
        ["what is failing in PR 412?", "and the tests too"]
    );
}

/// Addressing is first, in order: `#slug` (taken off the message), `/t slug`,
/// "in the <title> thread:", a place a thread alone lists, and a reply to a
/// thread's reply. Each is certain and asks Jev nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn addressing_comes_first_and_is_certain() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 8);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let a = submit(c, &dm, "#security-fix rotate the key").await;
    assert_eq!(a.session_id, session_of(c, "security-fix"));
    assert_eq!(
        (tag(&a).how, tag(&a).confidence),
        (RouteHow::Addressed, 1.0)
    );
    assert_eq!(
        texts_of(c, &a.session_id),
        ["rotate the key"],
        "the address comes off the message"
    );
    let b = submit(c, &dm, "/t pr-round check the lint run").await;
    assert_eq!(b.session_id, session_of(c, "pr-round"));
    let d = submit(c, &dm, "In the security fix thread: and the vault?").await;
    assert_eq!(d.session_id, session_of(c, "security-fix"));
    assert_eq!(
        texts_of(c, &d.session_id),
        ["rotate the key", "and the vault?"]
    );
    // A reply to a thread's reply goes to its thread, whatever it says.
    let e = submit_replying(c, &dm, "why is the build red", Some(&b.turn_id)).await;
    assert_eq!(e.session_id, session_of(c, "pr-round"));
    assert_eq!(tag(&e).how, RouteHow::Reply);
    // A thread that alone lists a place takes its messages.
    // T6 binds a place to a thread; until then the record's `surfaces` is set
    // through the board, as its writer does.
    c.runner
        .threads
        .write(
            &c.store,
            |board| {
                let mut t = board.get("pr-round").unwrap().clone();
                t.surfaces = vec![format!("discord:dm:{OWNER2}")];
                Ok(vec![t])
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
    let other = home(c, Some(&format!("dm:{OWNER2}")));
    let f = submit(c, &other, "hello there").await;
    assert_eq!(f.session_id, session_of(c, "pr-round"));
    assert_eq!(tag(&f).how, RouteHow::Addressed);
    // The turns' own inbound requests ask no thread question: nothing was
    // left to ask once the message named its thread.
    assert!(
        jev.seen().iter().all(|s| !s.body["questions"]
            .as_object()
            .unwrap()
            .contains_key("classify.v4/thread")),
        "addressing asks Jev no thread question"
    );
}

// ------------------------------------------------------------- Jev's choice

fn thread_answer(jev: &FakeJev, slug: &str, p: f64) {
    jev.script(
        "thread",
        Jev::Choice {
            option: slug.into(),
            confidence: p,
        },
    );
}

/// With the router unsure, Jev's `thread` question rides the one inbound
/// request (role and the rest ride it too), the turn takes the point over, and
/// the ledger shows one call for the message: the same as with no threads.
/// Every band: sure routes quietly, likely with a tag and a move handle.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jevs_thread_answer_rides_the_one_inbound_request_and_the_bands_route() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 6);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    thread_answer(&jev, "security-fix", 0.95);
    let a = submit(c, &dm, "why is the build red").await;
    assert_eq!(a.session_id, session_of(c, "security-fix"));
    let t = tag(&a);
    assert_eq!((t.how, t.movable), (RouteHow::Quiet, false));
    // One request, carrying role's question and the thread question.
    wait_for(|| jev.seen().len() == 1 && !rows(c, "judge.call").is_empty()).await;
    assert_eq!(jev.seen().len(), 1, "no call was added");
    let q = jev.seen()[0].body["questions"].clone();
    let ids: Vec<String> = q.as_object().unwrap().keys().cloned().collect();
    assert!(ids.contains(&"classify.v4/thread".to_string()), "{ids:?}");
    assert!(ids.contains(&"role.v1/role".to_string()), "{ids:?}");
    let mut opts: Vec<String> = q["classify.v4/thread"]["criteria"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    opts.sort();
    assert_eq!(
        opts,
        ["home", "new_thread", "pr-round", "security-fix"],
        "{opts:?}"
    );
    let routed = &rows(c, "thread.routed")[0];
    assert_eq!(routed.data["early"], true);
    assert_eq!(routed.data["jev"]["pick"], "security-fix");
    assert_eq!(routed.data["timed_out"], false);
    // The judgment rows belong to the turn that ran: the point was taken over.
    let calls = rows(c, "judge.call");
    assert!(
        calls
            .iter()
            .all(|j| j.turn_id.as_deref() == Some(a.turn_id.as_str())),
        "{calls:?}"
    );
    // Likely: a visible tag and a move handle.
    // (A place of its own for each band: the last thread is a prior.)
    thread_answer(&jev, "pr-round", 0.75);
    let dm = home(c, None);
    let b = submit(c, &dm, "does the migration need a flag").await;
    assert_eq!(b.session_id, session_of(c, "pr-round"));
    let t = tag(&b);
    assert_eq!((t.how, t.movable), (RouteHow::Visible, true));
    assert_eq!(
        t.words().as_deref(),
        Some("\u{2192} pr round \u{b7} \u{21aa} move")
    );
    // Home, sure: no thread, the place's own conversation, no tag.
    thread_answer(&jev, "home", 0.96);
    let dm = home(c, None);
    let h = submit(c, &dm, "what is the weather like").await;
    assert_eq!(h.session_id, dm);
    assert_eq!(tag(&h).words(), None);
}

/// The owner's words in a message the router places (theseus-slvl.5 on thread-route, the owner's 2026-10-15
/// "I think that sounds right"): `directive.v1` rides the router's early call, the one request the message makes,
/// and the record names the message the turn wrote, in the thread's session it ran in.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_message_is_captured_in_the_routing_call_and_names_its_message() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    thread_answer(&jev, "security-fix", 0.95);
    jev.script(
        "directive.v1/kind",
        Jev::Choice {
            option: "preference".into(),
            confidence: 0.93,
        },
    );
    jev.script(
        "directive.v1/scope",
        Jev::Choice {
            option: "everywhere".into(),
            confidence: 0.9,
        },
    );
    let a = submit(c, &dm, "no tables with me when we talk about the build").await;
    assert_eq!(a.session_id, session_of(c, "security-fix"));
    wait_for(|| !rows(c, "standing.captured").is_empty()).await;
    assert_eq!(jev.seen().len(), 1, "no call was added");
    let q = jev.seen()[0].body["questions"].clone();
    let ids: Vec<String> = q.as_object().unwrap().keys().cloned().collect();
    assert!(ids.contains(&"classify.v4/thread".to_string()), "{ids:?}");
    assert!(ids.contains(&"directive.v1/kind".to_string()), "{ids:?}");
    assert_eq!(rows(c, "thread.routed")[0].data["early"], true);
    let cap = &rows(c, "standing.captured")[0];
    let node = cap.data["node"].as_str().unwrap_or_default().to_string();
    assert!(node.starts_with("msg_"), "{:?}", cap.data);
    assert!(
        c.store
            .session_nodes(&a.session_id)
            .unwrap()
            .iter()
            .any(|(_, n)| n.id == node),
        "the record names the message the routed turn wrote"
    );
}

/// Jev not answering in `[threads] route_wait_ms` leaves the router's pick,
/// tagged a guess, and the ledger counts the timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_jev_leaves_the_routers_guess_and_the_timeout_is_counted() {
    let jev = FakeJev::start().unwrap();
    jev.set_latency(Duration::from_millis(1500), Duration::from_millis(1500));
    thread_answer(&jev, "security-fix", 0.95);
    let r = rig_on(texts(3), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 60;
    });
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    let t0 = Instant::now();
    let a = submit(c, &dm, "tidewatch builds slowly").await;
    let t = tag(&a);
    assert_eq!(
        a.session_id,
        session_of(c, "pr-round"),
        "the router's pick: the repo it names"
    );
    assert_eq!((t.how, t.movable), (RouteHow::Guess, true));
    assert!(t.words().unwrap().contains("(a guess)"), "{:?}", t.words());
    let row = &rows(c, "thread.routed")[0];
    assert_eq!(row.data["timed_out"], true);
    assert!(
        t0.elapsed() < Duration::from_millis(1400),
        "the message did not wait for Jev"
    );
}

// ------------------------------------------------------------------ the ask

/// Two threads named, each with evidence and close, the router asks in one
/// line and the turn waits (theseus-e31an: a lone weak lean is no contest); the
/// answer routes it; a question nobody answers runs the message at home.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn when_it_cannot_tell_it_asks_in_one_line_and_the_answer_routes_the_message() {
    let jev = FakeJev::start().unwrap();
    // Two contend: the message names the security fix's repo, and Jev
    // leans to the pr round (theseus-e31an: a lone lean is no contest).
    thread_answer(&jev, "pr-round", 0.55);
    let r = rig_on(texts(4), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 3000;
        c.threads.ask_wait_secs = 2;
    });
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::TURN_SUBMIT,
            params(&dm, "why is the build red in vault", None),
        )
        .await;
    let asked = w.notice("thread.asked").await;
    assert_eq!(asked["session_id"], dm.as_str());
    assert_eq!(
        asked["question"],
        "Is this about the security fix or the pr round?"
    );
    let keys: Vec<&str> = asked["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["1", "2", "home"]);
    assert_eq!(asked["message"], "why is the build red in vault");
    // The turn waits: no thread has a session yet.
    assert!(c.threads().get("pr-round").unwrap().session_id.is_none());
    // The owner answers "2" over a connection of their own.
    let mut a = Wire::open(c);
    let aid = a
        .send(
            method::THREAD_ANSWER,
            ThreadAnswerParams {
                ask_id: asked["ask_id"].as_str().unwrap().into(),
                choice: "2".into(),
                author: None,
                discord: None,
            },
        )
        .await;
    let ans = a.response(aid).await;
    assert_eq!(ans["result"]["routed"], "pr-round", "{ans}");
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    assert_eq!(res.session_id, session_of(c, "pr-round"));
    assert_eq!(tag(&res).how, RouteHow::Asked);
    let answered = rows(c, "thread.answered");
    assert_eq!(
        (answered.len(), answered[0].data["choice"].as_str()),
        (1, Some("2"))
    );
    assert_eq!(rows(c, "thread.asked").len(), 1);
    // A second answer finds no question waiting.
    let aid = a
        .send(
            method::THREAD_ANSWER,
            ThreadAnswerParams {
                ask_id: asked["ask_id"].as_str().unwrap().into(),
                choice: "1".into(),
                author: None,
                discord: None,
            },
        )
        .await;
    assert_eq!(
        a.response(aid).await["error"]["code"],
        theseus_protocol::error_code::NOT_FOUND
    );
    // Nobody answers: the message runs at home when the wait ends. (A place
    // of its own: the first one's last thread would be the follow-up.)
    let dm = home(c, None);
    let mut w2 = Wire::open(c);
    let id2 = w2
        .send(
            method::TURN_SUBMIT,
            params(&dm, "why is the build red again in vault", None),
        )
        .await;
    let _ = w2.notice("thread.asked").await;
    let res2: TurnSubmitResult =
        serde_json::from_value(w2.response(id2).await["result"].clone()).unwrap();
    assert_eq!(res2.session_id, dm);
    assert_eq!(tag(&res2).how, RouteHow::Home);
    assert_eq!(
        rows(c, "thread.answered").last().unwrap().data["choice"],
        "expired"
    );
}

/// Stickiness alone cannot overrule Jev (theseus-eqp1v; the threadbench's
/// msg-5 shape): right after a thread's reply, a new stream's first message
/// scores that thread 0.88 from stickiness and the chain alone. Jev dissents,
/// so the message is asked about (offering a new thread: it names a branch
/// and a ticket no thread holds), never routed `visible`. With
/// `sticky_alone_asks` off, the sticky thread takes it as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dissenting_jev_and_a_sticky_place_ask_instead_of_routing() {
    let fresh = "homesvc maintenance: branch chore/bump-1-8, ticket OPS-4214, bump the base image";
    for on in [true, false] {
        let jev = FakeJev::start().unwrap();
        jev.script_when(
            fresh,
            None,
            "thread",
            Jev::Choice {
                option: "security-fix".into(),
                confidence: 0.3,
            },
        );
        let r = rig_on(texts(4), Some(&jev), |c| {
            inbound_only(c);
            c.judge.total_secs = 5;
            c.threads.route_wait_ms = 3000;
            c.threads.ask_wait_secs = 2;
            c.threads.sticky_alone_asks = on;
        });
        let c = &r.core;
        two_threads(c);
        let dm = home(c, None);
        let first = submit(c, &dm, "what is failing in PR 412").await;
        assert_eq!(first.session_id, session_of(c, "pr-round"));
        let mut w = Wire::open(c);
        let id = w.send(method::TURN_SUBMIT, params(&dm, fresh, None)).await;
        let res: TurnSubmitResult =
            serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
        let routed = rows(c, "thread.routed");
        let last = &routed.last().unwrap().data;
        let pre = last["pre"].as_array().unwrap();
        assert!(
            pre.iter()
                .any(|x| (x["score"].as_f64().unwrap() - 0.88).abs() < 0.01),
            "{last}"
        );
        if on {
            let asked = rows(c, "thread.asked");
            assert_eq!(asked.len(), 1, "{routed:?}");
            let keys: Vec<&str> = w
                .heard
                .iter()
                .find(|n| n["method"] == "thread.asked")
                .map(|n| n["params"]["options"].as_array().unwrap().iter())
                .into_iter()
                .flatten()
                .map(|o| o["key"].as_str().unwrap())
                .collect();
            assert!(keys.contains(&"new"), "{keys:?}");
            assert_ne!(last["how"], "visible", "{last}");
            assert_eq!(res.session_id, dm, "nobody answered: it ran at home");
        } else {
            assert!(rows(c, "thread.asked").is_empty());
            assert_eq!(last["how"], "visible", "{last}");
            assert_eq!(res.session_id, session_of(c, "pr-round"));
        }
    }
}

/// A spoken turn the router cannot place is never held for the typed
/// question (theseus-sro04: the v1 voice check's call waited 215 s on one,
/// and every word said after it queued behind it): it runs at home at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spoken_turn_it_cannot_place_runs_at_home_without_asking() {
    let jev = FakeJev::start().unwrap();
    thread_answer(&jev, "security-fix", 0.45);
    let r = rig_on(texts(2), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 3000;
        c.threads.ask_wait_secs = 30;
    });
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    let mut w = Wire::open_as(c, Surface::Discord);
    let t0 = Instant::now();
    let mut spoken = params(&dm, "why is the build red", None);
    spoken.surface = Some("voice".into());
    let id = w.send(method::TURN_SUBMIT, spoken).await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    assert!(
        t0.elapsed() < Duration::from_secs(20),
        "the spoken turn waited for a question"
    );
    assert_eq!(res.session_id, dm);
    assert_eq!(tag(&res).how, RouteHow::Home);
    assert!(rows(c, "thread.asked").is_empty(), "nothing was asked");
    assert!(!w.heard.iter().any(|n| n["method"] == "thread.asked"));
}

/// A `new_thread` pick at the confirm bar offers a new thread; one tap makes
/// it (named from the message) and the message runs in it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_thread_is_offered_and_one_tap_makes_it() {
    let jev = FakeJev::start().unwrap();
    thread_answer(&jev, "new_thread", 0.8);
    let r = rig_on(texts(3), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 3000;
    });
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::TURN_SUBMIT,
            params(&dm, "calibrate the harbor gauge sensors", None),
        )
        .await;
    let asked = w.notice("thread.asked").await;
    assert!(
        asked["question"]
            .as_str()
            .unwrap()
            .starts_with("Start a new thread \u{ab}calibrate harbor gauge\u{bb}"),
        "{asked}"
    );
    let keys: Vec<&str> = asked["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["new", "home"]);
    let mut a = Wire::open(c);
    let aid = a
        .send(
            method::THREAD_ANSWER,
            ThreadAnswerParams {
                ask_id: asked["ask_id"].as_str().unwrap().into(),
                choice: "n".into(),
                author: None,
                discord: None,
            },
        )
        .await;
    a.response(aid).await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    let t = tag(&res);
    assert_eq!(
        (t.how, t.slug.as_deref()),
        (RouteHow::NewThread, Some("calibrate-harbor-gauge"))
    );
    assert_eq!(
        c.threads().get("calibrate-harbor-gauge").unwrap().title,
        "calibrate harbor gauge"
    );
    assert_eq!(res.session_id, session_of(c, "calibrate-harbor-gauge"));
}

// ------------------------------------------------------------------- the guard

fn write_to(path: &str) -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "",
            &[("w1", "fs_write", json!({"path": path, "content": "x\n"}))],
        ),
        Scripted::text("Written."),
        Scripted::text("ok"),
    ]
}

/// A turn placed below the act bar, whose first write names a path outside the
/// thread's roots, is held for one question; a turn placed at the bar writes
/// as it would have.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_outside_the_threads_roots_is_held_only_below_the_act_bar() {
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mine, theirs) = (dir.path().join("tidewatch"), dir.path().join("vault"));
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::create_dir_all(&theirs).unwrap();
    let outside = theirs.join("notes.txt").display().to_string();
    let root = dir.path().to_path_buf();
    let r = rig_on(write_to(&outside), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.tools.roots = vec![root.display().to_string()];
    });
    let c = &r.core;
    thread(
        c,
        "pr-round",
        "pr round",
        ws(&["#412"], &["tidewatch"], &[&mine.display().to_string()]),
    );
    thread(
        c,
        "security-fix",
        "security fix",
        ws(&["#977"], &["vault"], &[&theirs.display().to_string()]),
    );
    let dm = home(c, None);
    // Likely (0.75): visible tag, so the guard is armed.
    thread_answer(&jev, "pr-round", 0.75);
    let held = submit(c, &dm, "does the migration need a flag").await;
    assert!(held.awaiting_confirm.is_none(), "the write runs: {held:?}");
    let told = rows(c, "tool.notified");
    assert_eq!(told.len(), 1);
    let why = told[0].data["rule"].as_str().unwrap().to_string();
    assert!(
        why.contains("isn't in the pr round thread")
            && why.contains("the security fix")
            && why.contains("Undo: files.restore"),
        "{why}"
    );
    assert!(theirs.join("notes.txt").exists(), "the write went on");
    assert_eq!(rows(c, "thread.misroute_held").len(), 1);
    assert_eq!(
        rows(c, "thread.misroute_held")[0].data["thread"],
        "pr round"
    );
}

/// A thread's working set grows relative roots (`tidewatch`), read under the
/// directory its tools run in: a guess-placed write inside one is not held,
/// and one in another thread's directory still is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guess_placed_turn_inside_its_threads_relative_root_writes_without_a_hold() {
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mine, theirs) = (dir.path().join("tidewatch"), dir.path().join("vault"));
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::create_dir_all(&theirs).unwrap();
    let inside = mine.join("notes.txt").display().to_string();
    let root = dir.path().to_path_buf();
    let r = rig_on(write_to(&inside), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.tools.roots = vec![root.display().to_string()];
        c.tools.cwd = Some(root.display().to_string());
    });
    let c = &r.core;
    thread(
        c,
        "pr-round",
        "pr round",
        ws(&["#412"], &["tidewatch"], &["tidewatch"]),
    );
    thread(
        c,
        "security-fix",
        "security fix",
        ws(&["#977"], &["vault"], &["vault"]),
    );
    let dm = home(c, None);
    thread_answer(&jev, "pr-round", 0.75);
    let done = submit(c, &dm, "does the migration need a flag").await;
    assert!(done.awaiting_confirm.is_none(), "{done:?}");
    assert!(mine.join("notes.txt").exists());
    assert!(rows(c, "thread.misroute_held").is_empty());
}

/// The same relative roots still hold a write into the other thread's tree.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guess_placed_write_outside_its_relative_roots_is_still_held() {
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mine, theirs) = (dir.path().join("tidewatch"), dir.path().join("vault"));
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::create_dir_all(&theirs).unwrap();
    let outside = theirs.join("notes.txt").display().to_string();
    let root = dir.path().to_path_buf();
    let r = rig_on(write_to(&outside), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.tools.roots = vec![root.display().to_string()];
        c.tools.cwd = Some(root.display().to_string());
    });
    let c = &r.core;
    thread(
        c,
        "pr-round",
        "pr round",
        ws(&["#412"], &["tidewatch"], &["tidewatch"]),
    );
    thread(
        c,
        "security-fix",
        "security fix",
        ws(&["#977"], &["vault"], &["vault"]),
    );
    let dm = home(c, None);
    thread_answer(&jev, "pr-round", 0.75);
    let held = submit(c, &dm, "does the migration need a flag").await;
    assert!(held.awaiting_confirm.is_none(), "{held:?}");
    assert!(theirs.join("notes.txt").exists());
    assert_eq!(rows(c, "thread.misroute_held").len(), 1);
}

/// A program run from the session's directory (above every relative root)
/// names no tree: the guard stays armed, and the turn's next write decides.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guess_placed_program_from_the_session_directory_is_not_held() {
    for (target, held_rows) in [("tidewatch", 0), ("vault", 1)] {
        let jev = FakeJev::start().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (mine, theirs) = (dir.path().join("tidewatch"), dir.path().join("vault"));
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&theirs).unwrap();
        let file = dir.path().join(target).join("notes.txt");
        let mut script = vec![Scripted::tools(
            "",
            &[("p1", "proc_run", json!({"argv": ["true"]}))],
        )];
        script.extend(write_to(&file.display().to_string()));
        let root = dir.path().to_path_buf();
        let r = rig_on(script, Some(&jev), |c| {
            inbound_only(c);
            c.judge.total_secs = 5;
            c.tools.roots = vec![root.display().to_string()];
            c.tools.cwd = Some(root.display().to_string());
        });
        let c = &r.core;
        thread(
            c,
            "pr-round",
            "pr round",
            ws(&["#412"], &["tidewatch"], &["tidewatch"]),
        );
        thread(
            c,
            "security-fix",
            "security fix",
            ws(&["#977"], &["vault"], &["vault"]),
        );
        let dm = home(c, None);
        thread_answer(&jev, "pr-round", 0.75);
        let res = submit(c, &dm, "does the migration need a flag").await;
        let held = rows(c, "thread.misroute_held");
        assert_eq!(held.len(), held_rows, "{target}: {res:?}");
        // Below the act bar the guard notifies and the write goes on
        // (theseus-g85jy), so both rows write; what the table tells apart is
        // whether the guard spoke: a held fact and its notice line for the
        // vault, neither for tidewatch.
        assert!(res.awaiting_confirm.is_none(), "{target}: the write runs");
        assert!(file.exists(), "{target}: the write went on");
        let told = rows(c, "tool.notified")
            .iter()
            .filter(|r| {
                r.data["rule"]
                    .as_str()
                    .is_some_and(|s| s.contains("Undo: files.restore"))
            })
            .count();
        assert_eq!(told, held_rows, "{target}: the guard's notice");
        if let Some(h) = held.first() {
            assert!(h.data["path"]
                .as_str()
                .unwrap()
                .ends_with("vault/notes.txt"));
        }
    }
}

/// At the act bar the same write is not held.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_placed_at_the_act_bar_writes_without_the_guard() {
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mine, theirs) = (dir.path().join("tidewatch"), dir.path().join("vault"));
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::create_dir_all(&theirs).unwrap();
    let outside = theirs.join("notes.txt").display().to_string();
    let root = dir.path().to_path_buf();
    let r = rig_on(write_to(&outside), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.tools.roots = vec![root.display().to_string()];
    });
    let c = &r.core;
    thread(
        c,
        "pr-round",
        "pr round",
        ws(&["#412"], &["tidewatch"], &[&mine.display().to_string()]),
    );
    thread(
        c,
        "security-fix",
        "security fix",
        ws(&["#977"], &["vault"], &[&theirs.display().to_string()]),
    );
    let dm = home(c, None);
    thread_answer(&jev, "pr-round", 0.97);
    let done = submit(c, &dm, "does the migration need a flag").await;
    assert!(done.awaiting_confirm.is_none(), "{done:?}");
    assert!(theirs.join("notes.txt").exists());
    assert!(rows(c, "thread.misroute_held").is_empty());
}

// ------------------------------------------------------------------- the move

/// "That was for the security fix": the wrong thread gets one note, the
/// message runs once more in the right one, the correction is recorded, and
/// the next close message goes where the owner said.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_writes_one_note_resubmits_once_and_the_next_close_message_follows() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 8);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    thread_answer(&jev, "pr-round", 0.75);
    let wrong = submit(c, &dm, "rotate the signing keys on the gateway").await;
    assert_eq!(wrong.session_id, session_of(c, "pr-round"));
    let before = rows(c, "judge.label").len();
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::THREAD_MOVE,
            ThreadMoveParams {
                session_id: dm.clone(),
                to: "security-fix".into(),
                turn_id: None,
                author: None,
                discord: None,
            },
        )
        .await;
    let moved = w.response(id).await;
    assert_eq!(moved["result"]["from"], "pr-round", "{moved}");
    assert_eq!(moved["result"]["redirected"], true);
    let right = session_of(c, "security-fix");
    // One note in the wrong thread; the message once in the right one.
    let notes = texts_of(c, &wrong.session_id)
        .into_iter()
        .filter(|t| t.contains("disregard it here"))
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("\u{ab}security fix\u{bb}"), "{notes:?}");
    assert_eq!(
        texts_of(c, &right),
        ["rotate the signing keys on the gateway"]
    );
    assert_eq!(rows(c, "thread.moved").len(), 1);
    assert_eq!(
        rows(c, "thread.moved")[0].data["resubmitted"],
        moved["result"]["turn_id"]
    );
    // The owner's label on the thread question: the right class.
    let labels = rows(c, "judge.label");
    assert_eq!(labels.len(), before + 1);
    assert_eq!(labels.last().unwrap().data["question"], "thread");
    assert_eq!(labels.last().unwrap().data["label"], "security-fix");
    // A close next message goes where the owner said, with no Jev at all.
    // (Jev, asked, would say pr-round: the owner's word decided it.)
    thread_answer(&jev, "pr-round", 0.95);
    let next = submit(c, &dm, "rotate the signing keys on the gateway again").await;
    assert_eq!(next.session_id, right);
    assert_eq!(tag(&next).how, RouteHow::Quiet);
    assert!(
        !rows(c, "thread.routed").last().unwrap().data["early"]
            .as_bool()
            .unwrap(),
        "no wait for Jev"
    );
    // Moving to where it already is, and a second move of nothing, say so.
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::THREAD_MOVE,
            ThreadMoveParams {
                session_id: dm.clone(),
                to: "security-fix".into(),
                turn_id: None,
                author: None,
                discord: None,
            },
        )
        .await;
    assert!(w.response(id).await["error"]["message"]
        .as_str()
        .unwrap()
        .contains("already ran in security-fix"));
}

/// The owner's words "that was for the security fix" are a move, whose answer
/// is the turn that ran the message again; before any message was placed they
/// are a message like another.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn words_that_a_message_was_for_another_thread_move_it() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 8);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    // Nothing placed yet: the words are a message, routed like one.
    thread_answer(&jev, "home", 0.96);
    let said = submit(c, &dm, "that was for the security fix").await;
    assert_eq!(said.session_id, dm);
    assert!(rows(c, "thread.moved").is_empty());
    let dm = home(c, None);
    thread_answer(&jev, "pr-round", 0.75);
    let wrong = submit(c, &dm, "rotate the signing keys on the gateway").await;
    assert_eq!(wrong.session_id, session_of(c, "pr-round"));
    let moved = submit(c, &dm, "No, that was for the security fix.").await;
    assert_eq!(moved.session_id, session_of(c, "security-fix"));
    let t = tag(&moved);
    assert_eq!(
        (t.how, t.slug.as_deref()),
        (RouteHow::Moved, Some("security-fix"))
    );
    assert_eq!(
        texts_of(c, &moved.session_id),
        ["rotate the signing keys on the gateway"]
    );
    assert_eq!(rows(c, "thread.moved").len(), 1);
}

// ---------------------------------------------------- 40 messages, four threads

/// The fixture's threads, each with its working set.
fn make_fixture_threads(c: &Core, fx: &Value) {
    for t in fx["threads"].as_array().unwrap() {
        let list = |k: &str| -> Vec<&str> {
            t[k].as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect()
        };
        let mut w = ws(&list("refs"), &list("repos"), &[]);
        w.branches = list("branches").iter().map(|s| s.to_string()).collect();
        w.files = list("files").iter().map(|s| s.to_string()).collect();
        w.hosts = list("hosts").iter().map(|s| s.to_string()).collect();
        thread(
            c,
            t["slug"].as_str().unwrap(),
            t["title"].as_str().unwrap(),
            w,
        );
    }
}

/// Jev's scripted answer for each message that has one.
fn script_fixture_jev(jev: &FakeJev, msgs: &[Value]) {
    for m in msgs {
        if let Some(j) = m["jev"].as_object() {
            jev.script_when(
                m["text"].as_str().unwrap(),
                None,
                "thread",
                Jev::Choice {
                    option: j["pick"].as_str().unwrap().into(),
                    confidence: j["p"].as_f64().unwrap(),
                },
            );
        }
    }
}

/// The option that answers an ask for a message of `truth`: its thread's,
/// and `new` for a new stream's message (theseus-eqp1v); else home.
fn fixture_key(c: &Core, truth: &str, options: &Value) -> String {
    let want = match truth {
        "new" => None,
        t => Some(c.threads().get(t).unwrap().title.clone()),
    };
    options
        .as_array()
        .unwrap()
        .iter()
        .find(|o| match &want {
            Some(title) => o["label"] == title.as_str(),
            None => o["key"] == "new",
        })
        .map(|o| o["key"].as_str().unwrap().to_string())
        .unwrap_or_else(|| "home".into())
}

/// The scripted check (theseus-rha2): forty unaddressed messages across four
/// threads in one place (and since theseus-eqp1v three more: a probe, then a
/// switch and a new stream's first message that stickiness alone must not
/// keep), on the stand-in model and a scripted Jev (sure, or
/// unsure, or wrong, as the fixture says), every one through `turn.submit`.
/// The same messages are `scripts/thread-route-check.py`'s, which the owner
/// runs on a real Jev. Top-1 is the thread the message ran in against the one
/// it was for; an ask is answered with the right choice; the ask rate on the
/// clear messages (all but the ones scripted unclear) is the router's own.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forty_unaddressed_messages_find_their_thread_and_ask_rarely() {
    let fx: Value =
        serde_json::from_str(include_str!("../../fixtures/thread_route_40.json")).unwrap();
    let jev = FakeJev::start().unwrap();
    let msgs = fx["messages"].as_array().unwrap();
    script_fixture_jev(&jev, msgs);
    let r = rig_on(texts(48), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 3000;
        c.threads.ask_wait_secs = 30;
    });
    let c = &r.core;
    make_fixture_threads(c, &fx);
    let main = home(c, None);
    let (mut right, mut asks, mut clear, mut clear_asks, mut waited) = (0, 0, 0, 0, 0);
    let mut wrong = Vec::new();
    for m in msgs {
        let (text, truth) = (m["text"].as_str().unwrap(), m["thread"].as_str().unwrap());
        let unclear = m["kind"] == "unclear";
        let place = if m["cold"] == true {
            home(c, None)
        } else {
            main.clone()
        };
        let mut w = Wire::open(c);
        let id = w
            .send(method::TURN_SUBMIT, params(&place, text, None))
            .await;
        let res: TurnSubmitResult = loop {
            match w.message(Duration::from_secs(60)).await.expect("a message") {
                Message::Notification(n) if n.method == "thread.asked" => {
                    asks += 1;
                    if !unclear {
                        clear_asks += 1;
                    }
                    let key = fixture_key(c, truth, &n.params["options"]);
                    let aid = w
                        .send(
                            method::THREAD_ANSWER,
                            ThreadAnswerParams {
                                ask_id: n.params["ask_id"].as_str().unwrap().into(),
                                choice: key,
                                author: None,
                                discord: None,
                            },
                        )
                        .await;
                    let _ = aid;
                }
                Message::Response(r) if r.id == Id::Num(id) => {
                    break serde_json::from_value(
                        serde_json::to_value(r).unwrap()["result"].clone(),
                    )
                    .unwrap()
                }
                _ => {}
            }
        };
        if !unclear {
            clear += 1;
        }
        let landed = c
            .threads()
            .of_session(&res.session_id)
            .map_or("home".to_string(), |t| t.slug.clone());
        let fixture = |s: &str| {
            fx["threads"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["slug"] == s)
        };
        let new = truth == "new" && landed != "home" && !fixture(&landed);
        match landed == truth || new {
            true => right += 1,
            false => wrong.push(format!("{text:?} went to {landed}, was for {truth}")),
        }
        if tag(&res).how == RouteHow::Guess {
            waited += 1;
        }
    }
    let top1 = right as f64 / msgs.len() as f64;
    let ask_rate = clear_asks as f64 / clear as f64;
    eprintln!(
        "thread-route 40: top-1 {right}/{} = {:.0}%, asks {asks} ({clear_asks} of {clear} clear = {:.1}%), guesses {waited}; wrong: {wrong:?}",
        msgs.len(),
        top1 * 100.0,
        ask_rate * 100.0
    );
    assert!(top1 >= 0.90, "top-1 {top1}: {wrong:?}");
    assert!(
        ask_rate <= 0.10,
        "asked on {clear_asks} of {clear} clear messages"
    );
    assert_eq!(waited, 0, "Jev answered every time");
}

/// An answer from a stranger in a shared place does not count: the question
/// still waits, and the refusal says why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_strangers_answer_does_not_count() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 2);
    let c = &r.core;
    two_threads(c);
    let (tx, rx) = tokio::sync::oneshot::channel();
    c.runner.threads.route.ask(
        "ask_s1",
        crate::thread_flow::Pending {
            home: "ses_x".into(),
            text: "x".into(),
            options: crate::thread_route::offer_new("x"),
            slugs: vec![],
            tx,
        },
    );
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::THREAD_ANSWER,
            ThreadAnswerParams {
                ask_id: "ask_s1".into(),
                choice: "home".into(),
                author: None,
                discord: None,
            },
        )
        .await;
    // The CLI is the owner's own surface: it counts.
    assert!(w.response(id).await["result"].is_object());
    assert!(rx.await.is_ok());
    // A Discord press from an id that is no owner's does not.
    let (tx, rx) = tokio::sync::oneshot::channel();
    c.runner.threads.route.ask(
        "ask_s2",
        crate::thread_flow::Pending {
            home: "ses_x".into(),
            text: "x".into(),
            options: crate::thread_route::offer_new("x"),
            slugs: vec![],
            tx,
        },
    );
    let mut d = Wire::open_as(c, Surface::Discord);
    let id = d
        .send(
            method::THREAD_ANSWER,
            ThreadAnswerParams {
                ask_id: "ask_s2".into(),
                choice: "home".into(),
                author: Some("discord:mallory".into()),
                discord: Some(theseus_protocol::DiscordOrigin {
                    user_id: "777".into(),
                    channel_id: "888".into(),
                    guild_id: Some("9".into()),
                }),
            },
        )
        .await;
    let refused = d.response(id).await;
    assert_eq!(
        refused["error"]["code"],
        theseus_protocol::error_code::REFUSED,
        "{refused}"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), rx)
            .await
            .is_err(),
        "the question still waits"
    );
}

/// A session the client names with `theseus ask -s` is the owner's choice
/// (theseus-0rfjx): the router never moves it into a thread, however plainly
/// the message names one. The same words through a surface that names no
/// session choice (the chat's own home session) are placed as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_named_with_ask_s_is_never_placed_in_a_thread() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let pinned = home(c, None);
    let mut p = params(&pinned, "what is failing in PR 412?", None);
    p.via = Some("ask".into());
    let mut w = Wire::open(c);
    let id = w.send(method::TURN_SUBMIT, p).await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    assert_eq!(res.session_id, pinned, "it ran in the session it named");
    assert!(res.thread.is_none(), "no thread took it: {:?}", res.thread);
    assert!(
        c.threads().get("pr-round").unwrap().session_id.is_none(),
        "the thread opened no session for it"
    );
    assert!(rows(c, "thread.routed").is_empty());
    // The chat's own session, on the same words, is placed.
    let chat = home(c, None);
    let mut p = params(&chat, "what is failing in PR 412?", None);
    p.via = Some("tui".into());
    let id = w.send(method::TURN_SUBMIT, p).await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    assert_eq!(tag(&res).slug.as_deref(), Some("pr-round"));
}

/// What `theseus chat` does (theseus-wapgp): one connection sends
/// `session.typing`, then a long message the router cannot place. The turn
/// waits in the daemon for the owner's word, and the chat's own answer, sent
/// on that same connection while the turn waits, frees it at once (the wait
/// is a minute here: only the answer can). The message is stored when it
/// does, in the order it was sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chat_that_types_then_sends_a_long_message_frees_it_by_answering_on_its_connection() {
    let jev = FakeJev::start().unwrap();
    // Two contend: the message names the security fix's repo, and Jev
    // leans to the pr round (theseus-e31an: a lone lean is no contest).
    thread_answer(&jev, "pr-round", 0.55);
    let r = rig_on(texts(4), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 3000;
        c.threads.ask_wait_secs = 60;
    });
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    let mut w = Wire::open(c);
    let typing = w
        .send(
            method::SESSION_TYPING,
            json!({"session_id": dm, "author": "theseus chat"}),
        )
        .await;
    assert!(w.response(typing).await["result"].is_object());
    let long = format!(
        "why is the build red in vault? {}",
        "and the weather is fine ".repeat(7)
    );
    assert!(long.len() >= 160, "the length that hung: {}", long.len());
    let id = w.send(method::TURN_SUBMIT, params(&dm, &long, None)).await;
    let asked = w.notice("thread.asked").await;
    assert!(texts_of(c, &dm).is_empty(), "held for the question");
    let t0 = Instant::now();
    let a = w
        .send(
            method::THREAD_ANSWER,
            ThreadAnswerParams {
                ask_id: asked["ask_id"].as_str().unwrap().into(),
                choice: "home".into(),
                author: Some("theseus chat".into()),
                discord: None,
            },
        )
        .await;
    assert!(w.response(a).await["result"].is_object());
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    assert!(
        t0.elapsed() < Duration::from_secs(20),
        "freed by the answer"
    );
    assert_eq!(res.session_id, dm);
    assert_eq!(texts_of(c, &dm), [long]);
}

async fn wait_for(mut cond: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !cond() {
        assert!(t0.elapsed() < Duration::from_secs(20), "timed out waiting");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A plainly new, self-contained request that no thread claims runs at home
/// at once, with no "which thread?" card (theseus-e31an: the walkthrough's
/// step 1 was asked about at route confidence 0.38).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_request_no_thread_claims_runs_at_home_without_asking() {
    let jev = FakeJev::start().unwrap();
    thread_answer(&jev, "security-fix", 0.38);
    let r = rig_on(texts(2), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 3000;
        c.threads.ask_wait_secs = 2;
    });
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::TURN_SUBMIT,
            params(
                &dm,
                "start two tasks: a census of the disk, and a slow load",
                None,
            ),
        )
        .await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    assert_eq!(res.session_id, dm);
    assert_eq!(tag(&res).how, RouteHow::Home);
    assert!(rows(c, "thread.asked").is_empty(), "nothing was asked");
    assert!(!w.heard.iter().any(|n| n["method"] == "thread.asked"));
}

/// The sender hears where its message went when it is placed, not at the turn's
/// end (theseus-7kjb7): one `thread.placed` with the chat's session, the thread's,
/// the tag and the id the turn ran as. A message that stays where it was named
/// (`ask -s`) sends none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_placed_message_tells_the_submitting_connection() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let chat = home(c, None);
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::TURN_SUBMIT,
            params(&chat, "#security-fix rotate the key", None),
        )
        .await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    let placed: Vec<&Value> = w
        .heard
        .iter()
        .filter(|n| n["method"] == "thread.placed")
        .collect();
    assert_eq!(placed.len(), 1, "{:?}", w.heard);
    let p = &placed[0]["params"];
    assert_eq!(p["session_id"], json!(chat));
    assert_eq!(p["placed"], json!(session_of(c, "security-fix")));
    assert_eq!(p["turn_id"], json!(res.turn_id), "the id the turn ran as");
    assert_eq!(p["tag"]["slug"], json!("security-fix"));
    assert_eq!(p["tag"]["how"], json!("addressed"));
    // A session the client named stays where it is: nothing is placed.
    let pinned = home(c, None);
    let mut p = params(&pinned, "what is failing in PR 412?", None);
    p.via = Some("ask".into());
    let mut w = Wire::open(c);
    let id = w.send(method::TURN_SUBMIT, p).await;
    w.response(id).await;
    assert!(!w.heard.iter().any(|n| n["method"] == "thread.placed"));
}

/// A chat on a thread's own session is never routed again (`theseus chat SESSION`
/// sends no `via`, and keeps no pin: theseus-umb8a's decision): the turn runs
/// there and no `thread.placed` is sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chat_on_a_threads_own_session_is_never_rerouted() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let chat = home(c, None);
    let first = submit(c, &chat, "#security-fix rotate the key").await;
    let own = first.session_id;
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::TURN_SUBMIT,
            params(&own, "what is failing in PR 412?", None),
        )
        .await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    assert_eq!(res.session_id, own, "it ran where it was sent");
    assert!(res.thread.is_none(), "{:?}", res.thread);
    assert!(!w.heard.iter().any(|n| n["method"] == "thread.placed"));
}

// ------------------------------------------------------- the directory rule

/// The walkthrough's misplacement (theseus-vw8k7): a dry run typed in a
/// directory that is no thread's went to the place's last thread, which the
/// owner had once moved a dry run like it to. A thread whose roots are set
/// keeps only what the message names of it when the turn's directory lies
/// under none of them: the learned correction, stickiness and the reply chain
/// no longer place it, and Jev's pick decides. The same message from under
/// the thread's root, and one to a thread with no roots, go there as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_from_another_directory_is_not_placed_by_stickiness() {
    const ROOT: &str = "/w/projects/lighthouse";
    let dry = |n: u32| format!("[dry-run] Start 2 tasks, $1 each: bash slow.sh census {n}");
    // The dev thread's roots, the turn's directory, and where it must run.
    let cases: [(&[&str], &str, bool); 3] = [
        (&[ROOT], "/w/projects/walkthrough", false),
        (&[ROOT], "/w/projects/lighthouse/crates", true),
        (&[], "/w/projects/walkthrough", true),
    ];
    for (roots, dir, to_dev) in cases {
        let jev = FakeJev::start().unwrap();
        // The working sets stay as made: the priors alone are on trial here
        // (growth's file names are the next test's).
        let r = rig_on(texts(8), Some(&jev), |c| {
            inbound_only(c);
            c.judge.total_secs = 5;
            c.places.owner = Some(vec![format!("discord:{OWNER}")]);
            c.threads.working_set_grows = false;
        });
        let c = &r.core;
        thread(c, "dev", "lighthouse dev", ws(&["#412"], &[], roots));
        thread(c, "ops", "ops rota", ws(&["#977"], &[], &[]));
        let dm = home(c, Some(&format!("dm:{OWNER}")));
        // An earlier dry run went to ops, and the owner moved it to dev: the
        // correction is learned, and dev is the place's last thread.
        thread_answer(&jev, "ops", 0.75);
        let first = submit(c, &dm, &dry(1)).await;
        assert_eq!(first.session_id, session_of(c, "ops"));
        let mut w = Wire::open(c);
        let id = w
            .send(
                method::THREAD_MOVE,
                ThreadMoveParams {
                    session_id: dm.clone(),
                    to: "dev".into(),
                    turn_id: None,
                    author: None,
                    discord: None,
                },
            )
            .await;
        assert_eq!(w.response(id).await["result"]["redirected"], true);
        // Jev, asked, would keep it at home.
        thread_answer(&jev, "home", 0.95);
        let mut p = params(&dm, &dry(2), None);
        p.dir = Some(dir.into());
        let id = w.send(method::TURN_SUBMIT, p).await;
        let res: TurnSubmitResult =
            serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
        let routed = rows(c, "thread.routed");
        let last = &routed.last().unwrap().data;
        match to_dev {
            true => assert_eq!(
                res.session_id,
                session_of(c, "dev"),
                "{dir} {roots:?}: {last}"
            ),
            false => {
                assert_eq!(res.session_id, dm, "{dir} {roots:?}: {last}");
                let dev = last["pre"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|x| x["thread"] == "dev")
                    .unwrap_or_else(|| panic!("dev's score: {last}"));
                assert_eq!(
                    dev["score"].as_f64(),
                    Some(0.0),
                    "dev kept no prior: {last}"
                );
            }
        }
    }
}

/// The walkthrough's other half (theseus-vw8k7): the earlier dry run ran in
/// dev, whose working set grew the message's `slow.sh`; the next dry run,
/// typed in another directory, names `slow.sh` too, a hit on a bare file name
/// that is another directory's file. Apart from dev's roots it counts for
/// nothing, with stickiness and the chain, and Jev decides; from under dev's
/// root the same message still goes to dev.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_named_from_another_directory_is_not_the_threads() {
    const ROOT: &str = "/w/projects/lighthouse";
    let dry = |n: u32| format!("[dry-run] Start 2 tasks, $1 each: bash slow.sh census {n}");
    for (dir, to_dev) in [
        ("/w/projects/walkthrough", false),
        ("/w/projects/lighthouse", true),
    ] {
        let jev = FakeJev::start().unwrap();
        let r = rig(&jev, 8);
        let c = &r.core;
        // The thread holds `slow.sh` as an owner or an import gave it: growth
        // no longer adds a bare file name (theseus-iw50k).
        let mut dev = ws(&["#412"], &[], &[ROOT]);
        dev.files = vec!["slow.sh".into()];
        thread(c, "dev", "lighthouse dev", dev);
        thread(c, "ops", "ops rota", ws(&["#977"], &[], &["/w/srv/ops"]));
        let dm = home(c, Some(&format!("dm:{OWNER}")));
        thread_answer(&jev, "dev", 0.75);
        let first = submit(c, &dm, &dry(1)).await;
        assert_eq!(first.session_id, session_of(c, "dev"));
        let files = c.threads().get("dev").unwrap().working_set.files.clone();
        assert_eq!(files, ["slow.sh"], "grown: {files:?}");
        thread_answer(&jev, "home", 0.95);
        let mut p = params(&dm, &dry(2), None);
        p.dir = Some(dir.into());
        let mut w = Wire::open(c);
        let id = w.send(method::TURN_SUBMIT, p).await;
        let res: TurnSubmitResult =
            serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
        let routed = rows(c, "thread.routed");
        let last = &routed.last().unwrap().data;
        let want = match to_dev {
            true => session_of(c, "dev"),
            false => dm.clone(),
        };
        assert_eq!(res.session_id, want, "{dir}: {last}");
    }
}

/// A message the owner moved grows the thread it was moved to, and not the
/// one it ran in (theseus-iw50k): what its words grew into the wrong thread
/// is taken back before the message runs again, so the right thread, which
/// would not take an entity another live thread holds, grows from it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_moved_message_grows_the_thread_it_was_moved_to() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 8);
    let c = &r.core;
    thread(c, "dev", "lighthouse dev", ws(&["#412"], &[], &[]));
    thread(c, "ops", "ops rota", ws(&["#977"], &[], &[]));
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    thread_answer(&jev, "ops", 0.75);
    let first = submit(c, &dm, "look at OPS-4214 for the census").await;
    assert_eq!(first.session_id, session_of(c, "ops"));
    let refs = |slug: &str| c.threads().get(slug).unwrap().working_set.refs.clone();
    assert!(
        refs("ops").iter().any(|e| e == "OPS-4214"),
        "{:?}",
        refs("ops")
    );
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::THREAD_MOVE,
            ThreadMoveParams {
                session_id: dm.clone(),
                to: "dev".into(),
                turn_id: None,
                author: None,
                discord: None,
            },
        )
        .await;
    w.response(id).await;
    assert!(
        refs("dev").iter().any(|e| e == "OPS-4214"),
        "{:?}",
        refs("dev")
    );
    assert!(
        !refs("ops").iter().any(|e| e == "OPS-4214"),
        "{:?}",
        refs("ops")
    );
    assert!(
        refs("ops").iter().any(|e| e == "#977"),
        "what it held stays"
    );
}
