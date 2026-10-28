//! Gmail through the whole core, against the fake's mailbox (the Google
//! design §3.1 rows 1 to 5, §4.2, and the owner's decision that every send
//! asks): the five tools, a send bound to the draft the card showed, the
//! draft's exemption from the hold, and what a read marks as outside text.

use serde_json::{json, Value};
use theseus_google::fake::gmail::MsgSpec;
use theseus_google::fake::Reply;
use theseus_tools::{External, Plan, ToolCtx, ToolFailure, ToolOutput};

use super::mime;
use super::tests::*;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::test_support::turn;

const STRANGER: &str = "Sam Stranger <stranger@example.test>";

type Ran = Result<(ToolOutput, Option<External>), ToolFailure>;

async fn run(r: &Rig, tool: &str, input: Value) -> Ran {
    let t = r.core.tools.registry.get(tool).expect("registered").clone();
    t.run_async(&input, &ToolCtx::for_tests(r.dir.path())).await
}

fn plan(r: &Rig, tool: &str, input: Value) -> Result<Plan, String> {
    let t = r.core.tools.registry.get(tool).expect("registered").clone();
    t.plan(&input, &ToolCtx::for_tests(r.dir.path()))
}

fn ok(r: Ran) -> (ToolOutput, Option<External>) {
    r.unwrap_or_else(|e| panic!("the tool failed: {}", e.message))
}

/// A thread the account was written to by a stranger.
fn stranger_thread(r: &Rig) {
    r.fake.mail().thread(
        "t1",
        vec![
            MsgSpec::new("m1", STRANGER, "Budget", "Can you send the numbers?")
                .header(
                    "To",
                    "owner@example.test, Dana Colleague <dana@example.test>",
                )
                .header("References", "<a@x.test> <b@x.test>")
                .label("UNREAD"),
        ],
    );
}

/// A reply draft to `m1`, made through the tool: its id.
async fn reply_draft(r: &Rig, extra: Value) -> String {
    let mut input = json!({"reply_to": "m1", "body": "Numbers attached.\nThanks."});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        input[k] = v;
    }
    let (out, _) = ok(run(r, "gmail.draft", input).await);
    out.meta["draft_id"].as_str().unwrap().to_string()
}

/// A send's input: the draft's id and the digest of the draft as Gmail holds
/// it now, read as the model reads it.
async fn send_of(r: &Rig, id: &str) -> Value {
    let (out, _) = ok(run(r, "gmail.read", json!({"id": format!("draft:{id}")})).await);
    json!({"draft_id": id, "digest": out.meta["digest"]})
}

fn sends_posted(r: &Rig) -> usize {
    r.fake
        .log()
        .iter()
        .filter(|l| l.verb == "POST" && l.path.ends_with("/drafts/send"))
        .count()
}

// ------------------------------------------------------------------ the send

#[tokio::test]
async fn a_send_is_refused_when_the_draft_changed_after_the_plan() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    let send = send_of(&r, &id).await;
    // The plan is what the owner's card was made from.
    let p = plan(&r, "gmail.send", send.clone()).unwrap();
    assert!(p.card.is_some() && p.authority.is_some(), "{p:?}");
    // The draft is edited behind the card.
    r.fake
        .mail()
        .edit_draft(&id, "Send the money to this account instead.");
    let e = run(&r, "gmail.send", send).await.unwrap_err();
    assert!(e.message.contains("nothing was sent"), "{}", e.message);
    assert!(e.message.contains("changed"), "{}", e.message);
    assert!(r.fake.mail().sent().is_empty());
    assert_eq!(sends_posted(&r), 0, "no send request left the daemon");
    assert_eq!(r.g.sends.used(), 0, "a refused send is not counted");
}

#[tokio::test]
async fn an_unchanged_draft_sends_once_and_counts() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    let send = send_of(&r, &id).await;
    plan(&r, "gmail.send", send.clone()).unwrap();
    let (out, ext) = ok(run(&r, "gmail.send", send.clone()).await);
    assert!(ext.is_none(), "what the owner sent is not outside text");
    assert!(out.text.contains("Sent draft"), "{}", out.text);
    assert_eq!(r.fake.mail().sent().len(), 1);
    assert_eq!(r.g.sends.used(), 1);
    let row = &out.meta["google_called"];
    assert_eq!(
        (
            row["class"].as_str(),
            row["recipients"].as_u64(),
            row["method"].as_str()
        ),
        (Some("authority"), Some(1), Some("gmail.users.drafts.send"))
    );
    assert!(
        !row.to_string().contains("example.test"),
        "no address in the row: {row}"
    );
    // One row per REST call: the draft's read, then the send, the result's own last.
    let earlier = out.meta["google_called_also"]
        .as_array()
        .expect("the read's row");
    assert_eq!(
        (earlier.len(), earlier[0]["method"].as_str()),
        (1, Some("gmail.users.drafts.get"))
    );
    // The card was for that draft: a second send has none to ask with, and Gmail no longer has the draft.
    let e = plan(&r, "gmail.send", send.clone()).unwrap_err();
    assert!(e.contains("has not been shown"), "{e}");
    run(&r, "gmail.send", send).await.unwrap_err();
    assert_eq!(sends_posted(&r), 1);
}

