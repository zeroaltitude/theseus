//! The open request, per request (theseus-a1wvq.8): a message Jev judged
//! `open` at the act band may reach a model of restricted origin, carrying
//! only its open run, compiled in the open view, with a pass for that one
//! request. The rig is `tests_route`'s, with `dsflash`, a restricted profile
//! on the `zai` fake (the catalog judges it `cn`), the only profile of
//! `trivial`'s `cn_profiles`, no Haiku in `trivial`'s own list, the route
//! pack live, and the owner's material: a private context file and a
//! standing fact.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::band::Band;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::standing::{StandingAddParams, StandingKind, StandingScope};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::context_files::ContextEntry;
use crate::data::{self, Carried, Carry, Check, DataState, DataWhy, OpenRun, Taint, Writer};
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::routing::data::DataAnswer;
use crate::session::SessionRecord;
use crate::tests_judge::kinds;
use crate::tests_route::{call, decided, mode, rig_parts, Rig};
use crate::turn::TurnRequest;
use crate::Core;

pub(crate) const OWNER: u64 = crate::tests_places::OWNER;
/// The owner's private notes, a context file not marked public.
pub(crate) const SECRET: &str = "the vault code is 4417";
/// A standing fact of the owner's.
const FACT: &str = "the owner keeps bees at Hollowmere";
/// A message Jev judges private, with its marker.
pub(crate) const PRIVATE: &str = "Remind me what the Acme lease said, marker-7c1q.";
/// Messages Jev judges open.
pub(crate) const OPEN_A: &str = "What is the boiling point of water at sea level?";
pub(crate) const OPEN_B: &str = "And how many grams in a kilogram?";
pub(crate) const OPEN_C: &str = "Spell the word widget backwards.";

/// The config: `dsflash` (GLM's model on the `zai` fake, of restricted
/// origin) in `trivial`'s `cn_profiles` alone, `trivial`'s own list
/// without Haiku, the owner's DM, the private notes.
pub(crate) fn config(c: &mut crate::Config, notes: &std::path::Path) {
    c.copy_profile("dsflash", "glm");
    c.routing.classes.trivial.profiles = vec!["cheapest".into()];
    c.routing.classes.trivial.cn_profiles = vec!["dsflash".into()];
    c.routing.classes.trivial.cn_after = None;
    c.routing.trivial_context_turns = 6;
    c.places.owner = Some(vec![format!("discord:{OWNER}")]);
    c.context.files = vec![ContextEntry::Path(notes.to_string_lossy().into_owned())];
}

