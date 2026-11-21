//! A message goes to a thread by its address only (theseus-rha2; the owner,
//! 2026-11-21), through the core: a `#slug`, `/t slug`, "in the <title>
//! thread:", a place bound to the thread and a reply to a thread's reply each
//! place it, certain; a message with no address runs in the place's own
//! conversation, whatever it names and whatever Jev's `thread` answer says,
//! while that question still rides the one inbound request; and `thread.move`
//! with its one note and its one re-submission, which teaches nothing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::thread_route::{RouteHow, ThreadMoveParams, ThreadTag};
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

mod exchange;
mod tail;

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

fn thread_answer(jev: &FakeJev, slug: &str, p: f64) {
    jev.script(
        "thread",
        Jev::Choice {
            option: slug.into(),
            confidence: p,
        },
    );
}

/// A message that names a thread's PR but no address runs in the place's own
/// conversation at once: no thread takes it, whatever Jev's `thread` answer
/// says, and so does the follow-up right after the reply. That question still
/// rides the one inbound request (role and the rest ride it too, no call
/// added), the turn takes the point over, and the row names none of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_with_no_address_runs_at_home_whatever_it_names() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    thread_answer(&jev, "pr-round", 0.95);
    let res = submit(c, &dm, "what is failing in PR 412?").await;
    assert_eq!(res.session_id, dm, "the place's own conversation took it");
    let t = tag(&res);
    assert_eq!(
        (t.how, t.slug.as_deref(), t.movable),
        (RouteHow::Home, None, false)
    );
    assert_eq!(t.words(), None, "a quiet home message has no tag");
    assert!(
        c.threads().get("pr-round").unwrap().session_id.is_none(),
        "the thread opened no session for it"
    );
    // One request, carrying the thread question.
    wait_for(|| jev.seen().len() == 1 && !rows(c, "judge.call").is_empty()).await;
    assert_eq!(jev.seen().len(), 1, "no call was added");
    let q = jev.seen()[0].body["questions"].clone();
    let ids: Vec<String> = q.as_object().unwrap().keys().cloned().collect();
    assert!(ids.contains(&"classify.v4/thread".to_string()), "{ids:?}");
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
    let routed = rows(c, "thread.routed");
    assert_eq!(routed.len(), 1);
    assert_eq!(routed[0].data["to"], "home");
    assert_eq!(routed[0].data["early"], true);
    assert_eq!(
        routed[0].turn_id.as_deref(),
        Some(res.turn_id.as_str()),
        "the row rides the turn's own frame"
    );
    for gone in ["pre", "jev", "timed_out", "waited_ms", "ask"] {
        assert!(
            routed[0].data.get(gone).is_none(),
            "{gone}: {:?}",
            routed[0].data
        );
    }
    // The judgment rows belong to the turn that ran: the point was taken over.
    let calls = rows(c, "judge.call");
    assert!(
        calls
            .iter()
            .all(|j| j.turn_id.as_deref() == Some(res.turn_id.as_str())),
        "{calls:?}"
    );
    // The follow-up right after the reply, with no entity: home again.
    thread_answer(&jev, "pr-round", 0.95);
    let res2 = submit(c, &dm, "and the tests too").await;
    assert_eq!(res2.session_id, dm);
    assert_eq!(
        texts_of(c, &dm),
        ["what is failing in PR 412?", "and the tests too"]
    );
    assert!(rows(c, "thread.asked").is_empty(), "nothing is asked");
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

/// The owner's words in a message with no address, in a place with threads (theseus-slvl.5 on thread-route,
/// the owner's 2026-10-15 "I think that sounds right"): `directive.v2` rides the call begun before the turn,
/// the one request the message makes, and the record names the message the turn wrote, at home.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_at_home_is_captured_in_the_early_call_and_names_its_message() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    thread_answer(&jev, "security-fix", 0.95);
    jev.script(
        "directive.v2/kind",
        Jev::Choice {
            option: "preference".into(),
            confidence: 0.93,
        },
    );
    jev.script(
        "directive.v2/scope",
        Jev::Choice {
            option: "everywhere".into(),
            confidence: 0.9,
        },
    );
    let a = submit(c, &dm, "no tables with me when we talk about the build").await;
    assert_eq!(a.session_id, dm);
    wait_for(|| !rows(c, "standing.captured").is_empty()).await;
    assert_eq!(jev.seen().len(), 1, "no call was added");
    let q = jev.seen()[0].body["questions"].clone();
    let ids: Vec<String> = q.as_object().unwrap().keys().cloned().collect();
    assert!(ids.contains(&"classify.v4/thread".to_string()), "{ids:?}");
    assert!(ids.contains(&"directive.v2/kind".to_string()), "{ids:?}");
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
        "the record names the message the turn wrote"
    );
}

