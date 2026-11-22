//! The view's sources over a real store (issue 44; `view::sources_names`): what a message names reaches a
//! turn in a place set to `view` as its thing's card, through the turn's own gate. A thing the people
//! present may not see is no name, so it brings nothing and is counted nowhere.

use std::sync::Arc;

use theseus_protocol::index::{IndexNeighboursParams, IndexNeighboursResult};

use crate::config::assembly::{Assembly, AssemblySetting};
use crate::exchange::{self, Root};
use crate::memory_pass::{IndexFuture, PassIndex};
use crate::tender::TenderMiss;
use crate::tests_private_view::session_in;
use crate::tests_view_compile::{asked, kelp, texts, turn_as, view_rig};
use crate::view::select::Band;
use crate::view::Space;

/// An index that names "issue 33" as a numbered item, as the tender's extractor would.
struct Items;

impl PassIndex for Items {
    fn neighbours(&self, _: IndexNeighboursParams) -> IndexFuture<IndexNeighboursResult> {
        Box::pin(async { Err(TenderMiss::Down("a test's index".into())) })
    }

    fn entities(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        Box::pin(async move { Ok(vec![Vec::new(); texts.len()]) })
    }

    fn items(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        let named = |t: &String| {
            (t.contains("issue 33").then(|| "item:issue-33".to_string()))
                .into_iter()
                .collect()
        };
        let out = texts.iter().map(named).collect();
        Box::pin(async move { Ok(out) })
    }

    fn mode(&self) -> IndexFuture<String> {
        Box::pin(async { Ok("bm25_only".into()) })
    }
}

/// Until the naming task has touched `term`, a bounded wait.
async fn touched(store: &crate::store::Store, term: &str) {
    for _ in 0..50 {
        if !(exchange::newest(store, Root::Thing(term), 5).unwrap()).is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("{term} was never touched");
}

/// The pieces of a named thing's card the turn's view chose, showed or dropped, and the residue of the
/// bands that call things.
fn cards_counted(store: &crate::store::Store, turn: &str) -> usize {
    let records = crate::view::record::of_turn(store, turn).unwrap();
    assert!(!records.is_empty(), "the turn was a view turn");
    let card = |s: &Space| matches!(s, Space::Card { .. });
    (records.iter())
        .map(|r| {
            r.shown
                .iter()
                .filter(|s| card(&s.piece.range.space))
                .count()
                + r.dropped.iter().filter(|d| card(&d.range.space)).count()
                + (r.residue.iter())
                    .filter(|x| matches!(x.band, Band::Named | Band::InPlay))
                    .count()
        })
        .sum()
}

/// A session of the desk that serves `project`: its numbered items are scoped by it.
fn serving(c: &crate::Core, root: &std::path::Path, project: &str) -> String {
    let sid = session_in(c, None, root);
    (c.store)
        .update_session(&sid, |s| {
            s.project = Some(project.into());
            Ok(vec![])
        })
        .unwrap();
    sid
}

const SCOPED: &str = "item:project:prj_tide#issue-33";

/// The lines of the card whose title ends with `(<thing>)`, in `all` (a request as JSON: its newlines and
/// quotes are escaped, and read back here).
fn card_of(all: &str, thing: &str) -> Option<String> {
    let at = all.find(&format!("({thing})"))?;
    let lines: Vec<&str> = all[at..].split("\\n").take(6).collect();
    Some(lines.join("\n").replace("\\\"", "\""))
}

/// KEY: a message naming a numbered item brings the item's card beside it, scoped by the project of the
/// place it was said in: the words of the exchange it was said in before. An unscoped "issue 33" (a session
/// that serves no project) is a candidate, never one thing: it brings no card, and its words are not on the
/// scoped item's card.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_naming_a_numbered_item_brings_its_card() {
    let r = view_rig();
    let c = &r.core;
    c.runner.pass.set_index(Arc::new(Items));
    let loose = session_in(c, None, &r.root);
    turn_as(c, &loose, "the harbour log: issue 33 is the leak", true).await;
    touched(&c.store, "item:issue-33").await;
    let sid = serving(c, &r.root, "prj_tide");
    turn_as(c, &sid, "the tide gauge: see issue 33 first", true).await;
    touched(&c.store, SCOPED).await;
    let t = turn_as(c, &sid, "and where is issue 33 now?", true).await;
    let all = texts(&asked(&r, "and where is issue 33 now?").pop().unwrap());
    let card = card_of(&all, SCOPED).unwrap_or_else(|| panic!("no card: {all}"));
    assert!(
        card.contains("\"the tide gauge: see issue 33 first\""),
        "{card}"
    );
    assert!(
        !card.contains("harbour log"),
        "the unscoped one stays apart: {card}"
    );
    assert!(cards_counted(&c.store, &t.turn_id) >= 1);

    let t = turn_as(c, &loose, "and where is issue 33 today?", true).await;
    let all = texts(&asked(&r, "and where is issue 33 today?").pop().unwrap());
    assert!(
        !all.contains("(item:"),
        "an unscoped item is no one thing: {all}"
    );
    assert_eq!(cards_counted(&c.store, &t.turn_id), 0);
}

