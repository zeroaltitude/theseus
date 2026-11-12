//! The operator lane has no place of its own, so every kind staged to it names where it goes
//! (theseus-haw2b). Every `notice` the core stages to the operator, both AWS spend alerts among them, was refused for want of a
//! place and were never delivered: the `notice` arm asked the lane for its own channel, and the
//! operator lane has none. These tests drive an operator lane with one action of each kind the core
//! stages to the operator and want each to plan a write. The lane has its own fixture, so the locked
//! DM flow's rig is untouched: the owner's DM is bound through `Shared::add_dm`, as a bind does, and
//! opened through the fake Discord, as the daemon opens it.

use std::time::Instant;

use serde_json::{json, Value};
use theseus_core::outbox::{OPERATOR_KINDS, OPERATOR_TARGET};
use theseus_core::secrets::SecretBoard;
use theseus_sim::fake_discord::FakeDiscord;

use super::*;

/// The owner, and the channel the fake opens their DM at.
const USER: u64 = 271_828_182_845_904_523;
const DM: u64 = USER + 1;
const CHANNEL: u64 = 900_000_000_000_000_042;

/// A lane on `target` of `kind`; the owner's DM is bound only when `dm`.
fn lane_of(
    dir: &std::path::Path,
    fake: &FakeDiscord,
    lane: (&str, &'static str, Option<u64>),
    dm: bool,
) -> Lane {
    lane_silent(dir, fake, lane, dm, vec![])
}

/// As `lane_of`, with `[discord] silent` naming `silent`: the operator lane has no place settings
/// of its own, so the global list is the only one that reaches it.
fn lane_silent(
    dir: &std::path::Path,
    fake: &FakeDiscord,
    (target, kind, channel): (&str, &'static str, Option<u64>),
    dm: bool,
    silent: Vec<theseus_core::config::discord::Category>,
) -> Lane {
    let core = crate::runtime::tests::core_with(dir, SecretBoard::new([], Instant::now()), |c| {
        c.discord.rest_proxy = Some(fake.addr.clone());
        c.discord.silent = silent;
        c.places.owner = Some(vec![format!("discord:{USER}")]);
    });
    let shared = crate::runtime::shared_for_tests(&core);
    if dm {
        shared.add_dm(USER, "the owner".into());
    }
    Lane::new(shared, target.into(), kind, "a lane".into(), channel, None)
}

fn operator(dir: &std::path::Path, fake: &FakeDiscord, dm: bool) -> Lane {
    lane_of(dir, fake, (OPERATOR_TARGET, "operator", None), dm)
}

/// The smallest body of `kind` the courier can draw a post from.
fn body_of(kind: &str) -> Value {
    let mut b = json!({"kind": kind, "text": format!("A {kind}.")});
    match kind {
        "restarted" => b = json!({"kind": kind, "at_unix_ms": 1, "tables": []}),
        "disk" => {
            b = json!({"kind": kind, "state": "below_floor", "free_mb": 1, "total_mb": 9,
                       "floor_mb": 2, "warn_mb": 3})
        }
        "mcp_changed" => b["server"] = json!("harbor"),
        "mcp_prompt_changed" => {
            b["server"] = json!("harbor");
            b["prompt"] = json!("tide");
        }
        // A header names its questions: no card to look up.
        "needs_you" => {
            b["questions"] = json!(["act_one"]);
            b["header"] = json!(true);
        }
        _ => {}
    }
    b
}

/// What `kind` planned on an operator lane, for the guard below.
async fn planned(lane: &mut Lane, kind: &str) -> Result<Plan, SendErr> {
    let a = lane
        .shared
        .core
        .outbox
        .to_operator(None, body_of(kind))
        .unwrap();
    lane.plan(&a).await
}

/// Every kind the core stages to the operator plans a write on the operator lane, where a refused
/// kind is one the owner never hears (planted: the `notice` arm on `place_channel()` alone, and
/// `notice` is refused, "discord:operator is no place with a channel"). `jev_labeled` edits an earlier post in
/// place and has none to edit here, so it plans nothing and says why: not a refusal.
#[tokio::test]
async fn every_operator_kind_plans_a_write_on_the_operator_lane() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = operator(d.path(), &fake, true);
    // The vacuity guard: an empty list would pass this test and prove nothing.
    assert!(!OPERATOR_KINDS.is_empty(), "no operator kinds are listed");
    let mut seen = 0;
    let mut refused = vec![];
    for kind in OPERATOR_KINDS {
        match planned(&mut lane, kind).await {
            Ok(plan) if plan.writes.is_empty() && *kind == "jev_labeled" => {}
            Ok(plan) if plan.writes.is_empty() => refused.push(format!("{kind}: no write")),
            Ok(plan) => assert!(
                plan.writes.iter().all(|w| w.channel == DM),
                "{kind} left the owner's DM: {:?}",
                plan.writes.iter().map(|w| w.channel).collect::<Vec<_>>()
            ),
            Err(e) => refused.push(format!("{kind}: {}", e.message)),
        }
        seen += 1;
    }
    assert_eq!(seen, OPERATOR_KINDS.len(), "every listed kind was driven");
    assert!(refused.is_empty(), "planned no write: {refused:#?}");
}

/// The kinds are listed once each: a kind twice would be driven twice and counted twice.
#[test]
fn the_operator_kinds_are_distinct() {
    let mut kinds = OPERATOR_KINDS.to_vec();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), OPERATOR_KINDS.len(), "a kind is listed twice");
}

