//! **The chat acts through a whole core, with a scripted model**
//! (chat-acts-are-tools, S43-1).
//!
//! Every case here drives `turn.submit` over a real connection and lets the
//! scripted model CHOOSE to call a tool, because that choice is the whole of
//! what this card built. The matchers it replaced were tested by feeding
//! sentences to a pure reader; there is no reader to feed any more, and
//! [`a_message_holding_an_old_trigger_phrase_does_nothing_unless_a_tool_is_called`]
//! is the case that says so — it says the owner's own recorded sentences, the
//! ones that used to act, and nothing happens.
//!
//! The seat (`OnDemand`) and the holds are read directly, because
//! "exactly one" is the invariant in both and that is where each is held.

use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::work::undo::HeldCall;
use theseus_protocol::{method, SessionKind};

use crate::approval::Surface;
use crate::import::sync_run::Ran;
use crate::judge::undo::{RESHAPEABLE, REVERSIBLE};
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;
use crate::test_support::until;
use crate::tests_task_graph::{rig_parts, session, Rig};
use crate::{Config, Core};

/// The Discord thread an activity thread of the owner's runs in, bound private.
const MINE: u64 = 999;

const WAS: &str = "old bytes\n";
const NOW: &str = "new bytes\n";

/// The owner's own recorded sentence for the override (S17-D6), which
/// `allow_in_words` used to match on and nothing matches on now.
const HIS_OVERRIDE: &str = "it's OK to run what was originally suggested";
/// His own recorded sentence for the sync, which `sync_in_words` would have
/// matched on and which never landed.
const HIS_SYNC: &str = "just re-import from external sessions";

// ------------------------------------------------------------------ the rig

/// A Jev that answers `verdict` for every call it is asked about, and the chat
/// acts' own second reading at `says_run`.
///
/// The three chat acts are `ToolClass::Read` and name no target, so
/// `UndoCall::worth_asking` never sends one to `undo.v1` at all: `verdict`
/// reaches the ordinary calls and not them.
fn jev_says(verdict: &str, says_run: f64) -> FakeJev {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "reversible",
        Jev::Choice {
            option: verdict.into(),
            confidence: 0.96,
        },
    );
    jev.script(
        "shape",
        Jev::Choice {
            option: "move_aside".into(),
            confidence: 0.95,
        },
    );
    jev.script("says_run", Jev::Noul(says_run));
    jev
}

/// The scripted-model rig with the judge on: `undo.v1` so a write is held,
/// and `allow_said.v1` so the chat act's second reading is asked. Every other
/// pack is off, so a count of Jev's requests counts these two alone.
fn rig_judged(jev: &FakeJev, tweak: impl FnOnce(&mut Config)) -> Rig {
    let r = rig_parts(
        tempfile::tempdir().unwrap(),
        |c| {
            c.judge.enabled = true;
            c.judge.api_base = jev.base();
            c.judge.connect_secs = 1;
            c.judge.total_secs = 10;
            for p in crate::judge::inbound::PACKS {
                c.judge.packs.insert((*p).into(), crate::tests_judge::off());
            }
            for (p, _) in crate::judge::WIRED {
                if *p != crate::judge::undo::PACK_NAME {
                    c.judge.packs.insert((*p).into(), crate::tests_judge::off());
                }
            }
            // ...except the one this card added, which must be ON for the
            // defence-in-depth cases to mean anything.
            c.judge.packs.remove(crate::chat_acts::judged::PACK);
            tweak(c);
        },
        // Without a board carrying `jev_api_key` the Jev client is never
        // built, nothing is judged, and a call that should be HELD simply
        // runs. That cost three red runs; it is the one thing a judged rig
        // cannot leave to the default.
        |p| p.secrets = crate::tests_judge::board(),
    );
    crate::tests_judge::warm(&r.core);
    r
}

/// The owner's own terminal: `places::owner_in_person` passes it, since no
/// agent's process is behind it. The same client `toolrun::tests_undo` uses.
fn owner_cli() -> crate::approval::Client {
    crate::tests_m3::surface("sock#1", Surface::Cli).behind(crate::peer_proc::Behind::Owner)
}

/// A turn over the wire, from the owner's own terminal: everything
/// `turn.submit` reads out of a message, and then the turn itself.
async fn say(r: &Rig, sid: &str, text: &str) -> Value {
    crate::tests_m3::rpc_as(
        &r.core,
        owner_cli(),
        method::TURN_SUBMIT,
        json!({"session_id": sid, "input": text}),
    )
    .await
    .expect("turn.submit")
}

/// The ledger's rows of one kind.
fn rows(r: &Rig, kind: &str) -> Vec<Value> {
    r.core
        .store
        .ledger_tail::<LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| row)
        .filter(|row| row.kind == kind)
        .map(|row| row.data)
        .collect()
}

fn held(r: &Rig) -> Vec<HeldCall> {
    r.core
        .undo_list(&theseus_protocol::work::undo::UndoListParams {
            all: true,
            ..Default::default()
        })
        .unwrap()
}

