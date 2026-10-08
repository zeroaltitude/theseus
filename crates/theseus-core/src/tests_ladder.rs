//! The ladder (M5 26a; design §3, "26a"), through whole cores on stores of
//! their own: the config lowers a mode and never raises it; an automatic
//! promotion short of the bar is refused with the numbers, and the owner's
//! is ledgered as forced; a shared place's move is refused; a security
//! promotion is a card only the owner's answer carries, and a decline or an
//! expiry writes no mode; a rule's trigger rolls a pack back; the adoption
//! is written once, after serving; each adopted rule fires on its scripted
//! events and not on a near miss; a brake lapses at midnight; a restart
//! keeps the day's count; and `max_mode = "shadow"` caps every pack after a
//! restart. The CLI's refusal inside a job is `client`'s test.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_judge::learn::CanaryEvent;
use theseus_judge::Judgment;
use theseus_protocol::learning::JudgeLabelParams;
use theseus_protocol::packs::{PackModeRow, PackPromoteParams, PackRollbackParams};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::approval::{Answerer, Surface};
use crate::config::PackMode;
use crate::judge::ladder::{self, ArmOf, Rung};
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, judge_config, texts, turn, until_judged};
use crate::Config;

/// A core on `dir`'s store, built as the daemon builds one: the judge on
/// (its Jev at `jev`, else nowhere), every pack as the build wires it.
fn core_at(dir: &Path, jev: Option<&FakeJev>, tweak: impl FnOnce(&mut Config)) -> Arc<Core> {
    let mut cfg = judge_config(dir, jev);
    cfg.judge.enabled = true;
    if jev.is_none() {
        cfg.judge.api_base = "http://127.0.0.1:9".into();
    }
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(texts(4)));
    let mut p = Parts::for_tests(cfg, fake, store);
    p.secrets = board();
    Core::build(p).unwrap()
}

fn rows(core: &Core, pack: &str) -> Vec<PackModeRow> {
    core.store
        .scope_after(&ladder::scope(pack), 0)
        .unwrap()
        .into_iter()
        .map(|r| r.decode::<LedgerRow>().unwrap())
        .filter(|r| r.kind == LedgerKind::PackMode.as_str())
        .map(|r| serde_json::from_value(r.data).unwrap())
        .filter(|r: &PackModeRow| r.pack == pack)
        .collect()
}

fn promote(pack: &str, to: &str, share: Option<f64>) -> PackPromoteParams {
    PackPromoteParams {
        pack: pack.into(),
        to: to.into(),
        share,
        report: None,
    }
}

/// Someone in a shared guild channel: no approval counts from there.
fn stranger() -> Answerer {
    Answerer {
        label: "discord".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "42".into(),
            channel_id: "1001".into(),
            guild_id: Some("7".into()),
        }),
    }
}

fn refusal(e: &anyhow::Error) -> bool {
    e.downcast_ref::<crate::approval::Refusal>().is_some()
}

/// The config lowers what the ladder gives and never raises it: a live
/// row under `max_mode = "shadow"` or a pack's `mode = "shadow"` judges in
/// shadow, `off` calls nothing, and a pack's line of `live` lifts no
/// shadow pack.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_config_lowers_the_ladders_mode_and_never_raises_it() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    let j = &c.runner.judge;
    c.pack_promote(&promote("loop.v1", "live", None), "cli")
        .unwrap();
    assert_eq!(j.mode_for("loop.v1", "ses_a").mode, PackMode::Live);
    drop(c);
    for (line, want) in [
        ("max_mode = \"shadow\"", PackMode::Shadow),
        ("pack shadow", PackMode::Shadow),
        ("pack off", PackMode::Off),
    ] {
        let c = core_at(dir.path(), None, |cfg| match line {
            "pack shadow" => {
                cfg.judge.packs.entry("loop.v1".into()).or_default().mode = Some(PackMode::Shadow)
            }
            "pack off" => {
                cfg.judge.packs.entry("loop.v1".into()).or_default().mode = Some(PackMode::Off)
            }
            _ => cfg.judge.max_mode = PackMode::Shadow,
        });
        assert_eq!(
            c.runner.judge.mode_for("loop.v1", "ses_a").mode,
            want,
            "{line}"
        );
        drop(c);
    }
    // A pack's line of live lifts nothing the ladder left in shadow.
    let c = core_at(dir.path(), None, |cfg| {
        cfg.judge
            .packs
            .entry("classify.v1".into())
            .or_default()
            .mode = Some(PackMode::Live);
    });
    let g = c.runner.judge.mode_for("classify.v1", "ses_a");
    assert_eq!((g.mode, g.arm), (PackMode::Shadow, ArmOf::All));
}

