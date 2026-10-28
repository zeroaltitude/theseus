//! Scoped grants through whole cores (theseus-drjs): a question the tool's
//! posture asked offers "yes, and don't ask again" for the command's prefix
//! (or edits in the project), the answer that takes it echoes the offer, and
//! from then on the same prefix runs with a notice naming the grant, in that
//! conversation alone. A wider pattern, a shared place, the floor, an L1
//! job's ask, a shell around the command and a path out of the project are
//! each refused; a revoke, a retire and a restart do what they say.

use serde_json::json;
use theseus_protocol::grants::{GrantAnswer, GrantOffer, KIND_EDITS, KIND_PREFIX};
use theseus_protocol::{error_code, method, PlaceClass};

use super::*;
use crate::approval::Surface::{Cli, Mcp};
use crate::policy::grants::Grants;

fn run_of(id: &str, argv: &[&str]) -> Scripted {
    Scripted::tools("", &[(id, "proc_run", json!({"argv": argv}))])
}

/// The asked question's offer, as the push carried it and as
/// `confirm.list` reads it again from the record: the two agree.
fn offer_of(r: &Rig, sid: &str, corr: &str) -> Option<GrantOffer> {
    let asked = ledgered(r, "tool.confirm_requested")
        .into_iter()
        .rfind(|q| q["correlation_id"] == corr)
        .expect("the question's row");
    let pushed: Option<GrantOffer> = serde_json::from_value(asked["grant_offer"].clone())
        .ok()
        .flatten();
    let listed = r
        .core
        .pending_confirms(sid)
        .unwrap()
        .into_iter()
        .find(|q| q.correlation_id == corr)
        .expect("still pending")
        .grant_offer;
    assert_eq!(pushed, listed, "the push and the list say the same offer");
    pushed
}

fn grant(
    r: &Rig,
    corr: &str,
    pattern: &str,
) -> anyhow::Result<theseus_protocol::ActionConfirmResult> {
    r.core.confirm_granting(
        corr,
        None,
        "test".into(),
        false,
        &GrantAnswer {
            pattern: pattern.into(),
        },
    )
}

/// A session asks for `echo hello x`, the owner takes the offer, and the
/// turn runs it: the rig, the session and the grant's id.
async fn granted(extra: Vec<Scripted>) -> (Rig, String, String) {
    let mut script = vec![
        run_of("g0", &["echo", "hello", "x"]),
        Scripted::text("Ran."),
    ];
    script.extend(extra);
    let r = rig(script);
    let sid = new_session(&r);
    let res = turn(&r.core, Some(&sid), "run it").await;
    let corr = res.awaiting_confirm.clone().expect("the posture asks");
    let offer = offer_of(&r, &sid, &corr).expect("a posture's ask offers a grant");
    assert_eq!(
        (
            offer.scope.as_str(),
            offer.kind.as_str(),
            offer.pattern.as_str()
        ),
        ("session", KIND_PREFIX, "echo hello")
    );
    let done = grant(&r, &corr, "echo hello").unwrap();
    assert!(done.approved);
    let g = done.granted.expect("the grant");
    r.core
        .continue_execution(&res.execution_id.unwrap())
        .await
        .unwrap();
    (r, sid, g.id)
}

fn new_session(r: &Rig) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

