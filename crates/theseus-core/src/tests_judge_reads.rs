//! The judge's reads that once grew with all of history, each held to the
//! read it replaced over the same store, with the records each decodes
//! (theseus-wse2, theseus-b8e2): `judge.list` paged back from the newest
//! `judge.call` row, and the notices' brake read from today's rows.

use serde_json::{json, Value};
use theseus_protocol::judge::JudgeListParams;
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::ledger::LedgerRow;
use crate::rpc::Core;
use crate::tests_judge::{rig_on, Rig};

const DAY: u64 = 24 * 3600 * 1000;

fn rig() -> Rig {
    rig_on(Vec::new(), None, |_| {})
}

/// A ledger row of `kind` in `scope`, at `at_ms`, keyed `key`.
fn row(
    kind: LedgerKind,
    scope: &str,
    session: Option<&str>,
    data: Value,
    at_ms: u64,
    key: &str,
) -> NewRecord {
    let mut row = LedgerRow::new(kind, session, None, data);
    row.at_unix_ms = at_ms;
    let mut r = NewRecord::json(kinds::LEDGER, None, &row).unwrap();
    r.key = Some(key.to_string());
    r.scoped(scope)
}

/// `n` judgments over `days` days up to `now`, oldest first, across packs,
/// versions and sessions, each pack's scope also holding a label row; and
/// a judgment in a scope no embedded pack has, which no listing without
/// that pack reads.
fn judgments(core: &Core, n: usize, days: u64, now: u64) {
    const PACKS: [&str; 5] = [
        "loop.v1",
        "security.v1",
        "security.v3",
        "classify.v1",
        "route.v2",
    ];
    const SESSIONS: [&str; 3] = ["ses_heron", "ses_otter", "ses_wren"];
    let step = days * DAY / n as u64;
    let mut frame = Vec::new();
    for i in 0..n {
        let pack = PACKS[i % PACKS.len()];
        let session = SESSIONS[(i / 2) % SESSIONS.len()];
        let at = now - days * DAY + i as u64 * step;
        let id = format!("jdg_{i:05}");
        let scope = crate::rpc::judge::scope_of(pack);
        let data = json!({"id": id, "pack": pack, "context": {"session": session}});
        frame.push(row(
            LedgerKind::JudgeCall,
            &scope,
            Some(session),
            data,
            at,
            &id,
        ));
        if i % 7 == 0 {
            let data = json!({"id": format!("lbl_{i}"), "judgment": id, "pack": pack,
                "label": "right", "source": "operator"});
            frame.push(row(
                LedgerKind::JudgeLabel,
                &scope,
                None,
                data,
                at,
                &format!("lbl_{i}"),
            ));
        }
        if i == n / 2 {
            let data = json!({"id": "jdg_gone", "pack": "gone.v1", "context": {}});
            frame.push(row(
                LedgerKind::JudgeCall,
                "judge:gone",
                Some(session),
                data,
                at,
                "jdg_gone",
            ));
        }
        if frame.len() >= 400 {
            core.store.append(&frame).unwrap();
            frame.clear();
        }
    }
    if !frame.is_empty() {
        core.store.append(&frame).unwrap();
    }
}

