//! Self-improvement's gate, kill switch, log and digest (theseus-pw1q.2,
//! theseus-pw1q.4), through the core: `rsi::gate` under each mode and
//! switch, who may resume and from where, the halt across a restart, and
//! `self.log` with today's kinds.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::rsi::{SelfHaltParams, SelfLogParams, SelfMode, SelfResumeParams, SelfState};

use super::tests::test_core;
use super::{Core, Parts};
use crate::approval::{Answerer, Refusal, Surface};
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::rsi::{gate, Gate};

const OWNER: u64 = 271_828_182_845_904_523;
const SOMEONE: u64 = 314_159_265_358_979_323;
const DAY: u64 = 86_400_000;

/// A core whose `[self] mode` is `mode`, over `store` (a restart in place
/// when it is another core's).
fn core_over(store: &crate::store::Store, cfg: &crate::Config, mode: SelfMode) -> Arc<Core> {
    let mut cfg = cfg.clone();
    cfg.self_improve.mode = mode;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store.clone(),
    ))
    .unwrap()
}

fn core(mode: SelfMode) -> Arc<Core> {
    let c = test_core("the tide is out");
    core_over(&c.store, &c.cfg, mode)
}

/// Someone on Discord: in `channel` (a guild's, bound to nothing: shared)
/// or their DM.
fn discord(user: u64, channel: Option<u64>) -> Answerer {
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

fn cli() -> Answerer {
    Answerer::from("the CLI")
}

fn halt(c: &Core, who: &Answerer, why: &str) -> bool {
    let p = SelfHaltParams {
        why: Some(why.into()),
        ..Default::default()
    };
    c.self_halt(&p, who).unwrap().changed
}

fn resume(c: &Core, who: &Answerer, from_job: Option<&str>) -> anyhow::Result<bool> {
    let p = SelfResumeParams {
        from_job: from_job.map(str::to_string),
        ..Default::default()
    };
    c.self_resume(&p, who).map(|r| r.changed)
}

fn rows(c: &Core, kind: &str) -> Vec<serde_json::Value> {
    c.store
        .ledger_tail::<LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}

fn gate_of(c: &Core) -> Gate {
    gate(&c.self_ctx())
}

/// Mode off: `Off` whatever the switch says, a resume included.
#[test]
fn the_gate_is_off_while_the_mode_is_off() {
    let c = core(SelfMode::Off);
    assert_eq!(gate_of(&c), Gate::Off);
    assert!(resume(&c, &cli(), None).unwrap());
    assert_eq!(
        gate_of(&c),
        Gate::Off,
        "a resume runs nothing while the mode is off"
    );
    assert_eq!(c.self_state().gate, "off");
}

/// Mode act: a fresh store is halted; the owner's resume allows; a halt from
/// anyone, a shared place's stranger included, halts again; a second halt
/// writes nothing.
#[test]
fn the_gate_is_halted_until_the_owners_resume_and_after_anyones_halt() {
    let c = core(SelfMode::Act);
    match gate_of(&c) {
        Gate::Halted { why, at_ms: None } => assert!(why.contains("never resumed"), "{why}"),
        g => panic!("a fresh store is halted: {g:?}"),
    }
    let s: SelfState = c.self_state();
    assert!(s.halted && s.never_resumed && s.gate == "halted", "{s:?}");
    assert!(resume(&c, &cli(), None).unwrap());
    assert_eq!(gate_of(&c), Gate::Allowed);
    assert!(
        !resume(&c, &cli(), None).unwrap(),
        "a released switch stays"
    );
    assert!(halt(&c, &discord(SOMEONE, Some(77)), "it looks wrong"));
    match gate_of(&c) {
        Gate::Halted {
            why,
            at_ms: Some(_),
        } => assert!(why.contains("it looks wrong"), "{why}"),
        g => panic!("halted by anyone: {g:?}"),
    }
    assert!(!halt(&c, &cli(), "again"), "halting is idempotent");
    assert_eq!(rows(&c, "self.halted").len(), 1);
    assert_eq!(rows(&c, "self.resumed").len(), 1);
    let h = &rows(&c, "self.halted")[0];
    assert_eq!(h["why"], "it looks wrong", "{h}");
    assert_eq!(h["place"], "discord:77", "{h}");
    assert_eq!(
        h["undo"],
        "theseus self resume (the owner, from a private place)"
    );
}

/// A halt on a store never resumed is written, so the log says who.
#[test]
fn a_halt_of_a_store_never_resumed_is_written() {
    let c = core(SelfMode::Off);
    assert!(halt(&c, &cli(), "test"));
    assert!(!halt(&c, &cli(), "test"));
    let s = c.self_state();
    assert!(s.halted && !s.never_resumed, "{s:?}");
    assert_eq!(s.why.as_deref(), Some("test"));
}

/// A resume from a shared place, from a job's shell, from someone who is not
/// the owner, and through the MCP server is refused and ledgered, and the
/// switch holds; the owner's DM and the CLI count.
#[test]
fn only_the_owners_private_resume_counts_and_each_refusal_is_ledgered() {
    let c = core(SelfMode::Act);
    let mcp = Answerer {
        label: "an MCP client".into(),
        surface: Surface::Mcp,
        discord: None,
    };
    let refused = [
        (discord(OWNER, Some(77)), None, "shared place"),
        (cli(), Some("ses_0000aa1b2c3"), "job's shell"),
        (discord(SOMEONE, None), None, "is not an owner"),
        (mcp, None, "never an approval surface"),
    ];
    for (who, job, why) in &refused {
        let e = resume(&c, who, *job).unwrap_err();
        let r = e.downcast::<Refusal>().expect("a refusal");
        assert!(r.why.contains(why), "{}: {}", why, r.why);
        assert!(
            matches!(gate_of(&c), Gate::Halted { .. }),
            "{why}: still halted"
        );
    }
    let refusals: Vec<_> = rows(&c, "approval.refused")
        .into_iter()
        .filter(|r| r["act"] == "self.resume")
        .collect();
    assert_eq!(refusals.len(), refused.len(), "{refusals:?}");
    assert_eq!(refusals[1]["from_job"], "ses_0000aa1b2c3");
    assert!(rows(&c, "self.resumed").is_empty());
    let log = c.self_log(&SelfLogParams::default()).unwrap();
    assert_eq!(
        log.rows
            .iter()
            .filter(|r| r.kind == "approval.refused")
            .count(),
        refused.len(),
        "the log shows each refused resume"
    );
    assert!(
        resume(&c, &discord(OWNER, None), None).unwrap(),
        "the owner's DM"
    );
    assert_eq!(gate_of(&c), Gate::Allowed);
}

/// The halt survives a restart in place, and so does the owner's resume.
#[test]
fn the_switch_survives_a_restart_in_place() {
    let c = core(SelfMode::Act);
    assert!(resume(&c, &cli(), None).unwrap());
    assert!(halt(&c, &cli(), "before the restart"));
    let again = core_over(&c.store, &c.cfg, SelfMode::Act);
    match gate_of(&again) {
        Gate::Halted { why, .. } => assert!(why.contains("before the restart"), "{why}"),
        g => panic!("the halt survives a restart: {g:?}"),
    }
    assert!(resume(&again, &cli(), None).unwrap());
    let third = core_over(&c.store, &c.cfg, SelfMode::Act);
    assert_eq!(gate_of(&third), Gate::Allowed);
}

/// The gate reads a cached state: a thousand asks well under a millisecond
/// each.
#[test]
fn the_gate_costs_one_cached_read() {
    let c = core(SelfMode::Act);
    assert!(resume(&c, &cli(), None).unwrap());
    let t = std::time::Instant::now();
    for _ in 0..1_000 {
        assert_eq!(gate_of(&c), Gate::Allowed);
    }
    let each = t.elapsed() / 1_000;
    assert!(
        each < std::time::Duration::from_millis(1),
        "{each:?} an ask"
    );
}

/// Write a row of `kind` at `at`.
fn seed(c: &Core, kind: &str, at: u64, data: serde_json::Value) {
    let mut r = LedgerRow::named(kind, None, None, data);
    r.at_unix_ms = at;
    c.store.append_ledger(&r).unwrap();
}

/// `self.log` returns the self rows and today's kinds together, newest
/// first, each with what, its numbers and its undo, filtered by `since`;
/// a refusal of another act is no change to Theseus.
#[test]
fn the_log_holds_self_rows_and_todays_kinds_newest_first() {
    let c = core(SelfMode::Off);
    let now = theseus_protocol::now_unix_ms();
    seed(
        &c,
        "pack.mode",
        now - 10 * DAY,
        json!({"pack": "loop.v1", "mode": "live", "from": "shadow", "who": "owner", "why": "old"}),
    );
    seed(
        &c,
        "pack.mode",
        now - 3 * DAY,
        json!({"pack": "loop.v1", "mode": "canary", "from": "shadow", "who": "system", "why": "its bar", "share": 0.2}),
    );
    seed(
        &c,
        "extend.acked",
        now - 2 * DAY,
        json!({"name": "wordcount", "digest": "abcdef0123"}),
    );
    seed(
        &c,
        "approval.refused",
        now - 2 * DAY,
        json!({"act": "policy.trust", "who": "x", "why": "y"}),
    );
    seed(
        &c,
        "self.joined",
        now - DAY,
        json!({"what": "joined self/2026-10-01-faster-recall", "why": "recall p95 over budget", "numbers": {"cost_usd": 1.25}, "undo": "theseus self veto abc123", "branch": "self/2026-10-01-faster-recall"}),
    );
    seed(&c, "turn.ended", now - DAY, json!({}));
    assert!(halt(&c, &cli(), "a look first"));
    let all = c.self_log(&SelfLogParams::default()).unwrap();
    let kinds: Vec<&str> = all.rows.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "self.halted",
            "self.joined",
            "extend.acked",
            "pack.mode",
            "pack.mode"
        ],
        "newest first, today's kinds and the self rows together"
    );
    assert!(!all.more && !all.building);
    let joined = &all.rows[1];
    assert_eq!(joined.what, "joined self/2026-10-01-faster-recall");
    assert_eq!(joined.numbers["cost_usd"], 1.25);
    assert_eq!(joined.undo.as_deref(), Some("theseus self veto abc123"));
    assert_eq!(
        all.rows[2].undo.as_deref(),
        Some("theseus extend revoke wordcount")
    );
    assert_eq!(
        all.rows[3].undo.as_deref(),
        Some("theseus packs rollback loop.v1")
    );
    assert_eq!(all.rows[3].numbers["share"], 0.2);
    let week = c
        .self_log(&SelfLogParams {
            since_ms: Some(now - 7 * DAY),
            limit: None,
        })
        .unwrap();
    assert_eq!(
        week.rows.len(),
        4,
        "the 10-day-old move is outside the week"
    );
    let two = c
        .self_log(&SelfLogParams {
            since_ms: None,
            limit: Some(2),
        })
        .unwrap();
    assert_eq!(two.rows.len(), 2);
    assert!(two.more);
    assert!(all.state.halted);
}

