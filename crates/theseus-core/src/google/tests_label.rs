//! A Google result is a label source (theseus-gyjyo.5): a result raises
//! `confidential` (a sensitive calendar's raises `personal`) where it is
//! written, with source `google.<tool>`, and the operator removes it like any
//! other. Nothing else marks a session: no META record, no task-local scope, no
//! lineage walk, no recall follow. These six tests replace `tests_private_*`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use theseus_google::fake::{Fake, REFRESH_TOKEN};
use theseus_judge::fake::FakeJev;
use theseus_protocol::SessionKind;

use super::tests::{call, config, CLIENT};
use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::secrets::{Secret, SecretBoard};
use crate::session::SessionRecord;
use crate::tests_route::{rig_parts, Rig as RouteRig};
use crate::turn::TurnRequest;

/// `[google]` bound to an account (nothing is ever called), the default
/// provider as it is: for the rigs that only need the config to bind one.
pub(crate) fn google_on(c: &mut crate::Config) {
    c.google = crate::config::GoogleConfig {
        account: super::tests::ACCOUNT.into(),
        credentials: crate::config::GoogleCredentialNames::default(),
        ..Default::default()
    };
    c.secrets.insert(
        "google_client".into(),
        "op://vault/google client/notesPlain".into(),
    );
    c.secrets.insert(
        "google_refresh_token".into(),
        "file:/tmp/unused-google-refresh-token".into(),
    );
}

fn board() -> Arc<SecretBoard> {
    let b = SecretBoard::new(
        ["jev_api_key", "google_client", "google_refresh_token"].map(String::from),
        Instant::now(),
    );
    b.publish(
        BTreeMap::from([
            (
                "jev_api_key".to_string(),
                Ok(Secret::new("jev-test-key-0123456789".into())),
            ),
            ("google_client".to_string(), Ok(Secret::new(CLIENT.into()))),
            (
                "google_refresh_token".to_string(),
                Ok(Secret::new(REFRESH_TOKEN.into())),
            ),
        ]),
        "test",
    );
    b
}

/// The route rig of `tests_route` (`anthropic` and `zai`, the latter a
/// China-origin endpoint), with `[google]` pointed at `fake`, and the
/// Anthropic fake scripted by `script`.
fn rig(jev: Option<&FakeJev>, fake: &Fake, script: Vec<Scripted>) -> (RouteRig, Arc<FakeProvider>) {
    rig_on(jev, fake, script, false)
}

/// [`rig`], with `zai`'s `glm53` a Claude served from a host vouched EU when
/// `eu`: of no restricted origin, so only the floor for private content
/// keeps a turn off it (theseus-a1wvq.3 keeps every request off GLM itself).
fn rig_on(
    jev: Option<&FakeJev>,
    fake: &Fake,
    script: Vec<Scripted>,
    eu: bool,
) -> (RouteRig, Arc<FakeProvider>) {
    rig_tweaked(jev, fake, script, eu, |_| {})
}

/// [`rig_on`], with `tweak` applied to its config last.
pub(super) fn rig_tweaked(
    jev: Option<&FakeJev>,
    fake: &Fake,
    script: Vec<Scripted>,
    eu: bool,
    tweak: impl FnOnce(&mut crate::Config) + 'static,
) -> (RouteRig, Arc<FakeProvider>) {
    let claude = Arc::new(FakeProvider::scripted(script));
    let dir = tempfile::tempdir().unwrap();
    let google = config(dir.path(), fake, "file:/tmp/unused-google-refresh-token").google;
    let (c2, url) = (claude.clone(), fake.url());
    let r = rig_parts(
        jev,
        0,
        move |c| {
            c.google = google;
            c.google.endpoint = Some(url);
            c.secrets.insert(
                "google_client".into(),
                "op://vault/google client/notesPlain".into(),
            );
            c.secrets.insert(
                "google_refresh_token".into(),
                "file:/tmp/unused-google-refresh-token".into(),
            );
            if eu {
                c.serve_glm_unrestricted();
                c.vouch_zai(crate::catalog::ModelOrigin::Eu);
            }
            tweak(c);
        },
        move |p| {
            // After the load's check, which refuses a restricted profile in a
            // mode's list (theseus-a1wvq.4): the floor holds on one anyway.
            p.cfg.routing.classes.routine_coding.profiles = vec!["glm53".into(), "sonnet".into()];
            p.secrets = board();
            p.providers.insert("anthropic".into(), c2);
        },
    );
    (r, claude)
}