/// A rig as `config` says, its `zai` fake answering by `zai` when given;
/// and the fake that records what reached `dsflash`.
pub(crate) fn rig_with(
    jev: &FakeJev,
    zai: Option<Vec<Scripted>>,
    tweak: impl FnOnce(&mut crate::Config),
) -> (Rig, Arc<FakeProvider>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes.md");
    std::fs::write(&notes, format!("{SECRET}\n")).unwrap();
    let fake = zai.map(|s| Arc::new(FakeProvider::scripted(s)));
    let mine = fake.clone();
    let r = rig_parts(
        Some(jev),
        8,
        |c| {
            config(c, &notes);
            tweak(c);
        },
        move |p| {
            if let Some(f) = mine {
                p.providers.insert("zai".into(), f);
            }
        },
    );
    r.core
        .standing_add(
            &StandingAddParams {
                kind: StandingKind::Fact,
                text: FACT.into(),
                scope: StandingScope::Everywhere,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let zai = fake.unwrap_or_else(|| r.zai.clone());
    (r, zai, dir)
}

/// Jev: every message `trivial`, its data `private` at the act band, but
/// the messages in `open`, judged `open` at the act band (matched as the
/// state's `message`, never an earlier one the state quotes).
pub(crate) fn jev(open: &[&str]) -> FakeJev {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    for text in open {
        let message = format!("\"message\":\"{text}\"");
        jev.script_when(&message, None, "data", choice("open", 0.95));
    }
    jev.script("data", choice("private", 0.95));
    jev
}

pub(crate) fn choice(option: &str, confidence: f64) -> Jev {
    Jev::Choice {
        option: option.into(),
        confidence,
    }
}

/// A session posting to the owner's DM.
pub(crate) fn dm(core: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    core.outbox
        .bind_place(&format!("dm:{OWNER}"), &r.session_id)
        .unwrap();
    r.session_id
}

/// The owner's message in `sid`, on the live profile or `chosen`.
pub(crate) async fn say(
    core: &Arc<Core>,
    sid: &str,
    text: &str,
    chosen: Option<&str>,
) -> TurnSubmitResult {
    try_say(core, sid, text, chosen).await.unwrap()
}

pub(crate) async fn try_say(
    core: &Arc<Core>,
    sid: &str,
    text: &str,
    chosen: Option<&str>,
) -> anyhow::Result<TurnSubmitResult> {
    let rec = record(core, sid);
    let (live, _) = core.live_profile();
    let mut target = core.runner.resolve_target(&live, chosen, None, None)?;
    target.chosen = chosen.map(|p| format!("profile {p}"));
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(text.into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

pub(crate) fn record(core: &Core, sid: &str) -> SessionRecord {
    core.store.get_session(sid).unwrap().unwrap()
}

pub(crate) fn text(req: &ProviderRequest) -> String {
    serde_json::to_string(req).unwrap()
}

pub(crate) fn blocked(core: &Core) -> usize {
    kinds(&core.store, "data_gate.blocked").len()
}

pub(crate) fn tainted(core: &Core, sid: &str) -> Vec<Value> {
    kinds(&core.store, "session.data")
        .into_iter()
        .filter(|r| r.session_id.as_deref() == Some(sid) && r.data["row"] == "tainted")
        .map(|r| r.data)
        .collect()
}

/// The key test (replaces T18). In the owner's DM: a message judged private
/// (with its marker) and its reply, then one judged `open` at the act band
/// whose mode is `trivial`. `dsflash` gets exactly one request: the open
/// message, no node from before it, none of the owner's material; and it
/// went out with a pass for its digest (nothing was refused).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dm_open_message_reaches_dsflash_with_only_the_open_run() {
    let jev = jev(&[OPEN_A]);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    let sid = dm(&r.core);
    let first = say(&r.core, &sid, PRIVATE, None).await;
    assert_ne!(first.profile, "dsflash", "{:?}", first.route);
    assert!(zai.requests().is_empty(), "a private message stays off it");
    let res = say(&r.core, &sid, OPEN_A, None).await;
    assert_eq!(
        (res.profile.as_str(), res.model.as_str()),
        ("dsflash", "glm-5.3-flash"),
        "{:?}",
        res.route
    );
    let sent = zai.requests();
    assert_eq!(sent.len(), 1, "one request");
    let body = text(&sent[0]);
    assert!(body.contains(OPEN_A), "the open message: {body}");
    for marker in ["marker-7c1q", "Acme", SECRET, FACT] {
        assert!(!body.contains(marker), "{marker:?} reached dsflash: {body}");
    }
    assert_eq!(blocked(&r.core), 0, "its pass covered it");
    // The open reply extends the run: its turn stored it.
    let d = record(&r.core, &sid).data.unwrap();
    let run = d.run.unwrap();
    let nodes = r.core.store.transcript(&sid).unwrap();
    assert_eq!(run.through, nodes.last().unwrap().1.id);
    assert!(d.taint.is_none(), "a private-view session is never tainted");
}

/// Open, open (the run grows across the open request's own reply), private,
/// open: the last request starts at the last message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_message_in_the_dm_cuts_the_open_run() {
    let jev = jev(&[OPEN_A, OPEN_B, OPEN_C]);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    let sid = dm(&r.core);
    for m in [OPEN_A, OPEN_B, PRIVATE, OPEN_C] {
        say(&r.core, &sid, m, None).await;
    }
    let sent: Vec<String> = zai.requests().iter().map(text).collect();
    assert_eq!(sent.len(), 3, "the three open messages");
    assert!(
        sent[1].contains(OPEN_A) && sent[1].contains(OPEN_B),
        "the run grew: {}",
        sent[1]
    );
    assert!(sent[2].contains(OPEN_C));
    for m in [OPEN_A, OPEN_B, "marker-7c1q"] {
        assert!(!sent[2].contains(m), "{m:?} rode past the cut: {}", sent[2]);
    }
}

/// An admitted DM turn whose base is restricted and whose mode is not a
/// detour runs on the `[model]` default with `data_gate`; `dsflash` gets
/// nothing, and nothing reaches the chokepoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_non_detour_mode_in_a_private_view_session_never_builds_an_open_request() {
    let jev = jev(&[OPEN_A]);
    mode(&jev, "chat", 0.95);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    call(&r.core, "profile.use", json!({"name": "dsflash"})).await;
    let sid = dm(&r.core);
    let res = say(&r.core, &sid, OPEN_A, None).await;
    assert_eq!(res.profile, "default", "{:?}", res.route);
    assert_eq!(
        decided(&r.core.store).last().unwrap()["reason"],
        "data_gate"
    );
    assert!(zai.requests().is_empty());
    assert_eq!(blocked(&r.core), 0);
}

pub(crate) fn answer(choice: &str, band: Band) -> DataAnswer {
    DataAnswer {
        choice: choice.into(),
        confidence: 0.95,
        band,
    }
}

/// T7: `admits` as a unit, one assertion per reason it returns; only an
/// open answer at the act band, live, in time, with no attachment, no seal
/// and no taint, admits.
#[test]
fn fail_closed_matrix() {
    let open = answer("open", Band::Act);
    let ok = Check {
        input: "n3",
        before: Some("n2"),
        answer: Some(&open),
        late: false,
        live: true,
        sealed: false,
        attachment: false,
    };
    let run = |from: &str, through: &str| OpenRun {
        from: from.into(),
        through: through.into(),
    };
    assert_eq!(data::admits(None, &ok), Ok(run("n3", "n3")));
    let private = answer("private", Band::Act);
    let unclear = answer("unclear", Band::Act);
    let confirm = answer("open", Band::Confirm);
    for (check, why) in [
        (Check { live: false, ..ok }, DataWhy::PackNotLive),
        (
            Check {
                attachment: true,
                ..ok
            },
            DataWhy::Attachment,
        ),
        (Check { sealed: true, ..ok }, DataWhy::Sealed),
        (Check { late: true, ..ok }, DataWhy::Late),
        (Check { answer: None, ..ok }, DataWhy::NoVerdict),
        (
            Check {
                answer: Some(&private),
                ..ok
            },
            DataWhy::VerdictPrivate,
        ),
        (
            Check {
                answer: Some(&unclear),
                ..ok
            },
            DataWhy::VerdictUnclear,
        ),
        (
            Check {
                answer: Some(&confirm),
                ..ok
            },
            DataWhy::BelowAct,
        ),
    ] {
        assert_eq!(data::admits(None, &check), Err(why), "{why:?}");
    }
    // The chain: the stored run extends only over the node just before.
    let stored = DataState::new(None, Some(run("n1", "n2")), None);
    assert_eq!(data::admits(Some(&stored), &ok), Ok(run("n1", "n3")));
    let cut = DataState::new(None, Some(run("n0", "n1")), None);
    assert_eq!(data::admits(Some(&cut), &ok), Ok(run("n3", "n3")));
    // A taint closes it, whatever the answer.
    let taint = Taint {
        why: DataWhy::VerdictPrivate,
        node: None,
        at_ms: 1,
    };
    let tainted = DataState::new(None, Some(run("n1", "n2")), Some(taint));
    assert_eq!(
        data::admits(Some(&tainted), &ok),
        Err(DataWhy::VerdictPrivate)
    );
    // A turn with no new input: the stored run while its last is the last.
    assert_eq!(
        data::admits_again(Some(&stored), Some("n2")),
        Ok(run("n1", "n2"))
    );
    assert_eq!(
        data::admits_again(Some(&stored), Some("n9")),
        Err(DataWhy::Unvetted)
    );
    assert_eq!(
        data::admits_again(None, Some("n2")),
        Err(DataWhy::NoVerdict)
    );
}

/// T7's other half: `clearance` clears only the open view with every node
/// in the run and nothing but the loop's own past its last.
#[test]
fn fail_closed_matrix_of_the_pass() {
    let run = |from: &str, through: &str| OpenRun {
        from: from.into(),
        through: through.into(),
    };
    let node = |id: &str, at: u64, writer: Writer| Carry {
        id: id.into(),
        at,
        writer,
    };
    let r = run("n3", "n3");
    let good = Carried {
        open_view: true,
        from: Some(30),
        nodes: vec![node("n3", 30, Writer::Other), node("a3", 31, Writer::Loop)],
    };
    let c = data::clearance(&r, &good);
    assert!(c.is_cleared() && c.pass("d").unwrap().covers("d"));
    let with = |nodes: Vec<Carry>| Carried {
        nodes,
        ..good.clone()
    };
    for (carried, why) in [
        (
            Carried {
                open_view: false,
                ..good.clone()
            },
            DataWhy::PrivateView,
        ),
        (
            with(vec![
                node("n2", 20, Writer::Other),
                node("n3", 30, Writer::Other),
            ]),
            DataWhy::NotInRun,
        ),
        (
            Carried {
                from: None,
                ..good.clone()
            },
            DataWhy::NotInRun,
        ),
        (with(vec![node("a3", 31, Writer::Loop)]), DataWhy::NotInRun),
        (
            with(vec![
                node("n3", 30, Writer::Other),
                node("r3", 32, Writer::ToolResult),
            ]),
            DataWhy::ToolResult,
        ),
        (
            with(vec![
                node("n3", 30, Writer::Other),
                node("w3", 32, Writer::Other),
            ]),
            DataWhy::Unvetted,
        ),
    ] {
        let c = data::clearance(&r, &carried);
        assert_eq!(c.why(), Some(why), "{carried:?}");
        assert!(c.pass("d").is_none());
    }
}

/// A session opened as an open task's is (`data.opened`), on the live
/// profile, which is `dsflash`; Jev unsure of every message's mode, so the
/// session's own profile stays.
pub(crate) async fn open_session(r: &Rig) -> String {
    call(&r.core, "profile.use", json!({"name": "dsflash"})).await;
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    crate::tests_open_view::open(&r.core, &rec.session_id, None);
    rec.session_id
}

/// 8b (review F1 of cn-open-view): once an open session is tainted, nothing
/// at or after its taint reaches a restricted model: `admits` refuses it, so
/// the turn runs on the `[model]` default and never meets the chokepoint.
/// The second session's taint leaves its run whole (as a fork's copy of one
/// does), so only the taint stands between its next message and dsflash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tainted_open_task_sends_nothing_after_its_taint_to_a_restricted_model() {
    let jev = jev(&[OPEN_A, OPEN_B]);
    mode(&jev, "chat", 0.2);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    let sid = open_session(&r).await;
    let first = say(&r.core, &sid, PRIVATE, None).await;
    assert_eq!(first.profile, "default", "{:?}", first.route);
    let d = record(&r.core, &sid).data.unwrap();
    assert_eq!(d.taint.unwrap().why, DataWhy::VerdictPrivate);
    let after = say(&r.core, &sid, OPEN_A, None).await;
    assert_eq!(after.profile, "default", "{:?}", after.route);
    assert!(zai.requests().is_empty(), "nothing after the taint");

    let sid = open_session(&r).await;
    assert_eq!(say(&r.core, &sid, OPEN_A, None).await.profile, "dsflash");
    assert!(record(&r.core, &sid).data.unwrap().run.is_some());
    r.core
        .store
        .update_session(&sid, |rec| {
            let d = rec.data.as_mut().unwrap();
            d.taint = Some(Taint {
                why: DataWhy::ToolResult,
                node: None,
                at_ms: 1,
            });
            Ok(vec![])
        })
        .unwrap();
    let after = say(&r.core, &sid, OPEN_B, None).await;
    assert_eq!(after.profile, "default", "{:?}", after.route);
    assert_eq!(zai.requests().len(), 1, "nothing after the taint");
    assert_eq!(blocked(&r.core), 0, "refused before the chokepoint");
}