/// An automatic promotion short of the bar is refused with the numbers,
/// and writes nothing; the owner's is written, `forced`, with the same
/// numbers; a shared place's word is refused and writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn short_of_the_bar_the_system_is_refused_and_the_owner_forces() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    let e = c
        .promote_automatic(&promote("loop.v1", "canary", Some(0.2)))
        .unwrap_err();
    assert!(
        e.to_string()
            .contains("short of the bar: no learning report of loop.v1"),
        "{e}"
    );
    assert!(rows(&c, "loop.v1").is_empty(), "nothing written");
    let e = c
        .pack_promote(&promote("loop.v1", "canary", Some(0.2)), stranger())
        .unwrap_err();
    assert!(refusal(&e), "{e:#}");
    assert!(rows(&c, "loop.v1").is_empty(), "nothing written");
    let out = c
        .pack_promote(&promote("loop.v1", "canary", Some(0.2)), "cli")
        .unwrap();
    let row = out.row.unwrap();
    assert!(row.forced);
    assert_eq!(
        (
            row.mode.as_str(),
            row.from.as_str(),
            row.share,
            row.who.as_str()
        ),
        ("canary", "shadow", Some(0.2), "owner")
    );
    assert!(row
        .numbers
        .as_deref()
        .unwrap()
        .contains("no learning report"));
    assert_eq!(rows(&c, "loop.v1").len(), 1);
    assert_eq!(
        c.runner.judge.ladder().standing("loop.v1").rung,
        Rung::Canary
    );
    let e = c
        .pack_rollback(
            &PackRollbackParams {
                pack: "loop.v1".into(),
                why: None,
                off: false,
            },
            stranger(),
        )
        .unwrap_err();
    assert!(refusal(&e), "{e:#}");
    let back = c
        .pack_rollback(
            &PackRollbackParams {
                pack: "loop.v1".into(),
                why: Some("too eager".into()),
                off: false,
            },
            "cli",
        )
        .unwrap();
    assert_eq!(
        (back.mode.as_str(), back.who.as_str()),
        ("rolled_back", "owner")
    );
    assert_eq!(back.until_ms, None, "the owner's stands until a promotion");
    // Bad asks are invalid, not written.
    for bad in [
        promote("loop.v1", "canary", None),
        promote("loop.v1", "canary", Some(1.5)),
        promote("loop.v1", "live", Some(0.5)),
        promote("loop.v1", "sideways", None),
        promote("nope.v9", "live", None),
    ] {
        assert!(c.pack_promote(&bad, "cli").is_err(), "{bad:?}");
    }
}

/// The report's bar, from a stored `judge.report` row: one with a short
/// holdout is cited and its numbers kept; an automatic promotion citing it
/// is refused with them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_promotion_cites_its_report_and_its_holdouts_bounds() {
    use theseus_protocol::learning::{Holdout, PackReport};
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    let report = PackReport {
        pack: "loop.v1".into(),
        holdout: Holdout {
            start_ms: 1_000,
            end_ms: 2_000,
            labeled_per_question: [
                ("work_state".to_string(), 37),
                ("announced_unfinished".to_string(), 200),
            ]
            .into(),
            ..Holdout::default()
        },
        ..PackReport::default()
    };
    let key = "rpt_2026-10-01_loop.v1";
    let row = LedgerRow::new(
        LedgerKind::JudgeReport,
        None,
        None,
        json!({"date": "2026-10-01", "report": report}),
    );
    let mut rec = NewRecord::json(kinds::LEDGER, None, &row).unwrap();
    rec.key = Some(key.into());
    c.store.append(&[rec.scoped("judge:loop")]).unwrap();
    let mut p = promote("loop.v1", "live", None);
    p.report = Some(key.into());
    let e = c.promote_automatic(&p).unwrap_err();
    assert!(
        e.to_string().contains("work_state: labeled 37 of 200"),
        "{e}"
    );
    let row = c.pack_promote(&p, "cli").unwrap().row.unwrap();
    assert_eq!(row.report.as_deref(), Some(key));
    let h = row.holdout.unwrap();
    assert_eq!((h.start_ms, h.end_ms), (1_000, 2_000));
    assert_eq!(
        row.numbers.as_deref(),
        Some("work_state: labeled 37 of 200")
    );
}

