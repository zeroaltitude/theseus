//! `look.work`'s review notes, played through the whole core (issue 28): a session given another
//! conversation's marked words takes their flags on, so the data gate refuses it a restricted-origin model
//! at once; a project's next milestone is another conversation's words; and the confirm list's bounded walk
//! lists what the walk of every session did.

use serde_json::json;
use theseus_protocol::SessionKind;

use super::tests::{change, rig, rig_with, Rig};
use super::tests_work_global::{conversation, latest, mark, said, task};
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

/// A model-made project's next milestone is the words of the session that made it (the record says the
/// model wrote it, `origin.by_model`): given and taken on where that conversation is marked and the turn is
/// unrestricted, held back from a restricted one, and this conversation's own when it wrote it. (The owner's
/// own project is given as theirs: `an_owners_project_and_milestone_are_given_as_theirs`.)
#[tokio::test]
async fn a_projects_next_milestone_is_another_conversations_words() {
    // Unrestricted: given, and its flags are taken.
    let r = rig();
    let wrote = model_project(&r, conversation(&r).await).await;
    mark(&r, &wrote);
    let here = conversation(&r).await;
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(
        text.contains("Harbour survey") && text.contains("next Outer harbour"),
        "{text}"
    );
    assert!(flags(&r, &here).is_some(), "taken on");
    // Restricted: the title and the milestone are both that conversation's words, held.
    let r = restricted_rig();
    let wrote = model_project(&r, conversation(&r).await).await;
    mark(&r, &wrote);
    let here = conversation(&r).await;
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(!text.contains("Harbour survey"), "{text}");
    assert!(!text.contains("Outer harbour"), "{text}");
    assert!(text.contains("held back"), "{text}");
    assert!(flags(&r, &here).is_none());
    // Written in this conversation: its own words, whatever it holds.
    let r = rig();
    let wrote = model_project(&r, conversation(&r).await).await;
    let text = said(&r, &wrote, json!({"what": ["projects"]})).await;
    assert!(text.contains("next Outer harbour"), "{text}");
}

/// The owner's own project and milestones (`project.create`, at their own surface: `principal: operator`,
/// `by_model: false`, written from Home) are given as theirs and are nobody's conversation's words: nothing
/// is held from a restricted turn and nothing is taken on, from a conversation at the terminal and from
/// another at a place, whatever Home carries. Corrects the first pass's rule, which keyed them on Home.
#[tokio::test]
async fn an_owners_project_and_milestone_are_given_as_theirs() {
    for restricted in [false, true] {
        let r = match restricted {
            true => restricted_rig(),
            false => rig(),
        };
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
        let tasks = crate::task_graph::all(&r.core.store).unwrap();
        let milestone = tasks
            .iter()
            .find(|t| t.kind == theseus_protocol::tasks::TaskKind::Milestone)
            .unwrap();
        assert!(!milestone.origin.by_model && milestone.origin.principal == "operator");
        // Home carries a label: it is the session the record names, and it must not matter.
        mark(&r, &milestone.origin.session);
        let terminal = conversation(&r).await;
        let dm = r.session_at(&format!("dm:{}", super::tests::OWNER));
        for here in [terminal, dm] {
            let text = said(&r, &here, json!({"what": ["projects"]})).await;
            assert!(
                text.contains("Harbour survey") && text.contains("next Outer harbour"),
                "restricted {restricted}: {text}"
            );
            assert!(
                !text.contains("held back"),
                "restricted {restricted}: {text}"
            );
            assert!(
                flags(&r, &here).is_none(),
                "restricted {restricted}: {:?}",
                flags(&r, &here)
            );
        }
    }
}