/// KEY: an item whose message was moved into a sealed thread brings no excerpt and selects no card (the
/// node seal, `view_gate::Moved`, is asked before its words are read or counted).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_item_whose_ask_was_moved_into_a_sealed_thread_brings_nothing() {
    let r = kelp_rig(true);
    let c = &r.core;
    c.runner.pass.set_index(Arc::new(Items));
    let sid = serving(c, &r.root, "prj_tide");
    let first = turn_as(c, &sid, "the tide gauge: see issue 33 first", true).await;
    touched(&c.store, SCOPED).await;
    crate::ladder::tests_moved::move_ask(&c.store, &first.turn_id, "kelp");
    let t = turn_as(c, &sid, "and where is issue 33 now?", true).await;
    let all = texts(&asked(&r, "and where is issue 33 now?").pop().unwrap());
    assert!(card_of(&all, SCOPED).is_none(), "{all}");
    assert!(!all.contains("tide gauge"), "{all}");
    assert_eq!(cards_counted(&c.store, &t.turn_id), 0);
}

/// A card keeps each excerpt's author: words an agent's process typed (no person's mark) are outside text
/// inside the owner's card, never his.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cards_excerpt_keeps_its_author() {
    let r = view_rig();
    let c = &r.core;
    c.runner.pass.set_index(Arc::new(Items));
    let sid = serving(c, &r.root, "prj_tide");
    turn_as(c, &sid, "agent: see issue 33 first", false).await;
    touched(&c.store, SCOPED).await;
    turn_as(c, &sid, "and where is issue 33 now?", true).await;
    let all = texts(&asked(&r, "and where is issue 33 now?").pop().unwrap());
    let card = card_of(&all, SCOPED).unwrap_or_else(|| panic!("no card: {all}"));
    assert!(
        card.contains("author unknown (not the owner's instruction)"),
        "{card}"
    );
    assert!(!card.contains("\"agent: see issue 33 first\""), "{card}");
}

/// A rig in `view` with threads on and the thread `kelp`, sealed as `seal` says.
fn kelp_rig(seal: bool) -> crate::tests_places::Rig {
    crate::tests_places::rig_setup(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| {
            kelp(s, seal);
        },
    )
}

const KELP_CARD: &str = "Line of work: kelp";

/// KEY: a line of work named by its key brings its card to the owner alone; the same name said in a
/// channel (never alone, decision 5) is of another place the owner is not alone in, and brings nothing,
/// not even a count.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_of_another_place_brings_nothing_where_the_owner_is_not_alone() {
    let r = kelp_rig(false);
    let c = &r.core;
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: what about thread:kelp?", true).await;
    let q = asked(&r, "desk: what about thread:kelp?").pop().unwrap();
    assert!(texts(&q).contains(KELP_CARD), "{}", texts(&q));

    c.bind_places(vec![crate::places::BoundPlace {
        persona: None,
        target: format!("discord:channel:{}", crate::tests_places::LAB),
        name: "#lab".into(),
        private: true,
        guild: Some("100000000000000001".into()),
        ceiling: None,
    }]);
    let lab = format!("channel:{}", crate::tests_places::LAB);
    let channel = crate::tests_places::session(c, Some(&lab));
    let t = turn_as(c, &channel, "lab: what about thread:kelp?", true).await;
    let q = asked(&r, "lab: what about thread:kelp?").pop().unwrap();
    assert!(!texts(&q).contains(KELP_CARD), "{}", texts(&q));
    assert_eq!(cards_counted(&c.store, &t.turn_id), 0);
}