pub(super) async fn run_on(
    core: &Arc<crate::Core>,
    sid: &str,
    input: &str,
    profile: Option<&str>,
) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core
        .runner
        .resolve_target(&live, profile, None, None)
        .unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
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
}

pub(super) fn new_session(core: &crate::Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

use super::tests_calendar_cache::{ev, install, two, NOON, PERSONAL};
use crate::approval::{Answerer, Surface};
use crate::content::{ContentFlags, Flag, ItemKind};
use crate::node::Node;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// The session's content flags, as stored.
fn flags(core: &crate::Core, sid: &str) -> ContentFlags {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    rec.data.map(|b| *b).unwrap_or_default()
}

/// The last tool result `tool` wrote in `sid`.
fn result_of(core: &crate::Core, sid: &str, tool: &str) -> Node {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .rfind(|n| matches!(&n.body, crate::node::Body::ToolResult { tool: t, .. } if t == tool))
        .unwrap_or_else(|| panic!("no {tool} result in {sid}"))
}

/// Send `req` to the restricted provider as a call made for `sid`.
async fn send(
    core: &crate::Core,
    sid: &str,
    req: &crate::provider::ProviderRequest,
) -> anyhow::Result<()> {
    let zai = &core.runner.providers["zai"];
    let mut sink = |_: crate::provider::Delta<'_>| {};
    let made = crate::data_gate::for_session(sid, || zai.stream_message(req, &mut sink));
    made.await.map(drop)
}

fn read_files() -> Vec<Scripted> {
    vec![
        call("c1", json!({"method": "drive.files.list"})),
        Scripted::text("Done."),
    ]
}

fn files(fake: &Fake) {
    fake.json(
        "GET",
        "/drive/v3/files",
        json!({"files": [{"id": "f1", "name": "Quarterly plan"}]}),
    );
}

fn read_calendar(id: &str, which: &str) -> Scripted {
    Scripted::tools("", &[(id, "calendar_events", json!({"calendar": which}))])
}

/// Test 1. A Google result raises `confidential`, where it is written, from
/// the tool that made it: no model call (the rig has no Jev), one label, its
/// node the result's, its source `google.<tool>`, and the operator told as for
/// any label (a `content.labelled` row). No META record marks the session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_google_result_raises_confidential_from_its_tool() {
    let fake = Fake::start().await;
    files(&fake);
    let (r, _) = rig(None, &fake, read_files());
    let sid = new_session(&r.core);
    assert!(flags(&r.core, &sid).labels.is_empty());
    run_on(&r.core, &sid, "list my files", None).await.unwrap();
    let node = result_of(&r.core, &sid, "google.call");
    let f = flags(&r.core, &sid);
    assert_eq!(f.labels.len(), 1, "one label for the flag: {:?}", f.labels);
    let l = &f.labels[0];
    assert_eq!(l.flag, Flag::Confidential);
    assert_eq!(l.kind, ItemKind::ToolResult);
    assert_eq!(l.source, "google.call");
    assert_eq!(l.node.as_deref(), Some(node.id.as_str()));
    assert!(l.judgment.is_empty(), "no model raised it: {l:?}");
    let rows = super::tests::ledgered(&r.core, "content.labelled");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["source"], "google.call");
    assert!(
        r.core
            .store
            .get_meta::<String>(&format!("google.private.{sid}"))
            .unwrap()
            .is_none(),
        "the old marker writes no record"
    );
    // `theseus content` (session.list's labels) shows the source, and the
    // wire's data mark reads the label.
    let all: theseus_protocol::SessionListResult =
        serde_json::from_value(crate::tests_route::call(&r.core, "session.list", json!({})).await)
            .unwrap();
    let listed = all
        .sessions
        .into_iter()
        .find(|s| s.session_id == sid)
        .unwrap();
    assert_eq!(listed.labels.len(), 1, "{:?}", listed.labels);
    assert_eq!(listed.labels[0].source, "google.call");
    assert_eq!(listed.data, Some(theseus_protocol::ops::DataMark::Private));
    // A session that read nothing carries nothing.
    let other = new_session(&r.core);
    assert!(flags(&r.core, &other).labels.is_empty());
}

