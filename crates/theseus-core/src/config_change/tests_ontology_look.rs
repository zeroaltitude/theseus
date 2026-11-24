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
