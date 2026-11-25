//! The tidy Google acts run at their tool's posture and tell the owner once
//! (issue 57): moving mail or a Drive file to the Trash or back, removing
//! someone's access to a file, and `google.call`'s trash, untrash, a message
//! put into the mailbox and a calendar made. Each beside the act that still
//! asks: a share, a send, a delete, a clear, an ACL change. Against the
//! loopback fake only.

use serde_json::{json, Value};
use theseus_google::fake::drive::{Drive, File};
use theseus_google::fake::gmail::MsgSpec;
use theseus_tools::{Tool, ToolCtx, ToolOutput};

use super::tests::*;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::test_support::turn;

fn tool(r: &Rig, name: &str) -> std::sync::Arc<dyn Tool> {
    r.core.tools.registry.get(name).expect("registered").clone()
}

async fn ok(r: &Rig, name: &str, input: Value) -> ToolOutput {
    let ctx = ToolCtx::for_tests(r.dir.path());
    match tool(r, name).run_async(&input, &ctx).await {
        Ok((out, _)) => out,
        Err(e) => panic!("{name}: {}", e.message),
    }
}

fn plan(r: &Rig, name: &str, input: Value) -> theseus_tools::Plan {
    tool(r, name)
        .plan(&input, &ToolCtx::for_tests(r.dir.path()))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The FYI notes staged to the operator: each one's text, and the id its
/// Undo names (`owner_note::told`), or none for a note told without one.
fn notes(r: &Rig) -> Vec<(String, Option<String>)> {
    r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .map(|a| crate::outbox::body_of(a).clone())
        .filter(|b| {
            b["text"]
                .as_str()
                .is_some_and(|t| t.starts_with("ℹ️ FYI · "))
        })
        .map(|b| {
            let text = b["text"].as_str().unwrap().to_string();
            match b["kind"].as_str() {
                Some("told") => {
                    assert_eq!(b["undo"]["method"], "google.undo", "{b}");
                    let id = b["undo"]["id"].as_str().unwrap().to_string();
                    let line = format!("\nUndo: `theseus google undo {id}`");
                    assert!(text.ends_with(&line), "{text}");
                    (text.strip_suffix(&line).unwrap().to_string(), Some(id))
                }
                _ => {
                    assert_eq!(b["kind"], "notice", "{b}");
                    assert_eq!(b["announce"], true, "{b}");
                    (text, None)
                }
            }
        })
        .collect()
}

/// The owner on the CLI of this machine: a private place.
fn owner() -> crate::approval::Answerer {
    crate::approval::Answerer {
        label: "the CLI".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// Someone who is not the owner, in a shared place and in a DM: neither
/// counts. (An undo is judged as a routine's delete is: the CLI is the
/// owner's, and a job's shell is refused by the CLI before it sends.)
fn not_the_owner() -> [crate::approval::Answerer; 2] {
    let stranger = |guild: Option<&str>| crate::approval::Answerer {
        label: "discord:777".into(),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: "42".into(),
            guild_id: guild.map(String::from),
        }),
        agent: None,
    };
    [stranger(Some("7")), stranger(None)]
}

/// The one note a turn staged, its words and its Undo's id: a press by
/// anyone but the owner in a private place changes nothing in Google (`same`
/// still holds after each), and the owner's runs the act's opposite.
async fn press(r: &Rig, words: &str, same: impl Fn() -> bool) {
    let n = notes(r);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(n[0].0, words);
    let id = n[0].1.clone().expect("an Undo");
    for who in not_the_owner() {
        let e = r.core.google_undo(&id, &who).await.unwrap_err();
        assert_eq!(
            e.code,
            theseus_protocol::error_code::REFUSED,
            "{}",
            e.message
        );
        assert!(same(), "{} changed nothing", who.label);
    }
    r.core.google_undo(&id, &owner()).await.unwrap();
}

/// Each call a note's undo carries, run as its press would: through
/// `google.call`, which none of them makes ask.
async fn undo(r: &Rig, out: &ToolOutput) {
    let calls = out.meta[super::told::KEY]["undo"]
        .as_array()
        .unwrap()
        .clone();
    assert!(!calls.is_empty(), "{}", out.meta);
    for c in calls {
        assert!(plan(r, "google.call", c.clone()).authority.is_none(), "{c}");
        ok(r, "google.call", c).await;
    }
}

fn drive_rig_with(
    script: Vec<Scripted>,
    tweak: impl FnOnce(&mut crate::Config),
) -> impl std::future::Future<Output = Rig> {
    rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        script,
        tweak,
    )
}