/// The digest lists a seeded week's rows: counts by kind, the joins with
/// their numbers and undo, the halts, the cost.
#[test]
fn the_digest_lists_a_seeded_week() {
    let c = core(SelfMode::Off);
    let now = theseus_protocol::now_unix_ms();
    seed(
        &c,
        "self.joined",
        now - DAY,
        json!({"what": "joined self/x", "why": "a gap", "numbers": {"cost_usd": 2.5}, "undo": "theseus self veto m1"}),
    );
    seed(
        &c,
        "judge.proposal",
        now - 2 * DAY,
        json!({"parent": "classify.v1", "version": "classify.v101", "decision": "canary", "why": "rose", "fixed": 4, "broken": 0, "writer_usd": 0.5}),
    );
    seed(
        &c,
        "pack.mode",
        now - 9 * DAY,
        json!({"pack": "loop.v1", "mode": "live", "from": "shadow", "who": "owner"}),
    );
    assert!(halt(&c, &cli(), "a look"));
    let d = c
        .self_digest(&theseus_protocol::rsi::SelfDigestParams::default())
        .unwrap();
    let t = &d.text;
    assert!(
        t.starts_with("What Theseus changed about itself this week"),
        "{t}"
    );
    assert!(
        t.contains("3 changes: 1 judge.proposal, 1 self.halted, 1 self.joined."),
        "{t}"
    );
    assert!(t.contains("Cost: $3.00."), "{t}");
    assert!(
        t.contains("Joins:\n- ") && t.contains("undo: `theseus self veto m1`"),
        "{t}"
    );
    assert!(t.contains("The kill switch:\n- "), "{t}");
    assert!(
        t.contains("undo: `theseus packs rollback classify.v101`"),
        "{t}"
    );
    assert!(
        !t.contains("loop.v1"),
        "the 9-day-old move is not this week's: {t}"
    );
    assert_eq!(d.rows, 3);
    assert!(
        !c.cfg.self_improve.posts_digest(),
        "never posted while the mode is off"
    );
}

