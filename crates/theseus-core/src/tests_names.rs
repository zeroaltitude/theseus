//! Recognition (issue 44, `crate::names`), through a whole core and the fake
//! Jev: the topics `onto.v1` chose touch the message's exchange.

use std::sync::Arc;

use theseus_judge::fake::FakeJev;
use theseus_ontology::{Category, CategoryId, Guidance, MemberList, Membership, Origin, Record};
use theseus_protocol::index::{IndexNeighboursParams, IndexNeighboursResult};

use crate::exchange::{self, Root};
use crate::memory_pass::{IndexFuture, PassIndex};
use crate::names::card::{Admits, Source};
use crate::tender::TenderMiss;
use crate::tests_onto::{bound, rig, Rig};

const SAID: &str = "let's plan the Lantern Ledger migration";

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn operator_writes(r: &Rig, records: Vec<Record>) {
    r.core
        .runner
        .ontology
        .write(&r.core.store, records, Origin::Operator, |_| Ok(vec![]))
        .unwrap();
}

fn ledger_category(r: &Rig) {
    operator_writes(
        r,
        vec![Record::Category(Category::new(
            id("topic:ledger"),
            "Ledger",
            "ada",
        ))],
    );
}

/// A turn in `sid` and the sink's frame written: the turn's id.
async fn said(r: &Rig, sid: &str) -> String {
    let done = crate::tests_route::turn(&r.core, Some(sid), SAID, None).await;
    let judge = r.core.runner.judge.clone();
    tokio::task::spawn_blocking(move || judge.flush_sink())
        .await
        .unwrap();
    done.turn_id
}

fn timeline(r: &Rig, category: &str) -> Vec<exchange::Touch> {
    crate::names::topics::timeline(&r.core.store, &id(category), 10).unwrap()
}

/// KEY: a listed topic chosen at the act band touches the exchange of the
/// message it was asked for, at the exchange's own time.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_topic_chosen_at_the_act_band_touches_the_messages_exchange() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_category(&r);
    let sid = bound(&r.core);
    let turn = said(&r, &sid).await;
    let ex = exchange::exchange(&r.core.store, &turn)
        .unwrap()
        .expect("the turn's exchange");
    let [t] = timeline(&r, "topic:ledger").try_into().unwrap_or_else(|v| {
        panic!("one touch: {v:?}");
    });
    assert_eq!(
        (t.exchange.as_str(), t.session_id.as_str(), t.at_ms),
        (turn.as_str(), ex.session_id.as_str(), ex.start_ms)
    );
}

/// A topic the session already holds writes no filing, and its exchange is
/// touched all the same: the exchange was about it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_topic_the_session_already_holds_is_touched_all_the_same() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_category(&r);
    let sid = bound(&r.core);
    operator_writes(
        &r,
        vec![Record::Members(MemberList {
            session: sid.clone(),
            kind: "topic".into(),
            members: vec![Membership::operator(id("topic:ledger"), 1_000)],
        })],
    );
    let turn = said(&r, &sid).await;
    let touched: Vec<String> = timeline(&r, "topic:ledger")
        .into_iter()
        .map(|t| t.exchange)
        .collect();
    assert_eq!(touched, vec![turn]);
}

/// A topic minted at the act band starts its timeline with the message that
/// minted it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_minted_topic_starts_its_timeline_with_the_message_that_minted_it() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.9);
    jev.script_onto("topic_name", "n1", 0.9);
    let r = rig(jev);
    let sid = bound(&r.core);
    let turn = said(&r, &sid).await;
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let minted = o.member_list(&sid, "topic").expect("filed").members[0]
        .category
        .clone();
    let touched = crate::names::topics::timeline(&r.core.store, &minted, 10).unwrap();
    assert_eq!(touched.len(), 1, "{touched:?}");
    assert_eq!(touched[0].exchange, turn);
}