#[tokio::test]
async fn a_draft_written_again_unpins_the_card() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    let send = send_of(&r, &id).await;
    plan(&r, "gmail.send", send.clone()).unwrap();
    // The model rewrites the draft after the card: the card is not for it, and
    // the old digest is no longer one this daemon showed.
    ok(run(
        &r,
        "gmail.draft",
        json!({"draft_id": id, "reply_to": "m1", "body": "Different."}),
    )
    .await);
    let e = run(&r, "gmail.send", send.clone()).await.unwrap_err();
    assert!(e.message.contains("nothing was sent"), "{}", e.message);
    assert_eq!(sends_posted(&r), 0);
    assert!(plan(&r, "gmail.send", send).is_err());
    // A new plan, with the new digest, shows the new draft, and that one sends.
    let send = send_of(&r, &id).await;
    let p = plan(&r, "gmail.send", send.clone()).unwrap();
    assert!(p.summary.contains("Different."), "{}", p.summary);
    ok(run(&r, "gmail.send", send).await);
    assert_eq!(r.fake.mail().sent().len(), 1);
}

#[tokio::test]
async fn a_send_asks_at_every_posture_and_no_grant_loosens_it() {
    let open = |c: &mut crate::Config| {
        c.policy.enforcement = Posture::Open;
        c.policy.tools.insert("gmail.send".into(), Posture::Open);
    };
    // The draft the script sends is made the same way below, so its digest is known.
    let digest = {
        let r0 = rig(vec![]).await;
        stranger_thread(&r0);
        let id = reply_draft(&r0, json!({})).await;
        send_of(&r0, &id).await["digest"].clone()
    };
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![
            Scripted::tools("", &[("c1", "gmail_read", json!({"id": "t1"}))]),
            Scripted::text("Read."),
            Scripted::tools(
                "",
                &[(
                    "c2",
                    "gmail_send",
                    json!({"draft_id": "r1", "digest": digest}),
                )],
            ),
        ],
        open,
    )
    .await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    assert_eq!(id, "r1");
    let sid = session(&r.core);
    // The session reads mail and the owner trusts it away: as clean as a session gets.
    turn(&r.core, &sid, "read it").await;
    r.core.trust_session(&sid, "test").unwrap();
    // The plan names the authority, so the decision is Approve with every line
    // `open`, and the one question each loosening layer asks first (a
    // mode, a routine's grant) is whether
    // only the tool's posture decides the call. Here it does not.
    let tool = r.core.tools.registry.get("gmail.send").unwrap().clone();
    let p = plan(&r, "gmail.send", send_of(&r, &id).await).unwrap();
    let d = r.core.tools.policy.decide(tool.as_ref(), &p);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    assert!(!r.core.tools.policy.posture_only(tool.as_ref(), &p));
    // A routine may be granted the tool by name, and the grant still does not reach it.
    assert!(
        crate::routine::grant::check(&["gmail.send".to_string()], &|t| t == "gmail.send").is_ok()
    );
    // Through a whole turn: the send waits, and nothing was sent.
    let res = turn(&r.core, &sid, "send it").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(sends_posted(&r), 0, "nothing was sent");
    let asked = ledgered(&r.core, "tool.confirm_requested");
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0]["reason"].as_str().unwrap().contains("sends mail"),
        "{asked:?}"
    );
}

#[tokio::test]
async fn send_is_non_repeatable_and_reports_outcome_unknown() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    let send = send_of(&r, &id).await;
    plan(&r, "gmail.send", send.clone()).unwrap();
    // The send goes out, and the connection fails on the way back.
    r.fake.mail().after_send(Reply::server_error());
    let e = run(&r, "gmail.send", send).await.unwrap_err();
    assert!(e.message.contains("outcome unknown"), "{}", e.message);
    assert_eq!(e.meta["outcome_unknown"], true);
    assert_eq!(sends_posted(&r), 1, "never sent again by itself");
    assert_eq!(r.fake.mail().sent().len(), 1);
    assert_eq!(r.g.sends.used(), 1, "a send that may have gone is counted");
}

