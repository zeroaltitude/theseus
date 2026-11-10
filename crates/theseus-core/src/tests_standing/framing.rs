//! The standing blocks say what they are (theseus-ectzg): the owner's standing
//! records, carried on every request, not new, never acknowledged. Without
//! that a model read a correct description of a record as an instruction it
//! had just been handed and answered it ("Noted: in group chats I speak as
//! myself"). The tests hold the exact words of each head, and the stored
//! request of a Discord-scope preference across a cold resume.

use super::*;

const PREF: &str =
    "In group chats, speak as yourself (Theseus); when something needs the owner's voice, say so";

/// The words every head carries, whole, so a later edit cannot drop them.
const FRAMING: &str = "These are the owner's standing records, carried on every request: they are not new, and are never to be acknowledged, thanked or restated to the owner.";

pub(super) async fn turn_via(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
    recompile: Option<crate::compiler::Recompile>,
) {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: Some("discord"),
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            recompile,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

fn tail(req: &ProviderRequest) -> String {
    let last = req.messages.last().unwrap();
    match &last["content"] {
        Value::String(s) => s.clone(),
        c => c
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// A Discord-scope preference rides the tail of every request of a DM
/// session, the cold resume's included, under a head that says it is not new.
#[tokio::test]
async fn a_surface_preference_is_carried_with_its_framing_across_a_cold_resume() {
    let r = rig();
    r.add(
        StandingKind::Preference,
        StandingScope::Surface("discord".into()),
        PREF,
    );
    let sid = session(&r.core, None);
    turn_via(&r.core, &sid, "What is on today?", None).await;
    let first = r.last_request();
    turn_via(
        &r.core,
        &sid,
        "And tomorrow?",
        Some(crate::compiler::Recompile::Transcript),
    )
    .await;
    let resumed = r.last_request();
    let mut seen = 0;
    for req in [&first, &resumed] {
        let t = tail(req);
        assert!(t.contains(PREF), "the preference is in the request: {t}");
        assert!(t.contains(FRAMING), "the block says it is not new: {t}");
        seen += 1;
    }
    assert_eq!(seen, 2);
}

/// The header's and the session's heads carry the same words.
#[tokio::test]
async fn the_header_and_the_local_heads_carry_the_framing() {
    let r = rig();
    r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    let sid = session(&r.core, None);
    r.add(
        StandingKind::Preference,
        StandingScope::Session(sid.clone()),
        SHORT,
    );
    turn(&r.core, &sid, "hello").await;
    let req = r.last_request();
    let system = format!(
        "{}\n{}",
        system_text(&req),
        serde_json::to_string(&req.messages[0]).unwrap()
    );
    assert_eq!(
        system.matches(FRAMING).count(),
        2,
        "header head and local head each carry it: {system}"
    );
    assert!(system.contains(NO_PUSH) && system.contains(SHORT));
}