/// Below the act band no topic is touched, though the message's exchange is written as always.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn below_the_act_band_no_topic_is_touched() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.7);
    let r = rig(jev);
    ledger_category(&r);
    let sid = bound(&r.core);
    let turn = said(&r, &sid).await;
    assert!(timeline(&r, "topic:ledger").is_empty(), "the confirm band");
    let ex = exchange::exchange(&r.core.store, &turn).unwrap();
    assert!(ex.is_some(), "the turn's exchange is written");
    let place = exchange::newest(&r.core.store, Root::Place("discord:dm:42"), 5).unwrap();
    assert_eq!(
        place.len(),
        1,
        "the place holds the one exchange: {place:?}"
    );
}

/// An index that names one numbered item, as the tender's extractor would.
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
            t.contains("issue 33")
                .then(|| "item:issue-33".to_string())
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

/// KEY: a numbered item said in a session that serves a project touches
/// the exchange scoped by that project; in one that serves none it is an
/// unscoped candidate, and never the scoped thing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_numbered_item_is_touched_scoped_by_the_places_project_or_unscoped() {
    let r = rig(FakeJev::start().unwrap());
    r.core.runner.pass.set_index(Arc::new(Items));
    let sid = bound(&r.core);
    r.core
        .store
        .update_session(&sid, |s| {
            s.project = Some("prj_lantern".into());
            Ok(vec![])
        })
        .unwrap();
    let said = |sid: String| {
        let core = r.core.clone();
        async move {
            crate::tests_route::turn(&core, Some(&sid), "look at issue 33 again", None)
                .await
                .turn_id
        }
    };
    let turn = said(sid).await;
    assert_eq!(
        touched_in(&r, "item:project:prj_lantern#issue-33").await,
        vec![turn]
    );
    assert!(
        exchange::newest(&r.core.store, Root::Thing("item:issue-33"), 5)
            .unwrap()
            .is_empty(),
        "scoped, not bare"
    );

    let other =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    r.core.store.put_session(&other.session_id, &other).unwrap();
    let turn = said(other.session_id.clone()).await;
    assert_eq!(
        touched_in(&r, "item:issue-33").await,
        vec![turn],
        "an unscoped candidate"
    );
}

/// What a card's caller admits: everything, or no exchange.
struct Admit(bool);

impl crate::names::card::Admits for Admit {
    fn fact(&self, _: &theseus_protocol::StandingRecord) -> bool {
        true
    }

    fn exchange(&self, _: &crate::exchange::Exchange) -> bool {
        self.0
    }

    fn thing(&self, _: Source<'_>) -> bool {
        true
    }

    fn task(&self, _: &theseus_protocol::tasks::TaskRecord) -> bool {
        true
    }

    fn guidance(&self, _: &Guidance) -> bool {
        true
    }
}

/// KEY: the name table finds the topic by its name, and its card, read
/// once, holds the owner's guidance and the exchange the judge filed under
/// it, with the words that opened it; an exchange the caller's gate refuses
/// is not on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_topics_card_holds_its_guidance_and_the_exchanges_its_gate_admits() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_category(&r);
    operator_writes(
        &r,
        vec![Record::Guidance(Guidance::new(
            id("topic:ledger"),
            "Keep the ledger's own words.",
            1,
            "ada",
        ))],
    );
    let sid = bound(&r.core);
    let turn = said(&r, &sid).await;
    let table = crate::names::table::build(&r.core).unwrap();
    let found = table.find("how is the ledger");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0].how, found[0].things.as_slice()),
        (
            crate::names::table::How::Unique,
            &["topic:ledger".to_string()][..]
        )
    );
    let card = crate::names::card::card(&r.core, &table, "topic:ledger", &Admit(true))
        .unwrap()
        .expect("a card");
    assert_eq!(card.title, "Topic: Ledger (topic:ledger)");
    assert_eq!(card.exchanges, vec![turn]);
    assert!(
        card.lines
            .contains(&"Guidance: Keep the ledger's own words.".to_string()),
        "{:#?}",
        card.lines
    );
    assert!(
        card.lines
            .iter()
            .any(|l| l.ends_with(&format!("\"{SAID}\""))),
        "{:#?}",
        card.lines
    );
    let refused = crate::names::card::card(&r.core, &table, "topic:ledger", &Admit(false))
        .unwrap()
        .unwrap();
    assert!(refused.exchanges.is_empty());
    assert!(
        crate::names::card::card(&r.core, &table, "topic:none", &Admit(true))
            .unwrap()
            .is_none()
    );
}

