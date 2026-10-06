//! Property tests over the core's readers of outside text (theseus-s68,
//! review 2's H2): a fetched page (`html::to_text`), a model's event stream
//! (`SseLines`), the times a wake is given (`wake::parse_after`,
//! `wake::parse_at`), and the scrubber over any tool output (review 2's H9,
//! which also checks that a planted value never comes through). Arbitrary
//! text, and text built from the pieces each
//! reader looks for, must never panic: under the release profile's
//! `panic = "abort"` one panic takes the whole daemon down. Discord's
//! `split_text` has its own in `theseus-discord`. A run is a few seconds at
//! most; the seed is random, so the gate keeps looking.

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::{select, Index};
use proptest::test_runner::Config;
use reqwest::Url;

use crate::provider::SseLines;
use crate::wake::{local, parse_after, parse_at, span};
use crate::web::html::{decode, to_text};

fn cases(n: u32) -> Config {
    Config {
        cases: n,
        // A failure prints its shrunk input; nothing is written into the tree.
        failure_persistence: None,
        ..Config::default()
    }
}

/// What the HTML reader acts on: raw elements and their end tags (whole,
/// cut, and in other cases), comments, doctypes, entities, attributes,
/// numbers at the edges, and multibyte text.
const HTML_PIECES: &[&str] = &[
    "<script>",
    "</script>",
    "</SCRIPT",
    "</scr",
    "<style>",
    "</style>",
    "<title>",
    "</title>",
    "<textarea>",
    "</textarea>",
    "<svg/>",
    "<svg>",
    "</svg>",
    "</b>",
    "</",
    "<",
    ">",
    "/>",
    "<!--",
    "-->",
    "<!doctype html>",
    "<?xml ?>",
    "<pre>",
    "</pre>",
    "\n",
    "\r\n",
    " ",
    "<ol start=4294967295>",
    "<ol start=0>",
    "<ol start=-1>",
    "<ol>",
    "</ol>",
    "<li>",
    "</li>",
    "<ul>",
    "</ul>",
    "<a href=\"x\">",
    "<a href='",
    "<a href=javascript:x>",
    "</a>",
    "<base href=\"/b/\">",
    "<base href=\"::\">",
    "<img alt=\"中\">",
    "<img alt='",
    "<h1>",
    "<h6>",
    "</h1>",
    "<br>",
    "<td>",
    "<th>",
    "<p>",
    "</p>",
    "<div>",
    "&#x1F600;",
    "&#0;",
    "&#xFFFFFFFF;",
    "&#99999999999;",
    "&#x;",
    "&amp;",
    "&nbsp;",
    "&",
    "&#",
    ";",
    "\"",
    "'",
    "=",
    "/",
    "中文",
    "😀",
    "é",
    "\u{200b}",
];

fn html() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        3 => select(HTML_PIECES).prop_map(str::to_string),
        1 => any::<String>(),
    ];
    vec(piece, 0..32).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(cases(3000))]

    /// A page is read, never a panic, whatever it holds and wherever the
    /// byte cap cut it.
    #[test]
    fn any_page_reads_without_a_panic(page in html(), cut in any::<Index>()) {
        let base = Url::parse("https://example.com/docs/page.html").ok();
        let _ = to_text(&page, base.as_ref());
        let _ = to_text(&page, None);
        // The fetch caps a page at a byte count, then reads what fits.
        let mut at = cut.index(page.len() + 1);
        while !page.is_char_boundary(at) {
            at -= 1;
        }
        let _ = to_text(&page[..at], base.as_ref());
    }

    #[test]
    fn any_text_reads_without_a_panic(s in any::<String>()) {
        let _ = to_text(&s, None);
        let _ = decode(&s);
    }
}