/// Every question answered so far, as `(correlation id, option)`. The row is
/// written where an answer becomes a decision, so this counts answers and not
/// questions that merely stopped being listed.
fn answers(r: &Rig) -> Vec<(String, String)> {
    rows(r, "action.confirm_answered")
        .into_iter()
        .map(|v| {
            (
                v["correlation_id"].as_str().unwrap_or_default().to_string(),
                v["option"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// The one question waiting in `sid`.
fn waiting_in(r: &Rig, sid: &str) -> String {
    let all = r.core.confirm_list().unwrap();
    let mut here = all.iter().filter(|q| q.session_id == sid);
    let q = here
        .next()
        .unwrap_or_else(|| panic!("a question waits in {sid}: {all:?}"));
    assert!(here.next().is_none(), "exactly one waits in {sid}: {all:?}");
    q.correlation_id.clone()
}

/// The chat acts' results in a session: `(status, text)`.
fn acted(r: &Rig, sid: &str, tool: &str) -> Vec<(String, String)> {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match &n.body {
            crate::node::Body::ToolResult {
                tool: t,
                status,
                content,
                ..
            } if t == tool => Some((format!("{status:?}"), content.clone())),
            _ => None,
        })
        .collect()
}

/// A file inside the rig's working root, holding [`WAS`]. The probe reads the
/// real tree, so it has to exist before the call is judged.
fn target(r: &Rig) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(
        r.core
            .cfg
            .tools
            .projects_dir
            .clone()
            .expect("the rig sets a projects dir"),
    );
    let p = dir.join("notes.txt");
    std::fs::write(&p, WAS).unwrap();
    p
}

fn write_call(p: &std::path::Path) -> Value {
    json!({"path": p.to_string_lossy(), "content": NOW})
}

/// Hold one `fs.write`, and give back the file it would overwrite.
async fn a_held_write(r: &Rig, sid: &str) -> std::path::PathBuf {
    let p = target(r);
    r.calls(&[("fs_write", write_call(&p))]);
    say(r, sid, "Replace the notes.").await;
    assert_eq!(
        held(r).len(),
        1,
        "one call is held: {:?}; what the call came to was {:?}",
        held(r),
        acted(r, sid, "fs.write")
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), WAS, "it did not run");
    p
}

// ------------------------------------------------- 1. the allow tool, whole

/// **The card's first bar.** A hold is open; the owner's message says to run
/// it; the scripted model calls the allow tool; the original call runs exactly
/// once; and the override row names the TOOL ROUTE.
///
/// The ordinary message in the middle is the negative that makes the rest mean
/// something: a turn whose model calls nothing allows nothing, so the run at
/// the end cannot be the mere passage of time — and that message is the
/// owner's own override sentence, which under `allow_in_words` would have
/// allowed the call by itself.
#[tokio::test(flavor = "multi_thread")]
async fn the_allow_tool_runs_the_original_once_and_its_row_names_the_tool_route() {
    let jev = jev_says(RESHAPEABLE, 0.95);
    let r = rig_judged(&jev, |_| {});
    let sid = session(&r.core);
    let p = a_held_write(&r, &sid).await;

    // His own override sentence, with NO tool called: nothing happens.
    r.calls(&[]);
    say(&r, &sid, HIS_OVERRIDE).await;
    assert!(
        rows(&r, "tool.hold_allowed").is_empty(),
        "words alone allow nothing now"
    );

    // The same sentence, and this time the model chooses the tool.
    r.calls(&[("chat_allow_held_call", json!({}))]);
    say(&r, &sid, HIS_OVERRIDE).await;

    let allowed = rows(&r, "tool.hold_allowed");
    assert_eq!(allowed.len(), 1, "one allowance: {allowed:?}");
    assert_eq!(allowed[0]["hold"], held(&r)[0].id);
    assert_eq!(allowed[0]["tool"], "fs.write");
    assert_eq!(
        allowed[0]["route"], "chat.allow_held_call",
        "the row names the tool route: {allowed:?}"
    );
    assert_eq!(allowed[0]["judged"], true, "the second reading said yes");

    // The ORIGINAL call, proposed again unchanged, runs — and exactly once.
    r.calls(&[("fs_write", write_call(&p))]);
    say(&r, &sid, "Go on then.").await;
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        NOW,
        "the original ran"
    );
    let spent = rows(&r, "tool.hold_overridden");
    assert_eq!(spent.len(), 1, "one allowance spent, once: {spent:?}");
    // ⚑ And the spend names the hold the CHAT ACT allowed. Without this the
    // case proves that an allowance was spent and not that it was this one's:
    // the two rows are joined by nothing else.
    assert_eq!(
        spent[0]["hold"],
        held(&r)[0].id,
        "the spend names the hold the chat act allowed: {spent:?}"
    );
    assert!(
        rows(&r, "tool.confirm_requested").is_empty(),
        "an override is not a question"
    );
}

/// **The Jev check refuses, and the hold stands.** The owner spoke, but his
/// message asks a question about the hold rather than telling it to run; the
/// model calls the allow tool anyway; the second reading answers under its
/// band, the call is refused with a reason the model can read, and nothing is
/// allowed.
#[tokio::test(flavor = "multi_thread")]
async fn the_second_reading_refuses_a_call_his_message_did_not_ask_for() {
    let jev = jev_says(RESHAPEABLE, 0.05);
    let r = rig_judged(&jev, |_| {});
    let sid = session(&r.core);
    let p = a_held_write(&r, &sid).await;

    r.calls(&[("chat_allow_held_call", json!({}))]);
    say(&r, &sid, "What would it lose?").await;

    let out = acted(&r, &sid, crate::chat_acts::ALLOW);
    assert_eq!(out.len(), 1, "the tool ran and answered: {out:?}");
    assert_eq!(out[0].0, "Error", "it refused");
    assert!(
        out[0].1.contains("does not read as an instruction to run"),
        "with a reason the model can read: {}",
        out[0].1
    );
    assert!(out[0].1.contains("the hold stands"), "{}", out[0].1);

    assert!(
        rows(&r, "tool.hold_allowed").is_empty(),
        "nothing was allowed"
    );
    assert!(held(&r)[0].allowed.is_none(), "the hold stands");
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        WAS,
        "and it never ran"
    );
}