/// A notice on the operator lane lands in the owner's DM and says which (as `restarted`'s does),
/// pings when it is not quiet, and stays quiet when its sender says so: the delivery fix does not
/// re-ping the three notices that are quiet on purpose.
#[tokio::test]
async fn an_operator_notice_lands_in_the_owners_dm_and_keeps_its_ping_rule() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = operator(d.path(), &fake, true);
    let loud = planned(&mut lane, "notice").await.unwrap();
    assert_eq!(loud.writes.len(), 1);
    assert_eq!(loud.writes[0].channel, DM);
    assert!(loud.writes[0].ping, "a plain notice pings");
    assert_eq!(
        loud.extra["place"], "the owner",
        "the DM it went to, as the siblings say it"
    );

    let mut quiet = body_of("notice");
    quiet["quiet"] = json!(true);
    let a = lane.shared.core.outbox.to_operator(None, quiet).unwrap();
    let plan = lane.plan(&a).await.unwrap();
    assert_eq!(plan.writes[0].channel, DM);
    assert!(!plan.writes[0].ping, "a quiet notice stays quiet");
}

/// With no DM taking approvals and no place to fall back to, the notice is refused with its reason
/// (not dropped silently, and not posted anywhere shared).
#[tokio::test]
async fn an_operator_notice_with_no_dm_is_refused_with_its_reason() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = operator(d.path(), &fake, false);
    let e = planned(&mut lane, "notice").await.err().expect("refused");
    assert!(!e.away, "a refusal, not an outage: {}", e.message);
    assert!(e.message.contains("no DM takes approvals"), "{}", e.message);
}

/// A channel's own lane keeps its own place for a notice, and carries no `place` (it did not
/// before): the operator rule is the operator lane's alone.
#[tokio::test]
async fn a_channel_lane_still_posts_its_notice_in_its_own_channel() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let target = format!("discord:channel:{CHANNEL}");
    let mut lane = lane_of(d.path(), &fake, (&target, "channel", Some(CHANNEL)), true);
    let a = lane
        .shared
        .core
        .outbox
        .post("sess_harbor", "", &target, body_of("notice"))
        .unwrap();
    let plan = lane.plan(&a).await.unwrap();
    assert_eq!(plan.writes[0].channel, CHANNEL);
    assert_eq!(plan.extra, json!({}));
}

/// The two AWS spend-safety alerts (a runaway, an hour's spend) cannot be silenced: with
/// `silent = ["notices"]` an ordinary notice posts without a ping, and a `hands_alert` still pings,
/// in the owner's DM, even when its body says `quiet` (theseus-haw2b). The control is the notice:
/// a suite that passed with the control loud would pass a mutation that ignores `silent` everywhere.
#[tokio::test]
async fn a_hands_alert_pings_where_notices_are_silent() {
    use theseus_core::config::discord::Category;
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane_silent(
        d.path(),
        &fake,
        (OPERATOR_TARGET, "operator", None),
        true,
        vec![Category::Notices],
    );
    let control = planned(&mut lane, "notice").await.unwrap();
    assert!(!control.writes[0].ping, "a notice is silent here");

    let alert = planned(&mut lane, "hands_alert").await.unwrap();
    assert_eq!(alert.writes.len(), 1);
    assert_eq!(alert.writes[0].channel, DM);
    assert!(
        alert.writes[0].ping,
        "an AWS alert pings where notices are silent"
    );
    assert_eq!(
        alert.writes[0].content, "A hands_alert.",
        "worded as a notice is"
    );
    assert_eq!(alert.extra["place"], "the owner");

    let mut quiet = body_of("hands_alert");
    quiet["quiet"] = json!(true);
    let a = lane.shared.core.outbox.to_operator(None, quiet).unwrap();
    let plan = lane.plan(&a).await.unwrap();
    assert!(plan.writes[0].ping, "no `quiet` mutes an AWS alert");
}

/// A self build's body as `told_where_it_reports` stages it (`rsi/pr.rs:656`):
/// the kind, the item, the verdict, and the one line of text.
fn self_build_body(kind: &str) -> Value {
    json!({
        "kind": kind,
        "item": "trim-the-sails",
        "verdict": if kind == "self_build" { "green" } else { "red" },
        "text": "Trim the sails ended red in suite (exit 101, 467 s).",
    })
}

