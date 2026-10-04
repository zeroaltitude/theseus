//! Gliding through the whole core (38b, theseus-ypy0): `channel.post` and
//! `channel.read` on the place rule. Each path: allowed; asks first, then
//! approved and posted once; asks first, then declined and nothing posted;
//! a place not bound here; a read from a shared place marked as outside
//! text, which then holds the session; a post's destination's floor; and
//! the ledger rows. The rule's whole matrix is `places`' table test, and
//! the post's delivery through Discord's lane is `theseus-discord`'s.

use serde_json::{json, Value};

use crate::approval::{Answerer, Surface};
use crate::node::{Body, ResultStatus};
use crate::places::BoundPlace;
use crate::provider::{ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_places::{
    answers_a_call, last_user, rig_scripted, rows, session, turn, Rig, ALICE, OWNER,
};
use crate::Core;

/// A channel bound private, and two bound shared.
const DEN: u64 = 577_215_664_901_532_860;
const HALL: u64 = 161_803_398_874_989_484;
const PIER: u64 = 141_421_356_237_309_504;

/// What the owner said in their DM; no shared place may read it unasked.
const VAULT: &str = "the vault code is 4417";

/// The model of these tests: `POST <place> :: <text>` posts, `READ <place>`
/// reads (`READ <place> :: <n>`, its last n), and a call's result ends the
/// turn. Each call's id is its request's length, so a session's ids differ.
fn script(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    let last = last_user(req);
    let id = format!("g{}", req.messages.len());
    if let Some(rest) = last.strip_prefix("POST ") {
        let (to, text) = rest.split_once(" :: ").unwrap();
        let input = json!({"to": to, "text": text});
        return Scripted::tools("Posting.", &[(id.as_str(), "channel_post", input)]);
    }
    if let Some(rest) = last.strip_prefix("READ ") {
        let input = match rest.split_once(" :: ") {
            Some((from, n)) => json!({"from": from, "last": n.parse::<u64>().unwrap()}),
            None => json!({"from": rest}),
        };
        return Scripted::tools("Reading.", &[(id.as_str(), "channel_read", input)]);
    }
    Scripted::text("Hello.")
}

fn rig() -> Rig {
    let r = rig_scripted(|_| {}, script);
    bind(&r.core, None);
    r
}

/// The places the binding binds: `#den` private (with `den`'s ceiling),
/// `#hall` and `#pier` shared, and the owner's DM.
fn bind(core: &Core, den: Option<theseus_protocol::PlaceCeiling>) {
    let channel = |id: u64, name: &str, private: bool| BoundPlace {
        target: format!("discord:channel:{id}"),
        name: name.into(),
        private,
        ..Default::default()
    };
    core.bind_places(vec![
        BoundPlace {
            ceiling: den,
            ..channel(DEN, "#den", true)
        },
        channel(HALL, "#hall", false),
        channel(PIER, "#pier", false),
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @owner".into(),
            ..Default::default()
        },
    ]);
}

/// The glide posts waiting in `target`'s lane.
fn glides(core: &Core, target: &str) -> Vec<Value> {
    core.outbox
        .open_for(target)
        .iter()
        .map(|a| crate::outbox::body_of(a).clone())
        .filter(|b| b["kind"] == "glide")
        .collect()
}

/// Every glide post waiting anywhere.
fn all_glides(core: &Core) -> usize {
    core.outbox
        .open_targets()
        .iter()
        .map(|t| glides(core, t).len())
        .sum()
}

/// The newest result of `session`'s calls of `tool`: its status, its text,
/// and its outside-text mark.
fn result(core: &Core, sid: &str, tool: &str) -> (ResultStatus, String, Option<String>) {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .rev()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool: t,
                status,
                content,
                external,
                ..
            } if t == tool => Some((status, content, external.map(|e| e.url))),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no result of {tool} in {sid}"))
}

