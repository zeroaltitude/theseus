//! The sensitive level (theseus-jid7t.1): a read of a calendar the config
//! marks `sensitive` marks the reading session, in the frame that writes the
//! result, one level above private. The mark follows what the private mark
//! follows (a task's parent, recall), the floors keep the session's turns and
//! its background calls on a model of US origin, and its content never goes to
//! a shared place.

use serde_json::json;
use theseus_google::fake::Fake;
use theseus_protocol::task_start::TaskStartParams;
use theseus_protocol::SessionKind;

use super::private::{self, Level};
use super::tests_calendar_cache::{ev, install, two, NOON, PERSONAL};
use super::tests_private::{new_session, rig_tweaked, run_on};
use crate::approval::{Answerer, Surface};
use crate::provider::Scripted;
use crate::tests_keep_warm::{build, one_hour, reads, session, turn};
use crate::tests_private_background::{google_on, private_on};

fn read(id: &str, which: &str) -> Scripted {
    Scripted::tools("", &[(id, "calendar_events", json!({"calendar": which}))])
}

/// Mark `sid` as the frame of a sensitive result does: both records.
fn mark_sensitive(core: &crate::Core, sid: &str) {
    core.store
        .put_meta(&private::meta_key(sid), &"nd_test")
        .unwrap();
    core.store
        .put_meta(&private::sensitive_key(sid), &"nd_test")
        .unwrap();
}

fn level_of(core: &crate::Core, sid: &str) -> Level {
    private::level(&core.store, &core.cfg, sid)
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A read of the sensitive calendar marks the session sensitive and private; a
/// read of the personal one marks it private alone. The next turn of the first
/// refuses a model of China origin and one of EU origin, and runs on a US one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_read_keeps_the_next_turn_on_a_us_model() {
    for eu in [false, true] {
        let fake = Fake::start().await;
        let cals = install(&fake);
        cals.work.lock().unwrap().items = vec![ev("w1", "Board prep", NOON.0, NOON.1)];
        cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
        let (r, _) = rig_tweaked(
            None,
            &fake,
            vec![
                read("c1", "Work"),
                Scripted::text("Done."),
                Scripted::text("On the US model."),
            ],
            eu,
            two,
        );
        let sensitive = new_session(&r.core);
        assert_eq!(level_of(&r.core, &sensitive), Level::Open);
        run_on(&r.core, &sensitive, "what is on my work calendar?", None)
            .await
            .unwrap();
        assert_eq!(level_of(&r.core, &sensitive), Level::Sensitive, "eu={eu}");
        assert!(private::session(&r.core.store, &r.core.cfg, &sensitive));
        assert!(private::sensitive_session(
            &r.core.store,
            &r.core.cfg,
            &sensitive
        ));
        // The next turn, pinned to the model that is not of US origin, is
        // refused before a byte is sent.
        let before = r.zai.requests().len();
        let refused = format!(
            "{:?}",
            run_on(&r.core, &sensitive, "and now?", Some("glm53")).await
        );
        assert!(refused.contains("data_gate"), "eu={eu}: {refused}");
        assert_eq!(r.zai.requests().len(), before, "eu={eu}: nothing was sent");
        // A US model is not held back.
        let ok = run_on(&r.core, &sensitive, "and now?", Some("sonnet")).await;
        assert!(ok.is_ok(), "eu={eu}: {ok:?}");
    }
}

/// A read of the personal calendar marks the session private and not
/// sensitive.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_personal_read_marks_private_and_not_sensitive() {
    let fake = Fake::start().await;
    let cals = install(&fake);
    cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
    let (r, _) = rig_tweaked(
        None,
        &fake,
        vec![read("c1", PERSONAL), Scripted::text("Done.")],
        false,
        two,
    );
    let sid = new_session(&r.core);
    run_on(&r.core, &sid, "what is on my calendar?", None)
        .await
        .unwrap();
    assert_eq!(level_of(&r.core, &sid), Level::Private);
    assert!(!private::sensitive_session(
        &r.core.store,
        &r.core.cfg,
        &sid
    ));
    // Neither calendar's turn is marked by a read of the other.
    let other = new_session(&r.core);
    assert_eq!(level_of(&r.core, &other), Level::Open);
}