#[tokio::test]
async fn a_granted_prefix_runs_with_a_notice_in_its_own_conversation_alone() {
    let (r, sid, id) = granted(vec![
        run_of("g1", &["echo", "hello", "again"]),
        Scripted::text("Ran."),
        // It parks on its question, so the next answer is the other
        // session's.
        run_of("g2", &["echo", "bye"]),
        run_of("g3", &["echo", "hello", "there"]),
    ])
    .await;
    let row = ledgered(&r, "policy.granted");
    assert_eq!(row.len(), 1);
    assert_eq!(
        (row[0]["session_id"].as_str(), row[0]["pattern"].as_str()),
        (Some(sid.as_str()), Some("echo hello"))
    );
    // The same prefix runs, with one notice naming the grant.
    let res = turn(&r.core, Some(&sid), "again").await;
    assert!(res.awaiting_confirm.is_none(), "the grant runs it: {res:?}");
    let notices = ledgered(&r, "tool.notified");
    assert_eq!(notices.len(), 1);
    assert_eq!(
        notices[0]["setting"],
        format!("ran under your grant: echo hello, this conversation, {id}").as_str()
    );
    let rule = notices[0]["rule"].as_str().unwrap();
    assert!(
        rule.contains("ran under your grant: echo hello, this conversation"),
        "{rule}"
    );
    // A command off the prefix still asks.
    let res = turn(&r.core, Some(&sid), "bye").await;
    assert!(res.awaiting_confirm.is_some(), "off the prefix asks");
    // Another conversation's call asks: a grant never crosses sessions.
    let other = new_session(&r);
    let res = turn(&r.core, Some(&other), "hello there").await;
    assert!(res.awaiting_confirm.is_some(), "another session asks");
    assert_eq!(ledgered(&r, "tool.notified").len(), 1);
    // `policy.grants` lists it, beside the earned ones.
    let listed = rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::POLICY_GRANTS,
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(listed["session"][0]["id"], id.as_str());
    assert_eq!(listed["earned"], json!([]));
}

#[tokio::test]
async fn a_revoke_ends_it_and_the_prefix_asks_again() {
    let (r, sid, id) = granted(vec![run_of("g1", &["echo", "hello", "again"])]).await;
    let rv = rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::POLICY_REVOKE,
        json!({"id": id}),
    )
    .await
    .unwrap();
    assert_eq!(rv["session_grant"]["id"], id.as_str());
    assert_eq!(ledgered(&r, "policy.grant_ended")[0]["why"], "revoked");
    let res = turn(&r.core, Some(&sid), "again").await;
    assert!(res.awaiting_confirm.is_some(), "revoked: it asks");
    assert!(ledgered(&r, "tool.notified").is_empty());
    // A second revoke finds nothing.
    let e = rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::POLICY_REVOKE,
        json!({"id": id}),
    )
    .await
    .unwrap_err();
    assert!(e.message.contains("no live grant"), "{e:?}");
}

#[tokio::test]
async fn a_retired_session_s_grants_end_and_a_restart_rebuilds_the_rest() {
    let (r, sid, id) = granted(vec![]).await;
    // Another conversation's grant, which a restart keeps.
    let kept = new_session(&r);
    let g = crate::policy::grants::grant_of(
        &GrantOffer::prefix(&["echo".into(), "kept".into()]),
        &kept,
        "cor_kept",
        Some(&["echo".into(), "kept".into()]),
        ("the CLI", "cli".into(), "cli".into()),
        theseus_protocol::now_unix_ms(),
    )
    .unwrap();
    let row = crate::ledger::LedgerRow::new(
        theseus_protocol::LedgerKind::PolicyGranted,
        Some(&kept),
        None,
        serde_json::to_value(&g).unwrap(),
    );
    r.core
        .store
        .append(&[
            theseus_store::NewRecord::json(theseus_store::kinds::LEDGER, None, &row).unwrap(),
        ])
        .unwrap();
    r.core.tools.earned.scoped.add(g.clone());

    // A restart reads both back.
    let again = Grants::default();
    assert!(r.core.load_grants_into(&again).unwrap());
    assert_eq!(again.list(None).len(), 2);

    // Retiring the first session ends its grant, in its frame.
    r.core.retire_session(&sid, &"test".into()).unwrap();
    assert!(r.core.tools.earned.scoped.get(&id).is_none());
    let ended = ledgered(&r, "policy.grant_ended");
    assert_eq!(
        (ended.len(), ended[0]["why"].as_str()),
        (1, Some("retired"))
    );
    // And a restart after it keeps only the other.
    let after = Grants::default();
    assert!(r.core.load_grants_into(&after).unwrap());
    let ids: Vec<String> = after.list(None).into_iter().map(|g| g.id).collect();
    assert_eq!(ids, vec![g.id]);
    // A reopened session gets none back: its grant ended with the retire.
    r.core.reopen_session(&sid, &"test".into()).unwrap();
    let reopened = Grants::default();
    r.core.load_grants_into(&reopened).unwrap();
    assert!(reopened.list(Some(&sid)).is_empty());
}

