//! `persona.import` and `persona.card_confirm` through the core
//! (theseus-a8n7): the import writes the persona's folder under the state
//! dir and nothing else; `persona.list` says whether the card is still the
//! draft the import wrote, and Jev is offered it only after the owner keeps
//! it (or writes their own).

use std::path::Path;

use theseus_protocol::{PersonaCardConfirmParams, PersonaImportParams, PersonaListParams};

use crate::config::{MemoryMode, PersonaConfig};
use crate::persona_import::{run, Manifest};
use crate::tests_recall::rig_with;

fn fixture(agent: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agents")
        .join(agent)
        .display()
        .to_string()
}

fn info(c: &crate::Core, key: &str) -> theseus_protocol::PersonaInfo {
    c.persona_list(PersonaListParams::default())
        .unwrap()
        .personas
        .into_iter()
        .find(|p| p.key.as_deref() == Some(key))
        .unwrap()
}

/// A dry run writes nothing; the import writes the folder and its record; a
/// config whose card is the draft lists it as a draft, not offered to Jev;
/// keeping it offers it; a card the owner wrote is no draft.
#[test]
fn the_drafted_card_is_offered_to_jev_only_once_kept() {
    let draft = {
        let state = tempfile::tempdir().unwrap();
        run::import(
            state.path(),
            &PersonaImportParams {
                from: fixture("wren"),
                name: "wren".into(),
                ..Default::default()
            },
            &crate::config::PersonaSettings::default().import_files,
            64 * 1024,
            1,
        )
        .unwrap()
        .0
        .card
        .unwrap()
    };
    let owners = "Reads tide tables for the owner and keeps species logs.";
    let r = rig_with(MemoryMode::Off, |c| {
        c.personas.insert(
            "wren".into(),
            PersonaConfig {
                card: Some(draft.clone()),
                ..Default::default()
            },
        );
        c.personas.insert(
            "finch".into(),
            PersonaConfig {
                card: Some(owners.into()),
                ..Default::default()
            },
        );
    });
    let c = &r.core;
    let p = PersonaImportParams {
        from: fixture("wren"),
        name: "wren".into(),
        dry_run: true,
        ..Default::default()
    };
    let dry = c.persona_import(&p).unwrap();
    assert!(dry.dry_run && dry.version == 1);
    assert!(!run::dir_of(&c.cfg.state_dir(), "wren").exists());
    // Before any import there is no record: a card is a card.
    assert!(!info(c, "wren").card_draft && info(c, "wren").offered);

    let done = c
        .persona_import(&PersonaImportParams {
            dry_run: false,
            ..p
        })
        .unwrap();
    assert_eq!(done.card.as_deref(), Some(draft.as_str()));
    let dir = run::dir_of(&c.cfg.state_dir(), "wren");
    assert_eq!(Manifest::read(&dir).unwrap().agent, "wren");
    let w = info(c, "wren");
    assert!(w.card_draft && !w.offered, "{w:?}");
    assert_eq!(w.agent.as_deref(), Some("wren"));
    // The owner's own card on a persona with no import is offered.
    let f = info(c, "finch");
    assert!(!f.card_draft && f.offered);

    let kept = c
        .persona_card_confirm(&PersonaCardConfirmParams {
            name: "wren".into(),
        })
        .unwrap();
    assert_eq!(kept.card, draft);
    let w = info(c, "wren");
    assert!(!w.card_draft && w.offered, "{w:?}");
    // A persona that is not configured is refused, naming the ones that are.
    let e = c
        .persona_card_confirm(&PersonaCardConfirmParams {
            name: "nobody".into(),
        })
        .unwrap_err();
    assert!(format!("{e:#}").contains("wren"), "{e:#}");
}