/// A Calendar read through `google.call` marks the session sensitive when a
/// configured calendar is: the long tail cannot say which calendar it read, so
/// it fails closed (the join fix J1 of theseus-jid7t.1).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_calendar_read_through_google_call_is_sensitive() {
    let fake = Fake::start().await;
    let cals = install(&fake);
    cals.work.lock().unwrap().items = vec![ev("w1", "Board prep", NOON.0, NOON.1)];
    let input = json!({
        "method": "calendar.events.list",
        "params": {"calendarId": super::tests_calendar_cache::WORK},
    });
    let (r, _) = rig_tweaked(
        None,
        &fake,
        vec![
            Scripted::tools("", &[("c1", "google_call", input)]),
            Scripted::text("Done."),
        ],
        false,
        two,
    );
    let sid = new_session(&r.core);
    run_on(&r.core, &sid, "what is on my work calendar?", None)
        .await
        .unwrap();
    assert_eq!(level_of(&r.core, &sid), Level::Sensitive);
}

/// A background call for a sensitive session, made through `private::bound`,
/// is refused at the chokepoint for a model that is not of US origin: here the
/// default provider is served from a host vouched EU. The session holds the
/// sensitive record alone, which the base does not know.
#[tokio::test]
async fn a_background_call_of_a_sensitive_session_is_refused_at_the_chokepoint() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    private_on(&mut c);
    let (core, fake) = build(dir.path(), c);
    let (sensitive, plain) = (
        session(&core, SessionKind::Conversation, None),
        session(&core, SessionKind::Conversation, None),
    );
    turn(&core, &sensitive, "the first conversation").await;
    turn(&core, &plain, "the second conversation").await;
    core.store
        .put_meta(&private::sensitive_key(&sensitive), &"nd_test")
        .unwrap();
    assert_eq!(level_of(&core, &sensitive), Level::Sensitive);
    let (live, _) = core.live_profile();
    let out = core.runner.keep_warm_once(&sensitive, &live).await;
    assert!(
        matches!(out, crate::keep_warm::Outcome::Stopped(_)),
        "{out:?}"
    );
    assert!(
        reads(&fake).is_empty(),
        "nothing was sent: {:?}",
        reads(&fake)
    );
    let out = core.runner.keep_warm_once(&plain, &live).await;
    assert!(
        matches!(out, crate::keep_warm::Outcome::Read { .. }),
        "{out:?}"
    );
}

/// A task under a sensitive session is sensitive from its first call, and one
/// that was started before its parent took the mark reads it through its
/// lineage.
#[tokio::test]
async fn a_task_under_a_sensitive_session_is_sensitive() {
    let r = crate::tests_tasks::rig(|_| Scripted::text("ok"), private_on);
    let home = r.core.home().unwrap().session_id;
    let start = |brief: &str| TaskStartParams {
        brief: brief.into(),
        ..Default::default()
    };
    let early = r.core.task_start(&start("Count the boats"), cli()).unwrap();
    assert_eq!(level_of(&r.core, &early.session_id), Level::Open);
    mark_sensitive(&r.core, &home);
    let child = r.core.task_start(&start("Count the quays"), cli()).unwrap();
    assert_eq!(level_of(&r.core, &child.session_id), Level::Sensitive);
    assert!(private::sensitive_session(
        &r.core.store,
        &r.core.cfg,
        &child.session_id
    ));
    // The task that began first has no mark of its own, and its lineage has.
    assert!(!private::sensitive_session(
        &r.core.store,
        &r.core.cfg,
        &early.session_id
    ));
    assert_eq!(level_of(&r.core, &early.session_id), Level::Sensitive);
    assert!(private::sensitive_lineage(
        &r.core.store,
        &r.core.cfg,
        &early.session_id
    ));
    // A task of a merely private session is private, not sensitive.
    let other = r.core.task_start(&start("Count the weirs"), cli()).unwrap();
    assert_eq!(
        level_of(&r.core, &other.session_id),
        Level::Sensitive,
        "same parent"
    );
}

/// A session that recalls a note of a sensitive session becomes sensitive in
/// that turn, and the turn is routed off the model first in the mode's list
/// when it is not of US origin. A recall of a merely private note is private.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_that_recalls_a_sensitive_note_becomes_sensitive() {
    use crate::tests_route::mode;
    for (marked, want_level, want_profile) in [
        ("none", Level::Open, "glm53"),
        ("private", Level::Private, "sonnet"),
        ("sensitive", Level::Sensitive, "sonnet"),
    ] {
        let jev = theseus_judge::fake::FakeJev::start().unwrap();
        mode(&jev, "routine_coding", 0.95);
        let fake = Fake::start().await;
        let (r, _) = rig_tweaked(Some(&jev), &fake, vec![], true, |c| {
            c.memory.mode = crate::config::MemoryMode::Canary;
            c.memory.canary_fraction = 1.0;
        });
        let note = "Remember: the billing module's call sites live in ledger.rs.";
        let a = crate::tests_recall::session(&r.core, None, &[note]);
        match marked {
            "private" => r
                .core
                .store
                .put_meta(&private::meta_key(&a), &"nd_test")
                .unwrap(),
            "sensitive" => mark_sensitive(&r.core, &a),
            _ => {}
        }
        let b = new_session(&r.core);
        r.core
            .runner
            .memory
            .set_ask(crate::tests_recall::index_of(&r.core, vec![a]));
        let msg = "Rename the call sites in the billing module.";
        let res = run_on(&r.core, &b, msg, None).await.unwrap();
        assert_eq!(res.recalled, 1, "{marked}");
        assert_eq!(res.profile, want_profile, "{marked}: {}", res.output);
        assert_eq!(level_of(&r.core, &b), want_level, "{marked}");
    }
}

