//! The told Google acts (issue 57): Gmail messages moved to the Trash or out
//! of it, a Drive file trashed or restored, and a calendar made. Each runs at
//! its tool's posture through a whole turn, tells the owner once in fixed
//! words, and, where it can be put back, carries an Undo that restores what
//! each thing was before it: nothing more, nothing less. Beside them, what
//! still asks: a revoke, a share, a send, a message put into the mailbox, a
//! thread trashed through `google.call`, and a told act that carries anything
//! more. Against the loopback fake only.

use serde_json::{json, Value};
use theseus_google::fake::drive::{Drive, File};
use theseus_google::fake::gmail::MsgSpec;
use theseus_google::fake::Reply;
use theseus_tools::{Tool, ToolCtx, ToolOutput};

use super::tests::*;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::test_support::turn;

/// The owner's Discord handle, for a press of his own from a shared place.
const OWNER: &str = "4242";

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

/// A rig whose owner is named on Discord, so his press from a shared place
/// is his and is still refused.
fn rig_of(
    script: Vec<Scripted>,
    tweak: impl FnOnce(&mut crate::Config),
) -> impl std::future::Future<Output = Rig> {
    rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        script,
        |c| {
            c.places.owner = Some(vec![format!("discord:{OWNER}")]);
            tweak(c)
        },
    )
}

fn share(id: &str, input: Value) -> Scripted {
    Scripted::tools("", &[(id, "drive_share", input)])
}

fn label(id: &str, input: Value) -> Scripted {
    Scripted::tools("", &[(id, "gmail_label", input)])
}

/// The note's second line: the conversation by its short id.
fn by(sid: &str) -> String {
    format!(
        "In conversation {}, without asking you",
        theseus_protocol::short_id(sid)
    )
}