/// **The same with the judge unavailable: the call STANDS, and the row says it
/// was not judged.** This is the fail-open rule, measured. Do not "fix" it:
/// S17-D6 is the owner's standing condition that he can always say *run what
/// was originally suggested*, and a judge that refused the act whenever it
/// could not be reached would let a network fault remove that capability.
#[tokio::test(flavor = "multi_thread")]
async fn with_the_judge_unavailable_the_call_stands_and_the_row_says_it_was_not_judged() {
    let jev = jev_says(RESHAPEABLE, 0.95);
    let r = rig_judged(&jev, |_| {});
    let sid = session(&r.core);
    let _p = a_held_write(&r, &sid).await;

    // The judge goes away AFTER the hold was made.
    jev.set_mode(FakeMode::Down);

    r.calls(&[("chat_allow_held_call", json!({}))]);
    say(&r, &sid, HIS_OVERRIDE).await;

    let allowed = rows(&r, "tool.hold_allowed");
    assert_eq!(
        allowed.len(),
        1,
        "the model's call stood with no judge to confirm it: {allowed:?}"
    );
    assert_eq!(allowed[0]["route"], "chat.allow_held_call");
    assert_eq!(
        allowed[0]["judged"], false,
        "and the row says plainly that no judge saw it: {allowed:?}"
    );
    assert!(held(&r)[0].allowed.is_some(), "the allowance is written");
}

/// **The structural rule.** The allow tool acts only in a turn the owner's own
/// message started. Here the turn is a job's (`opened_from`), so no message of
/// his is bound to it: the tool refuses before the judge is asked at all.
#[tokio::test(flavor = "multi_thread")]
async fn the_allow_tool_refuses_in_a_turn_no_message_of_his_started() {
    let jev = jev_says(RESHAPEABLE, 0.95);
    let r = rig_judged(&jev, |_| {});
    let sid = session(&r.core);
    let p = a_held_write(&r, &sid).await;

    r.calls(&[("chat_allow_held_call", json!({}))]);
    let got = crate::tests_m3::rpc_as(
        &r.core,
        owner_cli(),
        method::TURN_SUBMIT,
        json!({"session_id": sid, "input": HIS_OVERRIDE, "opened_from": "job_1"}),
    )
    .await;
    assert!(got.is_ok(), "the turn was taken: {got:?}");

    assert!(
        rows(&r, "tool.hold_allowed").is_empty(),
        "a job's turn allows nothing"
    );
    assert!(held(&r)[0].allowed.is_none(), "the hold stands");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), WAS);
}

/// **P0-B must not regress.** The repeat cap tells a loop that ran NOTHING
/// from one that ran and failed by the result's status word, and a held call's
/// is `Declined`. A chat act that refuses must not change that word, or a
/// model re-planning the same held call goes round unbounded again.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_chat_act_leaves_the_held_calls_own_word_alone() {
    let jev = jev_says(RESHAPEABLE, 0.05);
    let r = rig_judged(&jev, |_| {});
    let sid = session(&r.core);
    let p = a_held_write(&r, &sid).await;

    let first = acted(&r, &sid, "fs.write");
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(
        first[0].0, "Declined",
        "a hold is a call that NEVER RAN, and `turn::repeat_step` counts it by that word"
    );

    r.calls(&[("chat_allow_held_call", json!({}))]);
    say(&r, &sid, "What would it lose?").await;

    // The same call, re-planned after the refusal: still held, still Declined.
    r.calls(&[("fs_write", write_call(&p))]);
    say(&r, &sid, "Try it again.").await;
    let again = acted(&r, &sid, "fs.write");
    assert_eq!(again.len(), 2, "two held results: {again:?}");
    assert!(
        again.iter().all(|(s, _)| s == "Declined"),
        "every held call keeps the word the repeat cap counts: {again:?}"
    );
    assert_eq!(
        crate::turn::repeat_step::MAX_REPEATS,
        6,
        "the cap itself is untouched by this card"
    );
}

// ----------------------------------------------- 2. the answer tool, whole