/// A gate that refuses one standing record.
struct Hides(&'static str);

impl crate::names::card::Admits for Hides {
    fn fact(&self, r: &theseus_protocol::StandingRecord) -> bool {
        r.id != self.0
    }

    fn exchange(&self, _: &crate::exchange::Exchange) -> bool {
        true
    }

    fn thing(&self, _: Source<'_>) -> bool {
        true
    }
}

fn fact(id: &str, text: &str, ends: Option<&str>) -> theseus_protocol::StandingRecord {
    theseus_protocol::StandingRecord {
        id: id.into(),
        kind: theseus_protocol::StandingKind::Fact,
        text: text.into(),
        scope: theseus_protocol::StandingScope::Session("ses_kiln".into()),
        since_ms: 1_000,
        until_ms: None,
        source: "cli".into(),
        ends: ends.map(str::to_string),
    }
}

/// A task is found by its title, and its card's facts are its
/// session's standing records as they stand now: a replaced one is dropped,
/// and one the caller's gate refuses is never read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tasks_card_holds_its_current_facts_as_its_gate_admits_them() {
    let r = rig(FakeJev::start().unwrap());
    put_task(&r, "tsk_kiln", "Kiln Fire", "ses_kiln");
    let facts = vec![
        fact("std_a", "The kiln runs at night.", None),
        fact("std_b", "The kiln runs at dawn.", Some("std_a")),
        fact("std_hidden", "Not for this view.", None),
    ];
    r.core
        .runner
        .standing
        .write(&r.core.store, |_| Ok(facts), |_, _| Ok(vec![]))
        .unwrap();
    let table = crate::names::table::build(&r.core).unwrap();
    let found = table.find("back to the kiln fire");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].things, ["task:tsk_kiln"]);
    let card = crate::names::card::card(&r.core, &table, "task:tsk_kiln", &Hides("std_hidden"))
        .unwrap()
        .expect("a card");
    assert_eq!(card.title, "Task: Kiln Fire (task:tsk_kiln)");
    assert_eq!(card.facts, ["std_b"]);
    assert!(
        card.lines
            .contains(&"Fact: The kiln runs at dawn.".to_string()),
        "{:#?}",
        card.lines
    );
}

/// A gate that refuses everything.
struct Deny;

impl Admits for Deny {
    fn fact(&self, _: &theseus_protocol::StandingRecord) -> bool {
        false
    }

    fn exchange(&self, _: &crate::exchange::Exchange) -> bool {
        false
    }
}

/// A channel's gate: the topic `topic:ledger-private`, the tasks of the
/// private session `private`, its guidance and its exchanges are not its.
struct Channel {
    private: String,
}

impl Admits for Channel {
    fn fact(&self, _: &theseus_protocol::StandingRecord) -> bool {
        true
    }

    fn exchange(&self, e: &crate::exchange::Exchange) -> bool {
        e.session_id != self.private
    }

    fn thing(&self, s: Source<'_>) -> bool {
        !matches!(s, Source::Topic(c) if c.id.as_str() == "topic:ledger-private")
    }

    fn task(&self, t: &theseus_protocol::tasks::TaskRecord) -> bool {
        t.session.as_deref() != Some(self.private.as_str())
    }

    fn guidance(&self, g: &Guidance) -> bool {
        g.category.as_str() != "topic:ledger-private"
    }
}

