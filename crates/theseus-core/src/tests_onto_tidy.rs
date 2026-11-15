//! The one-time cleanup of entries Theseus made by mistake (issue 9; the
//! owner's yes of 2026-11-14), proved on a scratch store: five build-directory
//! entries made by Theseus, each with the self-improvement loop's own
//! conversations filed under it, are retired by the daemon's own sweep, once,
//! each as one write that one undo puts back; an entry any other conversation
//! is filed under, one the operator made, and one of another kind are not
//! touched; the morning note says what was done, with each undo.

use std::sync::Arc;

use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record};
use theseus_protocol::{now_unix_ms, SessionKind};

use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Parts;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_self_budget::principal_session;
use crate::{Config, Core};

fn core() -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![Scripted::text("Hello.")]));
    (
        Core::build(Parts::for_tests(cfg, model, store)).unwrap(),
        dir,
    )
}

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn filed(session: &str, kind: &str, category: &str, origin: Origin) -> Record {
    Record::Members(MemberList {
        session: session.into(),
        kind: kind.into(),
        members: vec![Membership {
            category: id(category),
            origin,
            confidence: Some(0.9),
            as_of_ms: now_unix_ms(),
        }],
    })
}

/// A person's conversation.
fn person(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// `n` conversations of the self-improvement loop's own.
fn loop_sessions(core: &Core, parent: &str, n: usize) -> Vec<String> {
    (0..n)
        .map(|_| principal_session(core, parent, crate::rsi::SELF_PRINCIPAL, None).0)
        .collect()
}

/// Five build folders the loop made, and three that must stay; the sessions
/// the five hold, and the person's conversation `notes` is also filed under.
fn plant(core: &Core) -> (Vec<String>, String) {
    let parent = person(core);
    // Five build directories, each with three of the loop's own sessions.
    let mut records = Vec::new();
    let mut all_own = Vec::new();
    for i in 0..5 {
        let cat = format!("resource:build-{i}");
        records.push(Record::Category(Category::new(
            id(&cat),
            format!("build-{i}"),
            "jev:j-0001",
        )));
        for s in loop_sessions(core, &parent, 3) {
            records.push(filed(&s, "resource", &cat, Origin::Jev));
            all_own.push(s);
        }
    }
    // Not to be touched: a folder a person's conversation is also filed under.
    let shared_with_a_person = person(core);
    records.push(Record::Category(Category::new(
        id("resource:notes"),
        "notes",
        "jev:j-0001",
    )));
    records.push(filed(
        &shared_with_a_person,
        "resource",
        "resource:notes",
        Origin::Jev,
    ));
    records.push(filed(
        &loop_sessions(core, &parent, 1)[0],
        "resource",
        "resource:notes",
        Origin::Jev,
    ));
    // Not to be touched: another kind, though the loop's own sessions hold it.
    records.push(Record::Category(Category::new(
        id("topic:release-work"),
        "release work",
        "jev:j-0001",
    )));
    records.push(filed(
        &loop_sessions(core, &parent, 1)[0],
        "topic",
        "topic:release-work",
        Origin::Jev,
    ));
    core.ontology_machine_write(Origin::Jev, "j-0001", records)
        .unwrap();
    // Not to be touched: the operator's own folder, with the loop's session.
    let operator = vec![
        Record::Category(Category::new(id("resource:ops"), "ops", "ada")),
        filed(
            &loop_sessions(core, &parent, 1)[0],
            "resource",
            "resource:ops",
            Origin::Jev,
        ),
    ];
    core.runner
        .ontology
        .write(&core.store, operator, Origin::Operator, |_| Ok(vec![]))
        .unwrap();
    (all_own, shared_with_a_person)
}

#[test]
fn the_daemon_retires_the_entries_made_from_the_loops_own_messages_once_and_undoably() {
    let (core, _dir) = core();
    let now = now_unix_ms();
    let (all_own, shared_with_a_person) = plant(&core);
    let swept = core.onto_sweep(now).unwrap();
    assert_eq!((swept.tidied, swept.failed), (5, 0), "{swept:?}");
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    let mut left: Vec<String> = o
        .categories()
        .filter(|c| c.kind() == "resource")
        .map(|c| c.name.clone())
        .collect();
    left.sort();
    assert_eq!(left, ["notes", "ops"], "the five build folders are gone");
    assert!(o.category(&id("topic:release-work")).is_some());
    for s in &all_own {
        assert!(
            o.member_list(s, "resource")
                .is_none_or(|l| l.members.is_empty()),
            "{s} is no longer filed under a retired entry"
        );
    }
    assert!(o.member_list(&shared_with_a_person, "resource").is_some());

    // It runs once: a marker, and a later night looks at nothing.
    assert!(core.store.get_meta::<u64>("onto.tidied").unwrap().is_some());
    let again = core.onto_sweep(now + 1).unwrap();
    assert_eq!((again.tidied, again.failed), (0, 0), "{again:?}");

    // The morning note says it, a line a write, with the undo each takes.
    let tidy: Vec<String> = swept
        .writes
        .iter()
        .filter(|w| w.ends_with("-tidy"))
        .cloned()
        .collect();
    assert_eq!(tidy.len(), 5, "{:?}", swept.writes);
    let lines = core.onto_night_lines(now + 1_000);
    for w in &tidy {
        let line = lines
            .iter()
            .find(|l| l.contains(w.as_str()))
            .expect("a line");
        assert!(
            line.contains("retired the resource \"build-") && line.contains("3 sessions taken off"),
            "{line}"
        );
    }

    // One undo puts one entry and its three filings back, exactly.
    core.ontology_undo(&tidy[0], "test", "test").unwrap();
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    let back: Vec<&Category> = o
        .categories()
        .filter(|c| c.kind() == "resource" && c.name.starts_with("build-"))
        .collect();
    let [one] = back.as_slice() else {
        panic!("one entry is back: {back:?}")
    };
    let held = o
        .lists()
        .filter(|l| l.members.iter().any(|m| m.category == one.id))
        .count();
    assert_eq!(held, 3, "its filings are back");
}