/// The weekly post: nothing at all while the mode is off (no read, no
/// mark, no post), even with the owner's DM bound and a week due; with the
/// mode act, the first tick starts the week, and a week later one post goes
/// to the owner's DM with its mark.
#[test]
fn the_digest_is_posted_weekly_only_while_the_mode_acts() {
    let bind = |c: &Core| {
        c.runner.place_rule.bind_one(crate::places::BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @owner".into(),
            private: true,
            guild: None,
            ceiling: None,
        });
    };
    let off = core(SelfMode::Off);
    bind(&off);
    let old = theseus_protocol::now_unix_ms() - 8 * DAY;
    off.store
        .put_meta(crate::rsi::DIGEST_KEY, &json!({"at_ms": old}))
        .unwrap();
    assert!(
        !off.post_self_digest_if_due(),
        "never posted while the mode is off"
    );
    assert_eq!(
        off.store
            .get_meta::<serde_json::Value>(crate::rsi::DIGEST_KEY)
            .unwrap()
            .unwrap()["at_ms"],
        old,
        "nothing was written"
    );
    let act = core_over(&off.store, &off.cfg, SelfMode::Act);
    assert!(
        !act.post_self_digest_if_due(),
        "no DM bound: nowhere to post"
    );
    bind(&act);
    let before = act.store.last_position();
    assert!(act.post_self_digest_if_due(), "a week has passed");
    assert!(act.store.last_position() > before);
    assert!(!act.post_self_digest_if_due(), "once a week");
    let fresh = core(SelfMode::Act);
    bind(&fresh);
    assert!(
        !fresh.post_self_digest_if_due(),
        "the first tick starts the week"
    );
    assert!(fresh
        .store
        .get_meta::<serde_json::Value>(crate::rsi::DIGEST_KEY)
        .unwrap()
        .is_some());
    assert!(!fresh.post_self_digest_if_due());
}
