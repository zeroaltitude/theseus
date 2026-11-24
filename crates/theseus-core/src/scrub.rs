//! Exact-value substitution (spec §3.9): every secret the vault has given is
//! replaced by `[redacted:<name>]` wherever it appears: verbatim, in base64
//! (standard or URL-safe, at any offset inside a longer encoding, wrapped
//! across lines or not), percent-encoded (review 2's H9), and with any of its
//! characters JSON-escaped (theseus-ubp7, `escaped.rs`).
//!
//! It runs on a copy that leaves the machine, never on the record: a node, a
//! kept output, an imported episode and a watch's row are what the tool
//! printed, so the operator can read what was exposed. Nothing is guessed
//! here: a value the vault never gave is text, whatever its shape.
//!
//! The values are read from the secret board at each scrub, so a value is
//! known here the moment it resolves (theseus-qa0). A turn waits for the
//! board's first round to settle before any tool output passes through here.
//!
//! One pass does the work for every caller: [`Substitution`], every reading
//! of every value, verbatim and encoded, found on the same text (overlapping
//! readings included) and merged where they overlap, with the name of every
//! span it replaced.
//! `Scrubber` (Jev's state, the push line, the display sites) and the exits
//! (the provider request, the courier: `exits.rs`) both run it, so what leaves
//! the machine is held to the same standard whichever door it goes by.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use base64::Engine as _;

use crate::secrets::{SecretBoard, SecretState};

mod escaped;
#[cfg(test)]
mod tests_escaped;
#[cfg(test)]
mod tests_fold;
#[cfg(test)]
mod tests_nested;
#[cfg(test)]
mod tests_overlap;

#[derive(Default)]
pub struct Scrubber {
    exact: Vec<(String, String)>,
    board: Option<Arc<SecretBoard>>,
}

/// The shortest value scrubbed: a shorter one would match by chance.
pub(crate) const MIN_VALUE: usize = 8;

/// One replacement: a byte range of the text, and the name of the value it
/// held.
type Span<'a> = (usize, usize, &'a str);

impl Scrubber {
    /// Scrub every value the board holds ready.
    pub fn from_board(board: Arc<SecretBoard>) -> Self {
        Self {
            exact: Vec::new(),
            board: Some(board),
        }
    }

    #[cfg(test)]
    pub fn with_values(values: Vec<(String, String)>) -> Self {
        Self {
            exact: values,
            board: None,
        }
    }

    pub fn scrub(&self, text: &str) -> (String, u32) {
        let states = self.board.as_ref().map(|b| b.states());
        let mut values: Vec<(&str, &str)> = self
            .exact
            .iter()
            .map(|(v, n)| (v.as_str(), n.as_str()))
            .collect();
        for (name, s) in states.iter().flat_map(|m| m.iter()) {
            if let SecretState::Ready(v) = s {
                let v = v.expose().trim();
                if v.len() >= MIN_VALUE {
                    values.push((v, name));
                }
            }
        }
        let (out, counts) = Substitution::new(values).apply(text);
        (out.into_owned(), counts.values().sum())
    }
}

/// A set of values ready to be substituted in any number of strings: each
/// value's base64 needles are worked out once, so a request of a few thousand
/// strings pays for them once and not for each string.
pub(crate) struct Substitution<'a> {
    /// `(value, name)`, longest first so a secret containing another is
    /// replaced whole, ties by name so a value two secrets share always gets
    /// the same one.
    values: Vec<(&'a str, &'a str)>,
    needles: Vec<(String, &'a str)>,
}