/// `judge.list` paged back from the newest `judge.call` row gives the
/// scan's judgments in the scan's order for every filter (none, a pack id,
/// a version, a session, `since`, a limit, and each together), and the
/// scan's `matched` wherever it read to the start; past the limit it says
/// `more`, and `matched` is the floor it proved, never above the scan's.
/// The default listing decodes about its limit, where the scan decoded
/// every row of every scope.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn judge_list_pages_from_the_newest_and_answers_as_the_scan_did() {
    let r = rig();
    let c = &r.core;
    let now = theseus_protocol::now_unix_ms();
    let n = 10_000;
    judgments(c, n, 30, now);
    let all = JudgeListParams::default();
    let with = |f: &dyn Fn(&mut JudgeListParams)| {
        let mut p = all.clone();
        f(&mut p);
        p
    };
    let asks: Vec<(&str, JudgeListParams)> = vec![
        ("none", all.clone()),
        ("pack id", with(&|p| p.pack = Some("security".into()))),
        ("version", with(&|p| p.pack = Some("security.v3".into()))),
        (
            "a pack nothing has",
            with(&|p| p.pack = Some("loop.v2".into())),
        ),
        ("a gone pack", with(&|p| p.pack = Some("gone".into()))),
        (
            "session",
            with(&|p| p.session_id = Some("ses_otter".into())),
        ),
        ("since", with(&|p| p.since = Some(now - 2 * DAY))),
        ("limit", with(&|p| p.limit = Some(7))),
        ("the most", with(&|p| p.limit = Some(500))),
        (
            "together",
            with(&|p| {
                p.pack = Some("loop.v1".into());
                p.session_id = Some("ses_wren".into());
                p.since = Some(now - 10 * DAY);
                p.limit = Some(20);
            }),
        ),
        (
            "fewer than the limit",
            with(&|p| {
                p.pack = Some("classify".into());
                p.since = Some(now - DAY);
            }),
        ),
    ];
    let mut table = Vec::new();
    for (what, p) in &asks {
        let (paged, cost) = c.judge_list_read(p, true).unwrap();
        let (scanned, before) = c.judge_list_read(p, false).unwrap();
        assert!(cost.paged, "{what}: the index's shape is built");
        let ids = |r: &theseus_protocol::judge::JudgeListResult| -> Vec<(u64, String)> {
            r.judgments
                .iter()
                .map(|e| (e.position, e.data["id"].as_str().unwrap().to_string()))
                .collect()
        };
        assert_eq!(
            ids(&paged),
            ids(&scanned),
            "{what}: the same judgments, in order"
        );
        assert_eq!(paged.scopes, scanned.scopes, "{what}");
        assert!(!scanned.more, "{what}: a scan counts them all");
        let limit = p.limit.unwrap_or(50) as usize;
        if scanned.matched as usize > limit {
            assert!(paged.more, "{what}: more matched than shown");
            assert_eq!(
                paged.matched as usize,
                limit + 1,
                "{what}: the floor it proved"
            );
        } else {
            assert!(!paged.more, "{what}");
            assert_eq!(paged.matched, scanned.matched, "{what}: exact");
        }
        table.push((*what, before.decoded, cost.decoded));
    }
    eprintln!("judge.list at {n} judgments over 30 days: records decoded, scan then page");
    for (what, before, after) in &table {
        eprintln!("  {what:<22} {before:>6} {after:>6}");
    }
    let decoded = |what: &str| table.iter().find(|t| t.0 == what).unwrap();
    // The default listing: the newest 50 and the one that proves more.
    assert_eq!(decoded("none").2, 51, "{table:?}");
    assert!(decoded("none").1 > n, "the scan read every scope whole");
    assert_eq!(decoded("limit").2, 8, "{table:?}");
    assert_eq!(decoded("session").2, 51, "the session's own tag: {table:?}");
}

/// A brake's row in `judge:security` (or `scope`), at `at_ms`.
fn security(kind: LedgerKind, data: Value, at_ms: u64, key: &str) -> NewRecord {
    row(
        kind,
        crate::judge::notice::SCOPE,
        Some("ses_heron"),
        data,
        at_ms,
        key,
    )
}

/// Thirty days of security judgments, notices and noise labels before
/// today, a pause on an earlier day, and today's own rows with their near
/// misses: a notice and a noise label a millisecond before midnight, a
/// policy's notice, a system's noise, a noise on v1, and a judge's notice
/// in another scope.
fn brake_history(core: &Core, now: u64) {
    use crate::judge::notice::paused_key;
    use crate::judge::spend::local_day;
    let midnight = crate::learning::local_midnight(now);
    let notice = |by: &str| json!({"by": by, "tool": "proc.run"});
    let noise = |pack: &str, source: &str| json!({"pack": pack, "label": "noise", "source": source, "judgment": "jdg_x"});
    let mut frame = Vec::new();
    for i in 0..5_000u64 {
        let at = midnight - 30 * DAY + i * (30 * DAY / 5_000);
        let id = format!("jdg_s{i}");
        frame.push(security(
            LedgerKind::JudgeCall,
            json!({"id": id, "pack": "security.v3"}),
            at,
            &id,
        ));
        if i % 20 == 0 {
            frame.push(security(
                LedgerKind::ToolNotified,
                notice("judge"),
                at,
                &format!("n{i}"),
            ));
        }
        if i % 50 == 0 {
            frame.push(security(
                LedgerKind::JudgeLabel,
                noise("security.v3", "operator"),
                at,
                &format!("l{i}"),
            ));
        }
        if frame.len() >= 400 {
            core.store.append(&frame).unwrap();
            frame.clear();
        }
    }
    let earlier = local_day(midnight - 3 * DAY);
    let paused = |day: &str| {
        json!({"what": "notices", "pack": "security.v3", "day": day, "until": "tomorrow",
            "rule": "labels_per_day", "why": "3 noise", "short": "3 labeled noise today"})
    };
    frame.push(security(
        LedgerKind::JudgePaused,
        paused(&earlier),
        midnight - 3 * DAY,
        &paused_key(&earlier),
    ));
    frame.extend(brake_today(now));
    core.store.append(&frame).unwrap();
}