#[tokio::test]
async fn the_daily_send_cap_counts_google_call_sends() {
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![],
        |c| c.google.max_sends_per_day = 2,
    )
    .await;
    stranger_thread(&r);
    // One send through the escape hatch...
    r.fake.mail().put_draft(
        "outside",
        MsgSpec::new("dx", "owner@example.test", "Hi", "Hello."),
    );
    ok(run(
        &r,
        "google.call",
        json!({"method": "gmail.users.drafts.send", "body": {"id": "outside"}}),
    )
    .await);
    // ...one through gmail.send...
    let id = reply_draft(&r, json!({})).await;
    let send = send_of(&r, &id).await;
    plan(&r, "gmail.send", send.clone()).unwrap();
    ok(run(&r, "gmail.send", send).await);
    assert_eq!(r.g.sends.used(), 2);
    // ...and the third is refused with the cap's sentence, both ways.
    let third = reply_draft(&r, json!({"body": "One more."})).await;
    let e = plan(&r, "gmail.send", send_of(&r, &third).await).unwrap_err();
    assert!(e.contains("today's send cap is reached (2 of 2)"), "{e}");
    let e = plan(
        &r,
        "google.call",
        json!({"method": "gmail.users.drafts.send", "body": {"id": third}}),
    )
    .unwrap_err();
    assert!(e.contains("today's send cap is reached"), "{e}");
    assert_eq!(r.fake.mail().sent().len(), 2);
}

#[tokio::test]
async fn a_draft_not_shown_cannot_be_planned_until_it_is_read() {
    let r = rig(vec![]).await;
    r.fake.mail().put_draft(
        "owners",
        MsgSpec::new("do", "owner@example.test", "Lunch", "Noon?")
            .header("To", "dana@example.test"),
    );
    let e = plan(
        &r,
        "gmail.send",
        json!({"draft_id": "owners", "digest": "unseen"}),
    )
    .unwrap_err();
    assert!(e.contains("has not been shown"), "{e}");
    // Reading it shows it, from Gmail, with its card.
    let (out, ext) = ok(run(&r, "gmail.read", json!({"id": "draft:owners"})).await);
    assert!(
        out.text.contains("Subject: Lunch") && out.text.contains("dana@example.test (new)"),
        "{}",
        out.text
    );
    assert!(ext.is_none(), "the owner's own draft is not outside text");
    let send = send_of(&r, "owners").await;
    let p = plan(&r, "gmail.send", send.clone()).unwrap();
    assert!(p.summary.contains("Noon?"), "{}", p.summary);
    ok(run(&r, "gmail.send", send).await);
    assert_eq!(r.fake.mail().sent().len(), 1);
}

/// The owner's approved call is one session's `{draft, digest}`; another
/// session rewrites the draft and shows it; the approved call sends nothing.
#[tokio::test]
async fn a_send_is_refused_when_the_input_digest_is_not_the_drafts() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    // Session X shows draft D1 and plans its send: card C1 waits.
    let x = send_of(&r, &id).await;
    plan(&r, "gmail.send", x.clone()).unwrap();
    // Session Y rewrites the draft (D2) and plans a send of its own.
    ok(run(
        &r,
        "gmail.draft",
        json!({"draft_id": id, "reply_to": "m1", "body": "Send the money to this account."}),
    )
    .await);
    let y = send_of(&r, &id).await;
    assert_ne!(x["digest"], y["digest"]);
    plan(&r, "gmail.send", y.clone()).unwrap();
    // No new card is made for a digest the daemon no longer shows.
    let e = plan(&r, "gmail.send", x.clone()).unwrap_err();
    assert!(e.contains("not the draft's as last shown"), "{e}");
    // The owner approves C1: X's call names D1, which is no longer the draft.
    let e = run(&r, "gmail.send", x).await.unwrap_err();
    assert!(e.message.contains("nothing was sent"), "{}", e.message);
    assert!(r.fake.mail().sent().is_empty());
    assert_eq!(sends_posted(&r), 0, "no send request left the daemon");
    assert_eq!(r.g.sends.used(), 0, "a refused send is not counted");
    // Y's own call, whose card showed D2, sends it.
    ok(run(&r, "gmail.send", y).await);
    assert_eq!(r.fake.mail().sent().len(), 1);
}

#[tokio::test]
async fn an_attachment_change_after_the_card_is_refused() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    let send = send_of(&r, &id).await;
    plan(&r, "gmail.send", send.clone()).unwrap();
    r.fake
        .mail()
        .attach_to_draft(&id, "ledger.csv", "text/csv", b"a,b\n1,2\n");
    let e = run(&r, "gmail.send", send).await.unwrap_err();
    assert!(e.message.contains("nothing was sent"), "{}", e.message);
    assert_eq!(sends_posted(&r), 0);
}