/// "That was for the security fix": the wrong thread gets one note and the
/// message runs once more in the right one. Nothing is learned: no label is
/// written, and the next close message runs at home.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_writes_one_note_resubmits_once_and_teaches_nothing() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 8);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let wrong = submit(c, &dm, "#pr-round rotate the signing keys on the gateway").await;
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
    assert!(rows(c, "thread.moved")[0].data.get("labeled").is_none());
    // No label on the thread question: the move corrects no guess.
    assert_eq!(rows(c, "judge.label").len(), before);
    // A close next message runs at home: nothing was learned from the move.
    let next = submit(c, &dm, "rotate the signing keys on the gateway again").await;
    assert_eq!(next.session_id, dm);
    assert_eq!(tag(&next).how, RouteHow::Home);
    // Moving the moved message to where it already is says so.
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::THREAD_MOVE,
            ThreadMoveParams {
                session_id: dm.clone(),
                to: "security-fix".into(),
                turn_id: moved["result"]["turn_id"].as_str().map(str::to_string),
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

/// **The owner's words "that was for the security fix" move nothing on their
/// own; a model that chooses to call `chat.move_to_thread` moves the message
/// the router placed, once, and the same `thread.moved` row is written**
/// (issue 13; the owner's ruling of 2026-11-14 11:37). The successor of
/// `words_that_a_message_was_for_another_thread_move_it`, which asserted that
/// the words moved the message before any turn ran.
///
/// The model's script: a reply to each message, and, to the correcting one, a
/// call of the tool and a line saying it is moving. The move runs when that
/// turn ends: the note in the wrong thread, the one `thread.moved` row, the
/// message run again once in the right thread, and the correcting message left
/// where it ran (at home: it names no thread).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_model_that_calls_the_move_tool_moves_the_placed_message_and_the_bare_words_move_nothing()
{
    let jev = FakeJev::start().unwrap();
    let script = vec![
        Scripted::text("Done: the words."),
        Scripted::text("Done: the keys."),
        Scripted::tools(
            "",
            &[("mv1", "chat_move_to_thread", json!({"to": "security-fix"}))],
        ),
        Scripted::text("Moving it to the security fix."),
        Scripted::text("Done: rotated."),
        Scripted::text("Done: the other words."),
        Scripted::text("Done: spare."),
    ];
    let r = rig_on(script, Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.places.owner = Some(vec![
            format!("discord:{OWNER}"),
            format!("discord:{OWNER2}"),
        ]);
    });
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    // Nothing placed yet: the words are a message, run at home like one.
    let said = submit(c, &dm, "that was for the security fix").await;
    assert_eq!(said.session_id, dm);
    assert!(rows(c, "thread.moved").is_empty());
    let dm = home(c, None);
    let wrong = submit(c, &dm, "#pr-round rotate the signing keys on the gateway").await;
    assert_eq!(wrong.session_id, session_of(c, "pr-round"));

    // The correcting message, at home: the model chooses to call the tool.
    let said = submit(c, &dm, "No, that was for the security fix.").await;
    assert_eq!(said.session_id, dm, "{said:?}");
    let moved = rows(c, "thread.moved");
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(
        (moved[0].data["from"].as_str(), moved[0].data["to"].as_str()),
        (Some("pr-round"), Some("security-fix")),
        "{:?}",
        moved[0].data
    );
    assert_eq!(
        moved[0].data["redirected"], true,
        "the wrong thread was told"
    );
    assert_eq!(
        texts_of(c, &session_of(c, "security-fix")),
        ["rotate the signing keys on the gateway"],
        "the placed message ran again, once, in the right thread, and the words did not"
    );

    // The words alone, with a message placed before them: the model calls
    // nothing, so nothing moves.
    let before = rows(c, "thread.moved").len();
    submit(c, &dm, "that was for the security fix").await;
    assert_eq!(
        rows(c, "thread.moved").len(),
        before,
        "bare words move nothing"
    );
}