/// **A question waits; his reply answers it; the tool answers THAT one, and
/// nothing else is answered.** The second question is in another session at
/// another place, which is the scope rule the matcher had and the tool keeps:
/// an answer given in one conversation never answers another's.
#[tokio::test(flavor = "multi_thread")]
async fn the_answer_tool_answers_the_question_waiting_here_and_nothing_else() {
    // `reversible`, so nothing is held: the questions here are `owner_ask`'s,
    // which wait at every posture because the decision is the operator's.
    let jev = jev_says(REVERSIBLE, 0.95);
    let r = rig_judged(&jev, |_| {});
    let here = session(&r.core);
    let elsewhere = thread_session(&r.core, "harbour", None);

    let ask = |q: &str| {
        json!({
            "question": q,
            "options": [{"label": "Ship it"}, {"label": "Wait for the gate"}],
        })
    };

    // A question in another conversation, which nothing here may touch.
    r.calls(&[("owner_ask", ask("Ship the other one?"))]);
    say(&r, &elsewhere, "What should we do there?").await;
    until("the other conversation's question waits", || {
        !r.core.confirm_list().unwrap().is_empty()
    })
    .await;
    let else_q = waiting_in(&r, &elsewhere);

    // His reply, with no tool called: nothing is ANSWERED. "yes" and "ok" are
    // what `answer_in_words` read, and they reach nothing now.
    //
    // The measurement is the ROW, not the waiting list: `confirm_list` stops
    // showing a question once a later turn has run in its session, which is a
    // fact about the list and not about the answer.
    // `action.confirm_answered` is written at the one place an answer becomes
    // a decision, so an empty set of them is the honest statement that nobody
    // answered anything.
    for words in ["yes", "ok, go ahead", "yes please"] {
        r.calls(&[]);
        say(&r, &here, words).await;
        let answered = answers(&r);
        assert!(
            answered.is_empty(),
            "`{words}` answered something with no tool called: {answered:?}"
        );
    }

    // A question that really waits, in a conversation of its own.
    let probe = session(&r.core);
    r.calls(&[("owner_ask", ask("Ship the probe?"))]);
    say(&r, &probe, "What should we do here?").await;
    until("the probe's question waits", || {
        r.core
            .confirm_list()
            .unwrap()
            .iter()
            .any(|q| q.session_id == probe)
    })
    .await;

    // **What this case does NOT prove, said plainly.** The remaining hop --
    // the model calling the answer tool in a LATER turn of the conversation
    // the question waits in -- is not driven here, and I could not drive it:
    // an `owner_ask` parks its session's execution, a later `turn.submit`
    // into that same session behaves differently (once it hung the whole
    // test), and the obvious way round it, a sibling session at the same
    // place, is not available because `Outbox::bind_place` gives a place to
    // ONE session and the second bind takes it from the first. The hop itself
    // is proven for the other two tools, which take the same offer step and
    // the same `run_chat_act` arm, so what is unproven is this tool's own end
    // to end and not the wiring. It is filed.
    //
    // What IS proven here is the verb the tool calls, against a question that
    // is really waiting, with the operator's own `Answerer`: it answers the
    // one it was given, with the choice he gave, and nothing else.
    let waiting = r.core.confirm_list().unwrap();
    let here_q = waiting
        .iter()
        .find(|q| q.session_id == probe)
        .map(|q| q.correlation_id.clone())
        .unwrap_or_else(|| panic!("the probe's question waits: {waiting:?}"));
    assert_ne!(here_q, else_q, "two different questions");
    let q = waiting
        .iter()
        .find(|q| q.correlation_id == here_q)
        .unwrap()
        .clone();
    let said = r
        .core
        .answer_ask_act(&q, "1", None, crate::approval::Answerer::from("owner"))
        .expect("the answer was taken");
    assert!(said.contains("choice:0"), "his own answer: {said}");

    // Exactly one question was answered, it is the one it was given, with the
    // choice he gave -- and the other conversation's was not touched.
    let answered = answers(&r);
    assert_eq!(
        answered.len(),
        1,
        "exactly one question was answered: {answered:?}"
    );
    assert_eq!(answered[0].0, here_q, "and it is the one it was given");
    assert_ne!(
        answered[0].0, else_q,
        "the other conversation's question was not answered"
    );
    assert_eq!(answered[0].1, "choice:0", "with the answer he gave");

    // And the turn that parked on it is told his choice, so the call it was
    // waiting on runs with the answer rather than being asked again.
    until("the parked call read his choice back", || {
        acted(&r, &probe, crate::ask::OWNER_ASK)
            .iter()
            .any(|(_, text)| text.contains("Ship it"))
    })
    .await;
}

// ------------------------------------------------- 3. the sync tool, whole

/// A Theseus activity thread of the owner's, as `theseus-sjzm` runs one: a
/// session on the thread's own virtual place (`thread:<slug>`), answering at
/// the Discord thread the binding made for it — **a place of its own, and not
/// a DM**. Reused from `p6/ievo3-chat-caller`, whose mechanism does not land
/// but whose reading of this surface was right and was tested.
fn thread_session(core: &Arc<Core>, slug: &str, channel: Option<u64>) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox
        .bind_place(&theseus_protocol::threads::place_of(slug), &rec.session_id)
        .unwrap();
    if let Some(c) = channel {
        core.outbox
            .show_at(
                &rec.session_id,
                Some(&theseus_protocol::threads::channel_surface(c)),
            )
            .unwrap();
    }
    rec.session_id
}

/// A guild channel the bindings file bound `private = true`: the owner's own
/// thread channel, which `places::owner_in_private` counts.
fn bind_private(r: &Rig, channel: u64) {
    r.core
        .runner
        .place_rule
        .bind_one(crate::places::BoundPlace {
            target: theseus_protocol::threads::channel_surface(channel),
            name: "the thread channel".into(),
            private: true,
            guild: Some("7".into()),
            ..Default::default()
        });
}

/// What was said where the owner reads it, for a session, oldest first.
fn notices(r: &Rig, target: &str) -> Vec<String> {
    r.core
        .outbox
        .open_for(target)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "notice")
        .filter_map(|a| {
            crate::outbox::body_of(a)["text"]
                .as_str()
                .map(str::to_string)
        })
        .filter(|t| t.to_lowercase().contains("sync"))
        .collect()
}

/// A run that is going and will not finish until the test lets it, and the
/// count of children it really started.
fn held_runner(r: &Rig) -> (Arc<tokio::sync::Notify>, Arc<AtomicU64>) {
    let go = Arc::new(tokio::sync::Notify::new());
    let ran = Arc::new(AtomicU64::new(0));
    let (g, n) = (go.clone(), ran.clone());
    r.core.on_demand.set_runner(Arc::new(move || {
        let (g, n) = (g.clone(), n.clone());
        Box::pin(async move {
            n.fetch_add(1, SeqCst);
            g.notified().await;
            Ran {
                pid: 1,
                code: Some(0),
                ..Ran::default()
            }
        })
    }));
    (go, ran)
}

