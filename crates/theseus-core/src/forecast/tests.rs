//! The forecast on synthetic histories (theseus-zz752): invented tasks with
//! known costs and times give the percentiles worked out by hand, a class is
//! never crossed, the nearest brief fills the 25, fewer than five say so, and
//! the calibration counts what a backtest holds.

use super::*;

const HOUR: u64 = 3_600_000;

fn row(id: usize, class: &str, words: &str, usd: f64, min: f64, ended_ms: u64) -> Row {
    Row {
        task: format!("tsk_{id:06}"),
        class: class.into(),
        terms: terms_of(words, ""),
        usd,
        min,
        loops: None,
        started_ms: ended_ms.saturating_sub((min * 60_000.0) as u64),
        ended_ms,
    }
}

fn query(class: &str, words: &str) -> Query {
    Query {
        class: class.into(),
        terms: terms_of(words, ""),
    }
}

fn some(o: Outcome) -> Forecast {
    o.forecast()
        .unwrap_or_else(|| panic!("a forecast, not {o:?}"))
}

/// 40 tasks in two classes: each class gets the percentiles of its own 20,
/// by linear interpolation between ranks (p50 at rank 9.5 of 0 to 19, p90 at
/// 17.1).
#[test]
fn two_classes_get_their_own_percentiles() {
    let mut t = Table::default();
    for i in 1..=20usize {
        let at = i as u64 * HOUR;
        // `open`: $0.10 to $2.00, 1 to 20 minutes.
        t.add(row(
            i,
            "open",
            "migrate the fixtures",
            i as f64 / 10.0,
            i as f64,
            at,
        ));
        // `specified`: $10 to $200, 10 to 200 minutes.
        t.add(row(
            100 + i,
            "specified",
            "migrate the fixtures",
            i as f64 * 10.0,
            i as f64 * 10.0,
            at,
        ));
    }
    assert_eq!(t.len(), 40);
    let open = some(t.forecast(&query("open", "migrate the fixtures")));
    assert_eq!(open.n, 20);
    assert!((open.usd_p50 - 1.05).abs() < 1e-9, "{open:?}");
    assert!((open.usd_p90 - 1.81).abs() < 1e-9, "{open:?}");
    assert!((open.min_p50 - 10.5).abs() < 1e-9, "{open:?}");
    assert!((open.min_p90 - 18.1).abs() < 1e-9, "{open:?}");
    let big = some(t.forecast(&query("specified", "migrate the fixtures")));
    assert_eq!(big.n, 20);
    assert!((big.usd_p50 - 105.0).abs() < 1e-9, "{big:?}");
    assert!((big.usd_p90 - 181.0).abs() < 1e-9, "{big:?}");
    assert!((big.min_p90 - 181.0).abs() < 1e-9, "{big:?}");
    assert_eq!(open.loops_p50, None, "no row counted loops");
}

/// Fewer than five like it is no forecast, and says how many were.
#[test]
fn fewer_than_five_like_it_is_no_forecast() {
    let mut t = Table::default();
    for i in 0..4 {
        t.add(row(
            i,
            "open",
            "tidy the docs",
            1.0,
            2.0,
            (i as u64 + 1) * HOUR,
        ));
    }
    // A fifth, of another class, is not like it.
    t.add(row(9, "checked", "tidy the docs", 1.0, 2.0, 9 * HOUR));
    let o = t.forecast(&query("open", "tidy the docs"));
    assert_eq!(o, Outcome::Few(4));
    assert_eq!(o.words(), "no forecast yet: 4 like it");
    t.add(row(5, "open", "tidy the docs", 1.0, 2.0, 5 * HOUR));
    assert_eq!(some(t.forecast(&query("open", "tidy the docs"))).n, 5);
}

/// A task's class comes first: however well a brief matches, the tasks of
/// another class are not like it. (The planted revert sorts every class by
/// the brief alone.)
#[test]
fn the_class_comes_before_the_brief() {
    let mut t = Table::default();
    for i in 0..10 {
        // The class asked for: unrelated words, $1.
        t.add(row(
            i,
            "open",
            "rotate the log files",
            1.0,
            3.0,
            (i as u64 + 1) * HOUR,
        ));
        // Another class: the very words asked for, $50.
        t.add(row(
            50 + i,
            "specified",
            "audit the billing exports",
            50.0,
            90.0,
            (i as u64 + 1) * HOUR,
        ));
    }
    let f = some(t.forecast(&query("open", "audit the billing exports")));
    assert_eq!(f.n, 10, "only its own class");
    assert!((f.usd_p90 - 1.0).abs() < 1e-9, "{f:?}");
}

/// Past 25 like it, the nearest briefs are the ones kept, not the newest.
#[test]
fn the_nearest_briefs_fill_the_twenty_five() {
    let mut t = Table::default();
    // Five near ones, oldest, at $100; twenty-five far ones, newer, at $1.
    for i in 0..5 {
        t.add(row(
            i,
            "open",
            "upgrade the parser crate",
            100.0,
            30.0,
            (i as u64 + 1) * HOUR,
        ));
    }
    for i in 0..25 {
        t.add(row(
            10 + i,
            "open",
            "write the weekly summary",
            1.0,
            2.0,
            (i as u64 + 10) * HOUR,
        ));
    }
    let f = some(t.forecast(&query("open", "upgrade the parser crate")));
    assert_eq!(f.n, 25);
    // 20 far at $1 and 5 near at $100: the p90 is in the near ones.
    assert!((f.usd_p90 - 100.0).abs() < 1e-9, "{f:?}");
    assert!((f.usd_p50 - 1.0).abs() < 1e-9, "{f:?}");
}

