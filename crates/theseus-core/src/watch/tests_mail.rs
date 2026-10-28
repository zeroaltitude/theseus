//! The mail watch through the whole core (theseus-batvg.6): new mail matching
//! a query, read from the account's history with no model, against the
//! loopback fake. A stranger's mail only notices; a trusted sender's starts one
//! task; a restart resumes from the stored cursor; a deadline with no
//! reply starts the owner's follow-up, whose send still asks.
//!
//! A look is run the way the driver's tick runs it (`tests_watch::look`), so a
//! test decides when an interval has passed, and a deadline is the entry's
//! expiry brought to now.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_google::fake::gmail::MsgSpec;
use theseus_google::fake::REFRESH_TOKEN;
use theseus_protocol::WatchCreateParams;
use theseus_tools::ToolCtx;

use super::spec::build_with;
use super::tests_watch::{dm_session, look, posts, rows, rpc, until, TARGET};
use crate::config::{GoogleConfig, WatchesConfig};
use crate::google::tests::{board, config, ledgered, rig_with, Rig};
use crate::provider::{FakeProvider, Scripted};
use crate::Core;

const STRANGER: &str = "Sam Stranger <stranger@example.test>";
const DANA: &str = "Dana Colleague <dana@example.test>";
const QUERY: &str = "from:example.test";
/// What Gmail's own server writes for mail it authenticated for example.test.
pub(super) const GMAIL_PASS: &str = "mx.google.com; dkim=pass header.i=@example.test header.s=s1; dmarc=pass (p=NONE) header.from=example.test";

