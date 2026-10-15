//! `[card]` (theseus-slvl.6): its defaults, what it refuses, and that it stays
//! out of a sparse note when it is the default.

use super::Config;

fn doc(rest: &str) -> String {
    format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{rest}\n")
}

#[test]
fn the_defaults_are_the_documented_ones() {
    let (cfg, _) = Config::parse(&doc("")).unwrap();
    let c = &cfg.card;
    assert_eq!(
        (
            c.profile.as_str(),
            c.day_usd,
            c.max_open,
            c.ask_days,
            c.every_days
        ),
        ("haiku", 2.0, 30, 14, 7)
    );
    assert!(c.is_default());
}

#[test]
fn a_key_is_read_and_a_bad_one_is_refused_with_its_name() {
    let (cfg, _) = Config::parse(&doc("[card]\nday_usd = 0.5\nmax_open = 10\nask_days = 3\nevery_days = 0\nprofile = \"sonnet\"\n")).unwrap();
    assert_eq!(
        (
            cfg.card.day_usd,
            cfg.card.max_open,
            cfg.card.ask_days,
            cfg.card.every_days,
            cfg.card.profile.as_str()
        ),
        (0.5, 10, 3, 0, "sonnet")
    );
    for (bad, word) in [
        ("day_usd = -1", "day_usd"),
        ("max_open = 0", "max_open"),
        ("ask_days = 0", "ask_days"),
        ("every_days = 400", "every_days"),
        ("profile = \" \"", "profile"),
        ("nightly = true", "unknown field"),
    ] {
        let e = Config::parse(&doc(&format!("[card]\n{bad}\n"))).unwrap_err();
        assert!(format!("{e:#}").contains(word), "{bad}: {e:#}");
    }
}
