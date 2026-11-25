//! `[context]`'s look-back sizes (issue 48): read with today's values as defaults, and a value out of
//! range refused at load with its range, never clamped.

use super::{Sizes, EVERYWHERE_EXCHANGES_RANGE, HERE_EXCHANGES_RANGE, HISTORY_PAGE_EXCHANGES_RANGE};
use crate::config::Config;

fn doc(rest: &str) -> String {
    format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{rest}\n")
}

fn sizes(rest: &str) -> anyhow::Result<Sizes> {
    Ok(Config::parse(&doc(rest))?.0.context.sizes())
}

#[test]
fn the_context_sizes_default_to_todays_values() {
    let today = Sizes {
        here: crate::ladder::WINDOW_FLOOR,
        everywhere: 15,
        page: crate::view::history::PAGE_EXCHANGES,
    };
    assert_eq!((today.here, today.page), (30, 100));
    // An old settings file, with no `[context]` or none of the new keys, loads with the defaults.
    assert_eq!(sizes("").unwrap(), today);
    assert_eq!(sizes("[context]\npersona_max_tokens = 300").unwrap(), today);
    assert_eq!(Sizes::default(), today);
    // Each key is read as it is set.
    let set = "[context]\nhere_exchanges = 40\neverywhere_exchanges = 20\nhistory_page_exchanges = 50";
    assert_eq!(
        sizes(set).unwrap(),
        Sizes {
            here: 40,
            everywhere: 20,
            page: 50
        }
    );
}

#[test]
fn an_out_of_range_size_is_refused_at_load_with_its_range() {
    let cases = [
        (
            "here_exchanges",
            HERE_EXCHANGES_RANGE,
            "10 to 60 (the newest exchanges of a conversation kept whole; default 30)",
        ),
        (
            "everywhere_exchanges",
            EVERYWHERE_EXCHANGES_RANGE,
            "5 to 50 (the newest exchanges from every other place, laid beside the message; default 15)",
        ),
        (
            "history_page_exchanges",
            HISTORY_PAGE_EXCHANGES_RANGE,
            "20 to 200 (the exchanges on one page of the history tool; default 100)",
        ),
    ];
    for (key, range, says) in cases {
        let (low, high) = (*range.start(), *range.end());
        for n in [low - 1, high + 1, 0] {
            let e = sizes(&format!("[context]\n{key} = {n}")).unwrap_err();
            assert_eq!(
                format!("{e:#}"),
                format!("context.{key} = {n} is out of range: {says}"),
                "{key} = {n} is refused, never clamped"
            );
        }
        // Its bounds load as they are.
        for n in [low, high] {
            let got = sizes(&format!("[context]\n{key} = {n}")).unwrap();
            let read = match key {
                "here_exchanges" => got.here,
                "everywhere_exchanges" => got.everywhere,
                _ => got.page,
            };
            assert_eq!(read as u64, n, "{key} = {n}");
        }
    }
}