/// T8: a taint is bookkeeping. An open session's private message writes one
/// `tainted` row, and a second writes none; the turns run on a Claude
/// model as before; a request's clearance reads no taint, so an admitted
/// request elsewhere is still cleared over its own run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_taint_is_bookkeeping() {
    let jev = jev(&[OPEN_A]);
    mode(&jev, "chat", 0.2);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    let sid = open_session(&r).await;
    for _ in 0..2 {
        let res = say(&r.core, &sid, PRIVATE, None).await;
        assert!(res.output.starts_with("Done"), "{}", res.output);
    }
    let rows = tainted(&r.core, &sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["why"], "verdict_private");
    assert!(zai.requests().is_empty());
    // The DM beside it is not tainted: its open message still reaches dsflash.
    call(&r.core, "profile.use", json!({"name": "sonnet"})).await;
    let dm = dm(&r.core);
    mode(&jev, "trivial", 0.95);
    say(&r.core, &dm, OPEN_A, None).await;
    assert_eq!(zai.requests().len(), 1, "cleared over its own run");
}

/// T10: a node no blessed writer wrote, after the run's last, breaks the
/// chain: the next open message's run starts at it, and an open session's
/// turn with no new input after such a node is `unvetted`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_writer_breaks_the_chain() {
    let jev = jev(&[OPEN_A, OPEN_B]);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    let sid = dm(&r.core);
    say(&r.core, &sid, OPEN_A, None).await;
    let run = record(&r.core, &sid).data.unwrap().run.unwrap();
    // A wake's note, say: nobody blessed it.
    let mut n = crate::node::Node::user(&sid, None, "harness", "A wake fired: unknown-writer-3f.");
    n.origin = crate::node::Origin::Harness;
    r.core.store.append(&[n.record().unwrap()]).unwrap();
    say(&r.core, &sid, OPEN_B, None).await;
    let sent: Vec<String> = zai.requests().iter().map(text).collect();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].contains(OPEN_B));
    for m in [OPEN_A, "unknown-writer-3f"] {
        assert!(!sent[1].contains(m), "{m:?} rode the broken chain");
    }
    let stored = DataState::new(None, Some(run), None);
    assert_eq!(
        data::admits_again(Some(&stored), Some(&n.id)),
        Err(DataWhy::Unvetted)
    );
}