/// KEY: a sealed thread's thing is not named: its card is absent, and nothing of it is counted in the
/// view's record, shown, dropped or as residue.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sealed_threads_thing_is_absent_and_uncounted() {
    let r = kelp_rig(true);
    let c = &r.core;
    let desk = session_in(c, None, &r.root);
    let t = turn_as(c, &desk, "desk: what about thread:kelp?", true).await;
    let q = asked(&r, "desk: what about thread:kelp?").pop().unwrap();
    assert!(!texts(&q).contains(KELP_CARD), "{}", texts(&q));
    assert_eq!(cards_counted(&c.store, &t.turn_id), 0);
}

/// A question for the owner's answer, parked by a turn of session `sid`, as a tool the policy stopped for
/// approval parks it.
fn plant(core: &crate::Core, sid: &str) -> String {
    use serde_json::{json, Value};
    use theseus_kernel::{Authority, Proposal, RetryClass, TurnEnd, Wake};
    let k = &core.kernel;
    let sid = sid.to_string();
    let mut rec = (core.store)
        .get_session::<crate::session::SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    let operator = Authority {
        principal: crate::turn::OPERATOR.into(),
        ..Default::default()
    };
    let e = k
        .open_execution(&sid, rec.kind, operator, None, None)
        .unwrap();
    rec.execution_id = Some(e.id.clone());
    core.store.put_session(&sid, &rec).unwrap();
    k.wake_input(&e.id).unwrap();
    let guard = k.admit(&e.id).unwrap();
    let p = Proposal {
        tool: "aws.stack.apply".into(),
        args: json!({"stack": "example-harbour"}),
        resource: None,
        policy_context: Value::Null,
    };
    let a = k
        .plan_confirm(&guard, &p, RetryClass::NonRepeatable, Some(600_000))
        .unwrap();
    let wake = Wake::Confirm {
        confirm_id: a.correlation_id.clone(),
    };
    k.end_turn(guard, TurnEnd::Wait { wake }).unwrap();
    a.correlation_id
}

/// KEY: a question waiting for the owner's answer is listed beside his message, by its id and one line,
/// and once he has answered it, it is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_waiting_confirmation_is_listed_and_disappears_when_answered() {
    let r = view_rig();
    let c = &r.core;
    let question = plant(c, &session_in(c, None, &r.root));
    let line = format!("{question}: waiting for your answer: aws.stack.apply");
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: anything for me?", true).await;
    let q = asked(&r, "desk: anything for me?").pop().unwrap();
    assert!(texts(&q).contains(&line), "{}", texts(&q));

    c.confirm_action(&question, false, Some("not now"), "cli")
        .unwrap();
    turn_as(c, &desk, "desk: and now?", true).await;
    let q = asked(&r, "desk: and now?").pop().unwrap();
    assert!(!texts(&q).contains(&question), "{}", texts(&q));
}

/// KEY: a question parked by a sealed thread's turn is not listed, and the view does not depend on its
/// session: the turn is not compiled again and again for it (no `moved` fallback).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sealed_threads_pending_confirmation_is_absent_and_harmless() {
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| *thread.borrow_mut() = kelp(s, true),
    );
    let c = &r.core;
    let question = plant(c, &thread.into_inner());
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: anything for me?", true).await;
    let q = asked(&r, "desk: anything for me?").pop().unwrap();
    assert!(!texts(&q).contains(&question), "{}", texts(&q));
    let why = crate::tests_view_compile::fallbacks(c);
    assert!(!why.iter().any(|w| w == "moved"), "{why:?}");
}

/// A turn's newest view record.
fn record_of(c: &crate::Core, turn: &str) -> crate::view::record::ViewRecord {
    (crate::view::record::of_turn(&c.store, turn).unwrap())
        .into_iter()
        .next()
        .expect("a view record")
}

