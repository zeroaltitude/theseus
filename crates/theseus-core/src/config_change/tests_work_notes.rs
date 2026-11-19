//! `look.work`'s review notes, played through the whole core (issue 28): a session given another
//! conversation's marked words takes their flags on, so the data gate refuses it a restricted-origin model
//! at once; a project's next milestone is another conversation's words; and the confirm list's bounded walk
//! lists what the walk of every session did.

use serde_json::json;
use theseus_protocol::SessionKind;

use super::tests::{change, rig, rig_with, Rig};
use super::tests_work_global::{conversation, mark, said, task};
use crate::bus::EventSink;
use crate::content::Flag;
use crate::session::SessionRecord;
use crate::turn::TurnRequest;

fn restricted_rig() -> Rig {
    rig_with(|cfg, _| cfg.serve_default_from_cn())
}

/// What `session`'s record holds as flags, as the data gate reads them.
fn flags(r: &Rig, session: &str) -> Option<crate::content::ContentFlags> {
    crate::content_wait::flags_of(&r.core.store, session).filter(|f| !f.labels.is_empty())
}

/// A turn of a new session carrying `flags`, on the rig's own live profile: the floor reads the labels.
async fn turn_with(r: &Rig, flags: &crate::content::ContentFlags) -> anyhow::Result<()> {
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.data = Some(Box::new(flags.clone()));
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = EventSink::new(r.core.bus.clone(), &rec.session_id, None);
    r.core
        .runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some("say done".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .map(|_| ())
}

/// A look that gives another conversation's marked words to an unrestricted turn leaves this session
/// carrying that conversation's flag, from `look.work` as the source, and the data gate's own rule then
/// refuses the session a model of restricted origin at once: the turn is moved off it with the flag's word
/// on the route, no judgment awaited.
#[tokio::test]
async fn marked_words_given_to_a_turn_leave_their_flags_on_it_and_the_gate_refuses_a_restricted_model(
) {
    let r = rig();
    let (here, other) = (conversation(&r).await, conversation(&r).await);
    task(&r, &other, "Chart the outer reef", None);
    mark(&r, &other);
    assert!(flags(&r, &here).is_none());
    let text = said(&r, &here, json!({"what": ["tasks"]})).await;
    assert!(text.contains("Chart the outer reef"), "given: {text}");
    let f = flags(&r, &here).expect("the flags were taken on");
    let l = f.label(Flag::Confidential).expect("confidential");
    assert_eq!(l.source, crate::content::deterministic::LOOK_SOURCE);
    // The data gate's own rule reads them: the session is kept off a model of restricted origin (the
    // check `session.profile` makes, and the route's floor), where a clean one is not.
    assert_eq!(f.disqualifies_cn(), Some(Flag::Confidential));
    assert!(r.core.runner.profile_refusal(f.mark(), "glm").is_some());
    assert!(r
        .core
        .runner
        .profile_refusal(crate::content::ContentFlags::default().mark(), "glm")
        .is_none());
    // And a turn of a session carrying them, on a rig whose own model is of restricted origin, has
    // nowhere to run: it fails before any model call, in the gate's words, as it would on a label a
    // judgment had raised.
    let ended = turn_with(&restricted_rig(), &f).await;
    let said = format!("{ended:?}");
    assert!(
        said.contains("No model cleared for this request's data is usable"),
        "{said}"
    );
    assert!(said.contains("data_gate"), "{said}");
}

/// A look that gave no marked words raises nothing: an open conversation's words, this conversation's own,
/// a conversation marked but held back from a restricted turn, and a look of no other conversation's.
#[tokio::test]
async fn a_look_that_gave_no_marked_words_raises_nothing() {
    // Open: given, nothing to take.
    let r = rig();
    let (here, open) = (conversation(&r).await, conversation(&r).await);
    task(&r, &open, "Mend the lamp", None);
    task(&r, &here, "Sort the charts", None);
    let text = said(&r, &here, json!({"what": ["tasks"]})).await;
    assert!(text.contains("Mend the lamp"), "{text}");
    assert!(flags(&r, &here).is_none(), "{:?}", flags(&r, &here));
    // Marked, but held back from a restricted turn: not given, so not taken.
    let r = restricted_rig();
    let (here, marked) = (conversation(&r).await, conversation(&r).await);
    task(&r, &marked, "Chart the outer reef", None);
    mark(&r, &marked);
    let text = said(&r, &here, json!({"what": ["tasks"]})).await;
    assert!(text.contains("held back"), "{text}");
    assert!(flags(&r, &here).is_none(), "{:?}", flags(&r, &here));
    // Marked, and this look reads only what is this conversation's.
    let r = rig();
    let (here, marked) = (conversation(&r).await, conversation(&r).await);
    task(&r, &marked, "Chart the outer reef", None);
    mark(&r, &marked);
    said(&r, &here, json!({"what": ["tasks"], "scope": "here"})).await;
    assert!(flags(&r, &here).is_none(), "{:?}", flags(&r, &here));
}

/// A project's own title is the owner's and is always given; its next milestone's title is the words of the
/// session the milestone was written in: given and taken on where that conversation is marked and the turn
/// is unrestricted, held back from a restricted one, and this conversation's own when it wrote it.
#[tokio::test]
async fn a_projects_next_milestone_is_another_conversations_words() {
    let make = |r: &Rig| {
        r.core
            .project_create(
                &serde_json::from_value(json!({
                    "title": "Harbour survey", "objective": "Chart every mooring",
                    "acceptance": ["every mooring has a depth"],
                    "milestones": [{"title": "Outer harbour", "due": "in 30d"}],
                    "review": "none"
                }))
                .unwrap(),
                "the CLI",
            )
            .unwrap();
        crate::task_graph::all(&r.core.store)
            .unwrap()
            .into_iter()
            .find(|t| t.kind == theseus_protocol::tasks::TaskKind::Milestone)
            .unwrap()
            .origin
            .session
    };
    // Unrestricted: given, and its flags are taken.
    let r = rig();
    let here = conversation(&r).await;
    let wrote = make(&r);
    mark(&r, &wrote);
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(
        text.contains("Harbour survey") && text.contains("next Outer harbour"),
        "{text}"
    );
    assert!(flags(&r, &here).is_some(), "taken on");
    // Restricted: the title is the owner's and stays; the milestone's is held.
    let r = restricted_rig();
    let here = conversation(&r).await;
    let wrote = make(&r);
    mark(&r, &wrote);
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(text.contains("Harbour survey"), "{text}");
    assert!(!text.contains("Outer harbour"), "{text}");
    assert!(text.contains("held back"), "{text}");
    assert!(flags(&r, &here).is_none());
    // Written in this conversation: its own words, whatever it holds.
    let r = rig();
    let wrote = make(&r);
    let text = said(&r, &wrote, json!({"what": ["projects"]})).await;
    assert!(text.contains("next Outer harbour"), "{text}");
}

/// The confirm list walks the sessions that ask and no others, and lists what the walk of every session
/// did, in the same order: the most recently active session first.
#[tokio::test]
async fn the_confirm_list_reads_the_sessions_that_ask_in_the_order_it_always_did() {
    let r = rig();
    let sids = [
        r.session(),
        r.session(),
        r.session(),
        r.session(),
        r.session(),
    ];
    // Three of the five ask, in a different order from their keys; two never do.
    for (i, call) in [
        (3, change("import.notes.enabled", json!(false))),
        (0, change("narrative", json!(true))),
        (2, change("kernel.daily_spend_ceiling_usd", json!(300))),
    ] {
        r.turn(&sids[i], call).await;
    }
    let listed = r.core.confirm_list().unwrap();
    assert_eq!(listed.len(), 3, "{listed:?}");
    // The reference: every session by activity, kept when it has a question.
    let mut all: Vec<SessionRecord> = r.core.store.live_sessions().unwrap();
    all.sort_by(|a, b| {
        b.last_active_ms
            .max(b.created_at_unix_ms)
            .cmp(&a.last_active_ms.max(a.created_at_unix_ms))
    });
    let want: Vec<String> = all
        .iter()
        .flat_map(|s| {
            listed
                .iter()
                .filter(|q| q.session_id == s.session_id)
                .map(|q| q.correlation_id.clone())
        })
        .collect();
    let got: Vec<String> = listed.iter().map(|q| q.correlation_id.clone()).collect();
    assert_eq!(got, want);
    let _ = SessionKind::Conversation;
}
