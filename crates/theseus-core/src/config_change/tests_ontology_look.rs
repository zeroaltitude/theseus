//! The ontology read of `look.theseus` (issue 28, step 5), through the whole core on the settings rig: the
//! owner's own turn reads the kinds, the categories, the people and Jev's proposals when it names them, and
//! never a person's handles, the guidance text, or a session's memberships; and the command it replaces is
//! answered with it.

use serde_json::json;
use theseus_protocol::{OntologyCategoryAddParams, OntologyGuidanceSetParams};

use super::tests::{rig, rig_with, Rig};
use crate::node::ResultStatus;

const GUIDANCE: &str = "INVENTED-GUIDANCE: always check the tide table first";
const HANDLE_MAIL: &str = "ada.quill@example.test";
const HANDLE_ID: &str = "271828182845904523";

fn seeded() -> Rig {
    let r = rig_with(|cfg, state| cfg.tools.cwd = Some(state.to_string_lossy().into_owned()));
    r.core
        .ontology_category_add(
            &OntologyCategoryAddParams {
                name: "Harbor".into(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    r.core
        .ontology_guidance_set(
            &OntologyGuidanceSetParams {
                category: "Harbor".into(),
                text: GUIDANCE.into(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    r.core
        .ontology_category_add(
            &OntologyCategoryAddParams {
                kind: Some("person".into()),
                name: "Ada Quill".into(),
                handles: vec![
                    format!("email:{HANDLE_MAIL}"),
                    format!("discord:{HANDLE_ID}"),
                ],
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    r
}

/// Named, the ontology is read: its kinds, its categories with their guidance noted and not given, its
/// people by name, its proposals; and nothing that names a person's other places, the guidance, or a
/// session. No card, and nothing in the record.
#[tokio::test]
async fn the_owner_reads_the_ontology_and_never_a_handle_or_the_guidance() {
    let r = seeded();
    let sid = r.session();
    r.turn_with("look_theseus", &sid, json!({"what": ["ontology"]}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    for part in [
        "Ontology: ",
        "- kind topic",
        "- topic Harbor · 0 sessions · has guidance",
        "People held (1; handles are not shown):",
        "- Ada Quill · 0 sessions",
        "Proposals: none wait.",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
    let everything = format!(
        "{text}{:?}{:?}",
        r.core.store.session_nodes(&sid).unwrap(),
        r.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, row)| row.kind != "ontology.category_added"
                && !row.kind.starts_with("ontology."))
            .collect::<Vec<_>>()
    );
    for never in [GUIDANCE, HANDLE_MAIL, HANDLE_ID] {
        assert!(!text.contains(never), "{never} reached the result: {text}");
        assert!(!everything.contains(never), "{never} reached the record");
    }
}

/// "Everything" is still the four parts: the ontology is read only when it is named.
#[tokio::test]
async fn the_ontology_is_read_only_when_named() {
    let r = seeded();
    let sid = r.session();
    r.turn_with("look_theseus", &sid, json!({})).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("Profiles") && text.contains("Build:"),
        "{text}"
    );
    assert!(
        !text.contains("Ontology") && !text.contains("Ada Quill"),
        "{text}"
    );
}

/// A long list is bounded and says what it left out.
#[tokio::test]
async fn a_long_category_list_is_bounded_and_says_what_it_left_out() {
    let r = rig();
    for n in 0..45 {
        r.core
            .ontology_category_add(
                &OntologyCategoryAddParams {
                    name: format!("Topic {n:02}"),
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap();
    }
    let sid = r.session();
    r.turn_with("look_theseus", &sid, json!({"what": ["ontology"]}))
        .await;
    let (_, text) = r.result(&sid);
    assert!(text.contains("Categories (45, in their tree):"), "{text}");
    assert!(text.contains("… and 5 more"), "{text}");
}

/// The commands the read replaces are answered with it, each counted; a command that asks for more, or acts,
/// runs as it did.
#[tokio::test]
async fn the_ontology_commands_are_answered_with_it_and_the_acts_run() {
    for line in [
        "theseus ontology",
        "theseus ontology kinds",
        "theseus ontology categories | head -30",
        "theseus ontology proposals",
    ] {
        let r = seeded();
        let sid = r.session();
        r.turn_with("proc_run", &sid, json!({"command": line}))
            .await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Declined, "{line}: {text}");
        assert!(text.contains("`look_theseus`: use it"), "{text}");
        assert_eq!(r.rows("shell.answered").len(), 1, "{line}");
    }
    for line in [
        "theseus ontology proposals --limit 5",
        "theseus ontology accept jdg_1",
        "theseus ontology topic add Reefs",
        "theseus ontology person add Ada --handle email:a@example.test",
        "theseus ontology member ses_other +topic:harbor",
        "theseus ontology never --list",
    ] {
        let r = seeded();
        let sid = r.session();
        r.turn_with("proc_run", &sid, json!({"command": line}))
            .await;
        assert!(r.rows("shell.answered").is_empty(), "{line} was answered");
    }
}

/// The look at this turn's view walks the store to prove its folds and task cards: it is read on the blocking
/// pool, never on the runtime's thread that runs the tool's future (on this one-thread runtime, the test's own).
#[tokio::test]
async fn the_looks_view_is_read_off_the_runtimes_thread() {
    let r = rig();
    let sid = r.session();
    r.turn_with("look_theseus", &sid, json!({"what": ["view"]}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let on = crate::look::view::read_on(&sid);
    assert_eq!(on.len(), 1, "{text}");
    assert_ne!(
        on[0],
        std::thread::current().id(),
        "the view was read on the runtime's thread"
    );
}

/// Two looks at this turn's view in one turn, through the tool as the model calls it, the view record the
/// same for both: the first lists the view's fold, one of its exchanges is ended and that write returns, and
/// the second does not list it. Each look proves from the store as it stands; nothing the first said is kept
/// for the second.
#[tokio::test]
async fn each_look_at_the_view_proves_from_the_store_as_it_stands() {
    use crate::tests_view_record::{exchange_of, fold_node, fold_piece, keep};
    use std::sync::{Arc, Mutex};
    let r = rig();
    let sid = r.session();
    for turn in ["t-a", "t-b"] {
        exchange_of(&r.core, turn, &sid, "local");
    }
    fold_node(&r.core, "fold-whole", "local", 1, &["t-a", "t-b"], 10);
    // The turn's id, once it asks the model: its view is kept then, ahead of its first look.
    let turn: Arc<Mutex<Option<String>>> = Arc::default();
    let (c, s, t) = (r.core.clone(), sid.clone(), turn.clone());
    r.when_the_turn_asks_the_model(move || {
        let nodes = c.store.session_nodes(&s).unwrap();
        let id = (nodes.iter().rev()).find_map(|(_, n)| n.turn_id.clone());
        let id = id.expect("the turn's input is written");
        let p = fold_piece("local", 1, ("t-a", "t-b"), "fold-whole");
        keep(&c, &id, "cmp_look", "local", vec![p]);
        *t.lock().unwrap() = Some(id);
    });
    // The view the second look reads, as it stands when the first look has answered.
    let newest: Arc<Mutex<Option<String>>> = Arc::default();
    let (c, t, n) = (r.core.clone(), turn.clone(), newest.clone());
    r.after_the_first_step(move || {
        let id = t.lock().unwrap().clone().expect("the turn's id");
        let v = crate::view::record::newest(&c.store, &id).unwrap();
        *n.lock().unwrap() = v.map(|v| v.compilation);
        c.store
            .delete_meta(&format!("{}t-b", crate::exchange::EXCHANGE_PREFIX))
            .unwrap();
    });
    let look = json!({"what": ["view"]});
    r.steps(
        &sid,
        vec![("look_theseus", look.clone()), ("look_theseus", look)],
    )
    .await;
    let (status, first) = r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{first}");
    assert!(first.contains("fold-whole folds"), "proven: {first}");
    let newest = newest.lock().unwrap().clone();
    assert_eq!(newest.as_deref(), Some("cmp_look"), "the same view");
    let (status, second) = r.result_of(&sid, "p2");
    assert_eq!(status, ResultStatus::Ok, "{second}");
    assert!(
        !second.contains("fold-whole"),
        "an exchange of it is gone: {second}"
    );
}