/// Gmail gives an attachment a new id each time its draft is read, so the
/// digest names it by what the card shows (name, type, size): an unchanged
/// draft with an attachment still sends.
#[tokio::test]
async fn an_unchanged_draft_with_an_attachment_sends() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    r.fake
        .mail()
        .attach_to_draft(&id, "ledger.csv", "text/csv", b"a,b\n1,2\n");
    let send = send_of(&r, &id).await;
    plan(&r, "gmail.send", send.clone()).unwrap();
    ok(run(&r, "gmail.send", send).await);
    assert_eq!(sends_posted(&r), 1);
}

/// A file swapped after the card was shown, for another of the same name,
/// type and byte size, is a changed draft: the digest holds the bytes' hash.
#[tokio::test]
async fn a_same_name_same_size_attachment_swap_is_refused() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    r.fake
        .mail()
        .attach_to_draft(&id, "ledger.csv", "text/csv", b"a,b\n1,2\n");
    let send = send_of(&r, &id).await;
    plan(&r, "gmail.send", send.clone()).unwrap();
    r.fake
        .mail()
        .replace_draft_file(&id, "ledger.csv", b"a,b\n9,9\n");
    let e = run(&r, "gmail.send", send).await.unwrap_err();
    assert!(e.message.contains("nothing was sent"), "{}", e.message);
    assert_eq!(sends_posted(&r), 0);
}

/// The bytes are read to be hashed: a read that fails refuses the send, with
/// Gmail's error, and nothing is sent.
#[tokio::test]
async fn a_draft_attachment_read_failure_refuses_the_send() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    r.fake
        .mail()
        .attach_to_draft(&id, "ledger.csv", "text/csv", b"a,b\n1,2\n");
    let send = send_of(&r, &id).await;
    plan(&r, "gmail.send", send.clone()).unwrap();
    r.fake.route(
        "GET",
        "/gmail/v1/users/me/messages/*/attachments/*",
        |_| Reply::json(500, json!({"error": {"code": 500, "message": "Attachment unreadable", "errors": [{"reason": "backendError"}]}})),
    );
    let e = run(&r, "gmail.send", send).await.unwrap_err();
    assert!(e.message.contains("Attachment unreadable"), "{}", e.message);
    assert_eq!(sends_posted(&r), 0);
}

#[tokio::test]
async fn an_html_only_change_after_the_card_is_refused() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    let send = send_of(&r, &id).await;
    plan(&r, "gmail.send", send.clone()).unwrap();
    // The plain part, which the card shows, is untouched: only HTML is added.
    r.fake.mail().edit_draft_html(
        &id,
        "<p>Numbers attached.</p><a href=\"https://x.test\">pay</a>",
    );
    let e = run(&r, "gmail.send", send).await.unwrap_err();
    assert!(e.message.contains("nothing was sent"), "{}", e.message);
    assert_eq!(sends_posted(&r), 0);
}

#[tokio::test]
async fn the_send_question_carries_the_mail_preview() {
    let digest = {
        let r0 = rig(vec![]).await;
        stranger_thread(&r0);
        let id = reply_draft(&r0, json!({"cc": ["pat@example.test"]})).await;
        send_of(&r0, &id).await["digest"].clone()
    };
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![Scripted::tools(
            "",
            &[(
                "c1",
                "gmail_send",
                json!({"draft_id": "r1", "digest": digest}),
            )],
        )],
        |_| {},
    )
    .await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({"cc": ["pat@example.test"]})).await;
    assert_eq!(id, "r1");
    let _ = send_of(&r, &id).await;
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "send it").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    let asked = ledgered(&r.core, "tool.confirm_requested");
    let preview = &asked[0]["preview"];
    assert_eq!(preview["kind"], "mail", "{asked:?}");
    let body = preview["body"].as_str().unwrap();
    assert!(body.contains("pat@example.test (new)"), "{body}");
    assert!(body.contains("Subject: Re: Budget"), "{body}");
    let want = format!("Draft {}", &digest.as_str().unwrap()[..12]);
    assert!(body.lines().last().unwrap() == want, "{body}");
    // `confirm.list` words the same question the same way.
    let listed = r.core.confirm_list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].preview.as_ref().map(|p| p.body.as_str()),
        Some(body)
    );
    assert_eq!(sends_posted(&r), 0, "nothing was sent");
}

// ------------------------------------------------------------------ drafts