/// A security pack's promotion is a card on the ladder's session, listed
/// with every question: a shared place's answer does not carry it, the
/// owner's approval writes the mode, a decline writes a declined row and no
/// mode, and so does an expiry.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_security_promotion_is_the_owners_card() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    let ask = |c: &Core| {
        let out = c
            .pack_promote(&promote("security.v1", "live", None), "cli")
            .unwrap();
        assert!(out.row.is_none(), "no mode before the answer");
        out.question.unwrap()
    };
    let q = ask(&c);
    assert!(rows(&c, "security.v1").is_empty());
    let listed = c.confirm_list().unwrap();
    let card = listed
        .iter()
        .find(|r| r.correlation_id == q)
        .expect("listed");
    assert_eq!(card.tool, "pack.promote");
    assert!(
        card.reason.contains("Promote security.v1 to live?"),
        "{}",
        card.reason
    );
    // A shared place's answer does not carry it.
    let e = c.confirm_action(&q, true, None, stranger()).unwrap_err();
    assert!(refusal(&e), "{e:#}");
    assert!(rows(&c, "security.v1").is_empty());
    // The owner's approval writes it.
    let done = c.confirm_action(&q, true, None, "cli").unwrap();
    assert!(done.approved && !done.resumes);
    let written = rows(&c, "security.v1");
    assert_eq!(written.len(), 1);
    assert_eq!(
        (
            written[0].mode.as_str(),
            written[0].question.as_deref(),
            written[0].declined
        ),
        ("live", Some(q.as_str()), false)
    );
    assert_eq!(
        c.runner.judge.ladder().standing("security.v1").rung,
        Rung::Live
    );
    // A decline: a declined row, the mode unchanged.
    c.pack_rollback(
        &PackRollbackParams {
            pack: "security.v1".into(),
            why: None,
            off: false,
        },
        "cli",
    )
    .unwrap();
    let q2 = ask(&c);
    c.confirm_action(&q2, false, Some("not yet"), "cli")
        .unwrap();
    let last = rows(&c, "security.v1").pop().unwrap();
    assert!(last.declined && last.why == "not yet", "{last:?}");
    assert_eq!(
        c.runner.judge.ladder().standing("security.v1").rung,
        Rung::RolledBack
    );
    // No answer in time: declined by expiry, no mode.
    let q3 = ask(&c);
    let ttl = c.kernel.config().confirm_ttl_ms;
    assert_eq!(c.expire_questions(c.kernel.now_ms() + ttl + 1), 1);
    let last = rows(&c, "security.v1").pop().unwrap();
    assert!(
        last.declined && last.question.as_deref() == Some(q3.as_str()),
        "{last:?}"
    );
    assert!(last.why.contains("nobody answered"), "{}", last.why);
    assert_eq!(
        c.runner.judge.ladder().standing("security.v1").rung,
        Rung::RolledBack
    );
    assert!(c.confirm_list().unwrap().is_empty());
    // An automatic one is a card all the same (short of the bar, refused
    // first).
    assert!(c
        .promote_automatic(&promote("security.v1", "live", None))
        .is_err());
}