/// Test 2. The label is what keeps a model of restricted origin off the
/// session, and Remove is what lets one on: the next turn pinned to it is
/// refused with nothing sent, the chokepoint names the flag, and once the
/// operator removes it the same request is sent. (This is P3, deliberately.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_model_is_refused_by_the_label_and_sent_after_remove() {
    let fake = Fake::start().await;
    files(&fake);
    let (r, _) = rig(None, &fake, read_files());
    let sid = new_session(&r.core);
    run_on(&r.core, &sid, "list my files", None).await.unwrap();
    let ended = run_on(&r.core, &sid, "and now?", Some("glm53")).await;
    assert!(format!("{ended:?}").contains("data_gate"), "{ended:?}");
    assert!(r.zai.requests().is_empty(), "nothing went to the model");
    // The chokepoint's own word names the label.
    let req = crate::provider::ProviderRequest {
        model: "glm-5.3".into(),
        max_tokens: 16,
        system: vec![],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": "hi"}]})],
        tools: vec![],
        thinking: None,
        output_config: None,
        cache_control: None,
        betas: vec![],
        extra: Default::default(),
        image_tokens: 0,
    };
    let refused = send(&r.core, &sid, &req).await.unwrap_err().to_string();
    assert!(refused.contains("`confidential` label"), "{refused}");
    // The operator removes it.
    r.core.unlabel_session(&sid, "confidential", cli()).unwrap();
    send(&r.core, &sid, &req)
        .await
        .expect("after Remove it is sent");
    assert_eq!(r.zai.requests().len(), 1);
}

/// Test 3. A sensitive calendar's read raises `personal`, an ordinary one's
/// `confidential`; the source is the tool's, the calendar named nowhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_calendar_raises_personal_and_an_ordinary_one_confidential() {
    let fake = Fake::start().await;
    let cals = install(&fake);
    cals.work.lock().unwrap().items = vec![ev("w1", "Board prep", NOON.0, NOON.1)];
    cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
    let (r, _) = rig_tweaked(
        None,
        &fake,
        vec![
            read_calendar("c1", "Work"),
            Scripted::text("Done."),
            read_calendar("c2", PERSONAL),
            Scripted::text("Done."),
        ],
        false,
        two,
    );
    let sensitive = new_session(&r.core);
    run_on(&r.core, &sensitive, "what is on my work calendar?", None)
        .await
        .unwrap();
    let f = flags(&r.core, &sensitive);
    assert_eq!(f.labels.len(), 1, "{:?}", f.labels);
    assert_eq!(f.labels[0].flag, Flag::Personal);
    assert_eq!(f.labels[0].source, "google.calendar.events");
    let plain = new_session(&r.core);
    run_on(&r.core, &plain, "what is on my personal calendar?", None)
        .await
        .unwrap();
    let f = flags(&r.core, &plain);
    assert_eq!(f.labels.len(), 1, "{:?}", f.labels);
    assert_eq!(f.labels[0].flag, Flag::Confidential);
    assert_eq!(f.labels[0].source, "google.calendar.events");
}