/// Today's rows of `brake_history`: a pause, 4 notices and 2 noise labels
/// that count, and the near misses that don't.
fn brake_today(now: u64) -> Vec<NewRecord> {
    use crate::judge::notice::paused_key;
    use crate::judge::spend::local_day;
    let midnight = crate::learning::local_midnight(now);
    let notice = |by: &str| json!({"by": by, "tool": "proc.run"});
    let noise = |pack: &str, source: &str| json!({"pack": pack, "label": "noise", "source": source, "judgment": "jdg_x"});
    let paused = |day: &str| {
        json!({"what": "notices", "pack": "security.v3", "day": day, "until": "tomorrow",
            "rule": "labels_per_day", "why": "3 noise", "short": "3 labeled noise today"})
    };
    let mut frame = Vec::new();
    let today = local_day(now);
    frame.push(security(
        LedgerKind::JudgePaused,
        paused(&today),
        now,
        &paused_key(&today),
    ));
    for i in 0..4 {
        frame.push(security(
            LedgerKind::ToolNotified,
            notice("judge"),
            now,
            &format!("t{i}"),
        ));
    }
    for i in 0..2 {
        frame.push(security(
            LedgerKind::JudgeLabel,
            noise("security.v3", "operator"),
            now,
            &format!("tl{i}"),
        ));
    }
    frame.push(security(
        LedgerKind::ToolNotified,
        notice("judge"),
        midnight - 1,
        "just_before",
    ));
    frame.push(security(
        LedgerKind::JudgeLabel,
        noise("security.v3", "operator"),
        midnight - 1,
        "just_before_label",
    ));
    frame.push(security(
        LedgerKind::ToolNotified,
        notice("policy"),
        now,
        "by_policy",
    ));
    frame.push(security(
        LedgerKind::JudgeLabel,
        noise("security.v3", "system"),
        now,
        "by_system",
    ));
    frame.push(security(
        LedgerKind::JudgeLabel,
        noise("security.v1", "operator"),
        now,
        "on_v1",
    ));
    frame.push(row(
        LedgerKind::ToolNotified,
        "judge:loop",
        None,
        notice("judge"),
        now,
        "elsewhere",
    ));
    frame
}

/// The notices' brake reads its day from today's rows (theseus-b8e2): the
/// pause by its key, and today's notices and noise labels by a page from
/// the local midnight, the same day as the scan of `judge:security` gave,
/// with nothing before midnight counted; and it decodes the day's rows,
/// not the scope's judgments.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_brakes_day_is_todays_rows_as_the_scan_counted_them() {
    use crate::judge::notice::read_day;
    let r = rig();
    let c = &r.core;
    let now = theseus_protocol::now_unix_ms();
    brake_history(c, now);
    let today = crate::judge::spend::local_day(now);
    let (paged, cost) = read_day(&c.store, &today, now, true).unwrap();
    let (scanned, before) = read_day(&c.store, &today, now, false).unwrap();
    assert!(cost.paged);
    assert_eq!(paged, scanned, "the same day");
    assert_eq!((paged.notices, paged.noise), (4, 2), "{paged:?}");
    assert_eq!(
        paged.paused.as_ref().map(|p| p.day.as_str()),
        Some(today.as_str())
    );
    assert_eq!(cost.since_ms, Some(crate::learning::local_midnight(now)));
    eprintln!(
        "the brake's day at 5,000 security judgments over 30 days: decoded {} by the scan, {} by the page",
        before.decoded, cost.decoded
    );
    assert!(before.decoded > 5_000, "{before:?}");
    // Every notice and label row in the store has today's frame time (a
    // test can't set the store's clock), so the page reads them all; it
    // never decodes a judgment.
    assert!(cost.decoded < 500, "{cost:?}");
}

/// The page begins at the brake's local midnight: on the next day, nothing
/// written today (by the store's clock) is decoded at all, and the day is
/// the scan's, empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_brakes_page_begins_at_its_midnight() {
    use crate::judge::notice::read_day;
    let r = rig();
    let c = &r.core;
    let now = theseus_protocol::now_unix_ms();
    brake_history(c, now);
    let tomorrow = now + DAY;
    let day = crate::judge::spend::local_day(tomorrow);
    let (paged, cost) = read_day(&c.store, &day, tomorrow, true).unwrap();
    let (scanned, _) = read_day(&c.store, &day, tomorrow, false).unwrap();
    assert_eq!(paged, scanned);
    assert_eq!(
        (paged.notices, paged.noise, paged.paused.is_none()),
        (0, 0, true)
    );
    assert_eq!(
        cost.decoded, 0,
        "no row before the day's midnight is read: {cost:?}"
    );
}