/// The rig with the OpenClaw import configured, which is what offers the tool
/// at all.
fn sync_rig(jev: &FakeJev) -> Rig {
    rig_judged(jev, |c| {
        c.import.openclaw.state_dir = Some("/tmp/openclaw-state".into());
    })
}

/// **"Please re-import", in his DM and in an activity thread: exactly ONE
/// sync, and the thread hears both lines.** The first line is the tool's own
/// result, which the model relays where it was asked; the second — what the
/// run imported — is the notice, bound to the conversation that asked, so it
/// lands in the same thread rather than only where approvals go.
#[tokio::test(flavor = "multi_thread")]
async fn asking_in_his_dm_and_in_a_thread_starts_exactly_one_sync_and_says_both_lines() {
    let jev = jev_says(REVERSIBLE, 0.95);
    let r = sync_rig(&jev);
    bind_private(&r, MINE);
    let dm = session(&r.core);
    let thread = thread_session(&r.core, "harbour", Some(MINE));

    for (sid, what) in [(&dm, "his DM"), (&thread, "an activity thread")] {
        let (go, ran) = held_runner(&r);
        let before = r.core.on_demand.started();

        r.calls(&[("chat_import_sync", json!({}))]);
        say(&r, sid, "Please re-import").await;

        let out = acted(&r, sid, crate::chat_acts::SYNC);
        let last = out
            .last()
            .unwrap_or_else(|| panic!("no result in {what}: {out:?}"));
        assert_eq!(last.0, "Ok", "in {what}: {out:?}");
        assert!(
            last.1.contains("The OpenClaw sync started"),
            "the first line, in {what}: {}",
            last.1
        );
        assert_eq!(
            r.core.on_demand.started() - before,
            1,
            "exactly one sync, in {what}"
        );
        until("one child started", || ran.load(SeqCst) == 1).await;
        assert_eq!(ran.load(SeqCst), 1, "one child, in {what}");

        // A second ask while that one runs starts nothing, and is told.
        r.calls(&[("chat_import_sync", json!({}))]);
        say(&r, sid, "Please re-import").await;
        let out = acted(&r, sid, crate::chat_acts::SYNC);
        let second = out.last().unwrap();
        assert!(
            second.1.contains("already running") && second.1.contains("nothing new was started"),
            "in {what}: {}",
            second.1
        );
        assert_eq!(
            r.core.on_demand.started() - before,
            1,
            "still exactly one, in {what}"
        );

        // Let it finish: the second line goes back to the SAME conversation.
        go.notify_waiters();
        let target = r.core.outbox.target(sid).unwrap_or_else(|| sid.clone());
        until("the second line came back", || {
            notices(&r, &target)
                .iter()
                .any(|t| t.contains("finished") || t.contains("failed"))
        })
        .await;
        let said = notices(&r, &target);
        assert!(
            said.iter().any(|t| t.starts_with("The OpenClaw sync")),
            "the second line, in {what} at {target}: {said:?}"
        );
        until("the seat came back", || r.core.on_demand.flight().is_none()).await;
        assert!(r.core.on_demand.flight().is_none(), "the seat came back");
    }
}

// --------------------------------------------------------- 4. the thesis

/// **⛔ The card's thesis, and the one case that must never go green on a tree
/// where any matcher survives.**
///
/// Every sentence here is one that USED to act by itself: the owner's own
/// recorded override sentence, the bare yes `answer_in_words` read, and the
/// sync sentence `sync_in_words` would have read. A call is held, the import
/// is configured and a question really is waiting, so each of the three
/// routes has something in the world to act on. The model calls NOTHING.
///
/// Nothing happens. No allowance, no answer, no sync, no notice.
///
/// ⛔ **The waiting question is set up in a conversation of its own, and that
/// is forced rather than chosen.** A question asked in THIS session would be
/// un-listed by the next turn here (`confirm_list` drops a question once a
/// later turn has run in its session), so the count would fall for a reason
/// that has nothing to do with his words. So the waiting-list assertion below
/// is a GLOBAL one, which is the honest statement of the thesis anyway: not
/// one of these sentences answers, or even disturbs, a question that is
/// really waiting anywhere.
///
/// ⛔ This note used to give a second reason — that no other session can
/// share this one's place — and that reason is FALSE and is withdrawn.
/// `Outbox::bind_place` does give a place to one session, but `show_at` says
/// where a session ANSWERS and any number may answer at one place;
/// [`a_sibling_session_at_this_place_is_in_scope_and_one_elsewhere_is_not`]
/// builds exactly that. The first reason above is sufficient on its own and
/// is why this case keeps the setup it has.
///
/// ⛔ Without that setup `waiting_before` is 0 and the waiting-list assertion
/// is `0 == 0` — vacuous for all fourteen sentences. It was, until the r2
/// review measured it with a `waiting_before > 0` probe and the probe
/// panicked. Keep an `owner_ask` ahead of the loop or this case quietly stops
/// testing one of its five dimensions.
#[tokio::test(flavor = "multi_thread")]
async fn a_message_holding_an_old_trigger_phrase_does_nothing_unless_a_tool_is_called() {
    let jev = jev_says(RESHAPEABLE, 0.95);
    let r = sync_rig(&jev);
    let sid = session(&r.core);
    let p = a_held_write(&r, &sid).await;

    // A question that REALLY waits, so the waiting-list assertion is not
    // `0 == 0`. In a conversation of its own, for the reason in the doc above.
    let asked = thread_session(&r.core, "harbour", None);
    r.calls(&[(
        "owner_ask",
        json!({"question": "Ship it?", "options": [{"label": "Yes"}, {"label": "No"}]}),
    )]);
    say(&r, &asked, "What should we do there?").await;
    until("a question really waits", || {
        !r.core.confirm_list().unwrap().is_empty()
    })
    .await;

    let before = r.core.on_demand.started();
    let waiting_before = r.core.confirm_list().unwrap().len();
    assert!(
        waiting_before > 0,
        "the setup must leave a question waiting, or the assertion in the loop is vacuous"
    );

    for words in [
        HIS_OVERRIDE,
        "run the original",
        "run what was originally suggested",
        "allow the held call",
        "undo allow",
        "the original was right",
        "yes",
        "yes, do it",
        "ok",
        HIS_SYNC,
        "sync now",
        "run the sync",
        "openclaw sync",
        "re-import from external sessions",
    ] {
        r.calls(&[]);
        say(&r, &sid, words).await;
        assert!(
            rows(&r, "tool.hold_allowed").is_empty(),
            "`{words}` allowed a held call with no tool called"
        );
        assert!(
            held(&r).iter().all(|h| h.allowed.is_none()),
            "`{words}` left an allowance"
        );
        assert_eq!(
            r.core.on_demand.started(),
            before,
            "`{words}` started a sync with no tool called"
        );
        assert_eq!(
            r.core.confirm_list().unwrap().len(),
            waiting_before,
            "`{words}` answered something with no tool called"
        );
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            WAS,
            "`{words}` ran the held call"
        );
    }
    assert!(
        notices(&r, crate::outbox::OPERATOR_TARGET).is_empty(),
        "and none of them made Theseus say anything about a sync"
    );
}