/// A synthetic trigger of a pack's own rule rolls it back at once: a row
/// (`rolled_back`, `who: system`, the rule, its words), and it stands until
/// a promotion; a pack in shadow is not rolled back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rules_trigger_rolls_a_pack_back() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    let j = &c.runner.judge;
    let nudge = || CanaryEvent::NudgedTurnEnded {
        task: "tsk_heron".into(),
        new_tool_calls: 0,
        final_before: "I will run the tests next.".into(),
        final_after: "I will run the tests next.".into(),
    };
    j.land("loop.v1", nudge());
    assert!(
        rows(&c, "loop.v1").is_empty(),
        "shadow: nothing to roll back"
    );
    c.pack_promote(&promote("loop.v1", "canary", Some(0.5)), "cli")
        .unwrap();
    j.land("loop.v1", nudge());
    let back = rows(&c, "loop.v1").pop().unwrap();
    assert_eq!(
        (back.mode.as_str(), back.who.as_str(), back.rule.as_deref()),
        ("rolled_back", "system", Some("nudge_loop"))
    );
    assert!(back.words.unwrap().contains("tsk_heron"));
    assert_eq!(
        back.until_ms, None,
        "a file's rule stands until a promotion"
    );
    assert_eq!(j.mode_for("loop.v1", "ses_any").mode, PackMode::Shadow);
    let line = j
        .health()
        .packs
        .into_iter()
        .find(|l| l.starts_with("loop.v1"))
        .unwrap();
    assert_eq!(line, "loop.v1: rolled back (nudge_loop)");
    // A promotion starts the count again: the old event fires nothing.
    c.pack_promote(&promote("loop.v1", "canary", Some(0.5)), "cli")
        .unwrap();
    j.land("loop.v1", CanaryEvent::OnPath { ms: 10 });
    assert_eq!(j.ladder().standing("loop.v1").rung, Rung::Canary);
}

/// The packs live before the ladder are adopted once, at its first read
/// after serving (never by the core's build): `live`, `who: owner`, `why:
/// "decision of 2026-10-04"`. A restart adds none; a pack this build wires
/// in shadow (its live action not joined) is not adopted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_adoption_is_written_once_after_serving() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    c.runner.judge.ladder().wire("security.v3", PackMode::Live);
    // This build wires rerank.v1 live (32d): wire it in shadow, as a build
    // whose live action has not joined.
    c.runner.judge.ladder().wire("rerank.v1", PackMode::Shadow);
    assert!(rows(&c, "security.v3").is_empty(), "nothing at the build");
    assert!(!c.runner.judge.ladder().is_loaded());
    c.warm_ladder();
    let t0 = std::time::Instant::now();
    while rows(&c, "security.v3").is_empty() {
        assert!(t0.elapsed() < std::time::Duration::from_secs(10));
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let r = rows(&c, "security.v3");
    assert_eq!(r.len(), 1);
    assert_eq!(
        (
            r[0].mode.as_str(),
            r[0].who.as_str(),
            r[0].why.as_str(),
            r[0].via.as_str()
        ),
        ("live", "owner", "decision of 2026-10-04", "adoption")
    );
    assert!(
        rows(&c, "rerank.v1").is_empty(),
        "wired in shadow: not adopted"
    );
    let line = c
        .runner
        .judge
        .health()
        .packs
        .into_iter()
        .find(|l| l.starts_with("security.v3"))
        .unwrap();
    assert_eq!(line, "security.v3: live (owner: decision of 2026-10-04)");
    drop(c);
    let c = core_at(dir.path(), None, |_| {});
    c.runner.judge.ladder().wire("security.v3", PackMode::Live);
    assert_eq!(
        c.runner.judge.ladder().standing("security.v3").rung,
        Rung::Live
    );
    assert_eq!(rows(&c, "security.v3").len(), 1, "a restart adds none");
}