fn put_task(r: &Rig, id: &str, title: &str, session: &str) {
    let task = theseus_protocol::tasks::TaskRecord {
        id: id.into(),
        title: title.into(),
        session: Some(session.into()),
        ..Default::default()
    };
    r.core
        .store
        .append(&[
            theseus_store::NewRecord::json(theseus_store::kinds::TASK, Some(id), &task).unwrap(),
        ])
        .unwrap();
}

/// KEY (the review's P1): a gate that refuses everything gets no name and
/// no card, not even a title.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_gate_that_refuses_everything_gets_no_name_and_no_card() {
    let r = rig(FakeJev::start().unwrap());
    ledger_category(&r);
    let table = crate::names::table::build_for(&r.core, &Deny).unwrap();
    assert!(table.is_empty());
    assert!(table.find("the ledger, topic:ledger").is_empty());
    let whole = crate::names::table::build(&r.core).unwrap();
    assert!(
        crate::names::card::card(&r.core, &whole, "topic:ledger", &Deny)
            .unwrap()
            .is_none()
    );
}

/// KEY (the review's P1): a private topic asked about from a channel is no
/// name, no candidate and no card, and leaves the visible topic it shares a
/// name with unique; that topic's card shows none of the private session's
/// tasks, nor the private topic's guidance or id.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_topic_asked_from_a_channel_leaks_nothing() {
    let r = rig(FakeJev::start().unwrap());
    let private = bound(&r.core);
    operator_writes(
        &r,
        vec![
            Record::Category(Category::new(id("topic:ledger"), "Ledger", "ada")),
            Record::Category(Category::new(id("topic:vault"), "Vault", "ada")),
            // The same name under another parent: the ontology's own rule.
            Record::Category(Category {
                parent: Some(id("topic:vault")),
                ..Category::new(id("topic:ledger-private"), "Ledger", "ada")
            }),
            Record::Guidance(Guidance::new(
                id("topic:ledger-private"),
                "Secret ways.",
                1,
                "ada",
            )),
            Record::Members(MemberList {
                session: private.clone(),
                kind: "topic".into(),
                members: vec![
                    Membership::operator(id("topic:ledger"), 1_000),
                    Membership::operator(id("topic:ledger-private"), 1_000),
                ],
            }),
        ],
    );
    put_task(&r, "tsk_secret", "Settle the hidden account", &private);
    let gate = Channel { private };
    let table = crate::names::table::build_for(&r.core, &gate).unwrap();
    let found = table.find("the ledger");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0].how, found[0].things.as_slice()),
        (
            crate::names::table::How::Unique,
            &["topic:ledger".to_string()][..]
        )
    );
    let asked = table.find("topic:ledger-private");
    assert!(
        asked
            .iter()
            .flat_map(|f| &f.things)
            .all(|t| t != "topic:ledger-private"),
        "{asked:?}"
    );
    let whole = crate::names::table::build(&r.core).unwrap();
    let card = |thing: &str| crate::names::card::card(&r.core, &whole, thing, &gate).unwrap();
    assert!(card("topic:ledger-private").is_none(), "no card at all");
    let shown = card("topic:ledger").expect("the visible topic's card");
    let text = format!("{} {:?} {:?}", shown.title, shown.lines, shown.tasks);
    for leak in [
        "tsk_secret",
        "hidden account",
        "Secret ways",
        "ledger-private",
    ] {
        assert!(!text.contains(leak), "{leak} leaked: {text}");
    }
}

/// Writes exchange `n` of `session` at `place`, touching `thing`.
fn put_exchange(r: &Rig, n: u64, session: &str, place: &str, thing: &str) {
    let ex = crate::exchange::Exchange {
        turn_id: format!("turn_{n:03}"),
        session_id: session.into(),
        place: place.into(),
        private: true,
        opened_by: crate::exchange::Opener::Message,
        person: Some("owner".into()),
        start_ms: 1_000 + n,
        end_ms: Some(2_000 + n),
        ask: None,
        reply: None,
        messages: Vec::new(),
        stop: None,
        rebuilt: false,
    };
    let records = vec![
        exchange::exchange_record(&ex).unwrap(),
        exchange::touch(Root::Thing(thing), &ex.turn_id, session, ex.start_ms).unwrap(),
    ];
    r.core.store.append(&records).unwrap();
}