/// The owner's own reading of `rec` today, as `theseus view show` reads it.
fn owners_reading(
    c: &crate::Core,
    rec: &crate::view::record::ViewRecord,
) -> theseus_protocol::view::ViewShown {
    use crate::view::record::show::Reader;
    use crate::view::record::source::GateSource;
    use theseus_memory::gate::{PointOfView, Present};
    use theseus_memory::recall::Place;
    let anchor = match &rec.pov.anchor {
        crate::view::Anchor::Place(p) => p.clone(),
        crate::view::Anchor::Task(t) => format!("task:{t}"),
    };
    let source = GateSource {
        runner: &c.runner,
        pov: PointOfView {
            anchor,
            place: Place::Private,
            present: Present::OwnerAlone,
        },
        asker: None,
    };
    let reader = Reader {
        source: &source,
        text: None,
        restricted: false,
    };
    reader.show(rec)
}

fn counted(v: &theseus_protocol::view::ViewShown) -> u64 {
    v.totals.shown + v.totals.dropped
}

fn lists(v: &theseus_protocol::view::ViewShown, what: &str) -> bool {
    serde_json::to_string(v).unwrap().contains(what)
}

/// KEY: a view's record re-checks a card's sources before it shows or counts it: the owner's own reading
/// of a turn that was shown a thread's card no longer lists it once the thread is sealed, and its totals
/// count one piece fewer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recorded_card_of_a_thread_sealed_since_is_not_shown_or_counted() {
    let r = kelp_rig(false);
    let c = &r.core;
    let desk = session_in(c, None, &r.root);
    let t = turn_as(c, &desk, "desk: what about thread:kelp?", true).await;
    let rec = record_of(c, &t.turn_id);
    let before = owners_reading(c, &rec);
    assert!(lists(&before, "thread:kelp"));
    crate::tests_view_compile::seal_kelp(c);
    let after = owners_reading(c, &rec);
    assert!(!lists(&after, "thread:kelp"), "{after:?}");
    assert_eq!(counted(&after), counted(&before) - 1);
}

/// KEY: a card recorded with no provenance (a record written before it was kept), shown or dropped, is
/// unproven: neither is listed or counted, its source sealed or not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recorded_card_with_no_provenance_is_not_shown_or_counted() {
    use crate::view::budget::{DropWhy, Dropped};
    let r = kelp_rig(false);
    let c = &r.core;
    let desk = session_in(c, None, &r.root);
    let t = turn_as(c, &desk, "desk: what about thread:kelp?", true).await;
    let rec = record_of(c, &t.turn_id);
    let before = owners_reading(c, &rec);
    let strip = |space: &mut Space| {
        if let Space::Card { from, .. } | Space::Waiting { from, .. } = space {
            *from = None;
        }
    };
    let mut legacy = rec;
    legacy
        .shown
        .iter_mut()
        .for_each(|s| strip(&mut s.piece.range.space));
    let card = (legacy.shown.iter())
        .find(|s| matches!(s.piece.range.space, Space::Card { .. }))
        .expect("the card was shown")
        .clone();
    legacy.dropped.push(Dropped {
        range: card.piece.range.clone(),
        slot: card.piece.slot,
        band: card.piece.band,
        reasons: card.piece.reasons.clone(),
        handle: card.piece.handle.clone(),
        tokens: card.tokens,
        why: DropWhy::OverBudget,
    });
    let space = serde_json::to_value(&card.piece.range.space).unwrap();
    assert!(
        space["card"].get("from").is_none(),
        "as written before: {space}"
    );
    let legacy: crate::view::record::ViewRecord =
        serde_json::from_value(serde_json::to_value(&legacy).unwrap()).unwrap();
    for sealed in [false, true] {
        if sealed {
            crate::tests_view_compile::seal_kelp(c);
        }
        let read = owners_reading(c, &legacy);
        assert!(!lists(&read, "thread:kelp"), "sealed {sealed}: {read:?}");
        assert_eq!(counted(&read), counted(&before) - 1, "sealed {sealed}");
    }
}

/// KEY: an item card read from a clear session no longer shows in the view's record once the message it
/// read is moved, since, into a sealed thread (its session staying clear): the record re-asks today's moves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recorded_item_card_whose_source_moved_since_is_not_shown() {
    let r = kelp_rig(true);
    let c = &r.core;
    c.runner.pass.set_index(Arc::new(Items));
    let sid = serving(c, &r.root, "prj_tide");
    let first = turn_as(c, &sid, "the tide gauge: see issue 33 first", true).await;
    touched(&c.store, SCOPED).await;
    let t = turn_as(c, &sid, "and where is issue 33 now?", true).await;
    let rec = record_of(c, &t.turn_id);
    assert!(lists(&owners_reading(c, &rec), SCOPED));
    crate::ladder::tests_moved::move_ask(&c.store, &first.turn_id, "kelp");
    assert!(!lists(&owners_reading(c, &rec), SCOPED));
}