/// Each adopted rule fires on its scripted events, and not one short:
/// `route.v1`'s 3 pins, `rerank.v1`'s 2 breaker opens, `security.v3`'s 31
/// notices; each is a day's brake, until the next local midnight.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_adopted_rule_fires_on_its_events_and_not_on_a_near_miss() {
    let day = |c: &Core| c.runner.judge.today();
    type Event = fn(String) -> CanaryEvent;
    let cases: [(&str, usize, Event, &str); 3] = [
        (
            "route.v3",
            3,
            |day| CanaryEvent::Pinned { day },
            "pins_per_day",
        ),
        (
            "rerank.v1",
            2,
            |day| CanaryEvent::BreakerOpened { day },
            "opens_per_day",
        ),
        (
            "security.v3",
            31,
            |day| CanaryEvent::Notice { day },
            "notices_per_day",
        ),
    ];
    for (pack, n, event, rule) in cases {
        let dir = tempfile::tempdir().unwrap();
        let c = core_at(dir.path(), None, |_| {});
        let j = &c.runner.judge;
        j.ladder().wire(pack, PackMode::Live);
        for _ in 0..n - 1 {
            j.land(pack, event(day(&c)));
        }
        assert_eq!(
            j.ladder().standing(pack).rung,
            Rung::Live,
            "{pack}: a near miss"
        );
        j.land(pack, event(day(&c)));
        let s = j.ladder().standing(pack);
        assert_eq!(
            (s.rung, s.rule.as_deref()),
            (Rung::RolledBack, Some(rule)),
            "{pack}"
        );
        let until = s.until_ms.expect("a day's brake");
        assert_eq!(until, ladder::rules::next_midnight(j.ladder().now()));
        assert_eq!(j.mode_for(pack, "ses_a").mode, PackMode::Shadow);
    }
}

/// A judgment, as the sink writes it, of `pack` (`security.v3`).
fn judgment_row(id: &str, pack: &str) -> NewRecord {
    let j: Judgment = serde_json::from_value(json!({
        "id": id, "pack": pack, "version": 3, "pack_sha256": "00", "point": "gate",
        "mode": "live", "model": "jev-1.13.0", "answered_by": "jev-1.13.0",
        "model_drift": false,
        "state": {"sha256": "00", "bytes": 10, "tokens": 3, "cap_tokens": 4000,
            "builder": "gate", "builder_version": 1, "truncated": [], "dropped": []},
        "questions": 0, "answers": [], "call": null,
        "timing": {"queued_ms": 0, "http_ms": 5, "total_ms": 5},
        "usage": null, "cost_micros": 40, "reserve_micros": 50,
        "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
        "context": {"session": "ses_x"},
    }))
    .unwrap();
    let f = crate::fact::judge::JudgeCall {
        judgment: &j,
        budget: "shadow",
    };
    let row = LedgerRow::new(
        LedgerKind::JudgeCall,
        Some("ses_x"),
        None,
        crate::fact::Fact::row(&f),
    );
    let mut r = NewRecord::json(kinds::LEDGER, None, &row).unwrap();
    r.key = Some(id.into());
    r.scoped("judge:security")
}

fn noise(c: &Core, id: &str) {
    c.judge_label(
        &JudgeLabelParams {
            judgment: id.into(),
            question: None,
            label: json!("noise"),
            note: None,
            discord: None,
        },
        "cli",
    )
    .unwrap();
}

/// Security's notices' rule counts the owner's `noise` labels as they land
/// (through `judge.label`): two in a day keep it live, and after a restart
/// the day's two are read back, so the third rolls it back until midnight.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restart_keeps_the_days_count() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    c.runner.judge.ladder().wire("security.v3", PackMode::Live);
    let ids = ["jdg_ash", "jdg_birch", "jdg_cedar"];
    c.store
        .append(&ids.map(|i| judgment_row(i, "security.v3")))
        .unwrap();
    noise(&c, ids[0]);
    noise(&c, ids[1]);
    assert_eq!(
        c.runner.judge.ladder().standing("security.v3").rung,
        Rung::Live
    );
    drop(c);
    let c = core_at(dir.path(), None, |_| {});
    c.runner.judge.ladder().wire("security.v3", PackMode::Live);
    noise(&c, ids[2]);
    // The third trips security-notices' own brake first (v3 is live): its
    // pause, read by its key, holds the pack until midnight, so the ladder's
    // own rule finds nothing acting to roll back.
    let s = c.runner.judge.ladder().standing("security.v3");
    assert_eq!(
        (s.rung, s.rule.as_deref()),
        (Rung::RolledBack, Some("notices"))
    );
    let line = c
        .runner
        .judge
        .health()
        .packs
        .into_iter()
        .find(|l| l.starts_with("security.v3"))
        .unwrap();
    assert!(
        line.starts_with("security.v3: rolled back until ") && line.ends_with("(notices)"),
        "{line}"
    );
}