/// A model retitles the owner's project (`task.update { title }`: no yes is asked, and the record's origin does
/// not move): the title is then that model's words, in the session that retitled it, held from a restricted
/// turn and taken on by an unrestricted one; the owner's milestone beside it is still theirs. And a project a
/// model made in one conversation and another retitled has the retitling conversation's words, not the
/// first's. The owner has no verb that retitles, so the reverse (the owner retitling a model's) cannot occur.
#[tokio::test]
async fn a_title_a_model_retitled_is_that_conversations_words() {
    let owners = |r: &Rig| {
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
            .unwrap()
            .project
            .id
    };
    async fn retitle(r: &Rig, id: String, by: String) {
        let rec = crate::task_graph::all(&r.core.store)
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap();
        r.turn_with(
            "task_update",
            &by,
            json!({"id": id, "version": rec.version, "patch": {"title": "Reef survey"}}),
        )
        .await;
        let (status, text) = latest(r, &by);
        assert_eq!(status, crate::node::ResultStatus::Ok, "{text}");
    }
    // Unrestricted: the retitling conversation is marked; its flags are taken, the milestone is the owner's.
    let r = rig();
    let id = owners(&r);
    let renamer = conversation(&r).await;
    retitle(&r, id, renamer.clone()).await;
    mark(&r, &renamer);
    let here = conversation(&r).await;
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(
        text.contains("Reef survey") && text.contains("next Outer harbour"),
        "{text}"
    );
    assert!(flags(&r, &here).is_some(), "the renamer's flags are taken");
    // Restricted: the new title is held; the owner's milestone is not.
    let r = restricted_rig();
    let id = owners(&r);
    let renamer = conversation(&r).await;
    retitle(&r, id, renamer.clone()).await;
    mark(&r, &renamer);
    let here = conversation(&r).await;
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(
        !text.contains("Reef survey") && !text.contains("Harbour survey"),
        "{text}"
    );
    assert!(
        text.contains("held back") && text.contains("next Outer harbour"),
        "{text}"
    );
    // A model's project from a marked conversation, retitled in an unmarked one: the second's words.
    let r = restricted_rig();
    let made = model_project(&r, conversation(&r).await).await;
    mark(&r, &made);
    let id = crate::task_graph::all(&r.core.store).unwrap()[0].id.clone();
    let second = conversation(&r).await;
    retitle(&r, id, second).await;
    let here = conversation(&r).await;
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    // The title is the second conversation's (unmarked, given); the milestone the first made is still its.
    assert!(
        text.contains("Reef survey") && text.contains("next (held back"),
        "{text}"
    );
}