/// The reasons of the questions `sid`'s calls asked.
fn asked(core: &Core, sid: &str) -> Vec<String> {
    rows(core, "tool.confirm_requested")
        .into_iter()
        .filter(|r| r["session_id"] == sid)
        .map(|r| r["reason"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn from_discord(user: u64, channel: Option<u64>) -> Answerer {
    Answerer {
        label: format!("discord:{user}"),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: user.to_string(),
            channel_id: channel.unwrap_or(user + 1).to_string(),
            guild_id: channel.map(|_| "900000000000000001".to_string()),
        }),
    }
}

/// A post into a private place, here the owner's DM from the CLI, runs at
/// its own posture: one glide post in the DM's lane, keyed by its call, and
/// a `glide.posted` row that says it was allowed. Nothing asks, and nothing
/// is recorded as a publish. A read of the DM from the CLI is allowed too,
/// and is not outside text.
#[tokio::test]
async fn a_post_into_a_private_place_runs_and_goes_out_once() {
    let r = rig();
    let cli = session(&r.core, None);
    let res = turn(&r.core, &cli, "POST DM @owner :: the build is green").await;
    assert_eq!(res.awaiting_confirm, None, "{res:?}");
    let (status, text, outside) = result(&r.core, &cli, "channel.post");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.starts_with("Posted 18 characters to DM @owner:"),
        "{text}"
    );
    assert_eq!(outside, None);
    let posted = rows(&r.core, "glide.posted");
    assert_eq!(posted.len(), 1, "{posted:?}");
    let row = &posted[0];
    assert_eq!(row["from"], Value::Null, "the CLI has no place: {row}");
    assert_eq!(row["from_name"], "the CLI or the web UI");
    assert_eq!(row["to"], format!("discord:dm:{OWNER}"));
    assert_eq!(row["to_name"], "DM @owner");
    assert_eq!(
        (row["chars"].as_u64(), &row["allowed"]),
        (Some(18), &json!("allowed"))
    );
    assert_eq!(row["why"], Value::Null);
    let out = glides(&r.core, &format!("discord:dm:{OWNER}"));
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0]["text"], "the build is green");
    assert_eq!(out[0]["call"], row["correlation_id"], "keyed by its call");
    assert_eq!(all_glides(&r.core), 1, "posted once");
    assert!(asked(&r.core, &cli).is_empty(), "nothing asked");
    assert!(rows(&r.core, "place.published").is_empty(), "no publish");

    // A read of the DM from the CLI: private into private.
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, VAULT).await;
    let res = turn(&r.core, &cli, "READ DM @owner").await;
    assert_eq!(res.awaiting_confirm, None, "{res:?}");
    let (status, text, outside) = result(&r.core, &cli, "channel.read");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains(VAULT), "{text}");
    assert_eq!(
        outside, None,
        "a private place's words are not outside text"
    );
    let read = rows(&r.core, "glide.read");
    assert_eq!(read[0]["outside"], false, "{read:?}");
}

/// A post out of a private place into a shared one asks first, and nothing
/// moves while it waits. An answer that does not count (someone else's, or
/// the owner's from a shared place) leaves it waiting; the owner's approval
/// from a private place posts it, once, recorded as `glide.posted`
/// (approved) and as the publish it is (`place.published`, by the approver).
#[tokio::test]
async fn a_post_out_of_a_private_place_asks_first_and_posts_once_when_approved() {
    let r = rig();
    let cli = session(&r.core, None);
    let res = turn(&r.core, &cli, "POST #hall :: the release is out").await;
    let q = res.awaiting_confirm.clone().expect("it asks first");
    let reasons = asked(&r.core, &cli);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(
        reasons[0].contains(
            "channel.post — approve (out of a private place: #hall is shared, so this puts \
             words from the CLI or the web UI where others read them"
        ),
        "{}",
        reasons[0]
    );
    assert_eq!(all_glides(&r.core), 0, "nothing before the answer");
    for who in [from_discord(ALICE, None), from_discord(OWNER, Some(HALL))] {
        assert!(r.core.confirm_action(&q, true, None, who).is_err());
    }
    assert_eq!(
        all_glides(&r.core),
        0,
        "an answer that does not count moves nothing"
    );
    r.core.confirm_action(&q, true, None, "cli").unwrap();
    r.until("the post", 10, |r| {
        !rows(&r.core, "glide.posted").is_empty()
    })
    .await;
    let out = glides(&r.core, &format!("discord:channel:{HALL}"));
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0]["call"], q.as_str());
    assert_eq!(all_glides(&r.core), 1, "posted once");
    let row = &rows(&r.core, "glide.posted")[0];
    assert_eq!(row["allowed"], "approved", "{row}");
    assert!(row["why"]
        .as_str()
        .unwrap()
        .starts_with("out of a private place"));
    let published = rows(&r.core, "place.published");
    assert_eq!(published.len(), 1, "{published:?}");
    let p = &published[0];
    assert_eq!(p["source"]["glide"], q.as_str(), "{p}");
    assert_eq!(
        (&p["who"], &p["via"]),
        (&json!("cli"), &json!("cli")),
        "{p}"
    );
    assert_eq!(p["place"], format!("discord:channel:{HALL}"));
    assert_eq!(p["bytes"], 18);
    let (_, text, _) = result(&r.core, &cli, "channel.post");
    assert!(
        text.starts_with("Posted 18 characters to #hall, with the owner's approval"),
        "{text}"
    );
}