#[tokio::test]
async fn a_reply_draft_threads_with_in_reply_to_and_references() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    let id = reply_draft(&r, json!({})).await;
    let h = r.fake.mail().draft_headers(&id);
    let get = |n: &str| h.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
    assert_eq!(
        get("In-Reply-To").as_deref(),
        Some("<m1@mail.example.test>")
    );
    assert_eq!(
        get("References").as_deref(),
        Some("<a@x.test> <b@x.test> <m1@mail.example.test>")
    );
    assert_eq!(get("Subject").as_deref(), Some("Re: Budget"));
    assert_eq!(
        get("To").as_deref(),
        Some("Sam Stranger <stranger@example.test>")
    );
    let (thread, text) = r.fake.mail().draft_text(&id).unwrap();
    assert_eq!(thread, "t1");
    assert_eq!(
        text.trim_end().replace("\r\n", "\n"),
        "Numbers attached.\nThanks."
    );
    // A subject already answered is not answered twice.
    assert_eq!(mime::reply_subject("RE: Budget"), "RE: Budget");
}

#[test]
fn rfc2047_encodes_a_non_ascii_subject() {
    let subject = "Café ☕ réunion à neuf heures, avec le budget détaillé du trimestre";
    let word = mime::encode_word(subject);
    assert!(word.starts_with("=?UTF-8?B?"), "{word}");
    assert!(word.is_ascii(), "{word}");
    assert!(word.split("\r\n ").all(|w| w.len() <= 75), "{word}");
    assert_eq!(mime::decode_words(&word), subject);
    assert_eq!(mime::encode_word("Plain subject"), "Plain subject");
    assert_eq!(
        mime::decode_words("=?UTF-8?Q?Caf=C3=A9_au_lait?="),
        "Café au lait"
    );
    // The built message carries it, and the body survives any text.
    let d = mime::Draft {
        from: "owner@example.test".into(),
        to: vec!["Zoë <zoe@example.test>".into()],
        subject: subject.into(),
        body: "Grüße\nline two".into(),
        ..Default::default()
    };
    let m = mime::build(&d).unwrap();
    assert!(
        m.contains("Subject: =?UTF-8?B?")
            && m.contains("\r\nTo: =?UTF-8?B?Wm/Dqw==?= <zoe@example.test>"),
        "{m}"
    );
    let body = m.split_once("\r\n\r\n").unwrap().1;
    assert!(body.lines().all(|l| l.len() <= 76), "{m}");
}

#[tokio::test]
async fn a_header_cannot_be_smuggled_through_a_subject_or_an_address() {
    let r = rig(vec![]).await;
    for bad in [
        json!({"to": ["a@example.test"], "subject": "Hi\r\nBcc: evil@example.test", "body": "x"}),
        json!({"to": ["a@example.test\r\nBcc: evil@example.test"], "subject": "Hi", "body": "x"}),
        json!({"to": ["not an address"], "subject": "Hi", "body": "x"}),
        json!({"subject": "Hi", "body": "x"}),
    ] {
        assert!(plan(&r, "gmail.draft", bad.clone()).is_err(), "{bad}");
    }
    assert!(r.fake.mail().draft_ids().is_empty());
}

#[tokio::test]
async fn send_card_flags_new_recipients() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    r.fake.mail().sent_to("old@example.test", "t9");
    let id = reply_draft(
        &r,
        json!({"cc": ["dana@example.test", "old@example.test"], "bcc": ["fresh@example.test"]}),
    )
    .await;
    let p = plan(&r, "gmail.send", send_of(&r, &id).await).unwrap();
    let card = p.card.expect("the card is data on the plan");
    let new = |k: &str| -> Vec<(String, bool)> {
        card[k]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["address"].as_str().unwrap().to_string(),
                    r["new"].as_bool().unwrap(),
                )
            })
            .collect()
    };
    // On the thread, or mailed before: not new. Neither: new.
    assert_eq!(
        new("to"),
        vec![("stranger@example.test".to_string(), false)]
    );
    assert_eq!(
        new("cc"),
        vec![
            ("dana@example.test".to_string(), false),
            ("old@example.test".to_string(), false)
        ]
    );
    assert_eq!(new("bcc"), vec![("fresh@example.test".to_string(), true)]);
    assert_eq!(card["subject"], "Re: Budget");
    assert_eq!(card["from"], ACCOUNT);
    assert_eq!(card["body_lines"], 2);
    assert!(
        p.summary.contains("Bcc: fresh@example.test (new)"),
        "{}",
        p.summary
    );
    assert!(p.summary.contains("Numbers attached."), "{}", p.summary);
}