/// T14, cleared against uncleared: a turn not admitted never reaches the
/// restricted fake on any path (its base, a pin, a trivial detour; holds
/// and corrections are `tests_data_floor`'s, unchanged), and a cleared
/// trivial detour does, with its pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_uncleared_turn_never_reaches_dsflash_and_a_cleared_detour_does() {
    // Open, but only at the confirm band: never admitted.
    let jev = FakeJev::start().unwrap();
    jev.script("data", choice("open", 0.7));
    mode(&jev, "trivial", 0.95);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    let detour = say(&r.core, &dm(&r.core), OPEN_A, None).await;
    assert_ne!(detour.profile, "dsflash");
    let pinned = say(&r.core, &dm(&r.core), OPEN_A, Some("dsflash")).await;
    assert_eq!(pinned.profile, "default", "{:?}", pinned.route);
    mode(&jev, "chat", 0.2);
    call(&r.core, "profile.use", json!({"name": "dsflash"})).await;
    let base = say(&r.core, &dm(&r.core), OPEN_A, None).await;
    assert_eq!(base.profile, "default", "{:?}", base.route);
    assert!(zai.requests().is_empty());
    assert_eq!(blocked(&r.core), 0);

    let jev = jev_open_trivial();
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    let res = say(&r.core, &dm(&r.core), OPEN_A, None).await;
    assert_eq!(res.profile, "dsflash");
    assert_eq!(zai.requests().len(), 1);
    assert_eq!(blocked(&r.core), 0, "it carried its pass");
}