/// A routine the owner made (`routine.create` is judged as their act) is theirs and is never held or taken on,
/// whatever its own session carries; one a model made (`created_by: model:<session>`) is the words of the
/// session it wrote it in. A reminder's note is not provable (`wake.create` is not judged as an owner's act
/// and its `by` is a label any caller names), so it is the words of the session that holds it, whoever set it.
#[tokio::test]
async fn a_routine_is_in_the_hand_that_made_it_and_a_reminder_in_its_sessions() {
    use theseus_kernel::{NewWake, Purpose, WakeRun};
    for restricted in [false, true] {
        let r = match restricted {
            true => restricted_rig(),
            false => rig(),
        };
        let (marked, here) = (conversation(&r).await, conversation(&r).await);
        mark(&r, &marked);
        // The owner's routine (its own session marked as well), and a model's from the marked session.
        let owners = r
            .core
            .routine_create(
                &serde_json::from_value(json!({
                    "title": "Tide check", "schedule": "weekdays 08:00", "brief": "Read the tides."
                }))
                .unwrap(),
                "the CLI",
            )
            .unwrap()
            .routine;
        mark(&r, &owners.id);
        let by = format!("model:{marked}");
        let new = crate::routine::NewRoutine {
            title: "Lamp check".into(),
            schedule: "weekdays 09:00".into(),
            brief: "Look at the lamp.".into(),
            ..Default::default()
        };
        let prepared = crate::routine::prepare(
            &r.core.kernel,
            &r.core.runner.tools.routines.cfg,
            &new,
            &by,
            Some(&marked),
        )
        .unwrap();
        crate::routine::create(
            &r.core.kernel,
            &r.core.store,
            &r.core.runner.tools.routines,
            prepared,
            &by,
        )
        .unwrap();
        // The owner's reminder (set on the owner's word: `by` is their surface), in the marked session; a
        // model's from the same session (`by` is the action's id).
        let exec = |s: &str| {
            r.core
                .store
                .get_session::<SessionRecord>(s)
                .unwrap()
                .unwrap()
                .execution_id
                .unwrap()
        };
        let due = r.core.kernel.now_ms() + 7_200_000;
        let wake = |note: &str| NewWake {
            due_at_ms: due,
            note: note.into(),
            target: None,
            repeat: None,
            purpose: Purpose::Remind,
            run: WakeRun::Notice,
        };
        r.core
            .kernel
            .create_wake(&exec(&marked), None, wake("water the ferns"), "the CLI")
            .unwrap();
        r.core
            .kernel
            .create_wake(
                &exec(&marked),
                None,
                wake("feed the gulls"),
                "act_01abcdef0123456789",
            )
            .unwrap();
        let text = said(&r, &here, json!({"what": ["routines", "coming"]})).await;
        let held = |w: &str| text.lines().any(|l| l.contains(w));
        // The owner's routine is given whatever; a model's, and a reminder, follow the session's mark.
        assert!(held("Tide check"), "{restricted}: {text}");
        for words in ["Lamp check", "water the ferns", "feed the gulls"] {
            assert_eq!(
                held(words),
                !restricted,
                "{words}, restricted {restricted}: {text}"
            );
        }
        assert_eq!(text.contains("held back"), restricted, "{text}");
        // Taken on only where the model's marked words were given.
        assert_eq!(flags(&r, &here).is_some(), !restricted, "{restricted}");
    }
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

/// The project view is read to a limit: the newest are built and the older counted and said.
#[tokio::test]
async fn the_project_view_is_read_to_a_limit_and_says_how_many_older_it_left() {
    let r = rig();
    let here = conversation(&r).await;
    for i in 0..14 {
        r.core
            .project_create(
                &serde_json::from_value(json!({
                    "title": format!("Harbour survey {i:02}"), "objective": "Chart every mooring",
                    "acceptance": ["every mooring has a depth"],
                    "milestones": [{"title": "Outer harbour", "due": "in 30d"}],
                    "review": "none"
                }))
                .unwrap(),
                "the CLI",
            )
            .unwrap();
    }
    let (shown, older) = r.core.projects_upto(12).unwrap();
    assert_eq!((shown.len(), older), (12, 2));
    assert_eq!(
        r.core
            .project_list(&Default::default())
            .unwrap()
            .projects
            .len(),
        14,
        "the command lists them all"
    );
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(text.contains("Projects (14 open)."), "{text}");
    assert!(
        text.contains("… and 2 more older projects not listed here"),
        "{text}"
    );
    assert!(
        text.contains("Harbour survey 13") && !text.contains("Harbour survey 00"),
        "{text}"
    );
}

/// A project the model makes in `sid`, by `task_create`; `sid` back.
async fn model_project(r: &Rig, sid: String) -> String {
    r.turn_with(
        "task_create",
        &sid,
        json!({"kind": "project", "title": "Harbour survey", "review": "none",
               "milestones": [{"title": "Outer harbour", "due": "in 30d"}]}),
    )
    .await;
    let made = crate::task_graph::all(&r.core.store)
        .unwrap()
        .into_iter()
        .find(|t| t.kind == theseus_protocol::tasks::TaskKind::Project)
        .expect("the model made the project");
    assert!(made.origin.by_model, "{:?}", made.origin);
    assert_eq!(made.origin.session, sid);
    sid
}

/// A project a model made in a conversation (`task_create` with `kind: "project"`) has a title that
/// conversation's model wrote, not the owner's: it is that conversation's words, held back from a restricted
/// turn and taken on by an unrestricted one, as a task's title is. Review finding (issue 28, after landing):
/// `horizon::project_line` gives `p.title` as the owner's whatever made it.
#[tokio::test]
async fn a_project_a_model_made_has_that_conversations_words_for_its_title() {
    // Restricted: held back.
    let r = restricted_rig();
    let wrote = model_project(&r, conversation(&r).await).await;
    mark(&r, &wrote);
    let here = conversation(&r).await;
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(!text.contains("Harbour survey"), "{text}");
    assert!(text.contains("held back"), "{text}");
    // Unrestricted: given, and that conversation's flags are taken on.
    let r = rig();
    let wrote = model_project(&r, conversation(&r).await).await;
    mark(&r, &wrote);
    let here = conversation(&r).await;
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(text.contains("Harbour survey"), "{text}");
    assert!(flags(&r, &here).is_some(), "taken on");
}

/// Review note 1: when the flags the look would take on cannot be written, the look gives no words. (The
/// owner's removal of the flag from this session, dated after the look, makes the write a no-op: a
/// stand-in for a failed write that needs no broken store.)
#[tokio::test]
async fn a_look_whose_flags_cannot_be_written_gives_no_words() {
    use crate::content::{ContentFlags, Removed};
    let r = rig();
    let (here, other) = (conversation(&r).await, conversation(&r).await);
    task(&r, &other, "Chart the outer reef", None);
    mark(&r, &other);
    let mut rec: SessionRecord = r.core.store.get_session(&here).unwrap().unwrap();
    rec.data = Some(Box::new(ContentFlags {
        labels: vec![],
        removed: vec![Removed {
            flag: Flag::Confidential,
            by: "the owner".into(),
            via: "cli".into(),
            at_ms: u64::MAX,
        }],
    }));
    r.core.store.put_session(&here, &rec).unwrap();
    r.turn_with("look_work", &here, json!({"what": ["tasks"]}))
        .await;
    let (status, text) = latest(&r, &here);
    assert_eq!(status, crate::node::ResultStatus::Error, "{text}");
    assert!(text.contains("could not take on the flags"), "{text}");
    assert!(!text.contains("Chart the outer reef"), "{text}");
}

/// Review note 2: with content judging switched off no source raises a label, so none is taken and the look
/// still answers: `cn_models` alone decides what a restricted model is sent.
#[tokio::test]
async fn with_content_judging_off_the_look_takes_nothing_and_still_answers() {
    let r = rig_with(|cfg, _| {
        cfg.content_based_security.session_flags = false;
        cfg.content_based_security.cn_models =
            crate::config::content_based_security::CnModels::Always;
    });
    let (here, other) = (conversation(&r).await, conversation(&r).await);
    task(&r, &other, "Chart the outer reef", None);
    mark(&r, &other);
    let text = said(&r, &here, json!({"what": ["tasks"]})).await;
    assert!(text.contains("Chart the outer reef"), "{text}");
    assert!(flags(&r, &here).is_none());
}

/// A task's ended reason was written in the task's own session, which carries what it read: its labels mark
/// those words, not its parent's. The title (the brief's first line, written in the parent) follows the parent.
#[tokio::test]
async fn a_tasks_ended_reason_is_the_words_of_its_own_session() {
    let r = restricted_rig();
    let (here, parent) = (conversation(&r).await, conversation(&r).await);
    let t = task(&r, &parent, "Chart the outer reef", None);
    r.core
        .task_cancel_by(&t.session_id, "the owner said stop")
        .await
        .unwrap();
    let open = said(&r, &here, json!({"what": ["tasks"]})).await;
    assert!(open.contains("the owner said stop"), "{open}");
    mark(&r, &t.session_id);
    let text = said(&r, &here, json!({"what": ["tasks"]})).await;
    let line = text
        .lines()
        .find(|l| l.contains("Chart the outer reef"))
        .unwrap();
    assert!(
        line.contains("held back") && !line.contains("the owner said stop"),
        "{line}"
    );
}