impl<'a> Substitution<'a> {
    /// `values` as `(value, name)`, each at least [`MIN_VALUE`] long.
    pub(crate) fn new(mut values: Vec<(&'a str, &'a str)>) -> Self {
        values.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.1.cmp(b.1)));
        let needles = values
            .iter()
            .flat_map(|(v, name)| {
                base64_needles(v.as_bytes())
                    .into_iter()
                    .map(move |s| (s, *name))
            })
            .collect();
        Self { values, needles }
    }

    /// `text` with every value replaced by `[redacted:<name>]`, verbatim and
    /// in each encoding, and how many times each name was met. Borrowed when
    /// the text holds none. There is no pre-check on the verbatim value: an
    /// encoded one does not contain it.
    pub(crate) fn apply<'t>(&self, text: &'t str) -> (Cow<'t, str>, BTreeMap<String, u32>) {
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        let Some((out, names)) = splice(text, self.spans(text)) else {
            return (Cow::Borrowed(text), counts);
        };
        for name in names {
            *counts.entry(name.to_string()).or_default() += 1;
        }
        (Cow::Owned(out), counts)
    }

    /// Every reading of every value in `text`, verbatim and encoded, as byte
    /// ranges with the value's name: overlapping readings each, all found on
    /// the text as given.
    fn spans(&self, text: &str) -> Vec<Span<'a>> {
        let mut spans = Vec::new();
        for (v, name) in &self.values {
            spans.extend(every(text, v).map(|at| (at, at + v.len(), *name)));
        }
        spans.extend(encoded(text, &self.values, &self.needles));
        spans
    }
}

/// Where `needle` starts in `hay`, overlapping matches included: after a
/// match the search goes on from its second character.
fn every<'h>(hay: &'h str, needle: &'h str) -> impl Iterator<Item = usize> + 'h {
    let step = needle.chars().next().map_or(1, char::len_utf8);
    let mut from = 0;
    std::iter::from_fn(move || {
        if needle.is_empty() {
            return None;
        }
        let at = from + hay.get(from..)?.find(needle)?;
        from = at + step;
        Some(at)
    })
}

/// The spans `splice` replaces: those on character boundaries, sorted, with
/// overlapping ones merged into one. Spans that only touch stay apart. Each
/// merged span keeps the name of its first span. The names returned are one
/// for each span kept and one for each span that made a kept one longer.
fn merged<'a>(text: &str, mut spans: Vec<Span<'a>>) -> (Vec<Span<'a>>, Vec<&'a str>) {
    spans.sort_by_key(|s| (s.0, std::cmp::Reverse(s.1), s.2));
    let mut kept: Vec<Span> = Vec::new();
    let mut names = Vec::new();
    for s in spans {
        if s.0 >= s.1 || !text.is_char_boundary(s.0) || !text.is_char_boundary(s.1) {
            continue;
        }
        match kept.last_mut() {
            Some(k) if s.0 < k.1 => {
                if s.1 > k.1 {
                    k.1 = s.1;
                    names.push(s.2);
                }
            }
            _ => {
                kept.push(s);
                names.push(s.2);
            }
        }
    }
    (kept, names)
}

/// `text` with each span replaced by its stand-in, in one pass from the end,
/// overlapping spans merged first (`merged`), and the names counted. `None`
/// when no span was kept.
fn splice<'a>(text: &str, spans: Vec<Span<'a>>) -> Option<(String, Vec<&'a str>)> {
    let (kept, names) = merged(text, spans);
    if kept.is_empty() {
        return None;
    }
    let mut out = text.to_string();
    for (a, b, name) in kept.into_iter().rev() {
        out.replace_range(a..b, &format!("[redacted:{name}]"));
    }
    Some((out, names))
}

// ---------------------------------------------------------------- encoded values