/// Test 4. A task carries its parent's labels from its first call, whether
/// the model started it (`task.create`) or the operator did (`task.start`):
/// `fork` already copied them; a child of a session that read a calendar must
/// not be a way around the label.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_carries_its_parents_labels() {
    use crate::tests_tasks::{only_task, parent_session, rig, turn, ASK};
    let r = rig(
        |req| {
            if crate::tests_tasks::first_user(req).contains("CHILD") {
                return Scripted::text("Done.");
            }
            match crate::tests_tasks::answers_a_call(req) {
                true => Scripted::text("Started."),
                false => Scripted::tools(
                    "Starting it.",
                    &[(
                        "t_task",
                        "task_create",
                        json!({"brief": "CHILD: count", "arrangement": crate::tests_tasks::arranged()}),
                    )],
                ),
            }
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    let mut labelled = ContentFlags::default();
    labelled.raise(crate::content::deterministic::label(
        Flag::Confidential,
        "nod_read",
        ItemKind::ToolResult,
        "google.call".into(),
        1,
    ));
    r.core
        .store
        .update_session(&parent, |rec| {
            rec.data = Some(Box::new(labelled.clone()));
            Ok(vec![])
        })
        .unwrap();
    turn(&r.core, &parent, ASK).await;
    let child = only_task(&r.core, &parent).session_id;
    let f = flags(&r.core, &child);
    assert_eq!(f.labels.len(), 1, "task.create: {:?}", f.labels);
    assert_eq!(f.labels[0].flag, Flag::Confidential);
    assert_eq!(f.labels[0].source, "google.call");
    // `task.start`, from the same parent as its home.
    let p = theseus_protocol::task_start::TaskStartParams {
        brief: "CHILD: second".into(),
        home: Some(parent.clone()),
        ..Default::default()
    };
    let started = r
        .core
        .task_start(&p, crate::tests_task_brief::cli())
        .unwrap();
    let f = flags(&r.core, &started.session_id);
    assert_eq!(f.labels.len(), 1, "task.start: {:?}", f.labels);
    // A task of a session with no label has none.
    let plain = parent_session(&r.core);
    turn(&r.core, &plain, "hello").await;
    let p = theseus_protocol::task_start::TaskStartParams {
        brief: "CHILD: third".into(),
        home: Some(plain),
        ..Default::default()
    };
    let started = r
        .core
        .task_start(&p, crate::tests_task_brief::cli())
        .unwrap();
    assert!(flags(&r.core, &started.session_id).labels.is_empty());
}

/// Test 5. A Remove sticks against the result that raised the label, and a
/// result written after it raises the label again, as any item does
/// (the review of theseus-gyjyo.4, C1: `land_deterministic` takes the entry
/// time of the item, and `raise_after` refuses what the operator removed after
/// it entered).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removed_google_label_returns_only_with_a_later_result() {
    let fake = Fake::start().await;
    files(&fake);
    let (r, _) = rig(
        None,
        &fake,
        vec![
            call("c1", json!({"method": "drive.files.list"})),
            Scripted::text("Done."),
            Scripted::text("Nothing more to read."),
            call("c2", json!({"method": "drive.files.list"})),
            Scripted::text("Read again."),
        ],
    );
    let sid = new_session(&r.core);
    run_on(&r.core, &sid, "list my files", None).await.unwrap();
    let first = flags(&r.core, &sid).labels[0].node.clone();
    r.core.unlabel_session(&sid, "confidential", cli()).unwrap();
    assert!(flags(&r.core, &sid).labels.is_empty());
    // A turn that reads nothing raises nothing.
    run_on(&r.core, &sid, "thanks", None).await.unwrap();
    assert!(flags(&r.core, &sid).labels.is_empty(), "Remove sticks");
    // A later read raises it again, from the new result.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    run_on(&r.core, &sid, "list them again", None)
        .await
        .unwrap();
    let f = flags(&r.core, &sid);
    assert_eq!(f.labels.len(), 1, "{:?}", f.labels);
    assert_ne!(f.labels[0].node, first, "from the later result");
    assert_eq!(f.removed.len(), 1, "the removal stays on the record");
}

/// Test 6. With `[content_based_security] session_flags = false` the source is
/// off with the mechanism: a Google read raises nothing, writes no marker and
/// tells the operator nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_session_flags_off_a_google_read_raises_nothing() {
    let fake = Fake::start().await;
    files(&fake);
    let (r, _) = rig_tweaked(
        None,
        &fake,
        vec![
            call("c1", json!({"method": "drive.files.list"})),
            Scripted::text("Done."),
        ],
        false,
        |c| {
            c.content_based_security.session_flags = false;
            // With nothing judged there are no labels to follow: the config
            // says what a model of restricted origin is sent.
            c.content_based_security.cn_models =
                crate::config::content_based_security::CnModels::Always;
        },
    );
    let sid = new_session(&r.core);
    run_on(&r.core, &sid, "list my files", None).await.unwrap();
    assert!(flags(&r.core, &sid).labels.is_empty());
    assert!(super::tests::ledgered(&r.core, "content.labelled").is_empty());
    assert!(r
        .core
        .store
        .get_meta::<String>(&format!("google.private.{sid}"))
        .unwrap()
        .is_none());
}