fn share(id: &str, input: Value) -> Scripted {
    Scripted::tools("", &[(id, "drive_share", input)])
}

// ------------------------------------------------------------------ Drive

#[tokio::test]
async fn a_drive_trash_runs_unasked_is_told_once_and_its_undo_restores_it() {
    let trash = json!({"id": "doc1", "trash": true});
    let r = drive_rig_with(
        vec![share("c1", trash.clone()), Scripted::text("Done.")],
        |_| {},
    )
    .await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    assert!(plan(&r, "drive.share", trash.clone()).authority.is_none());
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "trash it").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(ledgered(&r.core, "tool.confirm_requested").is_empty());
    assert!(drive.file("doc1").unwrap().trashed);
    // Its undo puts back exactly what was: the file out of the Trash, all else as it was.
    press(
        &r,
        "ℹ️ FYI · Moved a Drive file to the Trash\nIn \"trash it\", without asking you\n\
             Moved Drive file doc1 to the Trash.\nUndo restores it.",
        || drive.file("doc1").unwrap().trashed,
    )
    .await;
    let after = drive.file("doc1").unwrap();
    assert!(!after.trashed);
    assert_eq!(
        (
            after.name,
            after.parents,
            after.shared,
            after.permissions.len()
        ),
        ("Plan".to_string(), vec!["root".to_string()], false, 0)
    );
    // A restore is its own opposite's undo.
    let out = ok(&r, "drive.share", json!({"id": "doc1", "trash": true})).await;
    undo(&r, &out).await;
    let out = ok(&r, "drive.share", json!({"id": "doc1", "trash": false})).await;
    assert!(!drive.file("doc1").unwrap().trashed);
    assert!(out.meta[super::told::KEY]["did"]
        .as_str()
        .unwrap()
        .contains("Restored Drive file doc1"));
    undo(&r, &out).await;
    assert!(drive.file("doc1").unwrap().trashed);
}

#[tokio::test]
async fn a_drive_revoke_runs_unasked_and_says_how_to_give_it_back_and_a_grant_still_asks() {
    let remove = json!({"id": "doc1", "email": "collaborator@example.test", "remove": true});
    let grant = json!({"id": "doc1", "email": "collaborator@example.test", "role": "writer"});
    let r = drive_rig_with(
        vec![share("c1", remove.clone()), Scripted::text("Done.")],
        |_| {},
    )
    .await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    // The grant is made directly (as the owner's approved call would be), so there is access to take.
    ok(&r, "drive.share", grant.clone()).await;
    assert!(plan(&r, "drive.share", remove.clone()).authority.is_none());
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "stop sharing it").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(drive.file("doc1").unwrap().permissions.is_empty());
    let n = notes(&r);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(n[0].1, None, "no Undo: giving it back asks");
    assert_eq!(
        n[0].0,
        "ℹ️ FYI · Stopped sharing a Drive file\nIn \"stop sharing it\", without asking you\n\
             Removed collaborator@example.test's access (writer) to Drive file doc1.\n\
             To give it back, ask Theseus to share Drive file doc1 with \
             collaborator@example.test as writer: that asks you first."
    );
    // No button: giving access back is a grant, and a grant asks at every posture.
    let out = ok(&r, "drive.share", grant.clone()).await;
    assert!(
        out.meta.get(super::told::KEY).is_none(),
        "a grant is not told: it asked"
    );
    let out = ok(&r, "drive.share", remove).await;
    assert_eq!(out.meta[super::told::KEY]["undo"], json!([]));
    let t = tool(&r, "drive.share");
    let p = plan(&r, "drive.share", grant);
    assert!(p.authority.is_some());
    assert!(!r.core.tools.policy.posture_only(t.as_ref(), &p));
    assert_eq!(
        r.core.tools.policy.decide(t.as_ref(), &p).posture,
        Posture::Approve
    );
    // A write into a shared folder still asks.
    drive.add(File::folder("team", "Team").shared());
    let into = json!({"name": "Notes", "content": "x", "folder": "team", "into_shared": true});
    let into_shared = plan(&r, "drive.write", into);
    assert!(into_shared.authority.is_some(), "{into_shared:?}");
}