/// **No chat ACT is reached by matching his text** (thread routing still
/// reads it and is its own question; see the module header). The three tools
/// are the only way in, and each is offered only where its own fact is true:
/// the answer tool where a question waits, the allow tool where a call is
/// held, and neither anywhere else. A model that is never offered a tool
/// cannot call it by accident, which is the half of "not a matching thing"
/// that lives in the catalog rather than in the turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_act_is_offered_only_where_its_own_fact_is_true() {
    let jev = jev_says(RESHAPEABLE, 0.95);
    let r = sync_rig(&jev);
    let sid = session(&r.core);
    let acts = &r.core.runner.tools.chat_acts;
    let offered = |turn: &str| -> Vec<String> {
        acts.heard.stage(
            &sid,
            crate::chat_acts::Said {
                text: "whatever he said".into(),
                who: crate::approval::Answerer::from("owner"),
                owner_input: true,
            },
        );
        acts.offers(&sid, turn, crate::places::PlaceClass::Private, false)
            .iter()
            .map(|d| d["name"].as_str().unwrap_or_default().to_string())
            .collect()
    };

    // Nothing waiting and nothing held: the sync tool alone, because the
    // import IS configured and that is its own fact.
    assert_eq!(offered("turn_0"), vec!["chat_import_sync".to_string()]);

    let _p = a_held_write(&r, &sid).await;
    let with_hold = offered("turn_1");
    assert!(
        with_hold.contains(&"chat_allow_held_call".to_string()),
        "{with_hold:?}"
    );
    assert!(
        !with_hold.contains(&"chat_answer_question".to_string()),
        "no question waits: {with_hold:?}"
    );

    // A question waits here, and now the answer tool IS offered, with the
    // question named in the description the model reads.
    r.calls(&[(
        "owner_ask",
        json!({"question": "Ship it?", "options": [{"label": "Yes"}, {"label": "No"}]}),
    )]);
    say(&r, &sid, "What should we do?").await;
    until("a question waits here", || {
        !r.core.confirm_list().unwrap().is_empty()
    })
    .await;
    let acts = &r.core.runner.tools.chat_acts;
    acts.heard.stage(
        &sid,
        crate::chat_acts::Said {
            text: "yes".into(),
            who: crate::approval::Answerer::from("owner"),
            owner_input: true,
        },
    );
    let defs = acts.offers(&sid, "turn_q", crate::places::PlaceClass::Private, false);
    let names: Vec<&str> = defs.iter().filter_map(|d| d["name"].as_str()).collect();
    assert!(
        names.contains(&"chat_answer_question"),
        "a question waits: {names:?}"
    );
    let said = defs
        .iter()
        .find(|d| d["name"] == "chat_answer_question")
        .and_then(|d| d["description"].as_str())
        .unwrap_or_default();
    assert!(
        said.contains("owner_ask") && said.contains("by its number from 1"),
        "the description names what is waiting and its answers: {said}"
    );

    // A continuation — a wake, a task's report — is offered none of them and
    // claims no message of his.
    assert!(acts
        .offers(&sid, "turn_2", crate::places::PlaceClass::Private, true)
        .is_empty());
    // Nor is a shared place.
    assert!(acts
        .offers(&sid, "turn_3", crate::places::PlaceClass::Shared, false)
        .is_empty());

    // And none of the three is ever in a place's kept catalog.
    let defs = r
        .core
        .runner
        .tools
        .definitions_for(crate::places::PlaceClass::Private.into());
    for n in crate::chat_acts::NAMES {
        let wire = theseus_tools::wire_name(n);
        assert!(
            !defs.iter().any(|d| d["name"] == wire.as_str()),
            "{n} is in the kept catalog, where it would be offered to every session of the place"
        );
    }
}

// ------------------------------- 5. the place scope, and what the turn is told