/// Test 7. The label's entry time is the result node's, not the moment it
/// lands: the time a Remove is compared against (`raise_after`). The label
/// itself is stamped when it was raised, so the two differ, and a write site
/// that passed the landing time for the entry time would let a Remove made
/// between the result's write and its label's landing be overridden by it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_labels_entry_time_is_the_result_nodes_and_not_when_it_lands() {
    let fake = Fake::start().await;
    files(&fake);
    let (r, _) = rig(None, &fake, read_files());
    let sid = new_session(&r.core);
    run_on(&r.core, &sid, "list my files", None).await.unwrap();
    let node = result_of(&r.core, &sid, "google.call");
    let later = node.created_at_ms + 60_000;
    let (label, entered_ms) = super::private::label(&node, later).expect("a Google result");
    assert_eq!(entered_ms, node.created_at_ms, "the node's entry time");
    assert_eq!(
        label.at_ms, later,
        "the label's own stamp is when it landed"
    );
    // A result that is no Google result raises nothing.
    let plain = Node::user(&sid, None, "test", "hello");
    assert!(super::private::label(&plain, later).is_none());
}

/// Test 8. A Remove sticks against the pack's verdict on the same Google
/// result. A Google result is both a deterministic label source and a content
/// item the pack judges, and a pack that lands after the operator's Remove
/// must not raise again from the item that entered before it (R3's C1, here on
/// a Google node, where R3's tests use a name's). The pack is held until the
/// Remove is made; its verdict is `confidential: yes`; it lands, and the flag
/// stays off, with the one `content.labelled` row the write site made.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_remove_sticks_against_the_packs_verdict_on_the_same_google_result() {
    use theseus_judge::fake::Scripted as Jev;
    let jev = FakeJev::start().unwrap();
    for q in ["injection", "secret", "confidential", "personal"] {
        jev.script(
            q,
            Jev::Choice {
                option: "no".into(),
                confidence: 0.95,
            },
        );
    }
    crate::tests_route::mode(&jev, "trivial", 0.95);
    // Only the call that reads the Google result's own text is held, and it
    // finds the result confidential.
    jev.script_when(
        "Quarterly plan",
        None,
        "confidential",
        Jev::Choice {
            option: "yes".into(),
            confidence: 0.95,
        },
    );
    jev.hold_when("Quarterly plan");
    let fake = Fake::start().await;
    files(&fake);
    let (r, _) = rig_tweaked(Some(&jev), &fake, read_files(), false, |c| {
        // content.v2 live (the rigs turn it off), the rest as it is.
        c.judge.packs.remove(crate::judge::content::PACK);
        c.judge.total_secs = 10;
    });
    let sid = new_session(&r.core);
    run_on(&r.core, &sid, "list my files", None).await.unwrap();
    // The write site raised it; the pack's verdict is still held.
    assert_eq!(flags(&r.core, &sid).labels.len(), 1);
    assert!(super::tests::ledgered(&r.core, "content.judged").is_empty());
    r.core.unlabel_session(&sid, "confidential", cli()).unwrap();
    let start = Instant::now();
    while super::tests::ledgered(&r.core, "content.judged").is_empty() {
        // The ask reaches Jev off the turn's path, some time after the turn:
        // release whatever is held until it has landed.
        jev.release();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "the pack never landed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let f = flags(&r.core, &sid);
    assert!(f.labels.is_empty(), "the removed flag came back: {f:?}");
    assert_eq!(f.removed.len(), 1);
    let told = super::tests::ledgered(&r.core, "content.labelled");
    assert_eq!(told.len(), 1, "only the write site's row: {told:?}");
    assert_eq!(told[0]["source"], "google.call");
}