/// The records a marked result adds to its frame: private for a private
/// result, and a second, sensitive one beside it; a later private mark never
/// lowers the level, since it is another key.
#[test]
fn a_sensitive_result_adds_a_second_record_and_nothing_lowers_it() {
    use crate::node::{Body, Node, ResultStatus};
    let result = |meta: serde_json::Value| {
        Node::tool_result(
            "s1",
            None,
            None,
            Body::ToolResult {
                tool_use_id: "c".into(),
                tool: "calendar.events".into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: "x".into(),
                correlation_id: None,
                bytes_total: 1,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta,
                image: None,
                external: None,
            },
        )
    };
    let keys = |meta| {
        private::record(&result(meta), "s1")
            .iter()
            .map(|r| r.key.clone().unwrap_or_default())
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(json!({})), Vec::<String>::new());
    assert_eq!(keys(json!({"private": true})), [private::meta_key("s1")]);
    assert_eq!(
        keys(json!({"private": true, "sensitive": true})),
        [private::meta_key("s1"), private::sensitive_key("s1")]
    );
    assert_eq!(private::meta_key("s1"), "google.private.s1");
    assert_eq!(private::sensitive_key("s1"), "google.sensitive.s1");
}

/// Content of a sensitive session never goes to a shared place, whoever asks;
/// a session that is merely private publishes as before, and so does a
/// sensitive one into a private place.
#[tokio::test]
async fn a_sensitive_node_is_not_published_to_a_shared_place() {
    use crate::places::BoundPlace;
    use crate::tests_places::{rig_with, session as place_session, LAB};
    let r = rig_with(google_on);
    r.core.bind_places(vec![
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: false,
            ..Default::default()
        },
        BoundPlace {
            target: "discord:channel:424242".into(),
            name: "#den".into(),
            private: true,
            ..Default::default()
        },
    ]);
    for place in [format!("channel:{LAB}"), "channel:424242".to_string()] {
        let sid = place_session(&r.core, Some(&place));
        crate::tests_places::turn(&r.core, &sid, "hello").await;
    }
    let node_in = |sid: &str, text: &str| {
        let n = crate::node::Node::user(sid, None, "test", text);
        r.core.store.append(&[n.record().unwrap()]).unwrap();
        n.id
    };
    let (sens, privy, plain) = (
        new_session(&r.core),
        new_session(&r.core),
        new_session(&r.core),
    );
    mark_sensitive(&r.core, &sens);
    r.core
        .store
        .put_meta(&private::meta_key(&privy), &"nd_test")
        .unwrap();
    let publish = |id: &str, to: &str| {
        r.core.publish(
            &theseus_protocol::PlacePublishParams {
                node_id: Some(id.into()),
                to: to.into(),
                ..Default::default()
            },
            "cli",
        )
    };
    let id = node_in(&sens, "the Board prep meeting is moved");
    let e = publish(&id, "#lab").unwrap_err().to_string();
    assert!(
        e.contains("sensitive content never goes to a shared place"),
        "{e}"
    );
    assert!(!e.contains("Board prep"), "{e}");
    assert!(crate::test_support::rows(&r.core, "place.published").is_empty());
    let den = publish(&id, "#den");
    assert!(den.is_ok(), "a private place may have it: {den:?}");
    assert!(publish(&node_in(&privy, "private only"), "#lab").is_ok());
    assert!(publish(&node_in(&plain, "plain"), "#lab").is_ok());
}

/// The calendar tools are not offered in a shared place at all, so a Discord
/// channel that others can read never reads a calendar.
#[test]
fn a_shared_place_is_offered_no_calendar_tool() {
    use crate::places::{offered, refusal};
    use theseus_protocol::{PlaceClass, Plan};
    for name in ["calendar.events", "calendar.put", "google.call"] {
        assert!(!offered(PlaceClass::Shared, name, &[]), "{name}");
        let why = refusal(PlaceClass::Shared, name, &Plan::default(), &[]).unwrap();
        assert!(why.contains("not offered in a shared place"), "{why}");
        assert!(offered(PlaceClass::Private, name, &[]));
    }
}
