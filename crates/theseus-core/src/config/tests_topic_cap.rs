//! `[context] topic_max_tokens` (issue 48): the topic block's own cap, read with 8000 as its default, and a
//! value out of range refused at load with its range, never clamped.

use super::context::TOPIC_MAX_TOKENS_RANGE;
use super::Config;

fn doc(rest: &str) -> String {
    format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{rest}\n")
}

fn cap(rest: &str) -> anyhow::Result<u64> {
    Ok(Config::parse(&doc(rest))?.0.context.topic_cap())
}

#[test]
fn the_topic_cap_defaults_to_eight_thousand_and_reads_as_set() {
    // An old settings file, with no `[context]` or without the key, loads with the default.
    assert_eq!(cap("").unwrap(), 8_000);
    assert_eq!(cap("[context]\npersona_max_tokens = 300").unwrap(), 8_000);
    assert_eq!(crate::view::budget::TOPIC_MAX_TOKENS, 8_000);
    assert_eq!(cap("[context]\ntopic_max_tokens = 2600").unwrap(), 2_600);
}

#[test]
fn an_out_of_range_topic_cap_is_refused_at_load_with_its_range() {
    let (low, high) = (
        *TOPIC_MAX_TOKENS_RANGE.start(),
        *TOPIC_MAX_TOKENS_RANGE.end(),
    );
    assert_eq!((low, high), (1_000, 20_000));
    for n in [low - 1, high + 1, 0] {
        let e = cap(&format!("[context]\ntopic_max_tokens = {n}")).unwrap_err();
        assert_eq!(
            format!("{e:#}"),
            format!(
                "context.topic_max_tokens = {n} is out of range: 1000 to 20000 (the topic block beside the message, recall cut first; default 8000)"
            ),
            "{n} is refused, never clamped"
        );
    }
    for n in [low, high] {
        assert_eq!(
            cap(&format!("[context]\ntopic_max_tokens = {n}")).unwrap(),
            n
        );
    }
}