#[tokio::test]
async fn a_cards_body_is_its_first_lines() {
    let r = rig(vec![]).await;
    let body: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    let (out, _) = ok(run(
        &r,
        "gmail.draft",
        json!({"to": ["dana@example.test"], "subject": "Long", "body": body}),
    )
    .await);
    let id = out.meta["draft_id"].as_str().unwrap();
    let p = plan(&r, "gmail.send", send_of(&r, id).await).unwrap();
    assert_eq!(
        p.card.as_ref().unwrap()["body"].as_array().unwrap().len(),
        super::mail::CARD_LINES
    );
    assert_eq!(p.card.unwrap()["body_lines"], 40);
    assert!(
        p.summary.contains("first 15 of 40 lines") && !p.summary.contains("line 16"),
        "{}",
        p.summary
    );
}

// ------------------------------------------------------------------ the hold

#[tokio::test]
async fn a_stranger_thread_holds_the_session() {
    let r = rig(vec![
        Scripted::tools("", &[("c1", "gmail_read", json!({"id": "t1"}))]),
        Scripted::text("Read."),
    ])
    .await;
    stranger_thread(&r);
    let sid = session(&r.core);
    turn(&r.core, &sid, "read the thread").await;
    let rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(rec.external.expect("a hold").tool, "gmail.read");
    let labels = rec
        .data
        .as_deref()
        .map(|f| f.labels.clone())
        .unwrap_or_default();
    assert_eq!(labels.len(), 1, "and labelled: {labels:?}");
    assert_eq!(labels[0].source, "google.gmail.read");
    // No token, however it was minted, reached a node or a row.
    let all = everything(&r.core, &[&sid]);
    assert!(!all.contains("ya29.") && !all.contains(theseus_google::fake::REFRESH_TOKEN));
    let rows = ledgered(&r.core, "google.called");
    assert!(
        !rows.is_empty() && !rows.iter().any(|r| r.to_string().contains("stranger")),
        "{rows:?}"
    );
}

#[tokio::test]
async fn a_thread_the_account_wrote_alone_is_not_external() {
    let r = rig(vec![]).await;
    r.fake.mail().thread(
        "t2",
        vec![
            MsgSpec::new("o1", "owner@example.test", "Notes", "Remember the milk."),
            MsgSpec::new("o2", "Owner <Owner@Example.Test>", "Re: Notes", "And eggs."),
        ],
    );
    let (out, ext) = ok(run(&r, "gmail.read", json!({"id": "t2"})).await);
    assert!(ext.is_none(), "{ext:?}");
    assert!(super::private::marked(&out.meta), "but it is private");
    assert!(
        out.text.contains("Remember the milk.") && out.text.contains("2 message(s)"),
        "{}",
        out.text
    );
    // One stranger among them, and it is outside text.
    r.fake.mail().thread(
        "t3",
        vec![
            MsgSpec::new("o3", "owner@example.test", "x", "mine"),
            MsgSpec::new("s3", STRANGER, "Re: x", "theirs"),
        ],
    );
    let (_, ext) = ok(run(&r, "gmail.read", json!({"id": "t3"})).await);
    assert_eq!(ext.unwrap().url, "gmail:thread/t3");
}

#[tokio::test]
async fn a_draft_runs_under_the_hold_and_a_label_asks() {
    let r = rig(vec![
        Scripted::tools("", &[("c1", "gmail_read", json!({"id": "t1"}))]),
        Scripted::tools(
            "",
            &[(
                "c2",
                "gmail_draft",
                json!({"reply_to": "m1", "body": "On it."}),
            )],
        ),
        Scripted::tools(
            "",
            &[(
                "c3",
                "gmail_label",
                json!({"ids": ["t1"], "remove": ["INBOX", "UNREAD"]}),
            )],
        ),
    ])
    .await;
    stranger_thread(&r);
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "read it, draft a reply, then archive it").await;
    // The read held the session; the draft still ran, with no card...
    assert_eq!(r.fake.mail().draft_ids().len(), 1, "{res:?}");
    // ...and the label, which an injected mail could use to hide itself, waited.
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert!(
        r.fake.mail().labels_of("m1").contains(&"INBOX".to_string()),
        "nothing was archived"
    );
    let asked = ledgered(&r.core, "tool.confirm_requested");
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0]["reason"].as_str().unwrap().contains("external"),
        "{asked:?}"
    );
}

// ------------------------------------------------------------------ reading

