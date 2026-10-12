//! Earned autonomy through whole cores (theseus-uttu): the owner approves
//! the same command again and again, a card is posted once, a yes from a
//! private place makes a grant that runs the prefix with a notice and no
//! other, a restart keeps it, and a revoke or a "should have asked" on its
//! notice ends it. A yes from a shared place does not count.

use serde_json::json;
use theseus_protocol::{error_code, method};

use super::*;
use crate::approval::Surface::{Cli, Discord, Mcp};
use crate::policy::earned::Earned;

/// A week, in ms.
const WEEK: u64 = 7 * 86_400_000;

fn earned_rig(script: Vec<Scripted>) -> Rig {
    let r = rig_with(script, |cfg| cfg.policy.earned.min_approvals = 3);
    bind_places(&r, false);
    r
}

/// `n` approved runs of `echo hello` in one session.
async fn approved_runs(r: &Rig, sid: &str, n: usize) {
    for i in 0..n {
        let res = turn(&r.core, Some(sid), &format!("run {i}")).await;
        let corr = res.awaiting_confirm.clone().expect("approve asks");
        r.core.confirm_action(&corr, true, None, "test").unwrap();
        let exec = res.execution_id.clone().unwrap();
        r.core.continue_execution(&exec).await.unwrap();
    }
}

fn runs(n: usize) -> Vec<Scripted> {
    runs_of("hello", n)
}

/// `n` scripted runs of `echo <word>`, each answered once it has run.
fn runs_of(word: &str, n: usize) -> Vec<Scripted> {
    (0..n)
        .flat_map(|i| [echo(&format!("{word}{i}"), word), Scripted::text("Ran.")])
        .collect()
}

fn id_of(r: &Rig) -> String {
    let card = r.core.tools.earned.snapshot().card.expect("a card waits");
    assert_eq!(card.candidates.len(), 1);
    card.candidates[0].id.clone()
}

async fn owner_accepts(
    r: &Rig,
    id: &str,
    accept: bool,
) -> Result<Value, theseus_protocol::RpcError> {
    rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::POLICY_EARNED_ANSWER,
        json!({"id": id, "accept": accept}),
    )
    .await
}

/// A rig with three approved runs of `echo hello` behind it, its session, and
/// `extra` calls scripted after them.
async fn after_three_approvals(extra: Vec<Scripted>) -> (Rig, String) {
    let mut script = runs(3);
    script.extend(extra);
    let r = earned_rig(script);
    let sid = new_session(&r);
    approved_runs(&r, &sid, 3).await;
    (r, sid)
}

#[tokio::test]
async fn the_finder_reads_the_record_and_one_card_is_posted_a_week() {
    let (r, _) = after_three_approvals(vec![]).await;
    // The finder reads the record: `echo hello`, 3 approvals.
    let now = theseus_protocol::now_unix_ms();
    let found = r
        .core
        .earned_candidates(now)
        .unwrap()
        .expect("the index is built");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].label, "echo hello");
    assert_eq!(found[0].record.approvals, 3);

    // The first card is due, and a DM takes it: one post, one row.
    let card = r
        .core
        .earned_propose(false)
        .unwrap()
        .expect("the first card is due");
    assert_eq!(card.candidates.len(), 1);
    assert_eq!(ledgered(&r, "policy.earned_proposed").len(), 1);
    let posts: Vec<_> = r
        .core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "earned_card")
        .collect();
    assert_eq!(posts.len(), 1, "the card is a post to the owner's DM");
    let body = crate::outbox::body_of(&posts[0]);
    assert!(
        body["text"].as_str().unwrap().contains("echo hello"),
        "{body}"
    );
    assert!(
        r.core.earned_next_card_ms() > now,
        "the next card is a week away"
    );
    assert!(
        r.core.earned_propose(false).unwrap().is_none(),
        "at most one a week"
    );
    assert!(
        r.core.earned_propose(true).unwrap().is_none(),
        "even forced, an open candidate is not proposed twice"
    );
    assert_eq!(ledgered(&r, "policy.earned_proposed").len(), 1);
}