fn jev_open_trivial() -> FakeJev {
    jev(&[OPEN_A])
}

/// Item 3's last point: an open detour's later loop, after a tool's result
/// (nothing is screened yet), leaves `dsflash` for the `[model]` default with
/// `data_gate`, as a plain detour; the turn stores no run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_loop_after_a_tool_result_leaves_the_restricted_model() {
    let jev = jev(&[OPEN_A]);
    let calls = vec![
        Scripted::tools(
            "Looking.",
            &[("f1", "tools_find", json!({"query": "clock"}))],
        ),
        Scripted::text("Done: from dsflash."),
    ];
    let (r, zai, _dir) = rig_with(&jev, Some(calls), |_| {});
    let sid = dm(&r.core);
    let res = say(&r.core, &sid, OPEN_A, None).await;
    assert_eq!(zai.requests().len(), 1, "the first loop alone");
    assert_eq!(res.profile, "default", "{:?}", res.route);
    let last = decided(&r.core.store);
    assert_eq!(last.last().unwrap()["reason"], "data_gate", "{last:?}");
    let to_claude = r.claude.requests();
    let after = text(to_claude.last().unwrap());
    assert!(
        after.contains("tools_find") || after.contains("f1"),
        "{after}"
    );
    assert_eq!(blocked(&r.core), 0);
    let run = record(&r.core, &sid).data.and_then(|d| d.run);
    assert!(run.is_none(), "{run:?}");
}