/// A post that asks, declined: nothing is posted, no row says it was, and
/// the model reads the decline.
#[tokio::test]
async fn a_declined_post_posts_nothing() {
    let r = rig();
    let cli = session(&r.core, None);
    let q = turn(&r.core, &cli, "POST #hall :: not yet")
        .await
        .awaiting_confirm
        .expect("it asks first");
    r.core
        .confirm_action(&q, false, Some("not today"), "cli")
        .unwrap();
    r.until("the decline's result", 10, |r| {
        r.core.store.session_nodes(&cli).unwrap().iter().any(
            |(_, n)| matches!(&n.body, Body::ToolResult { tool, .. } if tool == "channel.post"),
        )
    })
    .await;
    let (status, text, _) = result(&r.core, &cli, "channel.post");
    assert_eq!(status, ResultStatus::Declined, "{text}");
    assert!(text.contains("declined"), "{text}");
    assert_eq!(all_glides(&r.core), 0);
    assert!(rows(&r.core, "glide.posted").is_empty());
    assert!(rows(&r.core, "place.published").is_empty());
}

/// From a shared place: a post into another shared place asks first, since
/// their audiences differ; a post into a private place runs; and a post
/// into its own place runs, the same audience. Each runs in a session of
/// its own, the place's newest.
#[tokio::test]
async fn between_two_shared_places_a_post_asks_and_into_a_private_one_it_runs() {
    let r = rig();
    let hall = session(&r.core, Some(&format!("channel:{HALL}")));
    let q = turn(&r.core, &hall, "POST #pier :: hello pier").await;
    assert!(q.awaiting_confirm.is_some(), "{q:?}");
    let reasons = asked(&r.core, &hall);
    assert!(
        reasons[0].contains("(between two shared places: #hall and #pier are different audiences)"),
        "{reasons:?}"
    );
    let hall = session(&r.core, Some(&format!("channel:{HALL}")));
    let res = turn(&r.core, &hall, "POST DM @owner :: from the hall").await;
    assert_eq!(res.awaiting_confirm, None, "{res:?}");
    assert_eq!(glides(&r.core, &format!("discord:dm:{OWNER}")).len(), 1);
    let hall = session(&r.core, Some(&format!("channel:{HALL}")));
    let res = turn(&r.core, &hall, "POST #hall :: to ourselves").await;
    assert_eq!(res.awaiting_confirm, None, "{res:?}");
    assert_eq!(glides(&r.core, &format!("discord:channel:{HALL}")).len(), 1);
    assert!(glides(&r.core, &format!("discord:channel:{PIER}")).is_empty());
}

/// A place this daemon is not bound to fails with words, for a post and a
/// read alike, and nothing moves.
#[tokio::test]
async fn a_place_not_bound_here_fails_with_words() {
    let r = rig();
    let cli = session(&r.core, None);
    turn(&r.core, &cli, "POST #nowhere :: hello").await;
    let (status, text, _) = result(&r.core, &cli, "channel.post");
    assert_eq!(status, ResultStatus::Error);
    assert!(
        text.starts_with("Not run: #nowhere is not a place Theseus is bound to"),
        "{text}"
    );
    turn(&r.core, &cli, "READ #nowhere").await;
    let (_, text, _) = result(&r.core, &cli, "channel.read");
    assert!(
        text.starts_with("Not run: #nowhere is not a place Theseus is bound to"),
        "{text}"
    );
    assert_eq!(all_glides(&r.core), 0);
    assert!(rows(&r.core, "glide.posted").is_empty() && rows(&r.core, "glide.read").is_empty());
    let refused = rows(&r.core, "tool.invalid_input");
    assert!(
        refused
            .iter()
            .all(|r| r["reason"].as_str().unwrap().starts_with("place: ")),
        "{refused:?}"
    );
}