#[tokio::test]
async fn a_second_card_waits_for_the_week_even_with_a_new_candidate() {
    let mut script = runs_of("hello", 3);
    script.extend(runs_of("world", 3));
    let r = earned_rig(script);
    let sid = new_session(&r);
    approved_runs(&r, &sid, 3).await;
    let card = r
        .core
        .earned_propose(false)
        .unwrap()
        .expect("the first card");
    assert_eq!(card.candidates[0].label, "echo hello");
    owner_accepts(&r, &id_of(&r), false).await.unwrap();

    // A new group earns its candidacy, and the week's limit still holds.
    approved_runs(&r, &sid, 3).await;
    let now = theseus_protocol::now_unix_ms();
    let found = r.core.earned_candidates(now).unwrap().unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].label, "echo world");
    assert!(
        r.core.earned_propose(false).unwrap().is_none(),
        "one card a week: a new candidate waits"
    );
    assert_eq!(ledgered(&r, "policy.earned_proposed").len(), 1);

    // The week passes: the next card carries it.
    let row = crate::ledger::LedgerRow::new(
        theseus_protocol::LedgerKind::PolicyEarnedPassed,
        None,
        None,
        json!({}),
    );
    r.core
        .tools
        .earned
        .update(&r.core.store, &row, vec![], |s| {
            s.last_card_ms -= WEEK + 1;
            true
        })
        .unwrap();
    let card = r
        .core
        .earned_propose(false)
        .unwrap()
        .expect("the week has passed");
    assert_eq!(card.candidates[0].label, "echo world");
    assert_eq!(ledgered(&r, "policy.earned_proposed").len(), 2);
}

#[tokio::test]
async fn a_yes_from_a_shared_place_does_not_count_and_one_from_the_cli_grants() {
    let (r, _) = after_three_approvals(vec![]).await;
    let card = r.core.earned_propose(true).unwrap().expect("forced");
    let id = id_of(&r);
    let e = rpc_as(
        &r.core,
        surface("discord#1", Discord),
        method::POLICY_EARNED_ANSWER,
        json!({"id": id, "accept": true, "author": "discord:zeroaltitude",
               "discord": {"user_id": OWNER, "channel_id": CHANNEL, "guild_id": "g1"}}),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(r.core.tools.earned.grants().is_empty());
    assert_eq!(ledgered(&r, "approval.refused").len(), 1);

    // From the CLI: the grant, ledgered with the record's digest.
    let a = owner_accepts(&r, &id, true).await.unwrap();
    assert_eq!(
        (a["accepted"].as_bool(), a["already"].as_bool()),
        (Some(true), Some(false))
    );
    let granted = ledgered(&r, "policy.earned_granted");
    assert_eq!(granted.len(), 1);
    assert_eq!(
        granted[0]["digest"],
        card.candidates[0].record.digest.as_str()
    );
    assert_eq!(granted[0]["approvals"], 3);
    assert_eq!(r.core.tools.earned.grants().len(), 1);
}

#[tokio::test]
async fn a_granted_prefix_runs_with_a_notice_and_a_restart_keeps_the_grant() {
    let (r, sid) = after_three_approvals(vec![
        echo("g1", "hello"),
        Scripted::text("Ran."),
        echo("g2", "goodbye"),
    ])
    .await;
    r.core.earned_propose(true).unwrap().expect("forced");
    let id = id_of(&r);
    owner_accepts(&r, &id, true).await.unwrap();

    // It runs with one notice, no question; a command off the prefix asks.
    let res = turn(&r.core, Some(&sid), "run it").await;
    assert!(res.awaiting_confirm.is_none(), "the grant runs it: {res:?}");
    let notices = ledgered(&r, "tool.notified");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0]["setting"], format!("earned grant {id}").as_str());
    assert!(
        notices[0]["rule"]
            .as_str()
            .unwrap()
            .contains("notify (earned grant"),
        "{}",
        notices[0]
    );
    let res = turn(&r.core, Some(&sid), "something else").await;
    assert!(
        res.awaiting_confirm.is_some(),
        "a non-matching argv still asks"
    );
    assert_eq!(ledgered(&r, "tool.notified").len(), 1);

    // A restart keeps it: the state reads back from the store.
    let again = Earned::default();
    again.load(&r.core.store).unwrap();
    assert_eq!(again.grants(), r.core.tools.earned.grants());
    assert_eq!(again.grants().len(), 1);
    let listed = r
        .core
        .earned_read(&Default::default(), &"test".into())
        .unwrap();
    assert_eq!(listed.grants.len(), 1);
    assert!(
        listed.saved >= 3,
        "the grant would have saved the three asks: {}",
        listed.saved
    );
}