#[tokio::test]
async fn html_mail_reads_as_text() {
    let r = rig(vec![]).await;
    r.fake.mail().thread(
        "t4",
        vec![
            MsgSpec::new("h1", STRANGER, "Newsletter", "").html(
                "<html><body><p>Hello <b>team</b>,</p><p>Ship it &amp; relax.</p>\
                 <script>alert(1)</script>\
                 <div class=\"gmail_quote\">On Mon, Dana wrote:<blockquote>the old plan</blockquote></div>\
                 </body></html>",
            ),
            MsgSpec::new("h2", STRANGER, "Re: Newsletter", "Thanks!\n\nOn Mon, Dana wrote:\n> the old plan\n> line two\n\nBye."),
        ],
    );
    let (out, _) = ok(run(&r, "gmail.read", json!({"id": "t4"})).await);
    assert!(
        out.text.contains("Hello team") && out.text.contains("Ship it & relax."),
        "{}",
        out.text
    );
    assert!(
        !out.text.contains("alert(1)") && !out.text.contains("<p>") && !out.text.contains("<b>"),
        "{}",
        out.text
    );
    assert!(
        !out.text.contains("the old plan"),
        "quoted tails are folded: {}",
        out.text
    );
    assert!(out.text.contains("[quoted text"), "{}", out.text);
    assert!(
        out.text.contains("Thanks!") && out.text.contains("Bye."),
        "{}",
        out.text
    );
}

#[tokio::test]
async fn a_saved_attachment_lands_in_the_workspace() {
    let r = rig(vec![]).await;
    r.fake.mail().thread(
        "t5",
        vec![MsgSpec::new("a1", STRANGER, "Files", "See attached.")
            .file("report.txt", "text/plain", b"quarterly numbers")
            .file("../../evil.sh", "text/x-sh", b"echo hi")],
    );
    let input = json!({"id": "t5", "save_attachments": "inbox/files"});
    // The gate judges the directory as it does any write.
    let p = plan(&r, "gmail.read", input.clone()).unwrap();
    assert_eq!(p.class, Some(theseus_tools::ToolClass::Write));
    let dir = std::fs::canonicalize(r.dir.path())
        .unwrap()
        .join("inbox/files");
    assert_eq!(p.resources[0].path, dir);
    assert_eq!(p.resources[0].access, theseus_tools::Access::Write);
    let (out, _) = ok(run(&r, "gmail.read", input.clone()).await);
    assert!(
        out.text
            .contains("[attachment: report.txt (text/plain, 17 bytes)]"),
        "{}",
        out.text
    );
    assert_eq!(
        std::fs::read(dir.join("report.txt")).unwrap(),
        b"quarterly numbers"
    );
    // A name is only a name: it stays in the directory, and never replaces a file there.
    assert_eq!(std::fs::read(dir.join("evil.sh")).unwrap(), b"echo hi");
    ok(run(&r, "gmail.read", input).await);
    assert_eq!(
        std::fs::read(dir.join("1-report.txt")).unwrap(),
        b"quarterly numbers"
    );
    assert!(!r.dir.path().join("evil.sh").exists());
    // Without a directory nothing is saved and the call is a read.
    let p = plan(&r, "gmail.read", json!({"id": "t5"})).unwrap();
    assert_eq!(p.class, Some(theseus_tools::ToolClass::Read));
}

#[tokio::test]
async fn a_message_id_reads_when_it_is_no_threads_first() {
    let r = rig(vec![]).await;
    r.fake.mail().thread(
        "t6",
        vec![
            MsgSpec::new("n1", STRANGER, "Two", "first"),
            MsgSpec::new("n2", STRANGER, "Re: Two", "second"),
        ],
    );
    // The fake knows threads by their own ids: a message's id reads as that message.
    let (out, ext) = ok(run(&r, "gmail.read", json!({"id": "n2"})).await);
    assert!(
        out.text.contains("second") && out.text.contains("1 message(s)"),
        "{}",
        out.text
    );
    assert_eq!(ext.unwrap().url, "gmail:thread/t6");
    let e = run(&r, "gmail.read", json!({"id": "nowhere"}))
        .await
        .unwrap_err();
    assert!(
        e.message.contains("notFound")
            || e.message.contains("not found")
            || e.message.contains("404"),
        "{}",
        e.message
    );
    assert!(
        run(&r, "gmail.read", json!({"id": "../etc"}))
            .await
            .is_err()
            || plan(&r, "gmail.read", json!({"id": "../etc"})).is_err()
    );
}