/// The FYI notes staged to the operator: each one's text, and for a note
/// with an Undo (`owner_note::told`) its post, which `told.undo` takes; none
/// for a note told without one.
fn notes(r: &Rig) -> Vec<(String, Option<String>)> {
    r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .map(|a| (a.correlation_id.clone(), crate::outbox::body_of(a).clone()))
        .filter(|(_, b)| {
            b["text"]
                .as_str()
                .is_some_and(|t| t.starts_with("ℹ️ FYI · "))
        })
        .map(|(post, b)| {
            let text = b["text"].as_str().unwrap().to_string();
            match b["kind"].as_str() {
                Some("told") => {
                    assert_eq!(b["undo"]["method"], "google.take_back", "{b}");
                    // The door's own verb, with this note's post id.
                    let text = crate::owner_note::told::filled(&text, &post);
                    let line = format!("\nUndo: `theseus told undo {post}`");
                    assert!(text.ends_with(&line), "{text}");
                    (text.strip_suffix(&line).unwrap().to_string(), Some(post))
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

/// The owner on the CLI of this machine: a private place. (The daemon's
/// general owner rule: a local process on the owner's socket counts, as it
/// does for every owner command.)
fn owner() -> crate::approval::Answerer {
    crate::test_support::owner_cli()
}

/// Presses that do not count: a stranger in a shared place, a stranger in a
/// DM, and the owner himself in a shared place.
fn not_counted() -> [crate::approval::Answerer; 3] {
    let at = |user: &str, guild: Option<&str>| crate::approval::Answerer {
        label: format!("discord:{user}"),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: user.into(),
            channel_id: "42".into(),
            guild_id: guild.map(String::from),
        }),
        agent: None,
    };
    [at("777", Some("7")), at("777", None), at(OWNER, Some("7"))]
}

/// The one note a turn staged, its words, and its Undo pressed through the
/// one door, `told.undo`: a press that does not count changes nothing in
/// Google (`same` still holds after each), the owner's restores and says
/// so, and a second press does nothing.
async fn press(r: &Rig, words: &str, same: impl Fn() -> bool) {
    let n = notes(r);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(n[0].0, words);
    let post = n[0].1.clone().expect("an Undo");
    for who in not_counted() {
        let e = r.core.told_undo(&post, &who).await.unwrap_err();
        assert_eq!(
            e.code,
            theseus_protocol::error_code::REFUSED,
            "{}",
            e.message
        );
        assert!(same(), "{}'s press changed nothing", who.label);
    }
    let done = r.core.told_undo(&post, &owner()).await.unwrap();
    assert_eq!(done.text, format!("{words}\n↩️ Undone by the CLI."));
    let again = r.core.told_undo(&post, &owner()).await.unwrap_err();
    assert_eq!(again.data["undone_by"], "the CLI", "{}", again.message);
}

/// Every request the fake saw for `verb` on a path ending in `tail`.
fn seen(r: &Rig, verb: &str, tail: &str) -> usize {
    r.fake
        .log()
        .iter()
        .filter(|l| l.verb == verb && l.path.ends_with(tail))
        .count()
}

fn sorted(r: &Rig, m: &str) -> Vec<String> {
    let mut l = r.fake.mail().labels_of(m);
    l.sort();
    l
}

fn trashed_doc(id: &str) -> File {
    let mut f = File::doc(id, "Plan", "# Plan\n");
    f.trashed = true;
    f
}

// ------------------------------------------------------------------ Drive

#[tokio::test]
async fn a_drive_trash_runs_unasked_is_told_once_and_its_undo_restores_it() {
    let trash = json!({"id": "doc1", "trash": true});
    let r = rig_of(
        vec![share("c1", trash.clone()), Scripted::text("Done.")],
        |_| {},
    )
    .await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    assert!(plan(&r, "drive.share", trash).authority.is_none());
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "trash it").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(ledgered(&r.core, "tool.confirm_requested").is_empty());
    assert!(drive.file("doc1").unwrap().trashed);
    let words = format!(
        "ℹ️ FYI · Moved a Drive file to the Trash\n{}\nMoved 1 Drive file to the Trash.\n\
         Undo puts it back exactly as it was.",
        by(&sid)
    );
    press(&r, &words, || drive.file("doc1").unwrap().trashed).await;
    let after = drive.file("doc1").unwrap();
    assert_eq!(
        (
            after.trashed,
            after.name,
            after.parents,
            after.shared,
            after.permissions.len()
        ),
        (
            false,
            "Plan".to_string(),
            vec!["root".to_string()],
            false,
            0
        )
    );
}

#[tokio::test]
async fn a_drive_restore_runs_unasked_and_its_undo_puts_it_back_in_the_trash() {
    let restore = json!({"id": "doc1", "trash": false});
    let r = rig_of(vec![share("c1", restore), Scripted::text("Done.")], |_| {}).await;
    let drive = Drive::install(&r.fake);
    drive.add(trashed_doc("doc1"));
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "restore it").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(!drive.file("doc1").unwrap().trashed);
    let words = format!(
        "ℹ️ FYI · Restored a Drive file from the Trash\n{}\nRestored 1 Drive file from the \
         Trash.\nUndo puts it back exactly as it was.",
        by(&sid)
    );
    press(&r, &words, || !drive.file("doc1").unwrap().trashed).await;
    assert!(drive.file("doc1").unwrap().trashed);
}

/// A trash of a file already in the Trash changed nothing, so its Undo
/// changes nothing: the file stays in the Trash, and no write is sent.
#[tokio::test]
async fn a_trash_that_changed_nothing_has_an_undo_that_changes_nothing() {
    let trash = json!({"id": "doc1", "trash": true});
    let r = rig_of(vec![share("c1", trash), Scripted::text("Done.")], |_| {}).await;
    let drive = Drive::install(&r.fake);
    drive.add(trashed_doc("doc1"));
    let sid = session(&r.core);
    turn(&r.core, &sid, "trash it").await;
    let words = format!(
        "ℹ️ FYI · Moved a Drive file to the Trash\n{}\nMoved 1 Drive file to the Trash. It \
         was there already, so nothing changed.\nUndo puts it back exactly as it was.",
        by(&sid)
    );
    let writes = seen(&r, "PATCH", "/drive/v3/files/doc1");
    press(&r, &words, || drive.file("doc1").unwrap().trashed).await;
    assert!(drive.file("doc1").unwrap().trashed, "it stays in the Trash");
    assert_eq!(seen(&r, "PATCH", "/drive/v3/files/doc1"), writes);
}

/// A revoke asks, as on main: the access may be the account's own, and
/// giving it back is a share.
#[tokio::test]
async fn a_drive_revoke_still_asks_and_removes_nothing_unasked() {
    let remove = json!({"id": "doc1", "email": "collaborator@example.test", "remove": true});
    let grant = json!({"id": "doc1", "email": "collaborator@example.test", "role": "writer"});
    let r = rig_of(vec![share("c1", remove.clone())], |_| {}).await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    ok(&r, "drive.share", grant).await;
    let p = plan(&r, "drive.share", remove);
    let why = p.authority.clone().unwrap_or_default();
    assert!(
        why.contains("which reaches other people or removes a file, so the owner decides it"),
        "{why}"
    );
    let t = tool(&r, "drive.share");
    assert_eq!(
        r.core.tools.policy.decide(t.as_ref(), &p).posture,
        Posture::Approve
    );
    let res = turn(&r.core, &session(&r.core), "stop sharing it").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(drive.file("doc1").unwrap().permissions.len(), 1);
    assert!(notes(&r).is_empty());
}

#[tokio::test]
async fn an_explicit_approve_on_the_tool_still_asks_for_a_trash_and_tells_nothing() {
    let trash = json!({"id": "doc1", "trash": true});
    let r = rig_of(vec![share("c1", trash.clone())], |c| {
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

#[tokio::test]
async fn a_google_call_drive_trash_of_trashed_alone_is_told_and_undone_exactly() {
    let trash = json!({"method": "drive.files.update", "params": {"fileId": "doc1"},
        "body": {"trashed": true}});
    let r = rig_of(vec![call("c1", trash), Scripted::text("Done.")], |_| {}).await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "trash that file").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(drive.file("doc1").unwrap().trashed);
    let words = format!(
        "ℹ️ FYI · Moved a Drive file to the Trash\n{}\nMoved 1 Drive file to the Trash.\n\
         Undo puts it back exactly as it was.",
        by(&sid)
    );
    press(&r, &words, || drive.file("doc1").unwrap().trashed).await;
    assert!(!drive.file("doc1").unwrap().trashed);
}

/// A trash that also moves the file into another folder is not a trash: it
/// asks, and nothing is sent.
#[tokio::test]
async fn a_google_call_drive_update_that_also_moves_a_parent_asks() {
    let moved = json!({"method": "drive.files.update",
        "params": {"fileId": "doc1", "addParents": "team"}, "body": {"trashed": true}});
    let r = rig_of(vec![call("c1", moved)], |c| {
        c.policy.enforcement = Posture::Open;
        c.policy.tools.insert("google.call".into(), Posture::Open);
    })
    .await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    let res = turn(&r.core, &session(&r.core), "file it in team").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(seen(&r, "PATCH", "/drive/v3/files/doc1"), 0);
    assert!(!drive.file("doc1").unwrap().trashed);
    assert!(notes(&r).is_empty());
}

// ------------------------------------------------------------------ Gmail

/// A thread of three: one in the inbox and unread, one archived under a
/// label, one already in the Trash.
fn mixed_thread(r: &Rig) {
    let archived = MsgSpec {
        labels: vec!["Label_7".into()],
        ..MsgSpec::new("m2", "Sam <sam@example.test>", "B", "b")
    };
    let binned = MsgSpec {
        labels: vec!["TRASH".into()],
        ..MsgSpec::new("m3", "Sam <sam@example.test>", "C", "c")
    };
    r.fake.mail().thread(
        "t1",
        vec![
            MsgSpec::new("m1", "Sam <sam@example.test>", "A", "a").label("UNREAD"),
            archived,
            binned,
        ],
    );
}

#[tokio::test]
async fn a_mail_trash_is_told_and_its_undo_restores_each_message_exactly() {
    let trash = json!({"ids": ["t1"], "trash": true});
    let r = rig_of(
        vec![label("c1", trash.clone()), Scripted::text("Done.")],
        |_| {},
    )
    .await;
    mixed_thread(&r);
    let before: Vec<_> = ["m1", "m2", "m3"].iter().map(|m| sorted(&r, m)).collect();
    assert!(plan(&r, "gmail.label", trash).authority.is_none());
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "clear that out").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    for m in ["m1", "m2", "m3"] {
        assert!(sorted(&r, m).contains(&"TRASH".to_string()), "{m}");
    }
    // A message that joins the thread afterwards is no part of the act.
    r.fake
        .mail()
        .deliver("t1", MsgSpec::new("m4", "Sam <sam@example.test>", "D", "d"));
    let trashed: Vec<_> = ["m1", "m2", "m3", "m4"]
        .iter()
        .map(|m| sorted(&r, m))
        .collect();
    let words = format!(
        "ℹ️ FYI · Moved mail to the Trash\n{}\nMoved 3 Gmail messages to the Trash.\n\
         Undo puts each one back exactly as it was.",
        by(&sid)
    );
    let now = || -> Vec<_> {
        ["m1", "m2", "m3", "m4"]
            .iter()
            .map(|m| sorted(&r, m))
            .collect()
    };
    press(&r, &words, || now() == trashed).await;
    // Each back as it was: the unread one in the inbox, the archived one
    // still archived, the one already in the Trash still there.
    let after: Vec<_> = ["m1", "m2", "m3"].iter().map(|m| sorted(&r, m)).collect();
    assert_eq!(after, before);
    assert_eq!(sorted(&r, "m4"), vec!["INBOX".to_string()]);
    assert!(
        r.fake.log().iter().all(|l| !l.path.contains("/m4")),
        "the later message was never touched"
    );
}

/// The second message's trash is refused: the first moved and is told, with
/// an Undo for it alone; the third is as it was.
#[tokio::test]
async fn a_mail_trash_that_fails_partway_is_told_for_what_moved() {
    let trash = json!({"ids": ["t1"], "trash": true});
    let r = rig_of(
        vec![label("c1", trash), Scripted::text("It failed.")],
        |_| {},
    )
    .await;
    r.fake.mail().thread(
        "t1",
        ["m1", "m2", "m3"]
            .iter()
            .map(|m| MsgSpec::new(m, "Sam <sam@example.test>", "A", "a"))
            .collect(),
    );
    r.fake
        .route("POST", "/gmail/v1/users/me/messages/m2/trash", |_| {
            Reply::json(
                403,
                json!({"error": {"code": 403, "message": "Forbidden",
                    "errors": [{"reason": "forbidden"}]}}),
            )
        });
    let sid = session(&r.core);
    turn(&r.core, &sid, "clear that out").await;
    assert_eq!(sorted(&r, "m1"), vec!["TRASH".to_string()]);
    for m in ["m2", "m3"] {
        assert_eq!(sorted(&r, m), vec!["INBOX".to_string()], "{m}");
    }
    let words = format!(
        "ℹ️ FYI · Moved mail to the Trash\n{}\nMoved 1 of 3 Gmail messages to the Trash; then \
         Google refused one, and the rest are as they were.\nUndo puts each one back exactly \
         as it was.",
        by(&sid)
    );
    press(&r, &words, || sorted(&r, "m1") == ["TRASH"]).await;
    for m in ["m1", "m2", "m3"] {
        assert_eq!(sorted(&r, m), vec!["INBOX".to_string()], "{m}");
    }
}

/// An Undo that can put back only part says how much, never that nothing
/// was made: one message Google no longer has is said plainly, and the note
/// stays open for another press.
#[tokio::test]
async fn an_undo_that_restores_part_says_partly_restored() {
    let trash = json!({"ids": ["t1"], "trash": true});
    let r = rig_of(vec![label("c1", trash), Scripted::text("Done.")], |_| {}).await;
    r.fake.mail().thread(
        "t1",
        ["m1", "m2"]
            .iter()
            .map(|m| MsgSpec::new(m, "Sam <sam@example.test>", "A", "a"))
            .collect(),
    );
    turn(&r.core, &session(&r.core), "clear that out").await;
    // The second is gone for good: emptied from the Trash.
    r.fake.route("GET", "/gmail/v1/users/me/messages/m2", |_| {
        Reply::json(
            404,
            json!({"error": {"code": 404, "message": "Not Found",
                "errors": [{"reason": "notFound"}]}}),
        )
    });
    let n = notes(&r);
    let post = n[0].1.clone().expect("an Undo");
    let e = r.core.told_undo(&post, &owner()).await.unwrap_err();
    let said = "Partly restored: 1 of 2. 1 of them no longer exist in Google (deleted for \
                good, or emptied from the Trash), so they cannot come back.";
    assert_eq!(e.data["cause"], said, "{}", e.message);
    assert_eq!(e.data["unfinished"], true);
    assert!(e.message.contains(said), "{}", e.message);
    assert!(!e.message.contains("not made"), "{}", e.message);
    assert_eq!(sorted(&r, "m1"), vec!["INBOX".to_string()]);
    // Not marked undone: a press again tries again, and puts back no more.
    let again = r.core.told_undo(&post, &owner()).await.unwrap_err();
    assert_eq!(again.data["cause"], said, "{}", again.message);
}

/// The owner's explicit "approve" makes a Gmail trash ask, typed or through
/// `google.call`; nothing is trashed and nothing is told.
#[tokio::test]
async fn an_explicit_approve_still_asks_for_a_mail_trash_typed_or_generic() {
    for (tool_name, script) in [
        (
            "gmail.label",
            label("c1", json!({"ids": ["t1"], "trash": true})),
        ),
        (
            "google.call",
            call(
                "c1",
                json!({"method": "gmail.users.messages.trash", "params": {"id": "m1"}}),
            ),
        ),
    ] {
        let r = rig_of(vec![script], |c| {
            c.policy.tools.insert(tool_name.into(), Posture::Approve);
        })
        .await;
        r.fake.mail().thread(
            "t1",
            vec![MsgSpec::new("m1", "Sam <sam@example.test>", "A", "a")],
        );
        let res = turn(&r.core, &session(&r.core), "clear that out").await;
        assert!(res.awaiting_confirm.is_some(), "{tool_name}: {res:?}");
        assert_eq!(sorted(&r, "m1"), vec!["INBOX".to_string()], "{tool_name}");
        assert!(notes(&r).is_empty(), "{tool_name}");
    }
}

#[tokio::test]
async fn a_google_call_message_trash_is_told_and_its_undo_keeps_it_archived() {
    let trash = json!({"method": "gmail.users.messages.trash", "params": {"id": "m2"}});
    let r = rig_of(
        vec![call("c1", trash.clone()), Scripted::text("Done.")],
        |_| {},
    )
    .await;
    mixed_thread(&r);
    let p = plan(&r, "google.call", trash);
    assert!(p.authority.is_none(), "{p:?}");
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "bin that one").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(sorted(&r, "m2").contains(&"TRASH".to_string()));
    let words = format!(
        "ℹ️ FYI · Moved mail to the Trash\n{}\nMoved 1 Gmail message to the Trash.\n\
         Undo puts each one back exactly as it was.",
        by(&sid)
    );
    press(&r, &words, || {
        sorted(&r, "m2").contains(&"TRASH".to_string())
    })
    .await;
    assert_eq!(sorted(&r, "m2"), vec!["Label_7".to_string()]);
    // The undo's own calls are in the ledger, as any Google call is.
    let called = ledgered(&r.core, "google.called");
    assert!(
        called
            .iter()
            .any(|c| c["method"] == "gmail.users.messages.untrash"),
        "{called:?}"
    );
    // A node that is no told act's is refused, and so is an unknown one.
    for id in ["nod_nothing", ""] {
        let e = r.core.google_take_back(id).await.unwrap_err();
        assert!(e.message.contains("no Google act"), "{}", e.message);
    }
}

/// A whole thread trashed through `google.call` asks: what it would move is
/// not read first, so it could not be put back exactly.
#[tokio::test]
async fn a_google_call_thread_trash_asks() {
    let trash = json!({"method": "gmail.users.threads.trash", "params": {"id": "t1"}});
    let r = rig_of(vec![call("c1", trash)], |c| {
        c.policy.enforcement = Posture::Open;
        c.policy.tools.insert("google.call".into(), Posture::Open);
    })
    .await;
    mixed_thread(&r);
    let res = turn(&r.core, &session(&r.core), "bin the thread").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(seen(&r, "POST", "/threads/t1/trash"), 0);
    assert!(notes(&r).is_empty());
}

/// The note quotes nothing anyone else wrote and names no one: not the
/// conversation's title, not a subject, not an address, not an id.
#[tokio::test]
async fn a_note_names_no_title_subject_address_or_id() {
    let trash = json!({"ids": ["t1"], "trash": true});
    let r = rig_of(vec![label("c1", trash), Scripted::text("Done.")], |_| {}).await;
    r.fake.mail().thread(
        "t1",
        vec![MsgSpec::new(
            "m1",
            "Alice Example <alice@example.test>",
            "Secret merger plan",
            "x",
        )],
    );
    let mut rec =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.label = Some("Re: Secret merger plan from alice@example.test".into());
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id.clone();
    turn(&r.core, &sid, "bin the merger mail from alice@example.test").await;
    let n = notes(&r);
    assert_eq!(n.len(), 1, "{n:?}");
    assert!(n[0].0.contains(&by(&sid)), "{}", n[0].0);
    // Its own words, the conversation's short id aside.
    let words = n[0].0.replace(&by(&sid), "");
    for bad in [
        "Secret",
        "merger",
        "alice",
        "Alice",
        "example.test",
        "t1",
        "m1",
        "Re:",
    ] {
        assert!(!words.contains(bad), "{bad:?} in {}", n[0].0);
    }
}

// ------------------------------------------------------------------ calendar

#[tokio::test]
async fn a_calendar_made_through_a_turn_is_told_once_with_no_name_and_no_undo() {
    let made = json!({"method": "calendar.calendars.insert", "body": {"summary": "Trip with Sam"}});
    let r = rig_of(
        vec![call("c1", made.clone()), Scripted::text("Made.")],
        |_| {},
    )
    .await;
    r.fake.json(
        "POST",
        "/calendar/v3/calendars",
        json!({"id": "cal9@group.example.test", "summary": "Trip with Sam"}),
    );
    assert!(plan(&r, "google.call", made).authority.is_none());
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "make me a calendar for the trip").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    let n = notes(&r);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(
        n[0].1, None,
        "no Undo: taking it back is a delete, which asks"
    );
    let head = format!(
        "ℹ️ FYI · Made a calendar\n{}\nMade 1 Google calendar.\nTo take it back, delete it: \
         that asks you first.\nAct: ",
        by(&sid)
    );
    assert!(n[0].0.starts_with(&head), "{}", n[0].0);
    // The act is named by its result's id, and the calendar not at all.
    let act = &n[0].0[head.len()..];
    assert!(
        !act.is_empty() && !act.contains(char::is_whitespace),
        "{act}"
    );
    for bad in ["Trip", "Sam", "cal9", "example.test"] {
        assert!(!n[0].0[..head.len()].contains(bad), "{bad:?} in {}", n[0].0);
        assert!(!act.contains(bad), "{bad:?} in {act}");
    }
}