/// A grant as a card's yes leaves it, written by hand: `prefix` of `proc.run`.
fn by_hand(r: &Rig, id: &str, prefix: &[&str]) {
    let g = theseus_protocol::earned::EarnedGrant {
        group: theseus_protocol::earned::EarnedGroup {
            id: id.into(),
            tool: "proc.run".into(),
            prefix: prefix.iter().map(|s| s.to_string()).collect(),
            label: prefix.join(" "),
            open_ended: true,
            ..Default::default()
        },
        by: "the CLI".into(),
        ..Default::default()
    };
    let row = crate::ledger::LedgerRow::new(
        theseus_protocol::LedgerKind::PolicyEarnedGranted,
        None,
        None,
        json!({}),
    );
    r.core
        .tools
        .earned
        .update(&r.core.store, &row, vec![], move |s| {
            s.grants.push(g);
            true
        })
        .unwrap();
}

fn new_session(r: &Rig) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

#[tokio::test]
async fn not_this_one_is_remembered_and_a_revoke_asks_again() {
    let mut script = runs(3);
    script.extend([
        echo("g1", "hello"),
        Scripted::text("Ran."),
        echo("g2", "hello"),
    ]);
    let r = earned_rig(script);
    let sid = new_session(&r);
    approved_runs(&r, &sid, 3).await;
    r.core.earned_propose(true).unwrap().unwrap();
    let id = id_of(&r);

    // "Not this one": remembered, and the card is spent.
    let a = owner_accepts(&r, &id, false).await.unwrap();
    assert_eq!(a["accepted"], false);
    assert_eq!(ledgered(&r, "policy.earned_passed").len(), 1);
    assert!(r.core.tools.earned.grants().is_empty());
    let now = theseus_protocol::now_unix_ms();
    assert!(r.core.earned_candidates(now).unwrap().unwrap().is_empty());
    assert!(
        r.core.earned_propose(true).unwrap().is_none(),
        "never re-proposed for 90 days"
    );
    let e = owner_accepts(&r, &id, true).await.unwrap_err();
    assert!(e.message.contains("is on no open card"), "{e:?}");

    // A grant by hand, then a revoke: it asks again.
    by_hand(&r, "ern_00000001", &["echo", "hello"]);
    let res = turn(&r.core, Some(&sid), "run it").await;
    assert!(res.awaiting_confirm.is_none(), "granted");
    let rv = rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::POLICY_REVOKE,
        json!({"id": "ern_00000001"}),
    )
    .await
    .unwrap();
    assert_eq!(rv["id"], "ern_00000001");
    assert_eq!(ledgered(&r, "policy.earned_revoked").len(), 1);
    let res = turn(&r.core, Some(&sid), "run it again").await;
    assert!(res.awaiting_confirm.is_some(), "revoked: it asks again");
    assert!(
        r.core.tools.earned.skips("ern_00000001", now, WEEK),
        "and is not proposed for 90 days"
    );
    assert!(!r
        .core
        .tools
        .earned
        .skips("ern_00000001", now + 91 * 86_400_000, WEEK));
}

#[tokio::test]
async fn should_have_asked_on_a_grants_notice_ends_it_and_the_tool_keeps_its_posture() {
    let r = earned_rig(vec![echo("g3", "hello")]);
    let sid = new_session(&r);
    by_hand(&r, "ern_00000001", &["echo", "hello"]);
    let res = turn(&r.core, Some(&sid), "run it").await;
    assert!(res.awaiting_confirm.is_none());
    let notice = ledgered(&r, "tool.notified").pop().unwrap();
    let corr = notice["correlation_id"].as_str().unwrap();
    let t = r.core.tighten("proc.run", Some(corr), "test").unwrap();
    assert!(
        t.setting.contains("earned grant ern_00000001 ended"),
        "{}",
        t.setting
    );
    assert!(r.core.tools.earned.grants().is_empty());
    assert!(
        r.core.health().tightenings.is_empty(),
        "the tool is not tightened"
    );
    let rows = ledgered(&r, "policy.earned_revoked");
    assert_eq!(rows.last().unwrap()["should_have_asked"], true);
    assert!(
        r.core
            .tools
            .earned
            .skips("ern_00000001", u64::MAX - 1, WEEK),
        "it counts as a decline for good"
    );
}