/// A sibling session that answers where `sid` answers, as a task's own asks
/// do: its parent's place, by `Outbox::show_at`.
///
/// ⛔ **`show_at`, not `bind_place`** — and that distinction is the whole
/// reason this construction was thought unavailable. `bind_place` gives a
/// place to ONE session and a second bind takes it from the first; `show_at`
/// says where a session ANSWERS, and any number of sessions may answer at one
/// place. The matchers' own cases used `show_at`, and the rule they proved
/// (`chat_acts::run::waiting_here` / `held_here`, moved here whole) is
/// unchanged, so the construction they used still works.
fn beside(core: &Arc<Core>, sid: &str) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let here = core
        .outbox
        .target(sid)
        .expect("this session answers somewhere, or there is no place to share");
    core.outbox.show_at(&rec.session_id, Some(&here)).unwrap();
    assert_eq!(
        core.outbox.target(&rec.session_id),
        core.outbox.target(sid),
        "the sibling answers where this session answers"
    );
    assert_ne!(rec.session_id, sid);
    rec.session_id
}

/// A question, as the scope rules and `stage_answer_fact` take them.
fn a_question(id: &str, tool: &str) -> theseus_protocol::ConfirmRequest {
    theseus_protocol::ConfirmRequest {
        correlation_id: id.into(),
        session_id: "ses_1".into(),
        tool: tool.into(),
        ..Default::default()
    }
}

/// Every model request that carried the turn's yes fact, in order: the block
/// is found by its own head, so nothing else in a request can pass for it.
fn facts(r: &Rig) -> Vec<String> {
    r.model
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter_map(|q| {
            q.messages.last()?["content"]
                .as_array()?
                .iter()
                .filter_map(|b| b["text"].as_str())
                .find(|t| t.starts_with(crate::yes_fact::HEAD))
                .map(str::to_string)
        })
        .collect()
}

/// **The POSITIVE half of the place scope.** A sibling session that answers
/// at this one's place is in scope here — a task's own asks and its own held
/// calls reach the owner where its parent's do — and a session at a place of
/// its own is not.
///
/// Both halves matter and only the negative one had a successor when the
/// matchers went: `waiting_here` and `held_here` moved out of
/// `rpc/answer_in_words.rs` and `rpc/allow_in_words.rs` WHOLE, filter
/// included, and no case on the branch had two sessions sharing a place, so
/// `core.outbox.target(&q.session_id) == here` was reached by nothing. A rule
/// that only ever excludes is indistinguishable from `q.session_id == session`.
///
/// The last act here is the whole-core half: the owner speaks in HIS
/// conversation, the model calls the allow tool, and the call held in the
/// SIBLING is the one allowed.
#[tokio::test(flavor = "multi_thread")]
async fn a_sibling_session_at_this_place_is_in_scope_and_one_elsewhere_is_not() {
    let jev = jev_says(RESHAPEABLE, 0.95);
    let r = rig_judged(&jev, |_| {});
    let here = session(&r.core);
    let sib_held = beside(&r.core, &here);
    let sib_ask = beside(&r.core, &here);
    let elsewhere = thread_session(&r.core, "harbour", None);

    // A call held in one sibling, a question waiting in the other. Neither
    // is this conversation's.
    let p = a_held_write(&r, &sib_held).await;
    r.calls(&[(
        "owner_ask",
        json!({"question": "Ship it?", "options": [{"label": "Yes"}, {"label": "No"}]}),
    )]);
    say(&r, &sib_ask, "What should we do there?").await;
    until("the sibling's question waits", || {
        !r.core.confirm_list().unwrap().is_empty()
    })
    .await;

    // In scope HERE, by the place they share — the assertion that had no
    // successor.
    let mine = crate::chat_acts::held_here(&r.core, &here).unwrap();
    assert_eq!(
        mine.len(),
        1,
        "the sibling's hold is in scope here: {mine:?}"
    );
    assert_eq!(mine[0].session_id, sib_held, "and it is the sibling's");
    let asked = crate::chat_acts::waiting_here(&r.core, &here).unwrap();
    assert_eq!(
        asked.len(),
        1,
        "the sibling's question is in scope here: {asked:?}"
    );
    assert_eq!(asked[0].session_id, sib_ask, "and it is the sibling's");

    // And a session at a place of its own sees neither, which is the half
    // that was already proven and is kept beside the new one.
    assert!(
        crate::chat_acts::held_here(&r.core, &elsewhere)
            .unwrap()
            .is_empty(),
        "another place sees nothing held here"
    );
    assert!(
        crate::chat_acts::waiting_here(&r.core, &elsewhere)
            .unwrap()
            .is_empty(),
        "another place sees nothing waiting here"
    );

    // Whole core: his own conversation, his own message, the model's choice —
    // and the SIBLING's hold is what the allowance names.
    r.calls(&[("chat_allow_held_call", json!({}))]);
    say(&r, &here, HIS_OVERRIDE).await;
    let allowed = rows(&r, "tool.hold_allowed");
    assert_eq!(allowed.len(), 1, "one allowance: {allowed:?}");
    assert_eq!(
        allowed[0]["hold"],
        held(&r)[0].id,
        "the hold allowed is the one held in the sibling: {allowed:?}"
    );
    assert_eq!(held(&r)[0].session_id, sib_held, "which is the sibling's");
    assert_eq!(allowed[0]["route"], "chat.allow_held_call");
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        WAS,
        "allowing it is not running it"
    );
}