/// A day's brake lapses at the next local midnight, on tokio's paused
/// clock: the pack stands live again, with nothing written.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_brake_lapses_at_midnight() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    let l = c.runner.judge.ladder();
    let base = theseus_protocol::now_unix_ms();
    let start = tokio::time::Instant::now();
    l.set_clock(Arc::new(move || base + start.elapsed().as_millis() as u64));
    l.wire("rerank.v1", PackMode::Live);
    let day = c.runner.judge.today();
    for _ in 0..2 {
        c.runner
            .judge
            .land("rerank.v1", CanaryEvent::BreakerOpened { day: day.clone() });
    }
    let s = l.standing("rerank.v1");
    assert_eq!(s.rung, Rung::RolledBack);
    let until = s.until_ms.unwrap();
    let written = rows(&c, "rerank.v1").len();
    tokio::time::advance(std::time::Duration::from_millis(until - l.now() - 1)).await;
    assert_eq!(
        l.standing("rerank.v1").rung,
        Rung::RolledBack,
        "a millisecond before"
    );
    tokio::time::advance(std::time::Duration::from_millis(1)).await;
    assert_eq!(
        l.standing("rerank.v1").rung,
        Rung::Live,
        "lapsed at midnight"
    );
    assert_eq!(
        c.runner.judge.mode_for("rerank.v1", "ses_a").mode,
        PackMode::Live
    );
    assert_eq!(rows(&c, "rerank.v1").len(), written, "nothing written");
    // The new day's count starts at none: one open is a near miss.
    c.runner.judge.land(
        "rerank.v1",
        CanaryEvent::BreakerOpened {
            day: c.runner.judge.today(),
        },
    );
    assert_eq!(l.standing("rerank.v1").rung, Rung::Live);
}

/// `max_mode = "shadow"` caps every pack after a restart: the ladder still
/// says what it gave, and health says the config's ceiling holds it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_mode_shadow_caps_every_pack_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    c.pack_promote(&promote("loop.v1", "canary", Some(1.0)), "cli")
        .unwrap();
    c.pack_promote(&promote("classify.v1", "live", None), "cli")
        .unwrap();
    drop(c);
    let c = core_at(dir.path(), None, |cfg| {
        cfg.judge.max_mode = PackMode::Shadow
    });
    c.warm_ladder();
    let j = &c.runner.judge;
    for (p, _) in crate::judge::WIRED {
        let g = j.mode_for(p, "ses_a");
        assert_eq!(g.mode, PackMode::Shadow, "{p}");
    }
    while !j.ladder().is_loaded() {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let lines = j.health().packs;
    assert!(lines.iter().all(|l| l.contains(": shadow")), "{lines:#?}");
    assert!(lines.contains(&
        "loop.v1: shadow (the config's ceiling; on the ladder: canary 1.0 (owner: forced by the owner))".to_string()),
        "{lines:#?}"
    );
}

/// A canary at 1.0 judges every session in its canary arm: the judgment's
/// row says `canary` and records its arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_canary_judgment_records_its_arm() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "work_state",
        Jev::Choice {
            option: "complete".into(),
            confidence: 0.95,
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), Some(&jev), |cfg| {
        for p in crate::judge::inbound::PACKS {
            cfg.judge.packs.insert(p.into(), crate::tests_judge::off());
        }
    });
    c.pack_promote(&promote("loop.v1", "canary", Some(1.0)), "cli")
        .unwrap();
    let res = turn(&c, None, "Say done.").await;
    let judged = until_judged(&c.store, 1).await;
    let d = &judged[0].1.data;
    assert_eq!(d["mode"], "canary", "{d}");
    assert_eq!(d["context"]["pack_arm"], "canary", "{d}");
    assert_eq!(
        d["context"]["session"].as_str(),
        Some(res.session_id.as_str())
    );
    let _: Value = d.clone();
}