/// Lines from `body` delivered in pieces cut at `cuts`.
fn lines_of(body: &[u8], cuts: &[Index]) -> Vec<String> {
    let mut at: Vec<usize> = cuts.iter().map(|i| i.index(body.len() + 1)).collect();
    at.push(0);
    at.push(body.len());
    at.sort_unstable();
    at.dedup();
    let mut r = SseLines::default();
    let mut out = Vec::new();
    for w in at.windows(2) {
        r.push(&body[w[0]..w[1]]);
        while let Some(l) = r.next_line() {
            out.push(l);
        }
    }
    out
}

fn stream() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        // Text: lines of any characters, most of them multibyte.
        vec(any::<String>(), 0..6).prop_map(|v| v.join("\n").into_bytes()),
        // Events as they come, some with `\r\n`.
        vec(
            (any::<String>(), any::<bool>()).prop_map(|(t, crlf)| format!(
                "event: content_block_delta{nl}data: {}{nl}{nl}",
                serde_json::json!({"type": "content_block_delta", "index": 0,
                    "delta": {"type": "text_delta", "text": t}}),
                nl = if crlf { "\r\n" } else { "\n" }
            )),
            0..4
        )
        .prop_map(|v| v.concat().into_bytes()),
        // Bytes that are not UTF-8 at all.
        vec(any::<u8>(), 0..160),
    ]
}

proptest! {
    #![proptest_config(cases(2000))]

    /// The stream's lines do not depend on where the network cut it: a
    /// character split across two chunks arrives whole.
    #[test]
    fn stream_lines_do_not_depend_on_where_the_chunks_split(
        body in stream(),
        cuts in vec(any::<Index>(), 0..12),
    ) {
        prop_assert_eq!(lines_of(&body, &cuts), lines_of(&body, &[]));
    }
}

/// Near-misses of RFC 3339: each field in and out of range, every offset
/// form, a fraction, and the whole cut at any byte.
fn near_rfc3339() -> impl Strategy<Value = String> {
    (
        (0u32..10_000, 0u32..100, 0u32..100, 0u32..100),
        (0u32..100, 0u32..100),
        prop_oneof![Just(String::new()), "\\.[0-9]{0,12}"],
        prop_oneof![
            Just("Z".to_string()),
            Just("z".to_string()),
            (any::<bool>(), 0u32..100, 0u32..100, any::<bool>()).prop_map(|(neg, h, m, colon)| {
                format!(
                    "{}{h:02}{}{m:02}",
                    if neg { '-' } else { '+' },
                    if colon { ":" } else { "" }
                )
            }),
            any::<String>(),
        ],
        select(vec!["T", "t", " ", "_"]),
        any::<Index>(),
    )
        .prop_map(|((y, mo, d, h), (mi, s), frac, off, sep, cut)| {
            let t = format!("{y:04}-{mo:02}-{d:02}{sep}{h:02}:{mi:02}:{s:02}{frac}{off}");
            let mut at = cut.index(t.len() + 1);
            while !t.is_char_boundary(at) {
                at -= 1;
            }
            // Mostly whole; now and then cut.
            if at % 4 == 0 {
                t[..at].to_string()
            } else {
                t
            }
        })
}

fn near_duration() -> impl Strategy<Value = String> {
    vec(
        (
            prop_oneof![Just(String::new()), "[0-9]{1,24}"],
            select(vec![
                "s", "m", "h", "d", "sec", "minutes", "Hours", "days", "", "x", "中",
            ]),
            select(vec!["", " ", "  "]),
        )
            .prop_map(|(n, u, sp)| format!("{n}{sp}{u}{sp}")),
        0..5,
    )
    .prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(cases(3000))]

    /// A wake's time is parsed or refused with a reason, never a panic.
    #[test]
    fn wake_times_parse_or_refuse_without_a_panic(
        s in any::<String>(),
        at in near_rfc3339(),
        after in near_duration(),
    ) {
        let _ = parse_after(&s);
        let _ = parse_at(&s);
        let _ = parse_at(&at);
        let _ = parse_after(&after);
    }

    /// A whole number of one unit is that many of the unit's milliseconds,
    /// or refused when it does not fit.
    #[test]
    fn a_number_of_units_is_its_milliseconds(
        n in any::<u64>(),
        (unit, ms) in select(vec![("s", 1_000u64), ("min", 60_000), ("hours", 3_600_000), ("d", 86_400_000)]),
    ) {
        let got = parse_after(&format!("{n}{unit}"));
        match n.checked_mul(ms) {
            Some(total) => prop_assert_eq!(got, Ok(total)),
            None => prop_assert!(got.is_err()),
        }
    }

    /// Any instant reads as a span and as a local time.
    #[test]
    fn any_instant_reads_as_a_span_and_a_local_time(ms in any::<u64>()) {
        let _ = span(ms);
        let l = local(ms);
        let _ = (l.hm(), l.hms(), l.full(), l.hms_on(&local(ms / 2)));
    }
}