/// The review's P2: thirty newer exchanges the gate refuses crowd out none
/// of the three older ones it admits; the card is the same with the
/// refused ones deleted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refused_exchanges_crowd_out_none_the_gate_admits() {
    let gate = Channel {
        private: "ses_private".into(),
    };
    let mut cards = Vec::new();
    for with_refused in [true, false] {
        let r = rig(FakeJev::start().unwrap());
        ledger_category(&r);
        for n in 0..3 {
            put_exchange(&r, n, "ses_open", "discord:channel:7", "topic:ledger");
        }
        if with_refused {
            for n in 10..40 {
                put_exchange(&r, n, "ses_private", "discord:dm:42", "topic:ledger");
            }
        }
        let table = crate::names::table::build(&r.core).unwrap();
        let c = crate::names::card::card(&r.core, &table, "topic:ledger", &gate)
            .unwrap()
            .unwrap();
        cards.push((c.lines, c.exchanges));
    }
    assert_eq!(cards[0].1, ["turn_002", "turn_001", "turn_000"]);
    assert_eq!(cards[0], cards[1]);
}

/// The thread `kiln-fire`'s old record (`tests_old_threads`).
fn kiln(r: &Rig) {
    let none = serde_json::json!({});
    crate::tests_old_threads::thread(&r.core, "kiln-fire", "Kiln Fire", none);
}

/// The review's P2: a correction the gate refuses ends nothing the card
/// shows: the card is the one the store would give without it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_correction_ends_no_fact_the_card_shows() {
    let mut cards = Vec::new();
    for with_correction in [true, false] {
        let r = rig(FakeJev::start().unwrap());
        put_task(&r, "tsk_kiln", "Kiln Fire", "ses_kiln");
        let mut facts = vec![fact("std_a", "The kiln runs at night.", None)];
        if with_correction {
            facts.push(fact("std_hidden", "The kiln runs at dawn.", Some("std_a")));
        }
        r.core
            .runner
            .standing
            .write(&r.core.store, |_| Ok(facts), |_, _| Ok(vec![]))
            .unwrap();
        let table = crate::names::table::build(&r.core).unwrap();
        let c = crate::names::card::card(&r.core, &table, "task:tsk_kiln", &Hides("std_hidden"))
            .unwrap()
            .unwrap();
        cards.push((c.lines, c.facts));
    }
    assert_eq!(cards[0].1, ["std_a"]);
    assert_eq!(cards[0], cards[1]);
}

/// An index whose entities are ready at once and whose items are late, or
/// never come.
struct LateItems {
    never: bool,
}

impl PassIndex for LateItems {
    fn neighbours(&self, _: IndexNeighboursParams) -> IndexFuture<IndexNeighboursResult> {
        Box::pin(async { Err(TenderMiss::Down("a test's index".into())) })
    }

    fn entities(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        let out = texts
            .iter()
            .map(|_| vec!["file:lantern.rs".to_string()])
            .collect();
        Box::pin(async move { Ok(out) })
    }

    fn items(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        let never = self.never;
        Box::pin(async move {
            if never {
                std::future::pending::<()>().await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Ok(vec![vec!["item:issue-33".to_string()]; texts.len()])
        })
    }

    fn mode(&self) -> IndexFuture<String> {
        Box::pin(async { Ok("bm25_only".into()) })
    }
}

/// The review's P2: items that are late, or never come, cost the message's
/// other things nothing: they are touched at the turn's end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_or_stuck_items_cost_the_things_named_beside_them_nothing() {
    for never in [false, true] {
        let r = rig(FakeJev::start().unwrap());
        r.core.runner.pass.set_index(Arc::new(LateItems { never }));
        let sid = bound(&r.core);
        let turn =
            crate::tests_route::turn(&r.core, Some(&sid), "see lantern.rs and issue 33", None)
                .await
                .turn_id;
        assert_eq!(
            touched_in(&r, "file:lantern.rs").await,
            vec![turn],
            "never = {never}"
        );
    }
}