/// KEY: a frozen view's next call does not send an item card's excerpt whose message was moved, between the
/// calls, into a sealed thread from another session: the card's exchanges are re-asked before each call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_frozen_views_next_call_drops_a_card_whose_source_moved() {
    use std::sync::OnceLock;
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
    let source: Arc<OnceLock<String>> = Arc::default();
    let (held, moved) = (core.clone(), source.clone());
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| {
            kelp(s, true);
        },
        move |q| {
            let last = crate::tests_places::last_user(q);
            if last.contains("move it now") && !crate::tests_places::answers_a_call(q) {
                let c = held.get().expect("the core");
                let turn = moved.get().expect("the first turn");
                crate::ladder::tests_moved::move_ask(&c.store, turn, "kelp");
                return crate::provider::Scripted::tools(
                    "Looking.",
                    &[("s1", "fs_list", serde_json::json!({"path": "."}))],
                );
            }
            crate::provider::Scripted::text("Done.")
        },
    );
    let _ = core.set(r.core.clone());
    let c = &r.core;
    c.runner.pass.set_index(Arc::new(Items));
    let a = serving(c, &r.root, "prj_tide");
    let first = turn_as(c, &a, "the tide gauge: see issue 33 first", true).await;
    let _ = source.set(first.turn_id.clone());
    touched(&c.store, SCOPED).await;
    let d = serving(c, &r.root, "prj_tide");
    turn_as(c, &d, "where is issue 33? move it now", true).await;
    let calls: Vec<_> = (r.requests().into_iter())
        .filter(|q| texts(q).contains("move it now"))
        .collect();
    assert_eq!(calls.len(), 2, "the turn made two calls");
    assert!(
        card_of(&texts(&calls[0]), SCOPED).is_some(),
        "{}",
        texts(&calls[0])
    );
    // The card may be laid again from this turn's own words (which name the item too); never the moved one's.
    let again = card_of(&texts(&calls[1]), SCOPED).unwrap_or_default();
    assert!(!again.contains("tide gauge"), "{again}");
    assert!(
        !texts(&calls[1]).contains("tide gauge"),
        "{}",
        texts(&calls[1])
    );
}

/// A thread `slug`, bound to its own session: its session.
fn thread_named(store: &crate::store::Store, slug: &str) -> String {
    let r = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    store.put_session(&r.session_id, &r).unwrap();
    let t = serde_json::json!({"slug": slug, "title": slug});
    store.put_meta(&format!("thread:{slug}"), &t).unwrap();
    store
        .put_meta(&format!("discord.session.thread:{slug}"), &r.session_id)
        .unwrap();
    r.session_id
}