// ------------------------------------------------------------------ what still asks

#[tokio::test]
async fn a_message_put_into_the_mailbox_still_asks_and_nothing_is_put_or_told() {
    // A forged sender and subject in the owner's own inbox could deceive him,
    // and a note that names an id cannot warn him well enough: it asks.
    let insert = json!({"method": "gmail.users.messages.insert", "body": {"raw": "eA"}});
    let import = json!({"method": "gmail.users.messages.import", "body": {"raw": "eA"}});
    let r = rig_of(
        vec![call("c1", insert.clone()), Scripted::text("Done.")],
        |_| {},
    )
    .await;
    let t = tool(&r, "google.call");
    for input in [&insert, &import] {
        let p = plan(&r, "google.call", input.clone());
        let why = p.authority.unwrap_or_default();
        assert!(
            why.starts_with("puts a message into the mailbox, so the owner decides it"),
            "{why}"
        );
        let p = plan(&r, "google.call", input.clone());
        assert!(!r.core.tools.policy.posture_only(t.as_ref(), &p), "{input}");
        assert_eq!(
            r.core.tools.policy.decide(t.as_ref(), &p).posture,
            Posture::Approve,
            "{input}"
        );
    }
    let res = turn(&r.core, &session(&r.core), "file this note in my inbox").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    let put = r
        .fake
        .log()
        .into_iter()
        .any(|l| l.verb == "POST" && l.path.starts_with("/gmail/v1/users/me/messages"));
    assert!(!put, "nothing was put into the mailbox");
    assert_eq!(ledgered(&r.core, "tool.confirm_requested").len(), 1);
    assert!(notes(&r).is_empty());
}

