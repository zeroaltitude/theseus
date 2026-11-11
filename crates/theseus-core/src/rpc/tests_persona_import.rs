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
    // The config names a state dir that is not beside the rig's store, as
    // `theseusd` leaves it once `--state-dir` is folded into it: the rig opens
    // its store in a directory of its own, so the store's parent and the state
    // dir are two places, and the persona's folder must go to the state dir.
    let elsewhere = tempfile::tempdir().unwrap();
    let r = rig_with(MemoryMode::Off, |c| {
        c.server.state_dir = elsewhere.path().display().to_string();
        c.personas.insert(
            "wren".into(),
            PersonaConfig {
                card: Some(draft.clone()),
                files: vec![crate::personas::test_soul("wren")],
                ..Default::default()
            },
        );
        c.personas.insert(
            "finch".into(),
            PersonaConfig {
                card: Some(owners.into()),
                files: vec![crate::personas::test_soul("finch")],
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
    assert!(!run::dir_of(&c.runner.state_root(), "wren").exists());
    // Before any import there is no record: a card is a card.
    assert!(!info(c, "wren").card_draft && info(c, "wren").offered);

    let done = c
        .persona_import(&PersonaImportParams {
            dry_run: false,
            ..p
        })
        .unwrap();
    assert_eq!(done.card.as_deref(), Some(draft.as_str()));
    let dir = run::dir_of(&c.runner.state_root(), "wren");
    // Under the state dir the config names, which is where the floor and
    // session block 3 look; never beside the store.
    assert_eq!(dir, elsewhere.path().join("personas/wren"));
    let store_parent = c.store.dir().parent().unwrap();
    assert_ne!(
        store_parent,
        elsewhere.path(),
        "the rig must keep them apart"
    );
    assert!(!store_parent.join("personas").exists(), "{store_parent:?}");
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

/// `state_root` is `Config::state_dir()` and nothing derived from the store:
/// with the store somewhere else, the root, the soul's home and a persona's
/// folder all follow the config. Red if `state_root` reads
/// `store.dir().parent()` again (the shape the tool floor had before
/// theseus-3i6d1), which `--stdio`'s `store-stdio` hides, since it too sits
/// directly under the state dir.
#[test]
fn the_state_root_is_the_configs_state_dir_and_not_the_stores_parent() {
    let flagged = tempfile::tempdir().unwrap();
    let r = rig_with(MemoryMode::Off, |c| {
        c.server.state_dir = flagged.path().display().to_string();
    });
    let c = &r.core;
    assert_ne!(c.store.dir().parent().unwrap(), flagged.path());
    assert_eq!(c.runner.state_root(), flagged.path());
    assert_eq!(
        c.soul_home("wren"),
        flagged.path().join("personas/wren/PERSONA.md")
    );
    assert_eq!(c.runner.state_root(), c.cfg.state_dir());
}

/// The layout a `--stdio` daemon builds: the store is `<state>/store-stdio`
/// and the config's state dir is `<state>`, so the root is `<state>`. This
/// passes under either derivation, since `store-stdio` sits directly under the
/// state dir as `store` does; it is here so the pairing is stated, and breaks
/// if the root ever becomes anything but the state dir.
#[test]
fn a_stdio_layout_has_the_same_root() {
    let state = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(&state.path().join("store-stdio")).unwrap();
    let mut cfg = crate::config::Config::example();
    cfg.server.state_dir = state.path().display().to_string();
    cfg.tools.roots = vec![];
    let model = std::sync::Arc::new(crate::provider::FakeProvider::default());
    let p = crate::rpc::Parts::for_tests(cfg, model, store);
    let core = crate::Core::build(p).unwrap();
    assert_eq!(core.runner.state_root(), state.path());
}