/// The three packs the owner put live before the ladder (the route pack,
/// rerank.v1, and security.v3's notices: this build wires each live) are
/// adopted at the ladder's first read after serving, one row each, `live`,
/// `who: owner`, "decision of 2026-10-04" (route.v2's and route.v3's "of 2026-10-07",
/// theseus-3okf, theseus-qe3v), through adoption, and health says so; a restart writes none
/// again (batch 5's join, theseus-9j7x).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_three_live_packs_are_adopted_once_after_serving() {
    let dir = tempfile::tempdir().unwrap();
    let live = ["route.v3", "rerank.v1", "security.v3"];
    let why = |p: &str| match p {
        "route.v3" => "decision of 2026-10-07",
        _ => "decision of 2026-10-04",
    };
    let c = core_at(dir.path(), None, |_| {});
    for p in live {
        assert_eq!(c.runner.judge.ladder().wired(p), PackMode::Live, "{p}");
        assert!(rows(&c, p).is_empty(), "{p}: nothing at the build");
    }
    c.warm_ladder();
    let t0 = std::time::Instant::now();
    while live.iter().any(|p| rows(&c, p).is_empty()) {
        assert!(t0.elapsed() < std::time::Duration::from_secs(10));
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let lines = c.runner.judge.health().packs;
    for p in live {
        let r = rows(&c, p);
        assert_eq!(r.len(), 1, "{p}: {r:?}");
        assert_eq!(
            (
                r[0].mode.as_str(),
                r[0].who.as_str(),
                r[0].why.as_str(),
                r[0].via.as_str()
            ),
            ("live", "owner", why(p), "adoption"),
            "{p}"
        );
        let line = format!("{p}: live (owner: {})", why(p));
        assert!(lines.contains(&line), "{line}: {lines:#?}");
    }
    drop(c);
    let c = core_at(dir.path(), None, |_| {});
    c.warm_ladder();
    let t0 = std::time::Instant::now();
    while !c.runner.judge.ladder().is_loaded() {
        assert!(t0.elapsed() < std::time::Duration::from_secs(10));
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    for p in live {
        assert_eq!(rows(&c, p).len(), 1, "{p}: a restart adds none");
        assert_eq!(c.runner.judge.ladder().standing(p).rung, Rung::Live, "{p}");
    }
}

/// security-notices' own brake, its `judge.paused` row (keyed by its day,
/// scoped `judge:security` among every security judgment), is read by its
/// key: a row of the same shape under no key is not it, the keyed one holds
/// security.v3 rolled back until the next local midnight (acting as shadow,
/// health saying so), and the next local day it is live again, with nothing
/// written (batch 5's join, theseus-9j7x).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_notices_brake_is_read_by_its_key_and_lapses_at_midnight() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    let j = &c.runner.judge;
    let l = j.ladder();
    let base = theseus_protocol::now_unix_ms();
    l.set_clock(Arc::new(move || base));
    let day = j.today();
    let paused = |key: Option<String>| {
        let f = crate::fact::judge::JudgeNoticesPaused {
            pack: "security.v3",
            day: &day,
            until: "tomorrow",
            rule: "labels_per_day",
            why: "the operator labeled 3 judgments noise",
            short: "3 labeled noise today",
        };
        let mut r = crate::fact::row(&f, None, None).unwrap();
        r.key = key;
        r.scoped(crate::judge::notice::SCOPE)
    };
    // A row of the brake's shape under no key, among the judgments: not it.
    c.store
        .append(&[judgment_row("jdg_alder", "security.v3"), paused(None)])
        .unwrap();
    assert_eq!(
        l.standing("security.v3").rung,
        Rung::Live,
        "no key, no brake"
    );
    // The brake as security-notices writes it, read again as its pause does.
    let key = crate::judge::notice::paused_key(&day);
    c.store.append(&[paused(Some(key))]).unwrap();
    l.reload();
    let s = l.standing("security.v3");
    assert_eq!(
        (s.rung, s.rule.as_deref(), s.until_ms),
        (
            Rung::RolledBack,
            Some("notices"),
            Some(ladder::rules::next_midnight(base))
        )
    );
    assert_eq!(j.mode_for("security.v3", "ses_a").mode, PackMode::Shadow);
    let line = j
        .health()
        .packs
        .into_iter()
        .find(|l| l.starts_with("security.v3"))
        .unwrap();
    assert!(
        line.starts_with("security.v3: rolled back until ") && line.ends_with("(notices)"),
        "{line}"
    );
    let written = rows(&c, "security.v3").len();
    // The next local day: the brake has lapsed.
    let next = ladder::rules::next_midnight(base) + 60_000;
    l.set_clock(Arc::new(move || next));
    assert_eq!(l.standing("security.v3").rung, Rung::Live, "lapsed");
    assert_eq!(j.mode_for("security.v3", "ses_a").mode, PackMode::Live);
    assert_eq!(rows(&c, "security.v3").len(), written, "nothing written");
}

