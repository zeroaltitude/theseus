//! The mail watch and the Google work on the surfaces that read the record
//! (theseus-batvg.6): the digest's Google line counted from the daemon's own
//! `google.called` rows and the open mail watches, and a mail watch's line on
//! the work board. With no `[google]` section none of it reads or adds a byte.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_google::fake::gmail::MsgSpec;
use theseus_protocol::digest_google::DigestGoogle;
use theseus_protocol::digest_text;
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_tools::ToolCtx;

use super::tests_mail::{mail_rig, mail_watch};
use super::tests_watch::{dm_session, life, rpc};
use crate::google::tests::Rig;
use crate::push::WorkScope;
use crate::Core;

const DANA: &str = "Dana Colleague <dana@example.test>";

/// Run a Google tool the way a turn does (its result's `google.called` row is
/// recorded), and give its output.
async fn tool(r: &Rig, name: &str, input: Value) -> theseus_tools::ToolOutput {
    let t = r.core.tools.registry.get(name).unwrap().clone();
    let (out, _) = t
        .run_async(&input, &ToolCtx::for_tests(r.dir.path()))
        .await
        .unwrap_or_else(|e| panic!("{name}: {}", e.message));
    crate::fact::google::of_result(&r.core.rec(None), &out.meta);
    out
}

fn digest(core: &Core) -> theseus_protocol::WorkDigestResult {
    core.digest_blocking(0, None, &WorkScope::default(), &HashSet::new(), true)
        .unwrap()
}

fn watch_views(core: &Arc<Core>) -> Vec<WorkView> {
    core.push
        .work_snapshot(0, &WorkScope::default())
        .1
        .into_iter()
        .filter(|v| v.kind == WorkKind::Watch)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn digest_counts_sent_mail() {
    let r = mail_rig(vec![], |_| {}).await;
    dm_session(&r.core);
    r.fake
        .mail()
        .thread("t1", vec![MsgSpec::new("m1", DANA, "Budget", "Numbers?")]);
    let first = tool(
        &r,
        "gmail.draft",
        json!({"reply_to": "m1", "body": "Numbers attached."}),
    )
    .await;
    let second = tool(
        &r,
        "gmail.draft",
        json!({"to": ["dana@example.test"], "subject": "Other", "body": "A second thing."}),
    )
    .await;
    // Both drafts wait; one is sent.
    let nothing_sent = digest(&r.core).google;
    assert_eq!(
        nothing_sent,
        Some(DigestGoogle {
            drafts_waiting: 2,
            ..Default::default()
        })
    );
    let (id, digest_of) = (&first.meta["draft_id"], &first.meta["digest"]);
    tool(
        &r,
        "gmail.send",
        json!({"draft_id": id, "digest": digest_of}),
    )
    .await;
    assert_ne!(first.meta["draft_id"], second.meta["draft_id"]);
    // A document written to Drive (a row as `drive.write` makes it), and a
    // mail watch waiting for a reply.
    let row = json!({"service": "drive", "tool": "drive.write", "method": "drive.files.create",
        "class": "write", "status": 200, "outcome": "ok"});
    r.core
        .rec(None)
        .record(&crate::fact::google::GoogleCalled { row: &row });
    mail_watch(&r.core, json!({})).await;
    let d = digest(&r.core);
    assert_eq!(
        d.google,
        Some(DigestGoogle {
            mail_sent: 1,
            drafts_waiting: 1,
            waiting_on_replies: 1,
            docs_written: 1
        })
    );
    let text = digest_text(&d, 0, &|_| "09:00".to_string(), 2000);
    assert!(
        text.contains("✉ mail sent 1 · drafts waiting 1 · waiting on 1 reply · docs written 1"),
        "{text}"
    );
    // A call that failed is not a send.
    let row = json!({"service": "gmail", "tool": "gmail.send", "method": "gmail.users.drafts.send",
        "class": "authority", "status": 503, "outcome": "unknown"});
    r.core
        .rec(None)
        .record(&crate::fact::google::GoogleCalled { row: &row });
    assert_eq!(digest(&r.core).google.unwrap().mail_sent, 1);
}

/// A mail watch's line on the board says how long it has waited and the day it
/// follows up; without a follow-up, only the age.
#[tokio::test(flavor = "multi_thread")]
async fn the_board_shows_a_reply_wait() {
    let r = mail_rig(vec![], |_| {}).await;
    dm_session(&r.core);
    mail_watch(
        &r.core,
        json!({"for_at_most": "3d", "follow_up": "Nudge Dana"}),
    )
    .await;
    mail_watch(
        &r.core,
        json!({"title": "no follow-up", "for_at_most": "3d"}),
    )
    .await;
    r.core.push.ensure(&r.core).await.unwrap();
    let views = watch_views(&r.core);
    let label = |title: &str| {
        views
            .iter()
            .find(|v| v.title == title)
            .and_then(|v| v.doing.as_ref())
            .map(|d| d.label.clone())
            .unwrap_or_else(|| panic!("a view of {title}: {views:?}"))
    };
    let with = label("dana replies?");
    let day = with.strip_prefix("waiting: reply on 'dana replies?' (0 min, follow-up ");
    let day = day
        .and_then(|d| d.strip_suffix(')'))
        .unwrap_or_else(|| panic!("{with}"));
    assert!(
        day.len() == 3 && day.chars().next().unwrap().is_ascii_uppercase(),
        "a weekday: {with}"
    );
    assert_eq!(
        label("no follow-up"),
        "waiting: reply on 'no follow-up' (0 min)"
    );
}

/// With no `[google]` section the digest, its text, its wire and a watch's
/// board line are what they were: nothing is read, nothing added.
#[tokio::test(flavor = "multi_thread")]
async fn without_google_the_digest_and_the_board_are_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let l = life(dir.path(), false, |_| {});
    dm_session(&l.core);
    let file = dir.path().join("flag");
    rpc(
        &l.core,
        "watch.create",
        json!({"title": "flag?", "check": {"kind": "file", "path": file}, "until": "exists"}),
    )
    .await
    .unwrap();
    let d = digest(&l.core);
    assert_eq!(d.google, None);
    assert!(!serde_json::to_string(&d).unwrap().contains("google"));
    assert!(!digest_text(&d, 0, &|_| "09:00".to_string(), 2000).contains('✉'));
    l.core.push.ensure(&l.core).await.unwrap();
    let views = watch_views(&l.core);
    assert_eq!(views[0].doing.as_ref().unwrap().label, "checking every 5m");
    // A mail watch is refused there, with the reason.
    let e = rpc(
        &l.core,
        "watch.create",
        json!({"title": "dana?", "check": {"kind": "mail", "q": "from:dana@example.test"}}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("mail watches need [google] with gmail"), "{e}");
}
