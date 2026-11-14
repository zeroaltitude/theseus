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
use crate::judge::undo::RESHAPEABLE;
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;
use crate::test_support::until;
use crate::tests_task_graph::{rig_on, session, Rig};
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

/// A Jev that holds a plain write (`reshapeable`) and answers the chat acts'
/// own second reading at `says_run`.
fn jev_says(says_run: f64) -> FakeJev {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "reversible",
        Jev::Choice {
            option: RESHAPEABLE.into(),
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
    let r = rig_on(tempfile::tempdir().unwrap(), |c| {
        c.judge.enabled = true;
        c.judge.api_base = jev.base();
        c.judge.connect_secs = 1;
        c.judge.total_secs = 3;
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
        c.judge
            .packs
            .remove(crate::chat_acts::judged::PACK);
        tweak(c);
    });
    // The Jev client is built lazily; a test that counts its requests must
    // warm it first, as `toolrun::tests_undo`'s rig does.
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
    let jev = jev_says(0.95);
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
    assert_eq!(std::fs::read_to_string(&p).unwrap(), NOW, "the original ran");
    let spent = rows(&r, "tool.hold_overridden");
    assert_eq!(spent.len(), 1, "one allowance spent, once: {spent:?}");
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
    let jev = jev_says(0.05);
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
    assert_eq!(std::fs::read_to_string(&p).unwrap(), WAS, "and it never ran");
}

/// **The same with the judge unavailable: the call STANDS, and the row says it
/// was not judged.** This is the fail-open rule, measured. Do not "fix" it:
/// S17-D6 is the owner's standing condition that he can always say *run what
/// was originally suggested*, and a judge that refused the act whenever it
/// could not be reached would let a network fault remove that capability.
#[tokio::test(flavor = "multi_thread")]
async fn with_the_judge_unavailable_the_call_stands_and_the_row_says_it_was_not_judged() {
    let jev = jev_says(0.95);
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
    let jev = jev_says(0.95);
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
    let jev = jev_says(0.05);
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
        crate::turn::repeat_step::MAX_REPEATS, 3,
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
    let jev = jev_says(0.95);
    let r = rig_judged(&jev, |c| {
        c.policy
            .tools
            .insert("fs.write".into(), crate::policy::Posture::Approve);
    });
    let here = session(&r.core);
    let elsewhere = thread_session(&r.core, "harbour", None);

    let a = target(&r);
    let b = a.with_file_name("elsewhere.txt");
    r.calls(&[("fs_write", json!({"path": b.to_string_lossy(), "content": NOW}))]);
    say(&r, &elsewhere, "Write the other one.").await;
    r.calls(&[("fs_write", write_call(&a))]);
    say(&r, &here, "Replace the notes.").await;
    until("two questions wait", || r.core.confirm_list().unwrap().len() == 2).await;
    let waiting = r.core.confirm_list().unwrap();
    assert_eq!(waiting.len(), 2, "two questions wait: {waiting:?}");

    // His reply, with no tool called: nothing is answered.
    r.calls(&[]);
    say(&r, &here, "yes").await;
    assert_eq!(
        r.core.confirm_list().unwrap().len(),
        2,
        "a bare yes answers nothing now"
    );

    // His reply, and the model chooses the tool.
    r.calls(&[("chat_answer_question", json!({"answer": "yes"}))]);
    say(&r, &here, "yes, go ahead").await;

    let out = acted(&r, &here, crate::chat_acts::ANSWER);
    assert_eq!(out.len(), 1, "the tool ran: {out:?}");
    assert_eq!(out[0].0, "Ok", "{out:?}");
    assert!(out[0].1.contains("Approved"), "{}", out[0].1);
    assert!(
        out[0].1.contains("Nothing else waiting here was answered"),
        "{}",
        out[0].1
    );

    until("the approved call ran", || {
        std::fs::read_to_string(&a).unwrap_or_default() == NOW
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(&a).unwrap(),
        NOW,
        "the approved call ran"
    );
    let still = r.core.confirm_list().unwrap();
    assert_eq!(
        still.len(),
        1,
        "the other conversation's question still waits: {still:?}"
    );
    assert_eq!(still[0].session_id, elsewhere);
    assert!(!b.exists(), "and its call did not run");
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
        .filter_map(|a| crate::outbox::body_of(a)["text"].as_str().map(str::to_string))
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
    let jev = jev_says(0.95);
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
        let last = out.last().unwrap_or_else(|| panic!("no result in {what}: {out:?}"));
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
/// sync sentence `sync_in_words` would have read. A question is waiting, a
/// call is held and the import is configured, so all three routes have
/// something to act on. The model calls NOTHING.
///
/// Nothing happens. No allowance, no answer, no sync, no notice.
#[tokio::test(flavor = "multi_thread")]
async fn a_message_holding_an_old_trigger_phrase_does_nothing_unless_a_tool_is_called() {
    let jev = jev_says(0.95);
    let r = sync_rig(&jev);
    let sid = session(&r.core);
    let p = a_held_write(&r, &sid).await;
    let before = r.core.on_demand.started();
    let waiting_before = r.core.confirm_list().unwrap().len();

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

/// **No code path still matches his text.** The three tools are the only way
/// in, and each is offered only where its own fact is true: the answer tool
/// where a question waits, the allow tool where a call is held, and neither
/// anywhere else. A model that is never offered a tool cannot call it by
/// accident, which is the half of "not a matching thing" that lives in the
/// catalog rather than in the turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_chat_act_is_offered_only_where_its_own_fact_is_true() {
    let jev = jev_says(0.95);
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
    assert!(with_hold.contains(&"chat_allow_held_call".to_string()), "{with_hold:?}");
    assert!(
        !with_hold.contains(&"chat_answer_question".to_string()),
        "no question waits: {with_hold:?}"
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