/// The rig with the driver taking tasks' turns, its Home last on Sonnet.
pub(crate) fn task_rig(
    jev: &FakeJev,
    tweak: impl FnOnce(&mut crate::Config),
) -> (Rig, Arc<FakeProvider>, tempfile::TempDir) {
    let (r, zai, dir) = rig_with(jev, None, tweak);
    tokio::spawn(crate::harness::drive(r.core.clone()));
    r.core.home().unwrap();
    (r, zai, dir)
}

pub(crate) fn open_task(
    brief: &str,
    profile: &str,
) -> theseus_protocol::task_start::TaskStartParams {
    theseus_protocol::task_start::TaskStartParams {
        open: true,
        ..crate::tests_task_brief::params(brief, Some(profile))
    }
}

/// T17: an open task on `dsflash` whose brief Jev judged open reaches it in
/// the open view with a pass, and its turn stores its run; a private node
/// after an open one ends the run: the turns after it run on the `[model]`
/// default, and it is the session's taint.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_task_reaches_dsflash_and_a_private_node_ends_its_run() {
    let jev = jev(&[OPEN_A, OPEN_B, OPEN_C]);
    mode(&jev, "chat", 0.2);
    let (r, zai, _dir) = task_rig(&jev, |_| {});
    let (started, rec) = crate::tests_task_brief::run(&r, &open_task(OPEN_A, "dsflash")).await;
    let sid = started.session_id;
    let sent = zai.requests();
    assert_eq!(sent.len(), 1, "the brief's turn");
    let body = text(&sent[0]);
    assert!(body.contains(OPEN_A));
    for marker in [SECRET, FACT] {
        assert!(!body.contains(marker), "{marker:?}: {body}");
    }
    assert_eq!(blocked(&r.core), 0);
    let d = rec.data.unwrap();
    let nodes = r.core.store.transcript(&sid).unwrap();
    assert_eq!(
        d.run,
        Some(OpenRun {
            from: nodes[0].1.id.clone(),
            through: nodes.last().unwrap().1.id.clone(),
        })
    );
    assert!(d.taint.is_none());
    // A task's session takes no message once its execution is complete, so
    // the rest runs in a session opened as an open task's is: its open
    // message reaches dsflash over its run, then a private one ends it.
    let sid = open_session(&r).await;
    for (m, on) in [
        (OPEN_B, "dsflash"),
        (PRIVATE, "default"),
        (OPEN_C, "default"),
    ] {
        let res = say(&r.core, &sid, m, None).await;
        assert_eq!(res.profile, on, "{m}: {:?}", res.route);
    }
    let sent = zai.requests();
    assert_eq!(sent.len(), 2, "the brief's, and the open message's");
    assert!(text(&sent[1]).contains(OPEN_B));
    let d = record(&r.core, &sid).data.unwrap();
    assert_eq!(d.taint.map(|t| t.why), Some(DataWhy::VerdictPrivate));
    assert_eq!(tainted(&r.core, &sid).len(), 1);
    assert_eq!(blocked(&r.core), 0);
}

/// T20, the clearance half: a brief judged open clears an open task's first
/// request, though its verdict came past `[routing] max_wait_ms`; a brief
/// judged private leaves it on the `[model]` default, with its taint row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_tasks_brief_is_judged_before_its_first_compile() {
    let jev = jev(&[OPEN_A]);
    mode(&jev, "chat", 0.2);
    let slow = std::time::Duration::from_millis(600);
    jev.set_latency(slow, slow);
    let (r, zai, _dir) = task_rig(&jev, |c| c.routing.max_wait_ms = 100);
    let (open, _) = crate::tests_task_brief::run(&r, &open_task(OPEN_A, "dsflash")).await;
    assert_eq!(zai.requests().len(), 1, "waited past max_wait_ms, cleared");
    assert!(tainted(&r.core, &open.session_id).is_empty());
    let (private, _) = crate::tests_task_brief::run(&r, &open_task(PRIVATE, "dsflash")).await;
    assert_eq!(zai.requests().len(), 1, "a private brief stays off it");
    let rows = tainted(&r.core, &private.session_id);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["why"], "verdict_private");
    let routed = decided(&r.core.store);
    assert!(
        routed.iter().any(|d| d["reason"] == "data_gate"),
        "{routed:?}"
    );
}

