//! A thread born in a bound place takes that place's persona as its prior
//! (theseus-a8n7): the persona the router's persona question will start
//! from. `thread.edit { persona }` changes it, and a place that names a
//! persona the config does not have gives the thread none.

use theseus_protocol::threads::{ThreadEditParams, ThreadNewParams};

use crate::approval::{Answerer, Surface};
use crate::config::{MemoryMode, PersonaConfig};
use crate::places::BoundPlace;
use crate::tests_recall::rig_with;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

#[test]
fn a_thread_born_in_a_bound_place_inherits_its_persona() {
    let r = rig_with(MemoryMode::Off, |c| {
        c.personas.insert("wren".into(), PersonaConfig::default());
        c.personas.insert("finch".into(), PersonaConfig::default());
    });
    let c = &r.core;
    let place = |target: &str, persona: &str| BoundPlace {
        target: target.into(),
        name: target.into(),
        persona: Some(persona.into()),
        ..Default::default()
    };
    c.bind_places(vec![
        place("discord:channel:1001", "wren"),
        place("discord:channel:1002", "ghost"),
    ]);
    let made = |slug: &str, at: Option<&str>| {
        c.thread_new(
            &ThreadNewParams {
                slug: slug.into(),
                place: at.map(str::to_string),
                ..Default::default()
            },
            cli(),
        )
        .unwrap()
        .thread
    };
    assert_eq!(
        made("tides", Some("discord:channel:1001"))
            .persona
            .as_deref(),
        Some("wren")
    );
    // A persona the config lacks, a place nothing binds, and no place give none.
    assert_eq!(made("ghosts", Some("discord:channel:1002")).persona, None);
    assert_eq!(made("nowhere", Some("discord:channel:9999")).persona, None);
    assert_eq!(made("plain", None).persona, None);
    // The owner changes it.
    let edit = |persona: &str| {
        c.thread_edit(
            &ThreadEditParams {
                slug: "tides".into(),
                persona: Some(persona.into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap()
        .thread
        .persona
    };
    assert_eq!(edit("finch").as_deref(), Some("finch"));
    assert_eq!(edit(""), None);
}