/// A self build's post as the courier plans it, on **the item's own thread**:
/// the `_ =>` branch of `self_build_post` (theseus-1eemp). The core's four
/// `mod told` tests assert where the post is *staged* and **never reach the
/// courier at all**, so until this one nothing asserted the courier's own half
/// - the thread branch, the `self_build:{corr}` key, the `extra` carrying
/// `item`, or the `TaskEnded`/`TaskFailed` mapping. The equivalence between
/// the two branches is a source reading, not a test; and the branch that
/// reading covers is the one **dead on the owner's live daemon** - a self
/// item's task hangs off `self:backlog` and no place syncs it - so it is the
/// half that would stay silent if it broke.
///
/// `silent = ["reports"]` is what makes the event mapping observable at all: a
/// `self_build` is `Event::TaskEnded`, whose category is `Reports`, and a
/// `self_build_red` is `Event::TaskFailed`, whose category is `Failures`. With
/// the default empty list both would ping and swapping the two arms would
/// change nothing. The quiet one being the green one is also the honest
/// behaviour: a report does not interrupt, a failure does.
#[tokio::test]
async fn a_self_build_post_on_a_threads_own_lane_carries_the_key_the_item_and_the_event() {
    use theseus_core::config::discord::Category;
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let target = format!("discord:channel:{CHANNEL}");
    let mut lane = lane_silent(
        d.path(),
        &fake,
        (&target, "channel", Some(CHANNEL)),
        true,
        vec![Category::Reports],
    );
    let post = |lane: &Lane, kind: &str| {
        lane.shared
            .core
            .outbox
            .post("sess_self", "exec_self", &target, self_build_body(kind))
            .unwrap()
    };

    let green = post(&lane, "self_build");
    let plan = lane.plan(&green).await.unwrap();
    assert_eq!(plan.writes.len(), 1, "one line where the item reports");
    let w = &plan.writes[0];
    assert_eq!(w.channel, CHANNEL, "the item's own thread, not a DM");
    assert_eq!(
        w.key,
        format!("self_build:{}", green.correlation_id),
        "one post per build: a retry edits it rather than making a second"
    );
    assert_eq!(
        w.content, "Trim the sails ended red in suite (exit 101, 467 s).",
        "the core's own words, not the courier's"
    );
    assert_eq!(
        plan.extra["item"], "trim-the-sails",
        "the item the post is about"
    );
    assert!(
        plan.extra.get("place").is_none(),
        "a thread's lane names no place: {:?}",
        plan.extra
    );
    assert!(
        !w.ping,
        "a green build is TaskEnded, and reports are silent here"
    );

    let red = post(&lane, "self_build_red");
    let plan = lane.plan(&red).await.unwrap();
    let w = &plan.writes[0];
    assert_eq!(
        w.key,
        format!("self_build:{}", red.correlation_id),
        "both kinds post under the one key prefix"
    );
    assert_ne!(red.correlation_id, green.correlation_id, "two builds");
    assert_eq!(plan.extra["item"], "trim-the-sails");
    assert!(
        w.ping,
        "a red build is TaskFailed, and failures are not silenced here: \
         swapping the two arms reds this and the line above together"
    );
}

/// The operator's DM: the branch the live daemon actually takes, and the one
/// whose `extra` must carry **both** halves - the `place` the operator lane
/// names because it has none of its own, and the `item` the post is about.
/// Nothing asserted that the `item` mutation leaves `place` standing.
#[tokio::test]
async fn a_self_build_post_to_the_operator_names_its_place_and_its_item() {
    use theseus_core::config::discord::Category;
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane_silent(
        d.path(),
        &fake,
        (OPERATOR_TARGET, "operator", None),
        true,
        vec![Category::Reports],
    );
    let a = lane
        .shared
        .core
        .outbox
        .to_operator(Some("sess_self"), self_build_body("self_build_red"))
        .unwrap();
    let plan = lane.plan(&a).await.unwrap();
    assert_eq!(plan.writes.len(), 1);
    let w = &plan.writes[0];
    assert_eq!(w.channel, DM, "where approvals go");
    assert_eq!(w.key, format!("self_build:{}", a.correlation_id));
    assert_eq!(
        w.content,
        "Trim the sails ended red in suite (exit 101, 467 s)."
    );
    assert_eq!(
        plan.extra["place"], "the owner",
        "the operator lane names the DM it went to, as its siblings do"
    );
    assert_eq!(
        plan.extra["item"], "trim-the-sails",
        "and the item, beside it: neither overwrites the other"
    );
    assert!(w.ping, "a red build pings where reports are silent");
}