/// Seal thread `slug` through the board's own write.
fn seal(c: &crate::Core, slug: &str) {
    c.runner
        .threads
        .write(
            &c.store,
            |b| {
                let mut v = serde_json::to_value(b.get(slug).expect("the thread")).unwrap();
                v["sealed"] = serde_json::json!({"by": "cli", "at_ms": 2, "reason": "embargo"});
                Ok(vec![serde_json::from_value(v)?])
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
}

/// A card reads only the standing records its facts need: a fact of an unrelated thread's session is not
/// part of kelp's card's provenance, and sealing that thread leaves kelp's recorded card listed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sealing_an_unrelated_standing_records_session_leaves_the_card() {
    use theseus_protocol::{StandingAddParams, StandingKind, StandingScope};
    let sessions = std::cell::RefCell::new((String::new(), String::new()));
    let r = crate::tests_places::rig_setup(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| *sessions.borrow_mut() = (thread_named(s, "kelp"), thread_named(s, "reef")),
    );
    let c = &r.core;
    let (kelp_sid, reef_sid) = sessions.into_inner();
    for (sid, text) in [
        (&kelp_sid, "kelp is kept dry"),
        (&reef_sid, "reef is a secret"),
    ] {
        let p = StandingAddParams {
            kind: StandingKind::Fact,
            text: text.into(),
            scope: StandingScope::Session(sid.clone()),
            ..Default::default()
        };
        c.standing_add(&p, "the CLI").unwrap();
    }
    let desk = session_in(c, None, &r.root);
    let t = turn_as(c, &desk, "desk: what about thread:kelp?", true).await;
    let q = asked(&r, "desk: what about thread:kelp?").pop().unwrap();
    assert!(texts(&q).contains("kelp is kept dry"), "{}", texts(&q));
    let rec = record_of(c, &t.turn_id);
    let from = (rec.shown.iter())
        .find_map(|s| match &s.piece.range.space {
            Space::Card { from: Some(p), .. } => Some(p.clone()),
            _ => None,
        })
        .expect("kelp's card, with its provenance");
    assert!(from.sessions.contains(&kelp_sid), "{from:?}");
    assert!(!from.sessions.contains(&reef_sid), "{from:?}");
    seal(c, "reef");
    assert!(lists(&owners_reading(c, &rec), "thread:kelp"));
}

/// KEY: the owner alone is told who is speaking (his held person's name) and his own guidance for it; the
/// role line written outside the people pack (here by the operator) is not laid, since no reader clears it;
/// an agent's process at the terminal, which is no person and never the owner alone, is told none of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_facts_about_the_person_reach_the_owner_alone() {
    use theseus_ontology::{Category, CategoryId, Guidance, Origin, Record};
    const ROLE: &str = "keeps the harbour's books and owns the tide gauge";
    const GUIDED: &str = "Answer Ada in short lines.";
    let r = view_rig();
    let c = &r.core;
    let me_again = || CategoryId::parse(&format!("person:{}", crate::tests_places::OWNER)).unwrap();
    let me = me_again();
    let mut held = Category::new(me, "Ada", "ada");
    held.description = ROLE.into();
    (c.runner.ontology)
        .write(
            &c.store,
            vec![
                Record::Category(held),
                Record::Guidance(Guidance::new(me_again(), GUIDED, 1, "ada")),
            ],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: good morning", true).await;
    let q = asked(&r, "desk: good morning").pop().unwrap();
    assert!(texts(&q).contains("Who is speaking: Ada"), "{}", texts(&q));
    assert!(texts(&q).contains(GUIDED), "{}", texts(&q));
    assert!(!texts(&q).contains(ROLE), "{}", texts(&q));

    let agent = session_in(c, None, &r.root);
    turn_as(c, &agent, "agent: good morning", false).await;
    let q = asked(&r, "agent: good morning").pop().unwrap();
    assert!(!texts(&q).contains("Who is speaking"), "{}", texts(&q));
    assert!(!texts(&q).contains(GUIDED), "{}", texts(&q));
}

/// A topic named by its key brings its card, with the owner's guidance, to the owner alone: a topic is the
/// owner's own filing, so an agent's process at the terminal is told nothing of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_topic_brings_its_card_to_the_owner_alone() {
    use theseus_ontology::{Category, CategoryId, Guidance, Origin, Record};
    const GUIDED: &str = "Keep the ledger's own words.";
    let r = view_rig();
    let c = &r.core;
    let ledger = CategoryId::parse("topic:ledger").unwrap();
    (c.runner.ontology)
        .write(
            &c.store,
            vec![
                Record::Category(Category::new(ledger.clone(), "Ledger", "ada")),
                Record::Guidance(Guidance::new(ledger, GUIDED, 1, "ada")),
            ],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: where is topic:ledger now?", true).await;
    let q = asked(&r, "desk: where is topic:ledger now?").pop().unwrap();
    assert!(
        texts(&q).contains("Topic: Ledger (topic:ledger)"),
        "{}",
        texts(&q)
    );
    assert!(texts(&q).contains(GUIDED), "{}", texts(&q));

    let agent = session_in(c, None, &r.root);
    turn_as(c, &agent, "agent: where is topic:ledger now?", false).await;
    let q = asked(&r, "agent: where is topic:ledger now?")
        .pop()
        .unwrap();
    assert!(!texts(&q).contains("Topic: Ledger"), "{}", texts(&q));
}