/// A rig of the inbound batch with the owner's DMs private and a scripted model.
fn move_rig(jev: &FakeJev, script: Vec<Scripted>) -> Rig {
    rig_on(script, Some(jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.places.owner = Some(vec![
            format!("discord:{OWNER}"),
            format!("discord:{OWNER2}"),
        ]);
    })
}

/// **A correction addressed to the thread it names still moves the placed
/// message** (round 2 of issue 13, B2): the correction runs in the very thread
/// the message should go to; the tool finds the placed message by the place's
/// own session, not by where the correction ran.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_correction_addressed_to_the_thread_it_names_still_moves_the_placed_message() {
    let jev = FakeJev::start().unwrap();
    let r = move_rig(
        &jev,
        vec![
            Scripted::text("Done: the keys."),
            Scripted::tools(
                "",
                &[("mv1", "chat_move_to_thread", json!({"to": "security-fix"}))],
            ),
            Scripted::text("Moving it to the security fix."),
            Scripted::text("Done: rotated."),
            Scripted::text("Done: spare."),
        ],
    );
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    let wrong = submit(c, &dm, "#pr-round rotate the signing keys on the gateway").await;
    assert_eq!(wrong.session_id, session_of(c, "pr-round"));

    let said = submit(c, &dm, "#security-fix No, that was for the security fix.").await;
    assert_eq!(said.session_id, session_of(c, "security-fix"), "{said:?}");
    let moved = rows(c, "thread.moved");
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].data["from"], "pr-round", "{:?}", moved[0].data);
    assert!(
        texts_of(c, &session_of(c, "security-fix"))
            .iter()
            .any(|t| t == "rotate the signing keys on the gateway"),
        "the placed message ran again in the right thread"
    );
}

/// **A move staged in a turn that fails is not kept for a later one** (round 2
/// of issue 13, B1): the model calls the tool, its next model call fails, the
/// turn ends in an error; the owner's next message, which ends well, carries
/// nothing out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_staged_in_a_turn_that_failed_is_not_carried_out_by_the_next_turn() {
    let jev = FakeJev::start().unwrap();
    let r = move_rig(
        &jev,
        vec![
            Scripted::text("Done: the keys."),
            Scripted::tools(
                "",
                &[("mv1", "chat_move_to_thread", json!({"to": "security-fix"}))],
            ),
            Scripted::Fail(crate::provider::ProviderError::InvalidRequest {
                status: 400,
                message: "refused".into(),
            }),
            Scripted::text("Done: the next message."),
            Scripted::text("Done: spare."),
        ],
    );
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    submit(c, &dm, "#pr-round rotate the signing keys on the gateway").await;

    let mut w = Wire::open(c);
    let id = w
        .send(
            method::TURN_SUBMIT,
            params(&dm, "No, that was for the security fix.", None),
        )
        .await;
    let failed = w.response(id).await;
    assert!(failed["error"].is_object(), "the turn failed: {failed}");

    submit(c, &dm, "and another thing").await;
    assert!(
        rows(c, "thread.moved").is_empty(),
        "a move from a failed turn was carried out later"
    );
}

/// A session the client names with `theseus ask -s` is the owner's choice
/// (theseus-0rfjx): the router never moves it into a thread, even when the
/// message addresses one. The same words through a surface that names no
/// session choice (the chat's own home session) are placed by their address.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_named_with_ask_s_is_never_placed_in_a_thread() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let pinned = home(c, None);
    let mut p = params(&pinned, "#pr-round what is failing in PR 412?", None);
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
    let mut p = params(&chat, "#pr-round what is failing in PR 412?", None);
    p.via = Some("tui".into());
    let id = w.send(method::TURN_SUBMIT, p).await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    assert_eq!(tag(&res).slug.as_deref(), Some("pr-round"));
}

async fn wait_for(mut cond: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !cond() {
        assert!(t0.elapsed() < Duration::from_secs(20), "timed out waiting");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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
    let first = submit(c, &dm, "#ops look at OPS-4214 for the census").await;
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
