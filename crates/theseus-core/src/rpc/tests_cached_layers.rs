//! The provider's cache across sessions, through the whole core: two sessions in two places on one project
//! send layers 1 to 3 byte for byte, from the view's own fixed front. (This held with the prefix library and
//! holds without it: the library went on 2026-11-22.)

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::{StandingAddParams, StandingKind, StandingScope};

use crate::bus::EventSink;
use crate::provider::{ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_day_ceiling::build;
use crate::turn::TurnRequest;
use crate::{Config, Core};

fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

async fn say(core: &Arc<Core>, sid: &str, input: &str) {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
            surface: None,
        })
        .await
        .unwrap();
}

/// The project: one system-level context file, and a persona with its own.
fn project(dir: &std::path::Path, c: &mut Config) {
    let (p, f) = (dir.join("surveyor.md"), dir.join("harbour.md"));
    std::fs::write(
        &p,
        format!("The surveyor measures. {}", "tide ".repeat(300)),
    )
    .unwrap();
    std::fs::write(&f, format!("The harbour log. {}", "wave ".repeat(600))).unwrap();
    c.context.files = vec![f.to_string_lossy().into_owned().into()];
    c.context.default_persona = Some("surveyor".into());
    c.personas.insert(
        "surveyor".into(),
        crate::config::PersonaConfig {
            files: vec![p.to_string_lossy().into_owned().into()],
            ..Default::default()
        },
    );
}

fn texts(n: usize) -> Vec<Scripted> {
    (0..n)
        .map(|i| Scripted::text(&format!("noted {i}")))
        .collect()
}

fn layers(r: &ProviderRequest) -> Vec<Value> {
    r.system[..3].to_vec()
}

fn text(r: &ProviderRequest, i: usize) -> String {
    r.system[i]["text"].as_str().unwrap_or_default().to_string()
}

/// Two sessions in two places on one project render layers 1 to 3 byte for
/// byte: nothing of a session, a place or a day is inside them (the provider's
/// cache). A session's own standing record goes to its own block. (Threads
/// were retired on 2026-11-22: their case left this test.)
#[tokio::test]
async fn sessions_threads_and_places_on_one_project_send_layers_one_to_three_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), texts(2), |c| project(dir.path(), c));
    // The daemon's start step: the project's context file is accepted, once.
    core.held_first_start().unwrap();
    crate::tests_day_ceiling::bind_dm(&core);
    let (a, b) = (session(&core, None), session(&core, Some("dm:5150")));
    let add = |scope: StandingScope, text: &str| {
        core.standing_add(
            &StandingAddParams {
                kind: StandingKind::Order,
                text: text.into(),
                scope,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    };
    add(StandingScope::Everywhere, "Quote every level in metres.");
    add(
        StandingScope::Session(a.clone()),
        "Sign each reply as the harbour clerk.",
    );
    say(&core, &a, "the levels today").await;
    say(&core, &b, "the levels today").await;
    let r = fake.requests();
    assert_eq!(r.len(), 2);
    assert_eq!(layers(&r[0]), layers(&r[1]), "two sessions, two places");
    assert!(text(&r[0], 0).contains("Quote every level in metres."));
    assert!(text(&r[0], 2).contains("The harbour log."));
    // The session's own order is in its own block, never in a layer.
    assert!(text(&r[0], 3).contains("Sign each reply as the harbour clerk."));
    for q in &r {
        for i in 0..3 {
            let t = text(q, i);
            for own in [&*a, &*b, "harbour clerk", "Directory:", "2026-"] {
                assert!(!t.contains(own), "{own} is inside layer {}", i + 1);
            }
        }
    }
    assert!(!text(&r[1], 3).contains("harbour clerk"));
}