/// Item 7 (review F4 of cn-open-tasks): an open task takes its parent's
/// hold with no URL and no query, so its reason, which reaches the task's
/// model, names only the tool and the time; a task that is not open keeps
/// both.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_tasks_hold_names_no_url_or_query() {
    let jev = jev(&[]);
    let (r, _zai, _dir) = task_rig(&jev, |_| {});
    let home = r.core.home().unwrap().session_id;
    let url = "https://example.com/widgetco-ledger";
    let query = "widgetco quarterly ledger";
    r.core
        .store
        .update_session(&home, |rec| {
            let mut h = crate::external::read("nod_web1", "web.search", url, Some(query), 1);
            h.via = None;
            rec.external = Some(h);
            Ok(vec![])
        })
        .unwrap();
    for open in [true, false] {
        let p = theseus_protocol::task_start::TaskStartParams {
            open,
            ..crate::tests_task_brief::params("Count the quay's bollards.", Some("sonnet"))
        };
        let started = r
            .core
            .task_start(&p, crate::tests_task_brief::cli())
            .unwrap();
        let h = record(&r.core, &started.session_id).external.unwrap();
        let why = crate::external::why(&h, crate::external::Mode::Ask);
        let named = why.contains(url) || why.contains(query);
        assert_eq!(named, !open, "open {open}: {why}");
        assert_eq!((h.url.is_empty(), h.query.is_none()), (open, open));
        assert_eq!(h.tool, "web.search", "the hold still names its tool");
    }
}

/// Item 8 (review F3 of cn-open-tasks): a brief's `derived_from` edge to its
/// parent's reply spreads both ways, but an open request's recall reads every
/// hit by `drawn_place`, the spread's included: the parent, a private
/// session, is drawn on by none. A private asker's spread does reach it.
#[tokio::test]
async fn an_open_tasks_recall_never_spreads_into_its_parent() {
    use crate::config::memory::MemoryArm;
    use crate::tests_recall::{index_of, recalls, rig_with, session, turn};
    const BRIEF: &str = "Find what fixed the Kestrel relay for the harbour.";
    const REPLY: &str = "The relay was fixed by the parent-secret-9k patch.";
    let r = rig_with(crate::config::MemoryMode::Live, |c| {
        c.memory.arm = MemoryArm::Activation;
        c.memory.recall_deadline_ms = crate::config::memory::MAX_RECALL_DEADLINE_MS;
    });
    let c = &r.core;
    let parent = session(c, None, &[REPLY]);
    let task = session(c, None, &[BRIEF]);
    crate::tests_open_view::open(c, &task, None);
    let only = |sid: &str| c.store.session_nodes(sid).unwrap().remove(0).1;
    let (reply, brief) = (only(&parent), only(&task));
    crate::tests_activation::edge(
        &c.store,
        crate::graph::EdgeKind::DerivedFrom,
        &brief,
        &reply,
        crate::graph::VIA_BRIEF,
    );
    c.runner.memory.set_ask(index_of(c, vec![task.clone()]));
    c.runner.memory.adjacency.build(&c.store, false).unwrap();
    let drew = |sid: &str| -> Vec<String> {
        let m = recalls(c, sid).pop().unwrap();
        m.admitted.iter().map(|a| a.session_id.clone()).collect()
    };
    let open = session(c, None, &[]);
    crate::tests_open_view::open(c, &open, None);
    turn(c, &open, "What fixed the Kestrel relay?").await;
    let got = drew(&open);
    assert!(got.contains(&task), "the open brief is drawn: {got:?}");
    assert!(
        !got.contains(&parent),
        "the spread reached the parent: {got:?}"
    );
    let private = session(c, None, &[]);
    turn(c, &private, "What fixed the Kestrel relay?").await;
    let got = drew(&private);
    assert!(
        got.contains(&parent),
        "a private asker's spread does: {got:?}"
    );
}