/// The touches of `term`, once one is in, within five seconds: a naming
/// writes its touches on a writer's next frame.
async fn touched_in(r: &Rig, term: &str) -> Vec<String> {
    for _ in 0..50 {
        let got = exchange::newest(&r.core.store, Root::Thing(term), 5).unwrap();
        if !got.is_empty() {
            return got.into_iter().map(|t| t.exchange).collect();
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Vec::new()
}

/// An index that never answers: a naming call outstanding for ever.
struct Stuck;

impl PassIndex for Stuck {
    fn neighbours(&self, _: IndexNeighboursParams) -> IndexFuture<IndexNeighboursResult> {
        Box::pin(std::future::pending())
    }

    fn entities(&self, _: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        Box::pin(std::future::pending())
    }

    fn items(&self, _: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        Box::pin(std::future::pending())
    }

    fn mode(&self) -> IndexFuture<String> {
        Box::pin(async { Ok("bm25_only".into()) })
    }
}

/// KEY (the rule of the naming tasks): a core dropped while a naming call
/// is still outstanding releases its store at once: a second core opens it
/// straight away, as a restart does. The tasks hold the core weakly and
/// upgrade it only to write.
#[tokio::test]
async fn a_core_dropped_while_a_naming_call_is_outstanding_releases_its_store_at_once() {
    use crate::provider::{FakeProvider, Scripted};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    let path = dir.path().join("store");
    {
        let store = crate::store::Store::open(&path).unwrap();
        let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text("Noted.")]));
        let core =
            crate::Core::build(crate::rpc::Parts::for_tests(cfg.clone(), fake, store)).unwrap();
        core.runner.pass.set_index(Arc::new(Stuck));
        crate::tests_route::turn(&core, None, "see lantern.rs and issue 33", None).await;
    }
    let at = std::time::Instant::now();
    let reopened = crate::store::Store::open(&path);
    assert!(
        reopened.is_ok(),
        "the store was still held: {:?}",
        reopened.err()
    );
    assert!(
        at.elapsed() < std::time::Duration::from_millis(1500),
        "the store was held for {:?}",
        at.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_threads_neither_make_cards_nor_compete_with_current_names() {
    let r = rig(FakeJev::start().unwrap());
    kiln(&r);
    put_task(&r, "tsk_kiln", "Kiln Fire", "ses_kiln");
    let table = crate::names::table::build(&r.core).unwrap();
    let found = table.find("the kiln fire");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].how, crate::names::table::How::Unique);
    assert_eq!(found[0].things, ["task:tsk_kiln"]);
    assert!(table.get("thread:kiln-fire").is_none());
    assert!(crate::names::card::card(
        &r.core,
        &table,
        "thread:kiln-fire",
        &crate::names::table::Whole
    )
    .unwrap()
    .is_none());
    assert!(
        r.core
            .runner
            .threads
            .all(&r.core.store)
            .unwrap()
            .get("kiln-fire")
            .is_some(),
        "the old record still reads"
    );
}

// ---------------------------------------------------- picture labels (issue 46)

/// Every picture label's touch key the store holds.
fn picture_keys(r: &Rig) -> Vec<String> {
    use theseus_store::Store as _;
    let prefix = format!(
        "{}{}",
        exchange::TOUCH_PREFIX,
        Root::Thing(crate::names::pictures::PREFIX).key()
    );
    r.core
        .store
        .inner()
        .latest_with_prefix(theseus_store::kinds::META, &prefix)
        .unwrap()
        .into_iter()
        .filter_map(|x| x.key)
        .collect()
}