/// Through `google.call`, every KEEP call and every call off the allow list
/// asks at every posture: what sends, deletes, shares, invites, hides mail
/// or changes how mail is handled.
#[tokio::test]
async fn the_keep_calls_and_the_unlisted_writes_still_ask_at_every_posture() {
    let r = rig_of(vec![], |c| {
        c.policy.enforcement = Posture::Open;
        c.policy.tools.insert("google.call".into(), Posture::Open);
    })
    .await;
    let t = tool(&r, "google.call");
    let attendees = json!({"summary": "x", "attendees": [{"email": "a@example.test"}]});
    for (m, params, body) in [
        (
            "calendar.calendars.delete",
            json!({"calendarId": "cal9"}),
            None,
        ),
        ("calendar.calendars.clear", json!({}), None),
        ("gmail.users.messages.send", json!({}), None),
        ("gmail.users.messages.insert", json!({}), None),
        ("gmail.users.messages.import", json!({}), None),
        ("gmail.users.messages.delete", json!({"id": "m77"}), None),
        ("gmail.users.messages.batchDelete", json!({}), None),
        ("drive.files.delete", json!({"fileId": "f1"}), None),
        ("drive.permissions.create", json!({"fileId": "f1"}), None),
        ("calendar.acl.insert", json!({}), None),
        (
            "calendar.events.insert",
            json!({"sendUpdates": "all"}),
            None,
        ),
        // Off the allow list (issue 57).
        (
            "drive.files.create",
            json!({}),
            Some(json!({"name": "x", "parents": ["team"]})),
        ),
        (
            "drive.files.copy",
            json!({"fileId": "f1"}),
            Some(json!({"parents": ["team"]})),
        ),
        ("calendar.events.insert", json!({}), Some(attendees.clone())),
        (
            "calendar.events.patch",
            json!({"eventId": "e1"}),
            Some(attendees),
        ),
        (
            "gmail.users.messages.modify",
            json!({"id": "m1"}),
            Some(json!({"removeLabelIds": ["INBOX"]})),
        ),
        (
            "gmail.users.messages.batchModify",
            json!({}),
            Some(json!({"ids": ["m1"], "addLabelIds": ["SPAM"]})),
        ),
        (
            "gmail.users.threads.modify",
            json!({"id": "t1"}),
            Some(json!({"addLabelIds": ["TRASH"]})),
        ),
        (
            "POST /gmail/v1/users/me/settings/filters",
            json!({}),
            Some(json!({"action": {}})),
        ),
        (
            "PUT /gmail/v1/users/me/settings/autoForwarding",
            json!({}),
            Some(json!({"enabled": true})),
        ),
        (
            "PUT /gmail/v1/users/me/settings/vacation",
            json!({}),
            Some(json!({"enableAutoReply": true})),
        ),
        ("POST /drive/v3/files/f1/watch", json!({}), Some(json!({}))),
    ] {
        let mut input = json!({"method": m, "params": params});
        if let Some(b) = body {
            input["body"] = b;
        }
        let p = plan(&r, "google.call", input.clone());
        assert!(p.authority.is_some(), "{input}");
        assert_eq!(
            r.core.tools.policy.decide(t.as_ref(), &p).posture,
            Posture::Approve,
            "{input}"
        );
    }
}