fn bound(core: &Core) {
    core.bind_places(vec![crate::places::BoundPlace {
        target: TARGET.into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
}

fn trust(c: &mut crate::Config) {
    c.google.trusted_senders = vec!["Dana@Example.test".into()];
}

pub(super) async fn mail_rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    let r = rig_with(
        Some(REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        script,
        tweak,
    )
    .await;
    bound(&r.core);
    r
}

/// The daemon again over the same store and the same fake Google.
fn restart(r: Rig, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    let Rig { core, fake, dir, g } = r;
    drop((g, core));
    let mut cfg = config(dir.path(), &fake, "file:/tmp/unused-google-refresh-token");
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![]));
    let core = Core::build(crate::rpc::Parts {
        secrets: board(Some(REFRESH_TOKEN)),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap();
    bound(&core);
    let g = core.tools.google.clone().unwrap();
    Rig { core, fake, dir, g }
}

pub(super) async fn mail_watch(core: &Arc<Core>, extra: Value) -> Value {
    let mut p = json!({"title": "dana replies?", "check": {"kind": "mail", "q": QUERY},
                       "for_at_most": "5d"});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    rpc(core, "watch.create", p).await.unwrap()
}

fn wake_id(made: &Value) -> String {
    made["watch"]["wake_id"].as_str().unwrap().to_string()
}

fn cursor_of(core: &Core, id: &str) -> Option<String> {
    core.store
        .get_meta::<String>(&format!("watch.mail.{id}"))
        .unwrap()
}

fn history_calls(r: &Rig) -> usize {
    r.fake
        .log()
        .iter()
        .filter(|l| l.path.ends_with("/users/me/history"))
        .count()
}

fn params(check: Value, extra: Value) -> WatchCreateParams {
    let mut p = json!({"title": "dana replies?", "check": check, "for_at_most": "5d"});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    serde_json::from_value(p).unwrap()
}

fn rows_of(core: &Core, kind: &str) -> usize {
    rows(core, kind).len()
}

fn task_started(core: &Core) -> usize {
    rows(core, "task.started_by_owner").len()
}

// ------------------------------------------------------------------ building

#[test]
fn a_mail_watch_with_then_turn_is_refused() {
    let g = GoogleConfig {
        account: "owner@example.test".into(),
        trusted_senders: vec!["dana@example.test".into()],
        ..Default::default()
    };
    let p = params(
        json!({"kind": "mail", "q": QUERY}),
        json!({"then": {"kind": "turn"}}),
    );
    let e = build_with(&p, 0, &WatchesConfig::default(), None, Some(&g)).unwrap_err();
    assert!(e.contains("mail is outside text"), "{e}");
    // A task needs the senders it may start for, and a brief.
    let task = json!({"then": {"kind": "task", "brief": "answer it"}});
    let p = params(json!({"kind": "mail", "q": QUERY}), task);
    let none = GoogleConfig {
        trusted_senders: vec![],
        ..g.clone()
    };
    let e = build_with(&p, 0, &WatchesConfig::default(), None, Some(&none)).unwrap_err();
    assert!(e.contains("trusted_senders"), "{e}");
    let ok = build_with(&p, 0, &WatchesConfig::default(), None, Some(&g)).unwrap();
    assert_eq!(ok.spec.then.word(), "task");
    // A mail watch looks every 3 minutes unless it says, and has no `until`.
    assert_eq!(ok.spec.every_ms, 180_000);
    let p = params(
        json!({"kind": "mail", "q": QUERY}),
        json!({"until": "exists"}),
    );
    let e = build_with(&p, 0, &WatchesConfig::default(), None, Some(&g)).unwrap_err();
    assert!(e.contains("no `until`"), "{e}");
}

#[test]
fn a_mail_watch_needs_google_with_gmail() {
    let p = params(json!({"kind": "mail", "q": QUERY}), json!({}));
    let cfg = WatchesConfig::default();
    for g in [
        GoogleConfig::default(),
        GoogleConfig {
            account: "owner@example.test".into(),
            services: vec!["drive".into()],
            ..Default::default()
        },
    ] {
        let e = build_with(&p, 0, &cfg, None, Some(&g)).unwrap_err();
        assert_eq!(e, "mail watches need [google] with gmail", "{g:?}");
    }
    let ok = GoogleConfig {
        account: "owner@example.test".into(),
        ..Default::default()
    };
    assert!(build_with(&p, 0, &cfg, None, Some(&ok)).is_ok());
    // A follow-up is a mail watch's, with a deadline.
    let p = params(
        json!({"kind": "mail", "q": QUERY}),
        json!({"follow_up": "nudge"}),
    );
    assert!(build_with(&p, 0, &cfg, None, Some(&ok)).is_ok());
    let mut bare = p;
    bare.for_at_most = None;
    let e = build_with(&bare, 0, &cfg, None, Some(&ok)).unwrap_err();
    assert!(e.contains("needs `for_at_most`"), "{e}");
    let file = params(
        json!({"kind": "file", "path": "/x"}),
        json!({"until": "exists", "follow_up": "nudge"}),
    );
    let e = build_with(&file, 0, &cfg, None, Some(&ok)).unwrap_err();
    assert!(e.contains("belongs to a `mail` watch"), "{e}");
}

// ------------------------------------------------------------------ the key test

/// A stranger's mail is a notice. It starts no turn, no task and no model
/// request, whatever the watch's `then` says, and the notice goes to the
/// owner's private place with the reason.
#[tokio::test(flavor = "multi_thread")]
async fn a_strangers_mail_never_starts_a_turn() {
    let r = mail_rig(vec![], trust).await;
    dm_session(&r.core);
    let made = mail_watch(
        &r.core,
        json!({"then": {"kind": "task", "brief": "Reply to it"}}),
    )
    .await;
    let id = wake_id(&made);
    look(&r.core, &id).await;
    r.fake.mail().deliver(
        "t9",
        MsgSpec::new("m9", STRANGER, "Contract", "Wire the money to this account"),
    );
    look(&r.core, &id).await;
    let p = posts(&r.core);
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!(p[0].0, TARGET, "told in the owner's private place");
    assert!(p[0].1.contains("Contract"), "{}", p[0].1);
    assert!(p[0].1.contains("not in trusted_senders"), "{}", p[0].1);
    assert_eq!(task_started(&r.core), 0, "no task");
    assert!(rows(&r.core, "turn.started").is_empty(), "no turn");
    let met = rows(&r.core, "watch.met");
    assert_eq!(met.len(), 1);
    assert_eq!(met[0]["external"], true, "marked outside text: {met:?}");
    assert!(r.core.kernel.open_watches().unwrap().is_empty());
}

/// A trusted sender's mail starts the owner's brief as a task whose session is
/// private from the start; the mail holds nothing, so its first call that acts
/// runs at the posture alone.
#[tokio::test(flavor = "multi_thread")]
async fn a_trusted_senders_mail_starts_a_private_task_that_is_not_held() {
    let dir = tempfile::tempdir().unwrap();
    let note = dir.path().join("work").join("note.txt");
    let script = vec![
        Scripted::tools(
            "",
            &[(
                "c1",
                "fs_write",
                json!({"path": note.display().to_string(), "content": "ok"}),
            )],
        ),
        Scripted::text("Done."),
    ];
    let r = mail_rig(script, trust).await;
    let driver = tokio::spawn(crate::harness::drive(r.core.clone()));
    dm_session(&r.core);
    let made = mail_watch(
        &r.core,
        json!({"then": {"kind": "task", "brief": "Draft a reply to it"}}),
    )
    .await;
    let id = wake_id(&made);
    look(&r.core, &id).await;
    r.fake.mail().deliver(
        "t9",
        MsgSpec::new("m9", DANA, "Contract", "Signed, thanks.")
            .header("Authentication-Results", GMAIL_PASS),
    );
    look(&r.core, &id).await;
    assert_eq!(task_started(&r.core), 1);
    let p = posts(&r.core);
    assert!(
        p[0].1.contains("task ") && p[0].1.contains("started"),
        "{p:?}"
    );
    let task = rows(&r.core, "task.started_by_owner")[0]["task"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(rows(&r.core, "session.external_read").is_empty());
    assert!(
        crate::google::private::session(&r.core.store, &r.core.cfg, &task),
        "and it is private from the start"
    );
    until("the task's first write to run", 20, || note.exists()).await;
    assert!(
        ledgered(&r.core, "tool.confirm_requested")
            .iter()
            .all(|q| q["tool"] != "fs.write"),
        "the write asked nothing"
    );
    driver.abort();
}

/// A From with no address in it is no one trusted: beside a trusted sender's
/// mail in the same look, it still makes the look a notice, not a task.
#[tokio::test(flavor = "multi_thread")]
async fn a_mail_with_no_address_beside_a_trusted_one_only_notices() {
    let r = mail_rig(vec![], trust).await;
    dm_session(&r.core);
    let made = mail_watch(
        &r.core,
        json!({"then": {"kind": "task", "brief": "Reply to it"}}),
    )
    .await;
    let id = wake_id(&made);
    look(&r.core, &id).await;
    r.fake.mail().deliver(
        "t9",
        MsgSpec::new("m9", DANA, "Contract", "Signed, thanks.")
            .header("Authentication-Results", GMAIL_PASS),
    );
    r.fake.mail().deliver(
        "t10",
        MsgSpec::new("m10", "Mallory <mallory at example.test>", "Re", "Wire it."),
    );
    look(&r.core, &id).await;
    assert_eq!(task_started(&r.core), 0, "no task");
    let p = posts(&r.core);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].1.contains("not in trusted_senders"), "{}", p[0].1);
}

// ------------------------------------------------------------------ the cursor

#[tokio::test(flavor = "multi_thread")]
async fn history_poll_resumes_from_the_last_history_id() {
    let r = mail_rig(vec![], |_| {}).await;
    dm_session(&r.core);
    let id = wake_id(&mail_watch(&r.core, json!({})).await);
    look(&r.core, &id).await;
    assert_eq!(
        cursor_of(&r.core, &id).as_deref(),
        Some("100"),
        "the baseline"
    );
    // Mail that does not match moves the mailbox on, and the cursor in memory.
    r.fake.mail().deliver(
        "t5",
        MsgSpec::new("m5", "News <news@elsewhere.test>", "Digest", "Weekly."),
    );
    look(&r.core, &id).await;
    assert_eq!(r.fake.mail().history_starts(), ["100"]);
    assert_eq!(
        r.core.watches.get(&id).unwrap().cursor.as_deref(),
        Some("101")
    );
    // The daemon restarts: the next look starts from what the frame kept.
    let r = restart(r, |_| {});
    r.core.watch_load();
    assert_eq!(
        r.core.watches.get(&id).unwrap().cursor.as_deref(),
        Some("100")
    );
    look(&r.core, &id).await;
    assert_eq!(r.fake.mail().history_starts(), ["100", "100"]);
    // A match after the restart is found from there.
    r.fake.mail().deliver(
        "t9",
        MsgSpec::new("m9", DANA, "Contract", "Signed.")
            .header("Authentication-Results", GMAIL_PASS),
    );
    look(&r.core, &id).await;
    assert_eq!(rows(&r.core, "watch.met").len(), 1);
    assert_eq!(
        cursor_of(&r.core, &id).as_deref(),
        Some(""),
        "cleared at the end"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_history_id_rebaselines_and_says_so() {
    let r = mail_rig(vec![], |_| {}).await;
    dm_session(&r.core);
    let id = wake_id(&mail_watch(&r.core, json!({})).await);
    look(&r.core, &id).await;
    r.fake
        .mail()
        .deliver("t9", MsgSpec::new("m9", DANA, "Contract", "Signed."));
    r.fake.mail().forget_history_before(10_000);
    look(&r.core, &id).await;
    // It started again from the profile's history id, and the row says so.
    assert!(rows(&r.core, "watch.met").is_empty());
    let checked = rows(&r.core, "watch.checked");
    assert_eq!(checked.len(), 2, "{checked:?}");
    let said = checked[1]["text"].as_str().unwrap();
    assert!(said.contains("started again from now"), "{said}");
    assert_eq!(cursor_of(&r.core, &id).as_deref(), Some("101"));
    assert!(posts(&r.core).is_empty(), "a row, not a post");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_poll_with_no_new_mail_makes_one_call() {
    let r = mail_rig(vec![], |_| {}).await;
    dm_session(&r.core);
    let id = wake_id(&mail_watch(&r.core, json!({})).await);
    look(&r.core, &id).await;
    let (log, history) = (r.fake.log().len(), history_calls(&r));
    // The look, then its application on this thread with no await between:
    // the frames it wrote.
    let e = r.core.watches.get(&id).unwrap();
    let mut rows = Vec::new();
    let result = crate::watch::check::observe(&r.core.watch_tools(), &e, &mut rows).await;
    let frames = theseus_store::frames_written_here();
    let result = r.core.watch_seen(&id, &rows, result);
    r.core.watch_finish(&id, result);
    assert_eq!(
        theseus_store::frames_written_here(),
        frames,
        "a quiet look writes no frame: its call is tallied, and nothing is the watch's"
    );
    assert_eq!(rows_of(&r.core, "watch.checked"), 1, "only the baseline");
    let new: Vec<String> = r.fake.log()[log..]
        .iter()
        .map(|l| format!("{} {}", l.verb, l.path))
        .collect();
    assert_eq!(new, ["GET /gmail/v1/users/me/history"], "{new:?}");
    assert_eq!(history_calls(&r), history + 1);
    // The call is in the entry's tally, not a row of its own, and the tally
    // row holds no address or subject.
    let called = ledgered(&r.core, "google.called");
    assert!(called
        .iter()
        .all(|c| c["method"] != "gmail.users.history.list"));
    let tally = r.core.watches.get(&id).unwrap().quiet.take().unwrap();
    assert_eq!(tally["method"], "gmail.users.history.list");
    assert_eq!(
        (tally["quiet_looks"].clone(), tally["calls"].clone()),
        (1.into(), 1.into())
    );
    assert!(tally["class"] == "read" && !tally.to_string().contains("example.test"));
}

// ------------------------------------------------------------------ the deadline

/// The deadline passes with no reply: a task starts with the owner's brief,
/// and the notice says so in place of "gave up".
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_deadline_fires_the_follow_up() {
    let r = mail_rig(vec![], |_| {}).await;
    dm_session(&r.core);
    let made = mail_watch(
        &r.core,
        json!({"for_at_most": "3d", "follow_up": "Write Dana a short nudge"}),
    )
    .await;
    let id = wake_id(&made);
    look(&r.core, &id).await;
    r.core
        .watches
        .with(&id, |e| e.spec.expires_ms = r.core.kernel.now_ms())
        .unwrap();
    look(&r.core, &id).await;
    let p = posts(&r.core);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(
        p[0].1.starts_with("👁 no reply on \"dana replies?\" by ")
            && p[0].1.contains(": follow-up task ")
            && p[0].1.ends_with(" started"),
        "{}",
        p[0].1
    );
    assert!(!p[0].1.contains("gave up"), "{}", p[0].1);
    assert_eq!(task_started(&r.core), 1);
    let task = rows(&r.core, "task.started_by_owner")[0]["task"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        !crate::external::read_any(&r.core.store, &task),
        "the owner's words: nothing outside"
    );
    assert_eq!(rows(&r.core, "watch.expired").len(), 1);
    assert_eq!(cursor_of(&r.core, &id).as_deref(), Some(""));
    // Without a follow-up the deadline only says it gave up.
    let plain = mail_watch(&r.core, json!({"title": "other?", "for_at_most": "3d"})).await;
    let pid = wake_id(&plain);
    look(&r.core, &pid).await;
    r.core
        .watches
        .with(&pid, |e| e.spec.expires_ms = r.core.kernel.now_ms())
        .unwrap();
    look(&r.core, &pid).await;
    assert!(posts(&r.core)[1].1.contains("gave up on \"other?\""));
    assert_eq!(task_started(&r.core), 1);
}

/// The follow-up task drafts, and its send waits for the owner like any send.
#[tokio::test(flavor = "multi_thread")]
async fn the_follow_up_drafts_and_its_send_still_asks() {
    let draft =
        json!({"to": ["dana@example.test"], "subject": "Following up", "body": "Any news?"});
    // The draft's digest, from the same draft made first in another mailbox.
    let digest = {
        let r0 = mail_rig(vec![], |_| {}).await;
        let t = r0.core.tools.registry.get("gmail.draft").unwrap().clone();
        let (out, _) = t
            .run_async(&draft, &ToolCtx::for_tests(r0.dir.path()))
            .await
            .unwrap();
        out.meta["digest"].clone()
    };
    let script = vec![
        Scripted::tools("", &[("c1", "gmail_draft", draft.clone())]),
        Scripted::tools(
            "",
            &[(
                "c2",
                "gmail_send",
                json!({"draft_id": "r1", "digest": digest}),
            )],
        ),
    ];
    let r = mail_rig(script, |_| {}).await;
    let driver = tokio::spawn(crate::harness::drive(r.core.clone()));
    dm_session(&r.core);
    let id = wake_id(
        &mail_watch(
            &r.core,
            json!({"for_at_most": "3d", "follow_up": "Draft a nudge to Dana and send it"}),
        )
        .await,
    );
    look(&r.core, &id).await;
    r.core
        .watches
        .with(&id, |e| e.spec.expires_ms = r.core.kernel.now_ms())
        .unwrap();
    look(&r.core, &id).await;
    until("the follow-up's send to ask", 20, || {
        ledgered(&r.core, "tool.confirm_requested")
            .iter()
            .any(|q| q["tool"] == "gmail.send")
    })
    .await;
    assert_eq!(r.fake.mail().draft_ids().len(), 1, "it drafted");
    assert!(r.fake.mail().sent().is_empty(), "and nothing was sent");
    driver.abort();
}