/// **`[standing] yes_fact` ON: the block rides exactly this turn's request,
/// with its contents.** The producer changed with this card — it was the
/// matcher reading a yes, it is `Core::stage_answer_fact` now — and the gate
/// it feeds (`turn::compile_step`, `yes_fact::attach`) did not. The two cases
/// that proved the key's whole-core behaviour went with the matcher, so this
/// restores them against the producer that replaced it.
///
/// Both halves of the block are asserted, as they were before: what the
/// answer tool answered, and what it did NOT and is still waiting — so the
/// turn cannot read a silence as either, and an empty block could not pass.
#[tokio::test(flavor = "multi_thread")]
async fn the_turn_is_told_what_was_answered_while_the_key_is_on() {
    let jev = jev_says(REVERSIBLE, 0.95);
    let r = rig_judged(&jev, |cfg| cfg.standing.yes_fact = true);
    let sid = session(&r.core);
    let took = a_question("cor_aaaa1111", "task.close");
    let left = a_question("cor_bbbb2222", "proc.run");
    let waiting = [took.clone(), left.clone()];
    r.core.stage_answer_fact(
        &sid,
        &crate::chat_acts::named_question(&took),
        &waiting,
        &took,
    );

    r.calls(&[]);
    say(&r, &sid, "and then?").await;

    let carried = facts(&r);
    assert_eq!(carried.len(), 1, "it rode exactly one request: {carried:?}");
    let fact = &carried[0];
    assert!(fact.contains("Approved: `task.close`"), "{fact}");
    assert!(
        fact.contains("NOT approved, and still waiting (1):"),
        "the half that matters: {fact}"
    );
    assert!(fact.contains("`proc.run`"), "and it is named: {fact}");
    assert!(
        fact.contains("do not ask for it again"),
        "and what the turn is to do: {fact}"
    );

    // The next turn in the same conversation does not carry it: the fact is
    // true of one turn and stale the moment he answers again.
    r.calls(&[]);
    say(&r, &sid, "what is waiting?").await;
    assert_eq!(
        facts(&r).len(),
        1,
        "the fact rode a second turn: {:?}",
        facts(&r)
    );
}

/// **The key at its default (off) tells the turn nothing**, and BOTH gates
/// are measured, not just the one that happens to be reached first.
///
/// `stage_answer_fact` returns early when the key is off, so staging nothing
/// would prove only the producer's gate. The second half stages the block
/// directly, as the producer would have, and the turn still carries nothing:
/// that is `turn::compile_step`'s own gate at `cfg.standing.yes_fact`.
///
/// The paired positive is that the turn really ran and really made its
/// request, so a run in which nothing happened at all cannot pass this.
#[tokio::test(flavor = "multi_thread")]
async fn the_turn_is_told_nothing_while_the_key_is_off() {
    let jev = jev_says(REVERSIBLE, 0.95);
    let r = rig_judged(&jev, |_| {});
    assert!(!r.core.cfg.standing.yes_fact, "the key's default is off");
    let sid = session(&r.core);
    let took = a_question("cor_aaaa1111", "task.close");
    let left = a_question("cor_bbbb2222", "proc.run");
    r.core.stage_answer_fact(
        &sid,
        &crate::chat_acts::named_question(&took),
        &[took.clone(), left],
        &took,
    );

    r.calls(&[]);
    say(&r, &sid, "and then?").await;
    assert!(facts(&r).is_empty(), "{:?}", facts(&r));
    let made = r.model.requests.lock().unwrap().len();
    assert_eq!(
        made, 1,
        "the turn ran and made its request, carrying no block"
    );

    // The compile step's own gate: the block is staged as the producer would
    // have staged it, and the key off still keeps it out of the request.
    let block = crate::yes_fact::render(&crate::yes_fact::Decided {
        why: "and the model called the answer tool for the one it named".into(),
        approved: Some("`task.close` (…aaaa1111)".into()),
        waiting: vec!["`proc.run` (…bbbb2222)".into()],
        next: "Do not ask for it again.".into(),
    });
    // The negative below is paired with this positive: what is staged really
    // is a yes fact, by the very head `facts` looks for, so its absence from
    // the request is the key's doing and not an empty string's.
    assert!(
        block.starts_with(crate::yes_fact::HEAD),
        "a real block is staged: {block}"
    );
    r.core.runner.yes_facts.stage(&sid, block);
    r.calls(&[]);
    say(&r, &sid, "and now?").await;
    assert!(
        facts(&r).is_empty(),
        "the compile step attached a staged block with the key off: {:?}",
        facts(&r)
    );
    assert_eq!(
        r.model.requests.lock().unwrap().len(),
        2,
        "and the second turn ran too"
    );
}

/// **The answer tool's own stale-yes refusal.** `answer_ask_act` reads the
/// question's action out of the kernel and refuses when it is not there any
/// more: the direct descendant of the matcher's "no longer waiting" notice,
/// new code on this branch, and reached by nothing until now.
///
/// It is a refusal and not a silence, and nothing is answered by it.
#[tokio::test(flavor = "multi_thread")]
async fn answering_a_question_that_is_no_longer_waiting_refuses_and_answers_nothing() {
    let jev = jev_says(REVERSIBLE, 0.95);
    let r = rig_judged(&jev, |_| {});
    let gone = a_question("cor_goneaway", crate::ask::OWNER_ASK);
    let why = r
        .core
        .answer_ask_act(&gone, "1", None, crate::approval::Answerer::from("owner"))
        .expect_err("a question the kernel does not hold is not answerable");
    assert!(why.contains("no longer waiting"), "{why}");
    assert!(answers(&r).is_empty(), "nothing was answered: {why}");
}