/// Where each value's base64, percent-encoded and JSON-escaped forms are in
/// `out`.
fn encoded<'a>(
    out: &str,
    values: &[(&str, &'a str)],
    needles: &[(String, &'a str)],
) -> Vec<Span<'a>> {
    let mut spans = base64_spans(out, needles, |_| true);
    if out.contains('%') {
        for (v, name) in values {
            for (a, b) in percent_spans(out.as_bytes(), v.as_bytes()) {
                spans.push((a, b, *name));
            }
        }
    }
    spans.extend(escaped::spans(out, values, needles));
    spans
}

/// Where a needle matches in the text's base64 runs that `keep` takes (given
/// the bytes a run's lines span), as the byte range of the lines the match
/// touches, with its name.
fn base64_spans<'a>(
    text: &str,
    needles: &[(String, &'a str)],
    keep: impl Fn(Range<usize>) -> bool,
) -> Vec<(usize, usize, &'a str)> {
    let mut spans = Vec::new();
    if needles.is_empty() {
        return spans;
    }
    let (mut joined, mut starts) = (String::new(), Vec::new());
    for lines in base64_runs(text).iter() {
        if !keep(lines[0].start..lines[lines.len() - 1].end) {
            continue;
        }
        // The run without its line breaks, and where each line starts in it:
        // a run of one line, most of them, is read where it lies.
        let run = if let [line] = lines {
            starts.clear();
            starts.push(0);
            &text[line.clone()]
        } else {
            joined.clear();
            starts.clear();
            for l in lines {
                starts.push(joined.len());
                joined.push_str(&text[l.clone()]);
            }
            joined.as_str()
        };
        let line_of = |at: usize| &lines[starts.partition_point(|s| *s <= at) - 1];
        for (needle, name) in needles {
            for at in every(run, needle) {
                let (first, last) = (line_of(at), line_of(at + needle.len() - 1));
                spans.push((first.start, last.end, *name));
            }
        }
    }
    spans
}

/// What every base64 encoding that holds `v` contains, whatever comes before
/// and after it: for each of the three offsets a value can start at, the
/// characters of its encoding that depend on its own bytes alone, in the
/// standard alphabet and the URL-safe one.
fn base64_needles(v: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for k in 0..3usize {
        let mut buf = vec![0u8; k];
        buf.extend_from_slice(v);
        let enc = base64::engine::general_purpose::STANDARD.encode(&buf);
        // Character j holds bits 6j to 6j+6: the filler's end at 8k, the
        // value's end at 8(k + len).
        let mid = &enc[(8 * k).div_ceil(6)..8 * (k + v.len()) / 6];
        let url: String = mid
            .chars()
            .map(|c| match c {
                '+' => '-',
                '/' => '_',
                c => c,
            })
            .collect();
        if url != mid {
            out.push(url);
        }
        out.push(mid.to_string());
    }
    out
}

fn is_base64(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'-' | b'_')
}

/// The text's runs of base64, each as the byte ranges of its lines: letters,
/// digits, `+/-_`, and at most two `=` at the end. A run goes on across a
/// line break when its line is 40 characters or more, as an encoding wrapped
/// at a fixed width is; a short line, a word or a name, ends it. A match
/// withholds the lines it touches, so a word on the line after a long one is
/// kept. The letter of a `\n`, `\r`, `\/` or `\u` escape starts no run: the
/// decoded text's pass reads what it stands for (theseus-cjyt). No value's
/// base64 starts with one of those four (its first character is the top six
/// bits of a UTF-8 lead byte: `A`-`Z`, `a`-`f`, `w`-`z`, `0`-`9`), so the
/// skip hides no needle. Any other escape's letter starts a run as any letter
/// does, so a value's base64 right after a lone backslash (a Windows path's
/// `C:\`) is read whole (theseus-g88t).
fn base64_runs(text: &str) -> Runs {
    let b = text.as_bytes();
    let mut runs = Runs::default();
    let mut i = 0;
    while i < b.len() {
        if !is_base64(b[i])
            || (matches!(b[i], b'n' | b'r' | b'/' | b'u') && escaped::is_escape_letter(b, i))
        {
            i += 1;
            continue;
        }
        let first = runs.lines.len();
        loop {
            let line = i;
            while i < b.len() && is_base64(b[i]) {
                i += 1;
            }
            let mut j = i;
            while j < b.len() && b[j] == b'=' && j - i < 2 {
                j += 1;
            }
            runs.lines.push(line..j);
            let lf = if b.get(j) == Some(&b'\r') { j + 1 } else { j };
            let wraps = j == i
                && i - line >= 40
                && b.get(lf) == Some(&b'\n')
                && b.get(lf + 1).is_some_and(|c| is_base64(*c));
            if !wraps {
                i = j;
                break;
            }
            i = lf + 1;
        }
        let len: usize = runs.lines[first..].iter().map(ExactSizeIterator::len).sum();
        if len >= 10 {
            runs.ends.push(runs.lines.len());
        } else {
            runs.lines.truncate(first);
        }
    }
    runs
}

/// The text's base64 runs: every run's lines in one list, and where each run
/// ends in it, so a scan allocates nothing for each word it passes
/// (theseus-oyrt: a run's own list was most of a scrub's allocations).
#[derive(Default)]
struct Runs {
    lines: Vec<Range<usize>>,
    ends: Vec<usize>,
}

impl Runs {
    /// Each run's lines, in order.
    fn iter(&self) -> impl Iterator<Item = &[Range<usize>]> {
        let starts = std::iter::once(0).chain(self.ends.iter().copied());
        starts.zip(&self.ends).map(|(a, b)| &self.lines[a..*b])
    }
}

/// Where `v` appears with at least one of its bytes percent-encoded (`%2F` or
/// `%2f`), as byte ranges. The other bytes may be encoded or not, as each
/// encoder chooses.
fn percent_spans(t: &[u8], v: &[u8]) -> Vec<(usize, usize)> {
    let hex = |c: Option<&u8>| c.and_then(|c| (*c as char).to_digit(16));
    let mut spans = Vec::new();
    let mut i = 0;
    if v.is_empty() {
        return spans;
    }
    // Reuse the two frontiers; most candidate starts fail on their first byte.
    let mut states = Vec::new();
    let mut next = Vec::new();
    while i < t.len() {
        if t[i] != v[0]
            && !(t[i] == b'%'
                && hex(t.get(i + 1))
                    .zip(hex(t.get(i + 2)))
                    .is_some_and(|(h, l)| h * 16 + l == u32::from(v[0])))
        {
            i += 1;
            continue;
        }
        states.clear();
        states.push((i, false));
        for &want in v {
            next.clear();
            for (j, encoded) in states.drain(..) {
                if t.get(j) == Some(&want) {
                    next.push((j + 1, encoded));
                }
                if t.get(j) == Some(&b'%')
                    && hex(t.get(j + 1))
                        .zip(hex(t.get(j + 2)))
                        .is_some_and(|(h, l)| h * 16 + l == u32::from(want))
                {
                    next.push((j + 3, true));
                }
            }
            next.sort_unstable();
            next.dedup();
            std::mem::swap(&mut states, &mut next);
            if states.is_empty() {
                break;
            }
        }
        if let Some(end) = states
            .iter()
            .filter(|(_, encoded)| *encoded)
            .map(|(j, _)| *j)
            .max()
        {
            spans.push((i, end));
        }
        // A reading that starts inside this one is tried as well.
        i += 1;
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scrubber knows what the vault gave it and nothing else: a key's or
    /// a token's shape, a private-key block, a JWT, a connection URL's
    /// password are text, as the tool printed them (the record is raw, and
    /// a guess has no place in what leaves it).
    #[test]
    fn only_a_value_the_vault_gave_is_replaced_and_no_shape_is() {
        let s = Scrubber::with_values(vec![("hunter2hunter2".into(), "db_password".into())]);
        let text = "pw=hunter2hunter2 key=sk-ant-api03-abcdefghijklmnop end \
                    ghp_inventedinventedinvented";
        assert_eq!(
            s.scrub(text),
            (
                "pw=[redacted:db_password] key=sk-ant-api03-abcdefghijklmnop end \
                 ghp_inventedinventedinvented"
                    .to_string(),
                1
            )
        );
        let none = Scrubber::default();
        for kept in [
            "Access key ID,Secret access key\nAKIAINVENTEDEXAMPLE7,wJalrXUtnFEMI/K7MDENG/bPxRfiCYINVENTEDKY\n",
            "aws_secret_access_key = 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",
            "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ\nINVENTEDINVENTEDINVENTED==\n-----END OPENSSH PRIVATE KEY-----",
            "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJpbnZlbnRlZCJ9.c2lnbmF0dXJlLWludmVudGVk\n",
            &format!("postgres://app:s3cr3t-invented{}db.internal/app", '@'),
            "ghp_x then ghp_inventedinventedinvented",
        ] {
            assert_eq!(none.scrub(kept), (kept.to_string(), 0), "{kept}");
        }
    }

    /// A value is scrubbed from the moment the board has it.
    #[test]
    fn values_come_from_the_board_as_they_resolve() {
        use crate::secrets::Secret;
        let board = SecretBoard::new(["db".to_string()], std::time::Instant::now());
        let s = Scrubber::from_board(board.clone());
        assert_eq!(s.scrub("pw=hunter2hunter2").1, 0, "nothing resolved yet");
        board.publish(
            [("db".to_string(), Ok(Secret::new("hunter2hunter2".into())))].into(),
            "fake",
        );
        let (out, n) = s.scrub("pw=hunter2hunter2");
        assert_eq!((out.as_str(), n), ("pw=[redacted:db]", 1));
    }

    /// An invented value, and the scrubber that knows it.
    const VALUE: &str = "Inv3nted/Value+For~Tests?x=1";

    fn knowing() -> Scrubber {
        Scrubber::with_values(vec![(VALUE.into(), "demo_secret".into())])
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Review 2's H9: a board value in base64, in either alphabet, at any of
    /// the three offsets inside a longer encoding, and wrapped across lines,
    /// is withheld with the base64 that holds it.
    #[test]
    fn a_value_in_base64_is_withheld_at_any_offset_and_wrapped() {
        let s = knowing();
        for before in ["", "u:", "us:", "user:"] {
            let blob = b64(format!("{before}{VALUE} and more").as_bytes());
            let (out, n) = s.scrub(&format!("Authorization: Basic {blob}\nok"));
            assert_eq!(
                (out.as_str(), n),
                ("Authorization: Basic [redacted:demo_secret]\nok", 1),
                "{before:?}"
            );
            let url = blob.replace('+', "-").replace('/', "_").replace('=', "");
            let (out, _) = s.scrub(&format!("?t={url}&"));
            assert_eq!(out, "?t=[redacted:demo_secret]&", "{before:?}");
        }
        // Wrapped at 40 columns, as `base64 -w 40` writes it.
        let long = b64(format!("{}{VALUE}{}", "x".repeat(37), "y".repeat(40)).as_bytes());
        let wrapped: Vec<&str> = long
            .as_bytes()
            .chunks(40)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        let (out, n) = s.scrub(&format!("{}\nend", wrapped.join("\n")));
        // The value is in lines 2 and 3; lines 1 and 4 hold only the filler.
        assert_eq!(
            (out, n),
            (
                format!(
                    "{}\n[redacted:demo_secret]\n{}\nend",
                    wrapped[0], wrapped[3]
                ),
                1
            )
        );
        // Plain words on their own lines are not joined into one run.
        let (out, n) = s.scrub("alpha\nbravo\ncharlie");
        assert_eq!((out.as_str(), n), ("alpha\nbravo\ncharlie", 0));
    }

    /// Review 2's H9: a board value percent-encoded, wholly or in part, in
    /// either case of hex.
    #[test]
    fn a_value_percent_encoded_is_withheld() {
        let s = knowing();
        let upper = "Inv3nted%2FValue%2BFor~Tests%3Fx%3D1";
        let lower = "Inv3nted%2fValue%2bFor~Tests%3fx%3d1";
        let every: String = VALUE.bytes().map(|c| format!("%{c:02X}")).collect();
        for enc in [upper.to_string(), lower.to_string(), every] {
            let (out, n) = s.scrub(&format!("GET /cb?token={enc}&next=1"));
            assert_eq!(
                (out.as_str(), n),
                ("GET /cb?token=[redacted:demo_secret]&next=1", 1),
                "{enc}"
            );
        }
        let (out, n) = s.scrub("100% done, 50%2F50");
        assert_eq!((out.as_str(), n), ("100% done, 50%2F50", 0));
    }

    /// The pass both doors run (`Scrubber`, and the exits' provider request and
    /// courier): every span kept is named, verbatim or encoded, and the same
    /// text and values give the same bytes and the same names whatever order
    /// the values come in, a value two secrets share getting one name.
    #[test]
    fn the_shared_pass_names_every_span_and_ignores_the_order_values_come_in() {
        let blob = b64(format!("user:{VALUE}").as_bytes());
        let text = format!("a {VALUE} b Authorization: Basic {blob} c hunter2hunter2 d");
        let values = [
            (VALUE, "demo_secret"),
            ("hunter2hunter2", "db_password"),
            // The same value under a second name, and a value inside another.
            ("hunter2hunter2", "also_db"),
            ("hunter2hunter2-and-more", "longer"),
        ];
        let run = |order: &[usize]| {
            let (out, counts) =
                Substitution::new(order.iter().map(|i| values[*i]).collect()).apply(&text);
            (out.into_owned(), counts)
        };
        let first = run(&[0, 1, 2, 3]);
        assert_eq!(
            first.0,
            "a [redacted:demo_secret] b Authorization: Basic [redacted:demo_secret] c \
             [redacted:also_db] d"
        );
        assert_eq!(
            first.1,
            BTreeMap::from([("also_db".into(), 1), ("demo_secret".into(), 2)])
        );
        for order in [[3, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1], [0, 2, 1, 3]] {
            assert_eq!(run(&order), first, "{order:?}");
        }
        // Nothing in the text: it is borrowed, with no names.
        let (out, counts) = Substitution::new(values.to_vec()).apply("ordinary words");
        assert!(matches!(out, Cow::Borrowed(_)) && counts.is_empty());
    }

    /// The live exact-value pass and its encoded scan on ordinary output: a
    /// scrubber that KNOWS a value finds nothing in a MiB of cargo, git and log
    /// lines (no false positive), changes nothing, and takes a bounded time.
    #[test]
    fn a_mib_of_ordinary_output_scrubs_in_bounded_time() {
        let s = Scrubber::with_values(vec![
            (VALUE.into(), "demo_secret".into()),
            ("hunter2hunter2".into(), "db_password".into()),
        ]);
        let lines = [
            "   Compiling theseus-core v0.1.0 (/work/src/crates/theseus-core)\n",
            "warning: unused import: `std::fmt` --> src/lib.rs:42:5 | 100% done\n",
            "[main 3fa9c2e] fix the thing\n 3 files changed, 41 insertions(+), 7 deletions(-)\n",
            "{\"level\":\"info\",\"msg\":\"GET /v1/items?id=42&page=3\",\"ms\":12}\n",
            "diff --git a/src/a.rs b/src/a.rs\nindex 3fa9c2e..8d1b7a4 100644\n",
            "aGVsbG8gd29ybGQ= dGVzdA== C:\\Users\\x\\file 50%2F50 caf\u{e9}\n",
            "test result: ok. 128 passed; 0 failed; finished in 1.84s\n",
        ];
        let mut text = String::new();
        while text.len() < 1 << 20 {
            for l in lines {
                text.push_str(l);
            }
        }
        let began = std::time::Instant::now();
        let (out, n) = s.scrub(&text);
        let took = began.elapsed();
        assert_eq!((n, out.len()), (0, text.len()));
        assert_eq!(out, text);
        eprintln!(
            "scrub: {} bytes of ordinary output, two values: {took:?}",
            text.len()
        );
        assert!(took < std::time::Duration::from_secs(20), "took {took:?}");
    }

    #[test]
    fn percent_values_accept_literal_and_encoded_percent() {
        for (text, value) in [
            ("abc%25defghi", "abc%defghi"),
            ("abc%2525def%67hi", "abc%25defghi"),
            ("abc%25def%67hi", "abc%25defghi"),
        ] {
            assert_eq!(
                percent_spans(text.as_bytes(), value.as_bytes()),
                vec![(0, text.len())]
            );
            let (out, counts) = Substitution::new(vec![(value, "demo")]).apply(text);
            assert_eq!(out, "[redacted:demo]");
            assert_eq!(counts.get("demo"), Some(&1));
        }
    }

    #[test]
    fn percent_dense_text_with_forty_values_is_measured() {
        let line = "%61%62%63%2F%25%20%3D%26%3F%41%42%43%2f%25%20%3d\n";
        let text = line.repeat((4 * 1024 * 1024) / line.len());
        let values: Vec<_> = (0..40)
            .map(|n| format!("abc/fixture-{n:02}-encoded-value-0123456789"))
            .collect();
        let start = std::time::Instant::now();
        for value in &values {
            assert!(percent_spans(text.as_bytes(), value.as_bytes()).is_empty());
        }
        println!(
            "PERCENT_MEASUREMENT {} ms for {} bytes with 40 values",
            start.elapsed().as_millis(),
            text.len()
        );
    }

    // The previous matcher, intentionally kept with fresh frontiers per start.
    fn percent_spans_reference(t: &[u8], v: &[u8]) -> Vec<(usize, usize)> {
        let hex = |c: Option<&u8>| c.and_then(|c| (*c as char).to_digit(16));
        let mut spans = Vec::new();
        let mut i = 0;
        if v.is_empty() {
            return spans;
        }
        while i < t.len() {
            if t[i] != v[0] && t[i] != b'%' {
                i += 1;
                continue;
            }
            let mut states = vec![(i, false)];
            for &want in v {
                let mut next = Vec::new();
                for (j, encoded) in states {
                    if t.get(j) == Some(&want) {
                        next.push((j + 1, encoded));
                    }
                    if t.get(j) == Some(&b'%')
                        && hex(t.get(j + 1))
                            .zip(hex(t.get(j + 2)))
                            .is_some_and(|(h, l)| h * 16 + l == u32::from(want))
                    {
                        next.push((j + 3, true));
                    }
                }
                next.sort_unstable();
                next.dedup();
                states = next;
                if states.is_empty() {
                    break;
                }
            }
            if let Some(end) = states
                .into_iter()
                .filter(|(_, encoded)| *encoded)
                .map(|(j, _)| j)
                .max()
            {
                spans.push((i, end));
                i += 1;
            } else {
                i += 1;
            }
        }
        spans
    }

    fn check_percent_reference(text: &[u8], value: &[u8]) {
        assert_eq!(
            percent_spans(text, value),
            percent_spans_reference(text, value),
            "text={:?}, value={:?}",
            String::from_utf8_lossy(text),
            String::from_utf8_lossy(value)
        );
    }

    #[test]
    fn percent_spans_match_the_reference_matcher() {
        for (text, value, expected) in [
            ("abc%25defghi", "abc%defghi", vec![(0, 12)]),
            ("%25abcdefghi", "%abcdefghi", vec![(0, 12)]),
            ("%abc%64efghi", "%abcdefghi", vec![(0, 12)]),
            ("%41BCDEFGH1", "ABCDEFGH1", vec![(0, 11)]),
            ("%61BCDEFGH1", "ABCDEFGH1", vec![]),
            ("%2f%2F", "//", vec![(0, 6)]),
            ("%61bc%61bc", "abc", vec![(0, 5), (5, 10)]),
            ("%61bc%4", "abc", vec![(0, 5)]),
            ("%4", "A", vec![]),
            ("", "", vec![]),
        ] {
            check_percent_reference(text.as_bytes(), value.as_bytes());
            assert_eq!(percent_spans(text.as_bytes(), value.as_bytes()), expected);
        }
        // Fixed xorshift seed: failures reproduce without a property-test dependency.
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut draw = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound as u64) as usize
        };
        let alphabet = b"aAb%2541fF/";
        for case in 0..10_000 {
            let len = 1 + draw(12);
            let value: Vec<_> = (0..len).map(|_| alphabet[draw(alphabet.len())]).collect();
            let mut text: Vec<u8> = (0..draw(8))
                .map(|_| alphabet[draw(alphabet.len())])
                .collect();
            for _ in 0..1 + draw(3) {
                for &byte in &value {
                    match draw(3) {
                        0 => text.push(byte),
                        1 => text.extend(format!("%{byte:02X}").bytes()),
                        _ => text.extend(format!("%{byte:02x}").bytes()),
                    }
                }
            }
            text.extend_from_slice([b"%".as_slice(), b"%4", b"%G1"][case % 3]);
            check_percent_reference(&text, &value);
        }
    }

    // Enumerate decodings, not matcher transitions: never prune by the wanted byte,
    // never deduplicate frontiers, and never skip a start after finding a match.
    pub(super) fn percent_oracle(text: &[u8], value: &[u8]) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        if value.is_empty() {
            return spans;
        }
        for start in 0..text.len() {
            let mut pending = vec![(start, Vec::new(), false)];
            while let Some((end, decoded, encoded)) = pending.pop() {
                if decoded.len() == value.len() {
                    if encoded && decoded == value {
                        spans.push((start, end));
                    }
                    continue;
                }
                if let Some(&byte) = text.get(end) {
                    let mut literal = decoded.clone();
                    literal.push(byte);
                    pending.push((end + 1, literal, encoded));
                }
                if let Some([b'%', hi, lo]) = text.get(end..end + 3) {
                    let digit = |b: u8| match b {
                        b'0'..=b'9' => Some(b - b'0'),
                        b'a'..=b'f' => Some(b - b'a' + 10),
                        b'A'..=b'F' => Some(b - b'A' + 10),
                        _ => None,
                    };
                    if let (Some(hi), Some(lo)) = (digit(*hi), digit(*lo)) {
                        let mut escaped = decoded;
                        escaped.push(16 * hi + lo);
                        pending.push((end + 3, escaped, true));
                    }
                }
            }
        }
        spans
    }

    pub(super) fn small_words(alphabet: &[u8], max_len: usize) -> Vec<Vec<u8>> {
        let mut words = vec![Vec::new()];
        let mut layer = vec![Vec::new()];
        for _ in 0..max_len {
            layer = layer
                .iter()
                .flat_map(|word| {
                    alphabet.iter().map(move |byte| {
                        let mut next = word.clone();
                        next.push(*byte);
                        next
                    })
                })
                .collect();
            words.extend(layer.iter().cloned());
        }
        words
    }

    // Overlapping readings can leave a fragment of a value after masking.
    // This tests that no complete encoded reading survives, not that every fragment is masked.
    #[test]
    fn percent_spans_leave_no_complete_encoded_copy() {
        let values = small_words(b"%aA", 3);
        for text in small_words(b"%25aA61", 5) {
            for value in &values {
                let actual = percent_spans(&text, value);
                let mut masked = text.clone();
                for &(start, end) in &actual {
                    masked[start..end].fill(b'#');
                }
                let remaining = percent_oracle(&masked, value);
                assert!(
                    remaining.is_empty(),
                    "text={:?}, value={:?}, remaining={remaining:?}, actual={actual:?}",
                    String::from_utf8_lossy(&text),
                    String::from_utf8_lossy(value)
                );
            }
        }
    }
}