/// The table holds the newest 2,000 and a task once.
#[test]
fn the_table_keeps_the_newest_two_thousand_once_each() {
    let mut t = Table::default();
    for i in 0..(KEEP + 50) {
        t.add(row(i, "open", "x y z", 1.0, 1.0, (i as u64 + 1) * 1000));
    }
    assert_eq!(t.len(), KEEP);
    t.add(row(KEEP + 49, "open", "x y z", 9.0, 9.0, 1));
    assert_eq!(t.len(), KEEP, "the same task is not a second row");
    assert!(
        t.rows.iter().all(|r| r.ended_ms > 50 * 1000),
        "the oldest left"
    );
}

/// The calibration is a backtest: each finished task is judged against the
/// forecast the tasks before it would have given, by class and window.
#[test]
fn the_calibration_counts_what_a_backtest_holds() {
    let day = 24 * HOUR;
    let now = 100 * day;
    let mut t = Table::default();
    // Ten history tasks, 40 to 35 days back: $1 and 5 minutes each.
    for i in 0..10 {
        t.add(row(
            i,
            "open",
            "tidy the docs",
            1.0,
            5.0,
            now - (40 - i as u64) * day,
        ));
    }
    // Three that finished in the last week: two inside, one costly.
    t.add(row(20, "open", "tidy the docs", 1.0, 5.0, now - 3 * day));
    t.add(row(21, "open", "tidy the docs", 0.5, 4.0, now - 2 * day));
    t.add(row(22, "open", "tidy the docs", 9.0, 5.0, now - day));
    // One in the last month, a class with no history of its own.
    t.add(row(
        30,
        "specified",
        "tidy the docs",
        1.0,
        5.0,
        now - 20 * day,
    ));
    let w = t.calibrate(now, &[7, 30]);
    let open7 = &w[0].classes[0];
    assert_eq!((w[0].days, open7.class.as_str()), (7, "open"));
    assert_eq!(
        (open7.finished, open7.forecast, open7.inside),
        (3, 3, 2),
        "{w:?}"
    );
    assert_eq!(
        w[0].classes.len(),
        1,
        "the specified task is older than a week"
    );
    let by_class = |days: usize, c: &str| {
        w[days]
            .classes
            .iter()
            .find(|x| x.class == c)
            .unwrap()
            .clone()
    };
    assert_eq!(by_class(1, "open").finished, 3);
    let spec = by_class(1, "specified");
    assert_eq!(
        (spec.finished, spec.forecast, spec.inside),
        (1, 0, 0),
        "no history, no forecast"
    );
}

/// The book says nothing until the store has been read, then says a task's
/// forecast once, however the table moves.
#[test]
fn the_book_waits_for_the_store_and_says_a_forecast_once() {
    let book = Book::default();
    let rec = theseus_protocol::tasks::TaskRecord {
        id: "tsk_aaaaaa".into(),
        title: "tidy the docs".into(),
        ..Default::default()
    };
    assert_eq!(book.for_task(&rec), Outcome::Loading);
    book.loaded(
        (0..6)
            .map(|i| row(i, "open", "tidy the docs", 2.0, 4.0, (i as u64 + 1) * HOUR))
            .collect(),
    );
    let first = some(book.for_task(&rec));
    assert_eq!(first.n, 6);
    book.ended(row(90, "open", "tidy the docs", 99.0, 99.0, 99 * HOUR));
    assert_eq!(some(book.for_task(&rec)), first, "the open's numbers stand");
    assert_eq!(book.len(), 7);
}

/// A word is a term once, case and short words aside.
#[test]
fn briefs_share_words_not_filler() {
    let a = terms_of("Migrate the fixtures", "to the new schema");
    let b = terms_of("migrate FIXTURES", "");
    assert_eq!(jaccard(&b, &a), 2.0 / 4.0);
    assert_eq!(jaccard(&[], &a), 0.0);
}

/// The forecast at a task's open is a memory read: at the table's full 2,000
/// rows the median of 50 asks is under 1 ms in a release build (the bound is
/// looser in a debug one, which runs the same scan unoptimized). Recorded:
/// `cargo nextest run --release -E 'test(a_forecast_is_a_memory_read)' --no-capture`.
#[test]
fn a_forecast_is_a_memory_read() {
    let mut t = Table::default();
    for i in 0..KEEP {
        let words = format!(
            "migrate fixtures batch{} service{} crate{}",
            i % 17,
            i % 5,
            i % 31
        );
        t.add(row(
            i,
            "open",
            &words,
            0.1 + (i % 40) as f64 / 10.0,
            1.0 + (i % 25) as f64,
            (i as u64 + 1) * 1000,
        ));
    }
    let q = query("open", "migrate the fixtures of service3 crate7");
    let mut ms: Vec<f64> = (0..50)
        .map(|_| {
            let t0 = std::time::Instant::now();
            assert!(t.forecast(&q).forecast().is_some());
            t0.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = ms[25];
    eprintln!(
        "forecast over {KEEP} rows: median {median:.3} ms, worst {:.3} ms",
        ms[49]
    );
    let bound = if cfg!(debug_assertions) { 25.0 } else { 1.0 };
    assert!(median < bound, "median {median:.3} ms over {bound} ms");
}