#[tokio::test]
async fn an_answer_wider_than_the_offer_is_refused_and_nothing_changes() {
    let r = rig(vec![run_of("w1", &["echo", "hello", "x"])]);
    let sid = new_session(&r);
    let res = turn(&r.core, Some(&sid), "run it").await;
    let corr = res.awaiting_confirm.unwrap();
    for wider in ["echo", "echo hello x y", "", "bash -c"] {
        let e = grant(&r, &corr, wider).unwrap_err();
        assert!(
            format!("{e:#}").contains("not the one offered"),
            "{wider}: {e:#}"
        );
    }
    // Over the protocol too, and a grant with a decline.
    let e = rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::ACTION_CONFIRM,
        json!({"correlation_id": corr, "approve": true, "grant": {"pattern": "echo"}}),
    )
    .await
    .unwrap_err();
    assert!(e.message.contains("not the one offered"), "{e:?}");
    let e = rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::ACTION_CONFIRM,
        json!({"correlation_id": corr, "approve": false, "grant": {"pattern": "echo hello"}}),
    )
    .await
    .unwrap_err();
    assert!(e.message.contains("goes with an approval"), "{e:?}");
    // The question still waits, and nothing was granted.
    assert!(r.core.pending_confirms(&sid).unwrap().len() == 1);
    assert!(ledgered(&r, "policy.granted").is_empty());
    assert!(!r.core.tools.earned.scoped.any());
    // An MCP connection is no approval surface, grant or not.
    let e = rpc_as(
        &r.core,
        surface("mcp", Mcp),
        method::ACTION_CONFIRM,
        json!({"correlation_id": corr, "approve": true, "grant": {"pattern": "echo hello"}}),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(ledgered(&r, "policy.granted").is_empty());
}