/// A promotion of loop.v1 while a learned version stands in its place
/// (theseus-nwa5): every point asks `placed` first, so loop.v1's canary
/// judges in no session while loop.v101 stands in shadow or live, and only
/// in loop.v101's control arm while it is a canary. The answer says so, with
/// the way out; the move is written all the same. With loop.v101 not placed
/// (no row, only a declined one, or rolled back), no such sentence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_promotion_names_the_learned_version_standing_in_its_place() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path(), None, |_| {});
    crate::tests_prove::learned_loop(&c);
    let l = c.runner.judge.ladder();
    let said = |c: &Core| {
        let r = c
            .pack_promote(&promote("loop.v1", "canary", Some(0.5)), "cli")
            .unwrap();
        assert_eq!(
            r.row.as_ref().unwrap().mode,
            "canary",
            "written all the same"
        );
        r.said
    };
    let placed = |mode: &str, share: Option<f64>, declined: bool| PackModeRow {
        pack: "loop.v101".into(),
        mode: mode.into(),
        from: "off".into(),
        share,
        who: "system".into(),
        why: "learned (prp_heron): the ladder's test".into(),
        declined,
        ..PackModeRow::default()
    };
    let plain = "loop.v1 is canary 0.5, forced short of the bar (";
    // No row: loop.v101 stands nowhere.
    let s = said(&c);
    assert!(s.starts_with(plain) && !s.contains("stands in"), "{s}");
    // Only a declined row places nothing.
    l.write(placed("shadow", None, true)).unwrap();
    let s = said(&c);
    assert!(!s.contains("stands in"), "{s}");
    // In shadow: nowhere.
    l.write(placed("shadow", None, false)).unwrap();
    let s = said(&c);
    assert!(
        s.ends_with(
            ". loop.v101 stands in loop.v1's place in shadow, so loop.v1's canary 0.5 judges in \
             no session until loop.v101 moves (`theseus packs rollback loop.v101`)."
        ),
        "{s}"
    );
    // A canary: only its control arm.
    l.write(placed("canary", Some(0.3), false)).unwrap();
    let s = said(&c);
    assert!(
        s.ends_with(
            ". loop.v101 stands in loop.v1's place as canary 0.3, so loop.v1's canary 0.5 judges \
             only in loop.v101's control arm until loop.v101 moves (`theseus packs rollback \
             loop.v101`)."
        ),
        "{s}"
    );
    // Live: nowhere, and a move to live says the same.
    l.write(placed("live", None, false)).unwrap();
    let s = c
        .pack_promote(&promote("loop.v1", "live", None), "cli")
        .unwrap()
        .said;
    assert!(
        s.ends_with(
            ". loop.v101 stands in loop.v1's place live, so loop.v1 live judges in no session \
             until loop.v101 moves (`theseus packs rollback loop.v101`)."
        ),
        "{s}"
    );
    // Rolled back: the place is loop.v1's again.
    c.pack_rollback(
        &PackRollbackParams {
            pack: "loop.v101".into(),
            ..PackRollbackParams::default()
        },
        "cli",
    )
    .unwrap();
    let s = said(&c);
    assert!(s.starts_with(plain) && !s.contains("stands in"), "{s}");
    // The learned version's own move names nothing ahead of it.
    let s = c
        .pack_promote(&promote("loop.v101", "shadow", None), "cli")
        .unwrap()
        .said;
    assert!(!s.contains("stands in"), "{s}");
}