#[tokio::test]
async fn an_explicit_approve_on_the_tool_still_asks_for_a_trash_and_tells_nothing() {
    let trash = json!({"id": "doc1", "trash": true});
    let r = drive_rig_with(vec![share("c1", trash.clone())], |c| {
        c.policy
            .tools
            .insert("drive.share".into(), Posture::Approve);
    })
    .await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    let res = turn(&r.core, &session(&r.core), "trash it").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert!(!drive.file("doc1").unwrap().trashed);
    assert_eq!(ledgered(&r.core, "tool.confirm_requested").len(), 1);
    assert!(notes(&r).is_empty());
    // What the owner approved is not told back to him.
    let ctx = ToolCtx {
        approved: true,
        ..ToolCtx::for_tests(r.dir.path())
    };
    let (out, _) = tool(&r, "drive.share")
        .run_async(&trash, &ctx)
        .await
        .map_err(|e| e.message)
        .unwrap();
    assert!(out.meta.get(super::told::KEY).is_none(), "{}", out.meta);
}

// ------------------------------------------------------------------ Gmail

#[tokio::test]
async fn a_mail_trash_runs_unasked_is_told_once_and_its_undo_restores_the_labels() {
    let trash = json!({"ids": ["t1", "p2"], "trash": true});
    let r = rig(vec![
        Scripted::tools("", &[("c1", "gmail_label", trash.clone())]),
        Scripted::text("Done."),
    ])
    .await;
    let mail = r.fake.mail();
    mail.thread(
        "t1",
        vec![MsgSpec::new("m1", "Sam <sam@example.test>", "Hi", "x").label("UNREAD")],
    );
    mail.thread(
        "t8",
        vec![
            MsgSpec::new("p1", "Sam <sam@example.test>", "A", "a"),
            MsgSpec::new("p2", "Sam <sam@example.test>", "B", "b").label("STARRED"),
        ],
    );
    let sorted = |m: &str| {
        let mut l = r.fake.mail().labels_of(m);
        l.sort();
        l
    };
    let before = (sorted("m1"), sorted("p1"), sorted("p2"));
    assert!(plan(&r, "gmail.label", trash.clone()).authority.is_none());
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "clear out my inbox").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(sorted("m1").contains(&"TRASH".to_string()));
    assert!(sorted("p2").contains(&"TRASH".to_string()));
    let trashed = (sorted("m1"), sorted("p1"), sorted("p2"));
    press(
        &r,
        "ℹ️ FYI · Moved mail to the Trash\nIn \"clear out my inbox\", without asking you\n\
             Moved 2 Gmail thread(s) to the Trash: t1, p2.\nUndo takes them out of the Trash.",
        || (sorted("m1"), sorted("p1"), sorted("p2")) == trashed,
    )
    .await;
    assert_eq!((sorted("m1"), sorted("p1"), sorted("p2")), before);
    // The undo names the kind that took each id: a thread, and a message.
    let out = ok(&r, "gmail.label", trash).await;
    assert_eq!(
        out.meta[super::told::KEY]["undo"],
        json!([
            {"method": "POST /gmail/v1/users/me/threads/t1/untrash"},
            {"method": "POST /gmail/v1/users/me/messages/p2/untrash"}
        ])
    );
}

// ------------------------------------------------------------------ google.call

