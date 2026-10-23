//! `[personas]` read once into the ontology (theseus-utfnl): the migration's
//! round trip, that it runs once, what it leaves out, and that the cached
//! prefix does not move.

use serde_json::Value;

use crate::config::PersonaView;
use crate::personas::Book;
use crate::tests_first_request::{rig_of, OWNER_SHAPE};
use crate::Core;

/// The owner's shape with its five personas given every kind of field the
/// config has: a public soul, a card, a name and emoji, a shareable one, and
/// `alder` the default with no card.
fn varied() -> String {
    OWNER_SHAPE
        .replace(
            "files = [\"/@PERSONAS@/brook.md\"]",
            "files = [\"/@PERSONAS@/brook.md\"]\ncard = \"Release manager: changelogs and packaging.\"\nname = \"Brooke\"\nemoji = \"🌊\"",
        )
        .replace(
            "files = [\"/@PERSONAS@/dune.md\"]",
            "files = [{ path = \"/@PERSONAS@/dune.md\", readers = \"public\" }]\nshareable = true\ninitiative = \"quiet\"",
        )
        .replace(
            "files = [\"/@PERSONAS@/ember.md\"]",
            "files = [\"/@PERSONAS@/ember.md\"]\ncard = \"Security engineer: CVEs and threat models.\"\ntts_voice = \"aura-2-thalia-en\"\navatar = \"https://example.test/ember.png\"",
        )
}

fn rows_of(core: &Core, kind: &str) -> Vec<Value> {
    crate::tests_judge::kinds(&core.store, kind)
        .into_iter()
        .map(|r| r.data)
        .collect()
}

#[test]
fn migration_round_trip() {
    let (core, _dir) = rig_of(&varied());
    let before = Book::of_config(&core.cfg);
    let after = core.personas();
    assert_eq!(after.len(), 5);
    assert_eq!(after.default_key(), Some("alder"));
    assert!(after.get("alder").unwrap().card.is_none());
    for (key, want) in before.live() {
        assert_eq!(after.get(key), Some(want), "{key}");
    }
    assert!(after.get("dune").unwrap().shareable);
    assert_eq!(
        after.get("dune").unwrap().files[0].readers(),
        crate::context_files::ContextReaders::Public
    );
    let migrated = rows_of(&core, "persona.migrated");
    assert_eq!(migrated.len(), 1, "{migrated:?}");
    assert_eq!(migrated[0]["personas"].as_array().unwrap().len(), 5);
    assert!(migrated[0]["personas"][0]["digest"].is_string());
    assert_eq!(migrated[0]["dropped"], serde_json::json!([]));
    // The owner is told once, in the head every unasked message has.
    let notes: Vec<String> = core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter_map(|a| {
            crate::outbox::body_of(a)["text"]
                .as_str()
                .map(str::to_string)
        })
        .filter(|t| t.contains("Personas moved into the ontology"))
        .collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(crate::owner_note::is_headed(&notes[0]), "{}", notes[0]);
}

#[test]
fn migration_is_idempotent() {
    let (core, dir) = rig_of(&varied());
    let categories = |c: &Core| {
        c.runner
            .ontology
            .snapshot(&c.store)
            .unwrap()
            .categories()
            .filter(|c| c.kind() == "persona")
            .cloned()
            .collect::<Vec<_>>()
    };
    let first = categories(&core);
    assert_eq!(first.len(), 5);
    let marks = |c: &Core| {
        (
            rows_of(c, "persona.migrated").len(),
            rows_of(c, "ontology.category").len(),
        )
    };
    let before = marks(&core);
    // A second start over the same store, the same config.
    let again = crate::rpc::Core::build(crate::rpc::Parts::for_tests(
        core.cfg.as_ref().clone(),
        std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![])),
        core.store.clone(),
    ))
    .unwrap();
    assert_eq!(categories(&again), first);
    assert_eq!(marks(&again), before, "it wrote no persona and no row");
    assert_eq!(before.0, 1);
    let _ = &dir;
}

#[test]
fn a_second_file_is_named_not_carried() {
    let toml = OWNER_SHAPE.replace(
        "files = [\"/@PERSONAS@/cinder.md\"]",
        "files = [\"/@PERSONAS@/cinder.md\", \"/@PERSONAS@/runbook.md\"]",
    );
    let (core, _dir) = rig_of(&toml);
    let cinder = core.personas().get("cinder").unwrap().clone();
    assert_eq!(cinder.files.len(), 1);
    assert!(cinder.files[0].path().ends_with("cinder.md"));
    let migrated = rows_of(&core, "persona.migrated");
    let dropped = migrated[0]["dropped"].as_array().unwrap();
    assert!(
        dropped.len() == 1 && dropped[0].as_str().unwrap().contains("runbook.md"),
        "{dropped:?}"
    );
    let warnings = core.cfg.persona_warnings();
    assert!(
        warnings.len() == 1 && warnings[0].contains("runbook.md"),
        "{warnings:?}"
    );
}

/// The persona layer and the project layer of a fresh DM's first request are
/// the same bytes from the ontology as the config's `[personas]` made them:
/// the same keys, paths and readers in the same order, so the same sections.
#[test]
fn migration_changes_no_prefix_byte() {
    let (core, dir) = rig_of(&varied());
    let from_config = core
        .cfg
        .context_paths(&Book::of_config(&core.cfg), None, true);
    let from_ontology = core.cfg.context_paths(&core.personas(), None, true);
    assert_eq!(from_ontology, from_config);
    let files = |paths: &[crate::context_files::ContextPath]| {
        core.runner
            .context_files
            .peek(paths)
            .into_iter()
            .map(|f| f.section)
            .collect::<Vec<_>>()
    };
    assert_eq!(files(&from_ontology), files(&from_config));
    // And they are what the request carries, in that order.
    let (live, _) = core.live_profile();
    let next = core.runner.census_request(None, "dm", &live).unwrap();
    let sections = files(&from_ontology);
    let carried = format!("{}{}", next.spec.persona_text, next.spec.context_text);
    let mut at = 0;
    for s in sections.iter().filter(|s| s.contains("(persona ")) {
        let found = carried[at..].find(s.as_str());
        assert!(
            found.is_some(),
            "a persona section is missing or out of order: {s:.60}"
        );
        at += found.unwrap();
    }
    let _ = &dir;
}

#[tokio::test]
async fn no_persona_changed_row_after_migration() {
    let (core, dir) = rig_of(&varied());
    let sid = crate::tests_persona::session(&core, Some("dm:42"));
    crate::tests_persona::turn(&core, &sid, "one").await;
    // A start again over the same store and config: nothing the persona was
    // seen as differs, so nothing changed.
    let again = crate::rpc::Core::build(crate::rpc::Parts::for_tests(
        core.cfg.as_ref().clone(),
        std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![
            crate::provider::Scripted::text("Hello."),
            crate::provider::Scripted::text("Hello."),
        ])),
        core.store.clone(),
    ))
    .unwrap();
    let sid = crate::tests_persona::session(&again, Some("dm:42"));
    crate::tests_persona::turn(&again, &sid, "two").await;
    assert!(rows_of(&again, "persona.changed").is_empty());
    // The sightings the config's views and the book's make are one.
    let book = again.personas();
    for (key, view) in Book::of_config(&again.cfg).live() {
        let sight = |v: &PersonaView| crate::persona::sighting(v, &[], 0).unwrap();
        assert_eq!(sight(book.get(key).unwrap()), sight(view), "{key}");
    }
    let _ = &dir;
}