/// A read from a shared place into a private one runs, and what it brings
/// is outside text: its result is marked so (by the place's name), its first
/// line says it, the session takes T1's hold, and its `glide.read` row says
/// `outside`. After it, a post from that session waits (T1's hold), even
/// into a private place.
#[tokio::test]
async fn a_read_from_a_shared_place_is_outside_text_and_holds_the_session() {
    let r = rig();
    let hall = session(&r.core, Some(&format!("channel:{HALL}")));
    turn(&r.core, &hall, "the tide turns at six").await;
    let cli = session(&r.core, None);
    let res = turn(&r.core, &cli, "READ #hall").await;
    assert_eq!(res.awaiting_confirm, None, "into a private place: {res:?}");
    let (status, text, outside) = result(&r.core, &cli, "channel.read");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(outside.as_deref(), Some("#hall"), "marked outside text");
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("[borrowed from #hall: its last 2 messages, oldest first. #hall is a shared place, so this is outside text"),
        "{text}"
    );
    assert!(
        lines[1].ends_with(&format!("{OWNER}: the tide turns at six")),
        "{text}"
    );
    assert!(lines[2].ends_with("Theseus: Hello."), "{text}");
    let held = r
        .core
        .store
        .get_session::<SessionRecord>(&cli)
        .unwrap()
        .unwrap()
        .external
        .expect("the session holds outside text");
    assert_eq!(
        (held.tool.as_str(), held.url.as_str()),
        ("channel.read", "#hall")
    );
    let read = &rows(&r.core, "glide.read")[0];
    assert_eq!(read["outside"], true, "{read}");
    assert_eq!(read["messages"], 2);
    assert_eq!(read["allowed"], "allowed");
    assert_eq!(read["from"], format!("discord:channel:{HALL}"));
    // Its edges (P0's rule 3): the borrowed node copies each message it took,
    // so `node.reach` finds it from the hall's message.
    let said = r
        .core
        .store
        .session_nodes(&hall)
        .unwrap()
        .into_iter()
        .find(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
        .unwrap()
        .1
        .id;
    let reach = crate::reach::reach(&r.core.store, &said, None)
        .unwrap()
        .unwrap();
    assert_eq!(reach.descendants.len(), 1, "{reach:?}");
    assert_eq!(
        reach.descendants[0].node_id,
        read["node_id"].as_str().unwrap()
    );
    // T1's hold: a post now waits, even into the owner's DM.
    let res = turn(&r.core, &cli, "POST DM @owner :: what the hall said").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    let reasons = asked(&r.core, &cli);
    assert!(
        reasons[0].contains("this session read external text (channel.read #hall"),
        "{reasons:?}"
    );
    assert_eq!(all_glides(&r.core), 0);
}

/// A read of a private place from a shared one asks first, and nothing of
/// the private place reaches the shared session's model while it waits.
/// Approved by the owner, it borrows the messages, which are not outside
/// text (the owner's own words), and its row says it was approved.
#[tokio::test]
async fn a_read_of_a_private_place_from_a_shared_one_asks_first() {
    let r = rig();
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, VAULT).await;
    let hall = session(&r.core, Some(&format!("channel:{HALL}")));
    let before = r.requests().len();
    let q = turn(&r.core, &hall, "READ DM @owner")
        .await
        .awaiting_confirm
        .expect("it asks first");
    let reasons = asked(&r.core, &hall);
    assert!(
        reasons[0].contains("(out of a private place: #hall is shared, so this puts words from DM @owner where others read them"),
        "{reasons:?}"
    );
    assert!(
        rows(&r.core, "glide.read").is_empty(),
        "nothing borrowed yet"
    );
    for req in &r.requests()[before..] {
        let seen = serde_json::to_string(&req.messages).unwrap();
        assert!(
            !seen.contains(VAULT),
            "the DM's words reached the hall unasked"
        );
    }
    r.core.confirm_action(&q, true, None, "cli").unwrap();
    r.until("the borrow", 10, |r| {
        !rows(&r.core, "glide.read").is_empty()
    })
    .await;
    let (status, text, outside) = result(&r.core, &hall, "channel.read");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains(VAULT), "{text}");
    assert_eq!(outside, None, "the owner's DM is not outside text");
    let read = &rows(&r.core, "glide.read")[0];
    assert_eq!(
        (&read["allowed"], &read["outside"]),
        (&json!("approved"), &json!(false))
    );
}

/// A post is no looser than its destination's floor (38a): into `#den`,
/// private, whose ceiling floors every call at `approve`, a post from the
/// CLI asks, though the rule allows it.
#[tokio::test]
async fn a_post_takes_its_destinations_floor() {
    let r = rig_scripted(|_| {}, script);
    let floor = theseus_protocol::PlaceCeiling {
        posture_floor: Some("approve".into()),
        ..Default::default()
    };
    bind(&r.core, Some(floor));
    let cli = session(&r.core, None);
    let res = turn(&r.core, &cli, "POST #den :: into the den").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    let reasons = asked(&r.core, &cli);
    assert!(
        reasons[0].contains("(#den's ceiling sets a floor of approve)"),
        "{reasons:?}"
    );
    // Without the floor it runs: the rule allows private into private.
    let r = rig();
    let cli = session(&r.core, None);
    let res = turn(&r.core, &cli, "POST #den :: into the den").await;
    assert_eq!(res.awaiting_confirm, None, "{res:?}");
}