/// The touches of picture label `root`.
fn pictures(r: &Rig, root: &str) -> Vec<exchange::Touch> {
    exchange::newest(&r.core.store, Root::Thing(root), 10).unwrap()
}

/// Scripts the onto pack's two picture questions.
fn script_pictures(jev: &FakeJev, subject: (&str, f64), screen: f64) {
    jev.script_onto("image_subject", subject.0, subject.1);
    jev.script(
        &format!("{}/shows_screen_or_document", crate::judge::onto::ONTO_PACK),
        theseus_judge::fake::Scripted::Noul(screen),
    );
}

/// Whether a request to the fake asked the onto pack's subject question.
fn subject_asked(jev: &FakeJev) -> bool {
    let q = format!("{}/image_subject", crate::judge::onto::ONTO_PACK);
    let asked = |s: &theseus_judge::fake::Seen| s.body["questions"].get(&q).is_some();
    jev.seen().iter().any(asked)
}

/// A turn in `sid` with one picture, then the sink's frame written: the
/// turn's id.
async fn pictured(r: &Rig, sid: &str) -> String {
    let raw = theseus_judge::fake::gps_png(500, 500);
    let done = crate::tests_route::turn_attached(
        &r.core,
        Some(sid),
        "what is this?",
        None,
        vec![crate::tests_onto::picture_file("shot.png", &raw)],
    )
    .await;
    let judge = r.core.runner.judge.clone();
    tokio::task::spawn_blocking(move || judge.flush_sink())
        .await
        .unwrap();
    done.turn_id
}

/// KEY: a picture of a document, at the act band, touches the exchange of the
/// message that carried it under its subject and under the screen label, at
/// the exchange's own time.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_picture_of_a_document_touches_its_exchange_under_its_subject() {
    let jev = FakeJev::start().unwrap();
    script_pictures(&jev, ("document", 0.95), 0.9);
    let r = rig(jev);
    let sid = bound(&r.core);
    let turn = pictured(&r, &sid).await;
    let ex = exchange::exchange(&r.core.store, &turn)
        .unwrap()
        .expect("the turn's exchange");
    for root in ["picture:document", "picture:shows_screen_or_document"] {
        let [t] = pictures(&r, root).try_into().unwrap_or_else(|v| {
            panic!("{root}: one touch: {v:?}");
        });
        assert_eq!(
            (t.exchange.as_str(), t.session_id.as_str(), t.at_ms),
            (turn.as_str(), ex.session_id.as_str(), ex.start_ms),
            "{root}"
        );
    }
    assert_eq!(picture_keys(&r).len(), 2, "{:?}", picture_keys(&r));
}

/// A photo that shows no screen has its subject's label alone: a no to the
/// screen question writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_photo_that_shows_no_screen_has_its_subject_alone() {
    let jev = FakeJev::start().unwrap();
    script_pictures(&jev, ("photo", 0.95), 0.1);
    let r = rig(jev);
    let sid = bound(&r.core);
    let turn = pictured(&r, &sid).await;
    let touched: Vec<String> = pictures(&r, "picture:photo")
        .into_iter()
        .map(|t| t.exchange)
        .collect();
    assert_eq!(touched, vec![turn]);
    assert!(pictures(&r, "picture:shows_screen_or_document").is_empty());
    assert_eq!(picture_keys(&r).len(), 1, "{:?}", picture_keys(&r));
}

/// Below the act band no picture is labelled, though both were asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_picture_below_the_act_band_is_not_labelled() {
    let jev = FakeJev::start().unwrap();
    script_pictures(&jev, ("document", 0.7), 0.7);
    let r = rig(jev);
    let sid = bound(&r.core);
    let turn = pictured(&r, &sid).await;
    assert!(subject_asked(&r.jev), "the picture was asked about");
    assert!(exchange::exchange(&r.core.store, &turn).unwrap().is_some());
    assert!(picture_keys(&r).is_empty(), "{:?}", picture_keys(&r));
}