#[tokio::test]
async fn a_google_call_trash_runs_unasked_and_its_undo_is_the_untrash() {
    let trash = json!({"method": "gmail.users.threads.trash", "params": {"id": "t1"}});
    let r = rig(vec![call("c1", trash.clone()), Scripted::text("Done.")]).await;
    r.fake.mail().thread(
        "t1",
        vec![MsgSpec::new("m1", "Sam <sam@example.test>", "Hi", "x")],
    );
    let p = plan(&r, "google.call", trash.clone());
    assert!(p.authority.is_none(), "{p:?}");
    let t = tool(&r, "google.call");
    assert!(r.core.tools.policy.posture_only(t.as_ref(), &p));
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "trash that thread").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert_eq!(r.fake.mail().labels_of("m1"), vec!["TRASH".to_string()]);
    press(
        &r,
        "ℹ️ FYI · A Google call was made\nIn \"trash that thread\", without asking you\n\
             gmail.users.threads.trash: POST /gmail/v1/users/me/threads/t1/trash \
             (moves to or from the trash).\nUndo runs its opposite.",
        || r.fake.mail().labels_of("m1") == ["TRASH"],
    )
    .await;
    assert_eq!(r.fake.mail().labels_of("m1"), vec!["INBOX".to_string()]);
    // The undo's own call is in the ledger, as any Google call is.
    let called = ledgered(&r.core, "google.called");
    assert!(
        called
            .iter()
            .any(|c| c["method"].as_str().is_some_and(|m| m.contains("untrash"))),
        "{called:?}"
    );
    // A node that is no tidy act's is refused, and so is an unknown one.
    for id in ["nod_nothing", ""] {
        let e = r.core.google_undo(id, &owner()).await.unwrap_err();
        assert!(e.message.contains("no Google act"), "{}", e.message);
    }
}

#[tokio::test]
async fn a_message_put_in_or_a_calendar_made_is_told_with_how_to_take_it_back() {
    let r = rig(vec![]).await;
    r.fake.json(
        "POST",
        "/gmail/v1/users/me/messages",
        json!({"id": "m77", "threadId": "t77"}),
    );
    r.fake.json(
        "POST",
        "/calendar/v3/calendars",
        json!({"id": "cal9@group.example.test", "summary": "x"}),
    );
    let insert = json!({"method": "gmail.users.messages.insert", "body": {"raw": "eA"}});
    assert!(plan(&r, "google.call", insert.clone()).authority.is_none());
    let out = ok(&r, "google.call", insert).await;
    let told = &out.meta[super::told::KEY];
    assert_eq!(told["undo"], json!([]));
    assert_eq!(
        told["back"],
        "To take it back, move message m77 to the Trash."
    );
    let made = json!({"method": "calendar.calendars.insert", "body": {"summary": "Trips"}});
    assert!(plan(&r, "google.call", made.clone()).authority.is_none());
    let out = ok(&r, "google.call", made).await;
    let told = &out.meta[super::told::KEY];
    assert_eq!(told["undo"], json!([]));
    assert_eq!(
        told["back"],
        "To take it back, delete calendar cal9@group.example.test: that asks you first."
    );
    // That delete, and every other KEEP call, still asks at every posture.
    let t = tool(&r, "google.call");
    for (m, params) in [
        ("calendar.calendars.delete", json!({"calendarId": "cal9"})),
        ("calendar.calendars.clear", json!({})),
        ("gmail.users.messages.send", json!({})),
        ("gmail.users.messages.delete", json!({"id": "m77"})),
        ("gmail.users.messages.batchDelete", json!({})),
        ("drive.files.delete", json!({"fileId": "f1"})),
        ("drive.permissions.create", json!({"fileId": "f1"})),
        ("calendar.acl.insert", json!({})),
        ("calendar.events.insert", json!({"sendUpdates": "all"})),
    ] {
        let p = plan(&r, "google.call", json!({"method": m, "params": params}));
        assert!(p.authority.is_some(), "{m}");
        assert_eq!(
            r.core.tools.policy.decide(t.as_ref(), &p).posture,
            Posture::Approve,
            "{m}"
        );
    }
}
