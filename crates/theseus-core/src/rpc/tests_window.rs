//! `ledger.window` (theseus-m2tiu): the postmortem's read answers only the
//! kinds it words, scrubbed, narrowed to a session and its tasks, for a
//! surface that may read them, and writes nothing.

use serde_json::json;
use theseus_protocol::{LedgerWindowParams, SessionOpenParams, POSTMORTEM_KINDS};

use super::tests::test_core;
use super::Core;
use crate::approval::Surface;

fn open(c: &Core) -> String {
    c.open_session(SessionOpenParams::default())
        .unwrap()
        .session_id
}

fn put(c: &Core, at: u64, kind: &str, session: Option<&str>, data: serde_json::Value) {
    let mut row = json!({"at_unix_ms": at, "kind": kind, "data": data});
    if let Some(s) = session {
        row["session_id"] = json!(s);
    }
    c.store.append_ledger(&row).unwrap();
}

fn kinds(r: &theseus_protocol::LedgerWindowResult) -> Vec<&str> {
    r.rows.iter().map(|x| x.kind.as_str()).collect()
}

/// The window holds the postmortem's kinds and no others, inside its times.
#[test]
fn the_window_holds_the_postmortems_kinds_inside_its_times() {
    let c = test_core("ok");
    let s = open(&c);
    // The index counts a row at the time it was written; the window's own
    // times then decide by each row's `at_unix_ms`.
    let t = theseus_protocol::now_unix_ms();
    put(&c, t - 5_000, "turn.started", Some(&s), json!({"i": 0}));
    put(&c, t - 4_000, "turn.started", Some(&s), json!({"i": 1}));
    put(
        &c,
        t - 3_900,
        "context.compiled",
        Some(&s),
        json!({"big": "no one reads"}),
    );
    put(&c, t - 3_800, "turn.ended", Some(&s), json!({"i": 2}));
    put(&c, t + 700_000, "turn.started", Some(&s), json!({"i": 3}));
    let r = c
        .ledger_window(
            LedgerWindowParams {
                since_ms: t - 4_500,
                until_ms: Some(t + 600_000),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap();
    let ours: Vec<&str> = r
        .rows
        .iter()
        .filter(|x| x.session_id.is_some() && x.kind.starts_with("turn."))
        .map(|x| x.kind.as_str())
        .collect();
    assert!(!r.building, "built");
    let idx: Vec<u64> = r.rows.iter().filter_map(|x| x.data["i"].as_u64()).collect();
    assert_eq!(idx, [1, 2], "the rows inside the window's own times");
    assert_eq!(ours, ["turn.started", "turn.ended"], "{:?}", kinds(&r));
    assert!(!kinds(&r).contains(&"context.compiled"));
    assert!(r.next.is_none() && !r.shared && r.scope.is_empty());
    assert!(POSTMORTEM_KINDS.contains(&"turn.started"));
}

/// A page ends where it was cut, and the next starts after that row: the
/// pages together are the window, each row once.
#[test]
fn pages_walk_the_window_each_row_once() {
    let c = test_core("ok");
    let s = open(&c);
    let t = theseus_protocol::now_unix_ms();
    for i in 0..25u64 {
        put(&c, t + i, "turn.started", Some(&s), json!({"i": i}));
        put(&c, t + i, "turn.trace", Some(&s), json!({}));
    }
    let mut seen = Vec::new();
    let mut after = None;
    loop {
        let r = c
            .ledger_window(
                LedgerWindowParams {
                    since_ms: 0,
                    after,
                    limit: Some(7),
                    ..Default::default()
                },
                Surface::Cli,
            )
            .unwrap();
        seen.extend(r.rows.iter().filter_map(|x| x.data["i"].as_u64()));
        match r.next {
            Some(n) => after = Some(n),
            None => break,
        }
    }
    assert_eq!(seen, (0..25).collect::<Vec<_>>());
}

/// Every string of a row passes the scrubber, as a tool's output does.
#[test]
fn a_value_the_vault_holds_is_scrubbed() {
    let c = test_core_knowing(
        "vault-held-ghp-abcdefghijklmnopqrstuvwxyz0123456789",
        "gh_token",
    );
    let s = open(&c);
    put(
        &c,
        1_000,
        "tool.job_started",
        Some(&s),
        json!({"argv": ["curl", "-H", "Authorization: vault-held-ghp-abcdefghijklmnopqrstuvwxyz0123456789"]}),
    );
    let r = c
        .ledger_window(
            LedgerWindowParams {
                since_ms: 0,
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap();
    let text = r
        .rows
        .iter()
        .find(|x| x.kind == "tool.job_started")
        .unwrap()
        .data
        .to_string();
    assert!(!text.contains("vault-held-ghp-abcdef"), "{text}");
    assert!(text.contains("[redacted:gh_token]"), "{text}");
}

/// A core whose scrubber knows `value` as the vault's secret `name`.
fn test_core_knowing(value: &str, name: &str) -> std::sync::Arc<Core> {
    use super::Parts;
    use crate::provider::FakeProvider;
    let dir = crate::scratch::dir("t");
    let store = crate::store::Store::open(&dir.join("store")).unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    Core::build(Parts {
        scrubber: std::sync::Arc::new(crate::scrub::Scrubber::with_values(vec![(
            value.into(),
            name.into(),
        )])),
        ..Parts::for_tests(cfg, std::sync::Arc::new(FakeProvider::default()), store)
    })
    .unwrap()
}

/// A session narrows the window to its own rows and the daemon's.
#[test]
fn a_session_narrows_the_window_to_its_rows_and_the_daemons() {
    let c = test_core("ok");
    let (a, b) = (open(&c), open(&c));
    put(&c, 1_000, "server.started", None, json!({}));
    put(&c, 1_001, "turn.started", Some(&a), json!({}));
    put(&c, 1_002, "turn.started", Some(&b), json!({}));
    put(&c, 1_003, "turn.ended", Some(&a), json!({}));
    let r = c
        .ledger_window(
            LedgerWindowParams {
                since_ms: 0,
                session_id: Some(a.clone()),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap();
    let ours: Vec<&str> = r
        .rows
        .iter()
        .filter(|x| x.session_id.is_some() && x.kind.starts_with("turn."))
        .map(|x| x.kind.as_str())
        .collect();
    assert_eq!(ours, ["turn.started", "turn.ended"]);
    assert!(
        kinds(&r).contains(&"server.started"),
        "the daemon's rows stay"
    );
    assert!(r
        .rows
        .iter()
        .all(|x| x.session_id.as_deref().is_none_or(|s| s == a)));
    assert_eq!(r.scope, std::slice::from_ref(&a));
    let err = c
        .ledger_window(
            LedgerWindowParams {
                since_ms: 0,
                session_id: Some("ses_nobody".into()),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap_err();
    assert!(err.message.contains("no session"), "{}", err.message);
}

/// A surface that reads no private text must name the session, and then
/// sees that session's rows alone.
#[test]
fn a_shared_place_sees_only_its_own_session() {
    let c = test_core("ok");
    let (mine, other) = (open(&c), open(&c));
    put(&c, 1_001, "turn.started", Some(&mine), json!({}));
    put(
        &c,
        1_002,
        "turn.started",
        Some(&other),
        json!({"author": "someone else's"}),
    );
    let all = LedgerWindowParams {
        since_ms: 0,
        ..Default::default()
    };
    let e = c.ledger_window(all, Surface::Discord).unwrap_err();
    assert!(e.message.contains("name the session"), "{}", e.message);
    let r = c
        .ledger_window(
            LedgerWindowParams {
                since_ms: 0,
                session_id: Some(mine.clone()),
                ..Default::default()
            },
            Surface::Discord,
        )
        .unwrap();
    assert!(r.shared);
    let sessions: Vec<&str> = r
        .rows
        .iter()
        .filter_map(|x| x.session_id.as_deref())
        .collect();
    assert!(
        !sessions.is_empty() && sessions.iter().all(|s| *s == mine),
        "{sessions:?}"
    );
    assert!(!serde_json::to_string(&r).unwrap().contains("someone else"));
}

/// The read writes no frame: the ledger and the log end where they ended.
#[test]
fn the_read_writes_no_frame() {
    let c = test_core("ok");
    let s = open(&c);
    put(&c, 1_000, "turn.started", Some(&s), json!({}));
    let before = (c.store.last_position(), c.store.ledger_len().unwrap());
    for surface in [Surface::Cli, Surface::Discord] {
        c.ledger_window(
            LedgerWindowParams {
                since_ms: 0,
                session_id: Some(s.clone()),
                ..Default::default()
            },
            surface,
        )
        .unwrap();
    }
    assert_eq!(
        (c.store.last_position(), c.store.ledger_len().unwrap()),
        before
    );
}

/// A busy day's window pages in well under 2 s: rows of kinds no report
/// reads are a share of the ledger that costs nothing, since the read goes by
/// the kinds' own postings, and each page costs about its answer.
#[test]
fn a_busy_days_window_pages_in_under_two_seconds() {
    let c = test_core("ok");
    let s = open(&c);
    let t = theseus_protocol::now_unix_ms();
    let mut wanted = 0;
    let setup = std::time::Instant::now();
    let mut batch = Vec::new();
    for i in 0..12_000u64 {
        let (kind, data) = match i % 4 {
            0 => ("turn.started", json!({"author": "owner"})),
            1 => ("context.compiled", json!({"digest": "x".repeat(200)})),
            2 => ("loop.started", json!({"loop": i})),
            _ => (
                "provider.call",
                json!({"model": "sonnet", "cost_usd": 0.001}),
            ),
        };
        wanted += usize::from(i % 4 == 0 || i % 4 == 3);
        let row =
            json!({"at_unix_ms": t - 3_600_000 + i, "kind": kind, "data": data, "session_id": s});
        batch.push(
            theseus_store::NewRecord::json(theseus_store::kinds::LEDGER, None, &row).unwrap(),
        );
        // A frame a thousand rows: the window's read is what is timed, not 12,000 synced appends.
        if batch.len() == 1000 {
            c.store.append(&std::mem::take(&mut batch)).unwrap();
        }
    }
    c.store.append(&batch).unwrap();
    eprintln!(
        "ledger.window: the day's rows written in {:?}",
        setup.elapsed()
    );
    let started = std::time::Instant::now();
    let (mut rows, mut pages, mut after) = (0, 0, None);
    loop {
        let r = c
            .ledger_window(
                LedgerWindowParams {
                    since_ms: t - 86_400_000,
                    after,
                    ..Default::default()
                },
                Surface::Cli,
            )
            .unwrap();
        rows += r
            .rows
            .iter()
            .filter(|x| x.session_id.is_some() && x.kind != "execution.opened")
            .count();
        pages += 1;
        match r.next {
            Some(n) => after = Some(n),
            None => break,
        }
    }
    let took = started.elapsed();
    assert_eq!(rows, wanted);
    assert!(pages >= 12, "{pages} pages");
    assert!(
        took < std::time::Duration::from_secs(2),
        "{rows} rows took {took:?}"
    );
    eprintln!("ledger.window: {rows} rows in {pages} pages took {took:?}");
}
