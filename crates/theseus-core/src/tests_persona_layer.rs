//! The persona layer per place (theseus-nsp9l.1): where a message can engage another persona (a DM, a channel,
//! a thread) the request carries every persona's files, so a switch changes no byte of the prefix
//! (theseus-c1uz); in a cli, task or headless session, whose persona is fixed, it carries only the persona in
//! play. Measured on the owner's shape: five personas, 10,552 bytes of files.

use std::time::Duration;

use theseus_protocol::SessionKind;

use crate::config::PersonaConfig;
use crate::first_request::budget;
use crate::tests_first_request::{rig_of, OWNER_SHAPE};
use crate::tests_keep_warm as kw;
use crate::Core;

const PERSONAS: [&str; 5] = ["alder", "brook", "cinder", "dune", "ember"];

/// The persona layer's text and the project layer's of a fresh session's first request on `place`.
fn layers(core: &Core, place: &str) -> (String, String) {
    let (live, _) = core.live_profile();
    let next = core.runner.census_request(None, place, &live).unwrap();
    (next.spec.persona_text, next.spec.context_text)
}

/// The personas whose files a text carries, by each file's own heading.
fn carried(text: &str) -> Vec<&'static str> {
    PERSONAS
        .into_iter()
        .filter(|p| text.contains(&format!("# {}\n", p[..1].to_uppercase() + &p[1..])))
        .collect()
}

/// A cli's and a task's request carry the persona in play and no other: the default, `alder`, which is a
/// fifth of the owner's persona bytes; a DM and a channel carry all five.
#[test]
fn a_cli_and_a_task_session_carry_only_the_persona_in_play() {
    let (core, _dir) = rig_of(OWNER_SHAPE);
    for place in ["cli", "task", "headless"] {
        let (persona, _) = layers(&core, place);
        assert_eq!(carried(&persona), ["alder"], "{place}");
        assert!(persona.len() < 1_500, "{place}: {} bytes", persona.len());
    }
    for place in ["dm", "channel"] {
        let (persona, _) = layers(&core, place);
        // A shared channel withholds files not marked public: the DM is the one that holds them all.
        if place == "dm" {
            assert_eq!(carried(&persona), PERSONAS, "{place}");
        }
    }
    let first = |p: &str| core.first_request(p).unwrap().tokens;
    eprintln!(
        "first request, the owner's shape: cli {} task {} dm {} (budgets {:?} {:?} {:?})",
        first("cli"),
        first("task"),
        first("dm"),
        budget("cli"),
        budget("task"),
        budget("dm")
    );
}

/// A persona in play that is not the default is the one a cli carries, and a persona change in such a session is
/// a changed system (a `system_changed` recompile), as an edited context file is.
#[test]
fn a_clis_persona_is_the_one_in_play_and_its_change_changes_the_system() {
    let a = rig_of(OWNER_SHAPE).0;
    let toml = OWNER_SHAPE.replace("default_persona = \"alder\"", "default_persona = \"dune\"");
    let d = rig_of(&toml).0;
    let (pa, _) = layers(&a, "cli");
    let (pd, _) = layers(&d, "cli");
    assert_eq!(carried(&pa), ["alder"]);
    assert_eq!(carried(&pd), ["dune"]);
    assert_ne!(pa, pd);
}

/// In the DM a persona switch changes no byte of the prefix: the default persona is only a line in the tail, so
/// two DMs whose persona in play differs send the same system blocks and tools.
#[test]
fn in_the_dm_a_persona_switch_changes_no_byte_of_the_prefix() {
    let (alder, adir) = rig_of(OWNER_SHAPE);
    let toml = OWNER_SHAPE.replace("default_persona = \"alder\"", "default_persona = \"ember\"");
    let (ember, edir) = rig_of(&toml);
    let (live, _) = alder.live_profile();
    let one = alder.runner.census_request(None, "dm", &live).unwrap();
    let two = ember.runner.census_request(None, "dm", &live).unwrap();
    // Each rig has a directory of its own, which the header and the session's block name.
    let text = |next: &crate::census::NextRequest, dir: &tempfile::TempDir| {
        let system = serde_json::to_string(&next.compiled.request.system).unwrap();
        system.replace(dir.path().to_str().unwrap(), "<dir>")
    };
    assert_eq!(text(&one, &adir), text(&two, &edir));
    assert_eq!(one.compiled.request.tools, two.compiled.request.tools);
}

/// A keep-warm read of a cli session, with five personas, is the turn's request byte for byte: the persona in
/// play's files only, in the read as in the turn.
#[tokio::test(start_paused = true)]
async fn a_keep_warm_replay_matches_the_turns_request_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = kw::one_hour(24);
    for name in PERSONAS {
        let path = format!("{}/fixtures/personas/{name}.md", env!("CARGO_MANIFEST_DIR"));
        let p = PersonaConfig {
            files: vec![path.into()],
            ..Default::default()
        };
        config.personas.insert(name.into(), p);
    }
    config.context.default_persona = Some("brook".into());
    let (core, fake) = kw::build(dir.path(), config);
    let sid = kw::session(&core, SessionKind::Conversation, None);
    kw::turn(&core, &sid, "first, a question").await;
    let live = core.live_profile().0;
    assert!(matches!(
        core.runner.keep_warm_once(&sid, &live).await,
        crate::keep_warm::Outcome::Read { .. }
    ));
    kw::settle().await;
    tokio::time::advance(Duration::from_millis(1)).await;
    kw::turn(&core, &sid, "and the next message").await;
    let all = fake.requests();
    let read = kw::reads(&fake).pop().unwrap();
    let (first, next) = (&all[0], all.last().unwrap());
    let bytes = |v: &[serde_json::Value]| serde_json::to_string(v).unwrap();
    assert_eq!(
        bytes(&read.system),
        bytes(&next.system),
        "the read's system is the turn's"
    );
    assert_eq!(
        bytes(&first.system),
        bytes(&next.system),
        "and stays the same"
    );
    let text = bytes(&next.system);
    assert!(
        text.contains("# Brook") && !text.contains("# Alder"),
        "only brook's files"
    );
}