#[tokio::test]
async fn a_grant_never_passes_a_shell_the_floor_or_a_shared_place() {
    let sh = Scripted::tools(
        "",
        &[("s1", "proc_run", json!({"argv": ["bash", "-c", "echo hi"]}))],
    );
    let floor = Scripted::tools(
        "",
        &[("f1", "proc_run", json!({"argv": ["theseusd", "status"]}))],
    );
    let shared_run = echo("p1", "hello");
    let r = earned_rig(vec![sh, floor, shared_run, Scripted::text("done")]);
    let row = crate::ledger::LedgerRow::new(
        theseus_protocol::LedgerKind::PolicyEarnedGranted,
        None,
        None,
        json!({}),
    );
    // Hand-edited grants: a shell's prefix, the floor's program, and a fine one.
    for (id, prefix) in [
        ("ern_0000000a", ["bash", "-c"]),
        ("ern_0000000b", ["theseusd", "status"]),
        ("ern_0000000c", ["echo", "hello"]),
    ] {
        let g = theseus_protocol::earned::EarnedGrant {
            group: theseus_protocol::earned::EarnedGroup {
                id: id.into(),
                tool: "proc.run".into(),
                prefix: prefix.iter().map(|s| s.to_string()).collect(),
                label: prefix.join(" "),
                open_ended: true,
                ..Default::default()
            },
            ..Default::default()
        };
        r.core
            .tools
            .earned
            .update(&r.core.store, &row, vec![], move |s| {
                s.grants.push(g);
                true
            })
            .unwrap();
    }
    let res = turn(&r.core, None, "bash please").await;
    assert!(
        res.awaiting_confirm.is_some(),
        "`bash -c` asks, whatever a stored grant says"
    );
    let res = turn(&r.core, None, "floor").await;
    assert!(
        res.awaiting_confirm.is_some(),
        "the floor asks at every posture"
    );
    assert!(ledgered(&r, "tool.notified").is_empty());

    // A shared place: no grant acts there (the place rule refuses the call).
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.core
        .outbox
        .bind_place(&format!("channel:{CHANNEL}"), &rec.session_id)
        .unwrap();
    let res = turn(&r.core, Some(&rec.session_id), "echo hello").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(
        ledgered(&r, "tool.notified").is_empty(),
        "no grant acted in a shared place"
    );
}

/// `earned::gate` on a plan, in a rig whose approve list names `echo listed`
/// and with grants on three prefixes: the decision before it and after it.
fn gated(
    r: &Rig,
    input: serde_json::Value,
    class: theseus_protocol::PlaceClass,
) -> (crate::policy::Decision, crate::policy::Decision) {
    let rt = &r.core.tools;
    let tool = rt.registry.get("proc.run").unwrap().clone();
    let plan = tool.plan(&input, &rt.ctx).unwrap();
    let (d, bound) = crate::sandbox::unbrokered(rt, &*tool, &plan, &input, None);
    let after =
        crate::policy::earned::gate(rt, class, &*tool, &plan, &input, bound.l1(), d.clone());
    (d, after)
}

fn gate_rig() -> Rig {
    let r = rig_with(vec![], |cfg| {
        cfg.policy.approve_argv = vec![vec!["echo".into(), "listed".into()]];
    });
    by_hand(&r, "ern_00000001", &["echo", "hello"]);
    by_hand(&r, "ern_00000002", &["echo", "listed"]);
    by_hand(&r, "ern_00000003", &["theseusd", "status"]);
    r
}