#[tokio::test]
async fn search_lists_a_row_a_thread_and_marks_strangers() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    r.fake.mail().thread(
        "t2",
        vec![MsgSpec::new(
            "o1",
            "owner@example.test",
            "Notes",
            "Remember the milk.",
        )],
    );
    let (out, ext) = ok(run(&r, "gmail.search", json!({"q": "numbers"})).await);
    assert!(
        out.text.contains("1 thread(s)") && out.text.contains("t1 | "),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("Budget")
            && out.text.contains("unread")
            && out.text.contains("1 message(s)"),
        "{}",
        out.text
    );
    assert_eq!(ext.unwrap().url, "gmail:thread/t1");
    // Only the account's own mail: not outside text.
    let (_, ext) = ok(run(&r, "gmail.search", json!({"q": "milk"})).await);
    assert!(ext.is_none());
    // Nothing found: nothing to hold a session for.
    let (out, ext) = ok(run(&r, "gmail.search", json!({"q": "zzz-nothing"})).await);
    assert!(
        out.text.starts_with("0 thread(s)") && ext.is_none(),
        "{}",
        out.text
    );
    // One row for the list and each thread: the last is the result's own, the rest ride beside it.
    let (out, _) = ok(run(&r, "gmail.search", json!({"q": "", "max": 1})).await);
    let also = out.meta["google_called_also"].as_array().unwrap();
    assert_eq!(also.len(), 1, "{}", out.meta);
    assert_eq!(
        out.meta["google_called"]["method"],
        "gmail.users.threads.get"
    );
    assert!(out.text.contains("next page: page = 1"), "{}", out.text);
}

// ------------------------------------------------------------------ labels

#[tokio::test]
async fn label_changes_labels_by_name_and_trash_asks_by_authority() {
    let r = rig(vec![]).await;
    stranger_thread(&r);
    r.fake.mail().label("Label_7", "Receipts");
    let archive = json!({"ids": ["t1"], "add": ["receipts"], "remove": ["INBOX", "unread"]});
    let p = plan(&r, "gmail.label", archive.clone()).unwrap();
    assert!(p.authority.is_none(), "an archive is a notice, not a card");
    ok(run(&r, "gmail.label", archive).await);
    let labels = r.fake.mail().labels_of("m1");
    assert!(
        labels.contains(&"Label_7".to_string()) && !labels.contains(&"INBOX".to_string()),
        "{labels:?}"
    );
    // A message id works where a thread id does not.
    r.fake.mail().thread(
        "t8",
        vec![
            MsgSpec::new("p1", STRANGER, "A", "a"),
            MsgSpec::new("p2", STRANGER, "B", "b"),
        ],
    );
    ok(run(
        &r,
        "gmail.label",
        json!({"ids": ["p2"], "remove": ["INBOX"]}),
    )
    .await);
    assert!(!r.fake.mail().labels_of("p2").contains(&"INBOX".to_string()));
    // Trash asks, whatever any line says.
    let trash = json!({"ids": ["t1"], "trash": true});
    assert!(plan(&r, "gmail.label", trash.clone())
        .unwrap()
        .authority
        .is_some());
    ok(run(&r, "gmail.label", trash).await);
    assert!(r.fake.mail().labels_of("m1").contains(&"TRASH".to_string()));
    let e = run(
        &r,
        "gmail.label",
        json!({"ids": ["t1"], "add": ["No such label"]}),
    )
    .await
    .unwrap_err();
    assert!(
        e.message.contains("no Gmail label is named"),
        "{}",
        e.message
    );
    assert!(
        plan(&r, "gmail.label", json!({"ids": ["t1"]})).is_err(),
        "nothing to do"
    );
}

// ------------------------------------------------------------------ the census

#[tokio::test]
async fn every_mail_tool_is_deferred_and_its_census_row_is_small() {
    // As `tests_census` builds it: the test parts leave deferral off.
    let fake = theseus_google::fake::Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &fake, "file:/tmp/unused-google-refresh-token");
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![]));
    let mut parts = crate::rpc::Parts {
        secrets: board(Some(theseus_google::fake::REFRESH_TOKEN)),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    };
    parts.cfg.tools.defer.enabled = true;
    let core = crate::Core::build(parts).unwrap();
    let r = Rig {
        g: core.tools.google.clone().unwrap(),
        core,
        fake,
        dir,
    };
    for place in ["cli", "dm", "task"] {
        let (census, _, _) = r
            .core
            .context_census(&theseus_protocol::census::ContextCensusParams {
                session_id: None,
                place: Some(place.into()),
                exact: false,
            })
            .unwrap();
        let mail: Vec<_> = census
            .tools
            .iter()
            .filter(|t| t.name.starts_with("gmail_"))
            .collect();
        assert_eq!(mail.len(), super::mail::NAMES.len(), "{place}: {mail:?}");
        for t in mail {
            assert!(
                t.deferred,
                "{place}: {} is loaded in the first request",
                t.name
            );
            assert!(
                t.tokens <= 220,
                "{place}: {} costs {} tokens",
                t.name,
                t.tokens
            );
        }
    }
}

#[test]
fn no_mail_tool_is_in_a_family_that_is_never_hidden() {
    for name in super::mail::NAMES {
        let wire = theseus_tools::wire_name(name);
        assert_eq!(crate::tools_defer::family::of_wire(&wire), "mail", "{wire}");
    }
}