#[tokio::test]
async fn a_shell_or_a_wrapper_asks_and_offers_nothing() {
    let r = rig(vec![
        run_of("s1", &["bash", "-c", "echo hello x"]),
        Scripted::text("asked"),
        run_of("s2", &["env", "echo", "hello", "x"]),
        Scripted::text("asked"),
        run_of("s3", &["python3", "-c", "print(1)"]),
    ]);
    let sid = new_session(&r);
    for input in ["bash", "env", "inline"] {
        let res = turn(&r.core, Some(&sid), input).await;
        let corr = res.awaiting_confirm.clone().expect("asks");
        assert_eq!(offer_of(&r, &sid, &corr), None, "{input}: no offer");
        r.core.confirm_action(&corr, false, None, "test").unwrap();
        r.core
            .continue_execution(&res.execution_id.unwrap())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn under_a_grant_a_shell_around_the_command_still_asks() {
    let (r, sid, _) = granted(vec![
        run_of("b1", &["bash", "-c", "echo hello x"]),
        Scripted::text("asked"),
        run_of("b2", &["sh", "-c", "echo hello"]),
    ])
    .await;
    for input in ["bash", "sh"] {
        let res = turn(&r.core, Some(&sid), input).await;
        let corr = res.awaiting_confirm.clone().expect("a shell asks");
        r.core.confirm_action(&corr, false, None, "test").unwrap();
        r.core
            .continue_execution(&res.execution_id.unwrap())
            .await
            .unwrap();
    }
    assert!(ledgered(&r, "tool.notified").is_empty());
}

/// The order on a plan, for a session with a grant on `echo hello`: the
/// decision with the grant's step and every layer after it.
fn ordered(
    r: &Rig,
    sid: &str,
    input: serde_json::Value,
    class: PlaceClass,
) -> crate::policy::Decision {
    let rt = &r.core.tools;
    let tool = rt.registry.get("proc.run").unwrap().clone();
    let plan = tool.plan(&input, &rt.ctx).unwrap();
    let held = || Ok(None);
    let mcp = || None;
    let at = crate::toolrun::order::At {
        place: crate::ceiling::PlaceView {
            class,
            ceiling: None,
            voice: false,
            draft: None,
        },
        held: &held,
        labels: &|| None,
        mcp: &mcp,
        glide: None,
        session: Some(sid),
        ctx: &rt.ctx,
        routine: None,
    };
    rt.order(&at, &*tool, &plan, &input, &mut |_, _| {}).0
}

fn gate_rig(sid: &str) -> Rig {
    let r = rig_with(vec![], |cfg| {
        cfg.policy.approve_argv = vec![vec!["echo".into(), "listed".into()]];
    });
    for (prefix, corr) in [
        (["echo", "hello"], "cor_1"),
        (["echo", "listed"], "cor_2"),
        (["theseusd", "status"], "cor_3"),
    ] {
        let words: Vec<String> = prefix.iter().map(|s| s.to_string()).collect();
        let g = crate::policy::grants::grant_of(
            &GrantOffer::prefix(&words),
            sid,
            corr,
            Some(&words),
            ("the CLI", "cli".into(), "cli".into()),
            1,
        )
        .unwrap();
        r.core.tools.earned.scoped.add(g);
    }
    r
}

/// The gate applies a grant in its own session, in a private place, to a
/// covered call alone: never in a shared place, another session, with an
/// `env`, a URL after the prefix, a path outside the roots, the approve
/// list, the floor, a tightened tool, or an L1 job's egress ask.
#[test]
fn the_gate_applies_a_grant_where_an_earned_one_would_alone() {
    use PlaceClass::{Private, Shared};
    let r = gate_rig("ses_mine");
    let hello = json!({"argv": ["echo", "hello", "x"], "cwd": r.root});
    let d = ordered(&r, "ses_mine", hello.clone(), Private);
    assert_eq!(d.posture, Posture::Notify, "{}", d.reason);
    let setting = &d.notify.as_ref().unwrap().setting;
    assert!(
        setting.starts_with("ran under your grant: echo hello, this conversation, sgr_"),
        "{setting}"
    );
    assert_eq!(
        ordered(&r, "ses_mine", hello.clone(), Shared).posture,
        Posture::Approve
    );
    assert_eq!(
        ordered(&r, "ses_other", hello.clone(), Private).posture,
        Posture::Approve
    );
    for (why, input) in [
        (
            "an env",
            json!({"argv": ["echo", "hello", "x"], "cwd": r.root, "env": {"A": "b"}}),
        ),
        (
            "a URL",
            json!({"argv": ["echo", "hello", "https://example.invalid"], "cwd": r.root}),
        ),
        (
            "outside the roots",
            json!({"argv": ["echo", "hello", "/etc/hosts"], "cwd": r.root}),
        ),
        (
            "the approve list",
            json!({"argv": ["echo", "listed"], "cwd": r.root}),
        ),
        (
            "the floor",
            json!({"argv": ["theseusd", "status"], "cwd": r.root}),
        ),
        (
            "another path",
            json!({"argv": ["/tmp/x/echo", "hello", "x"], "cwd": r.root}),
        ),
        (
            "an L1 job's egress",
            json!({"argv": ["echo", "hello", "x"], "cwd": r.root,
                   "sandbox": {"egress": ["exfil.example:443"]}}),
        ),
    ] {
        let d = ordered(&r, "ses_mine", input, Private);
        assert_eq!(d.posture, Posture::Approve, "{why}: {}", d.reason);
        assert!(d.notify.is_none(), "{why}");
    }
    r.core.tighten("proc.run", None, "test").unwrap();
    assert_eq!(
        ordered(&r, "ses_mine", hello, Private).posture,
        Posture::Approve,
        "tightened"
    );
}

/// An edit's question offers edits in the project; the grant covers the
/// edit tools under it, and a `..` path or a link out of it still asks.
#[tokio::test]
async fn an_edits_grant_covers_the_project_and_nothing_out_of_it() {
    let write = |id: &str, path: &str| {
        Scripted::tools(
            "",
            &[(id, "fs_write", json!({"path": path, "content": "x\n"}))],
        )
    };
    let r = rig(vec![
        write("e1", "a.txt"),
        Scripted::text("Written."),
        write("e2", "src/b.txt"),
        Scripted::text("Written."),
        write("e3", "src/../../outside.txt"),
        Scripted::text("asked"),
        write("e4", "out/escaped.txt"),
    ]);
    std::fs::create_dir_all(r.root.join("src")).unwrap();
    let sid = new_session(&r);
    let res = turn(&r.core, Some(&sid), "write a").await;
    let corr = res.awaiting_confirm.clone().expect("a write asks");
    let offer = offer_of(&r, &sid, &corr).expect("an edit offers edits");
    assert_eq!(offer.kind, KIND_EDITS);
    assert_eq!(offer.pattern, r.root.display().to_string());
    assert_eq!(offer.key(), 'e');
    grant(&r, &corr, &offer.pattern).unwrap();
    r.core
        .continue_execution(&res.execution_id.unwrap())
        .await
        .unwrap();
    let res = turn(&r.core, Some(&sid), "write b").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert_eq!(
        std::fs::read_to_string(r.root.join("src/b.txt")).unwrap(),
        "x\n"
    );
    let res = turn(&r.core, Some(&sid), "dotdot").await;
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("`..` out of the project asks");
    r.core.confirm_action(&corr, false, None, "test").unwrap();
    r.core
        .continue_execution(&res.execution_id.unwrap())
        .await
        .unwrap();
    let outside = r.root.parent().unwrap().join("elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, r.root.join("out")).unwrap();
    let res = turn(&r.core, Some(&sid), "link").await;
    assert!(
        res.awaiting_confirm.is_some(),
        "a link out of the project asks"
    );
    assert!(!outside.join("escaped.txt").exists());
}

/// The offer is only for a question the posture alone asked: never the
/// approve list's, the floor's, a shared place's, one with an `env`, an L1
/// job's egress ask, or one the external-text hold raised.
#[test]
fn only_a_postures_ask_offers_a_grant() {
    use PlaceClass::{Private, Shared};
    let r = gate_rig("ses_none");
    let rt = &r.core.tools;
    let offered = |input: serde_json::Value, class: PlaceClass, external: bool| {
        let tool = rt.registry.get("proc.run").unwrap().clone();
        let plan = tool.plan(&input, &rt.ctx).unwrap();
        let (d, bound) = crate::sandbox::unbrokered(rt, &*tool, &plan, &input, None, None);
        let asked = crate::policy::grants::Asked {
            approve: d.posture == Posture::Approve,
            reason: &d.reason,
            floor: d.floor,
            external,
            l1: bound.l1(),
        };
        crate::policy::grants::offer(rt, class, None, &*tool, &plan, &input, &asked)
            .map(|o| o.pattern)
    };
    let hello = json!({"argv": ["echo", "hello", "x"], "cwd": r.root});
    assert_eq!(
        offered(hello.clone(), Private, false).as_deref(),
        Some("echo hello")
    );
    assert_eq!(
        offered(hello.clone(), Shared, false),
        None,
        "a shared place"
    );
    assert_eq!(offered(hello, Private, true), None, "the hold asked");
    for (why, input) in [
        (
            "the approve list",
            json!({"argv": ["echo", "listed"], "cwd": r.root}),
        ),
        (
            "the floor",
            json!({"argv": ["theseusd", "status"], "cwd": r.root}),
        ),
        (
            "an env",
            json!({"argv": ["echo", "hello"], "cwd": r.root, "env": {"A": "b"}}),
        ),
        (
            "outside the roots",
            json!({"argv": ["echo", "hello", "/etc/hosts"], "cwd": r.root}),
        ),
        (
            "an L1 job's egress",
            json!({"argv": ["echo", "hello"], "cwd": r.root,
                   "sandbox": {"egress": ["exfil.example:443"]}}),
        ),
    ] {
        assert_eq!(offered(input, Private, false), None, "{why}");
    }
}