// ---------------------------------------------------------------- the scrubber

/// What the scrubber acts on (review 2's H9): the starts of every shape, the
/// separators its labels take, line breaks, `%`, `=`, and multibyte text.
const SCRUB_PIECES: &[&str] = &[
    "-----BEGIN RSA PRIVATE KEY-----",
    "-----END RSA PRIVATE KEY-----",
    "-----BEGIN ",
    "-----",
    "eyJ",
    "eyJhbGciOiJub25lIn0",
    ".",
    "AKIA",
    "ASIA",
    "ABCDEFGHIJKLMNOP",
    "aws_secret_access_key",
    "SessionToken",
    "\": \"",
    " = ",
    "ghp_",
    "sk-ant-",
    "%",
    "%2F",
    "%2",
    "=",
    "==",
    "\n",
    "\r\n",
    "/",
    "+",
    "0123456789abcdefghijklmnopqrstuvwxyzABCD",
    "中文",
    "😀",
    "é",
    "\\",
    "\\\"",
    "\\u00",
    "\\ud83d",
];

/// An invented board value.
const SCRUB_VALUE: &str = "Inv3nted/Value+For~Tests?x=1";

fn scrub_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        3 => select(SCRUB_PIECES).prop_map(str::to_string),
        1 => any::<String>(),
    ];
    vec(piece, 0..48).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(cases(2000))]

    /// Any tool output is scrubbed without a panic.
    #[test]
    fn any_output_scrubs_without_a_panic(s in scrub_text()) {
        let scrub = crate::scrub::Scrubber::with_values(vec![(SCRUB_VALUE.into(), "demo".into())]);
        let _ = scrub.scrub(&s);
    }

    /// A board value never comes through, verbatim or encoded, whatever is
    /// around it.
    #[test]
    fn a_value_never_comes_through(before in scrub_text(), after in scrub_text(), how in 0..5usize) {
        use base64::Engine as _;
        let scrub = crate::scrub::Scrubber::with_values(vec![(SCRUB_VALUE.into(), "demo".into())]);
        let b64 = base64::engine::general_purpose::STANDARD;
        let (planted, look_for) = match how {
            0 => (SCRUB_VALUE.to_string(), SCRUB_VALUE.to_string()),
            1 => {
                let e = b64.encode(SCRUB_VALUE);
                (e.clone(), e)
            }
            2 => {
                let e = b64.encode(format!("u:{SCRUB_VALUE}"));
                (e.clone(), e)
            }
            3 => {
                let e = SCRUB_VALUE.replace('/', "%2F").replace('+', "%2B");
                (e.clone(), e)
            }
            _ => {
                // JSON-escaped: `/` as PHP writes it, `+` as a `\u` escape
                // (theseus-ubp7).
                let e = SCRUB_VALUE.replace('/', "\\/").replace('+', "\\u002B");
                (e.clone(), e)
            }
        };
        let (out, n) = scrub.scrub(&format!("{before} {planted} {after}"));
        prop_assert!(n >= 1);
        prop_assert!(!out.contains(&look_for), "{out}");
    }
}