/// A grant turns an approve into a notify for a posture's ask in a private
/// place, and not for a shared place, an `env`, a URL after the prefix, a
/// path outside the roots, or a command off the prefix.
#[test]
fn the_gate_applies_a_grant_in_a_private_place_to_a_covered_call_alone() {
    use theseus_protocol::PlaceClass::{Private, Shared};
    let r = gate_rig();
    let hello = json!({"argv": ["echo", "hello", "x"], "cwd": r.root});
    let (asked, after) = gated(&r, hello.clone(), Private);
    assert_eq!(asked.posture, Posture::Approve);
    assert_eq!(
        after.posture,
        Posture::Notify,
        "a grant: a notice, never open"
    );
    assert_eq!(
        after.notify.as_ref().unwrap().setting,
        "earned grant ern_00000001"
    );
    assert!(
        after
            .reason
            .contains("`theseus policy revoke ern_00000001` ends it"),
        "{}",
        after.reason
    );
    assert_eq!(
        gated(&r, hello, Shared).1.posture,
        Posture::Approve,
        "shared place"
    );
    for (why, input) in [
        (
            "an env",
            json!({"argv": ["echo", "hello", "x"], "cwd": r.root, "env": {"A": "b"}}),
        ),
        (
            "a URL",
            json!({"argv": ["echo", "hello", "https://example.org"], "cwd": r.root}),
        ),
        (
            "off the prefix",
            json!({"argv": ["echo", "bye"], "cwd": r.root}),
        ),
        (
            "outside the roots",
            json!({"argv": ["echo", "hello", "/etc/hosts"], "cwd": r.root}),
        ),
    ] {
        assert_eq!(
            gated(&r, input, Private).1.posture,
            Posture::Approve,
            "{why}"
        );
    }
}

/// A grant never loosens an L1 job's ask: hosts beyond `[sandbox] egress`
/// that the call names still wait for a yes, as L1's own posture does.
#[test]
fn the_gate_leaves_an_l1_jobs_ask_alone() {
    use theseus_protocol::PlaceClass::Private;
    let r = gate_rig();
    let beyond = json!({
        "argv": ["echo", "hello", "x"],
        "cwd": r.root,
        "sandbox": {"egress": ["exfil.example:443"]}
    });
    let (asked, after) = gated(&r, beyond, Private);
    assert_eq!(asked.posture, Posture::Approve, "{}", asked.reason);
    assert!(asked.reason.contains("egress"), "{}", asked.reason);
    assert_eq!(after.posture, Posture::Approve, "{}", after.reason);
    assert!(after.notify.is_none());
    let hello = json!({"argv": ["echo", "hello", "x"], "cwd": r.root});
    assert_eq!(gated(&r, hello, Private).1.posture, Posture::Notify);
}

/// A grant never passes what asks for another reason: the approve list, the
/// floor, or a tool someone tightened.
#[test]
fn the_gate_never_lets_a_grant_pass_the_lists_the_floor_or_a_tightening() {
    use theseus_protocol::PlaceClass::Private;
    let r = gate_rig();
    let listed = json!({"argv": ["echo", "listed"], "cwd": r.root});
    let (asked, after) = gated(&r, listed, Private);
    assert!(asked.reason.contains("approve list"), "{}", asked.reason);
    assert_eq!(after.posture, Posture::Approve);
    let floor = json!({"argv": ["theseusd", "status"], "cwd": r.root});
    let (asked, after) = gated(&r, floor, Private);
    assert!(asked.floor);
    assert!(after.floor && after.posture == Posture::Approve);
    let hello = json!({"argv": ["echo", "hello", "x"], "cwd": r.root});
    assert_eq!(gated(&r, hello.clone(), Private).1.posture, Posture::Notify);
    r.core.tighten("proc.run", None, "test").unwrap();
    assert_eq!(
        gated(&r, hello, Private).1.posture,
        Posture::Approve,
        "tightened"
    );
}

/// An MCP connection is no approval surface: it can neither answer a card
/// nor revoke a grant, and each refusal is ledgered with `via: mcp`.
#[tokio::test]
async fn an_mcp_connection_cannot_answer_a_card_or_revoke_a_grant() {
    let r = earned_rig(vec![]);
    by_hand(&r, "ern_00000001", &["echo", "hello"]);
    let calls = [
        (
            method::POLICY_EARNED_ANSWER,
            json!({"id": "ern_00000001", "accept": true}),
        ),
        (method::POLICY_REVOKE, json!({"id": "ern_00000001"})),
    ];
    for (m, p) in calls {
        let e = rpc_as(&r.core, surface("mcp", Mcp), m, p)
            .await
            .unwrap_err();
        assert_eq!(e.code, error_code::REFUSED, "{m}: {e:?}");
        assert_eq!(e.data["via"], "mcp", "{e:?}");
    }
    assert_eq!(r.core.tools.earned.grants().len(), 1);
    assert_eq!(ledgered(&r, "approval.refused").len(), 2);
}