/// A message with no picture writes no picture label, and its topic is
/// touched as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_without_a_picture_writes_no_picture_label() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    script_pictures(&jev, ("document", 0.95), 0.9);
    let r = rig(jev);
    ledger_category(&r);
    let sid = bound(&r.core);
    let turn = said(&r, &sid).await;
    assert!(!subject_asked(&r.jev), "no picture, no picture question");
    assert!(picture_keys(&r).is_empty(), "{:?}", picture_keys(&r));
    let touched: Vec<String> = timeline(&r, "topic:ledger")
        .into_iter()
        .map(|t| t.exchange)
        .collect();
    assert_eq!(touched, vec![turn], "the topic is touched as before");
}

/// KEY: the onto pack in shadow labels no picture: the gate is the one
/// topics use.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shadow_onto_judgment_labels_no_picture() {
    let jev = FakeJev::start().unwrap();
    script_pictures(&jev, ("document", 0.95), 0.9);
    let r = crate::tests_onto::rig_with(jev, Vec::new(), |c| {
        c.judge.packs.insert(
            crate::judge::onto::ONTO_PACK.into(),
            crate::config::JudgePackConfig {
                mode: Some(crate::config::PackMode::Shadow),
                sample: None,
                notices: None,
            },
        );
    });
    let sid = bound(&r.core);
    pictured(&r, &sid).await;
    assert!(subject_asked(&r.jev), "the shadow pack was asked");
    assert!(picture_keys(&r).is_empty(), "{:?}", picture_keys(&r));
}

/// KEY: the labels ride the judgment's own frame, so they and the answer
/// they rest on are written together or not at all; a frame that failed is
/// written again whole, and the labels land once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pictures_labels_ride_its_judgments_frame_and_land_once_after_a_retry() {
    use std::sync::Mutex;
    // The failed frame's records, each as its scope and its key.
    type Frame = Vec<(Option<String>, Option<String>)>;
    let jev = FakeJev::start().unwrap();
    script_pictures(&jev, ("document", 0.95), 0.9);
    let r = rig(jev);
    let sid = bound(&r.core);
    let prefix = format!(
        "{}{}",
        exchange::TOUCH_PREFIX,
        Root::Thing(crate::names::pictures::PREFIX).key()
    );
    let failed: Arc<Mutex<Frame>> = Arc::default();
    let seen = failed.clone();
    r.core.store.fail_plain_frame(move |rows| {
        let hit = rows
            .iter()
            .any(|x| x.key.as_deref().is_some_and(|k| k.starts_with(&prefix)));
        if hit {
            *seen.lock().unwrap() = rows
                .iter()
                .map(|x| (x.scope.clone(), x.key.clone()))
                .collect();
        }
        hit
    });
    let turn = pictured(&r, &sid).await;
    let judge = r.core.runner.judge.clone();
    tokio::task::spawn_blocking(move || judge.flush_sink())
        .await
        .unwrap();
    let frame = failed.lock().unwrap().clone();
    assert!(!frame.is_empty(), "a frame carried the picture labels");
    let rows: Vec<&str> = frame
        .iter()
        .filter(|(scope, _)| scope.as_deref() == Some("judge:onto"))
        .filter_map(|(_, key)| key.as_deref())
        .collect();
    assert!(
        rows.iter().any(|k| k.starts_with("jdg_")),
        "the onto judgment's row rode the same frame: {frame:?}"
    );
    assert_eq!(r.core.runner.judge.unwritten(), 0, "the retry wrote it");
    assert_eq!(picture_keys(&r).len(), 2, "{:?}", picture_keys(&r));
    for root in ["picture:document", "picture:shows_screen_or_document"] {
        let touched: Vec<String> = pictures(&r, root).into_iter().map(|t| t.exchange).collect();
        assert_eq!(touched, vec![turn.clone()], "{root}");
    }
}
