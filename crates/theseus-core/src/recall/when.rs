//! The time a question names (theseus-w9qv, fix B): "March 2026", "last
//! week", "2026-03-14", "from March to May", "since January", read from the
//! turn's own words by a small deterministic parser, in the daemon's time
//! zone (the owner's: `wake::local`), with no model call on the path. Recall
//! keeps to the span when one is named (`Memory::begin_within`).
//!
//! - **What it reads**: ISO days and months (`2026-03-14`, `2026-03`); a
//!   month by name, with a day and a year or without (`March 14, 2026`, `14
//!   March`, `Mar 2026`); a year after a cue (`in 2025`); `today`,
//!   `yesterday`, `this`/`last` `week`/`month`/`year` (last week is the
//!   calendar week before this one, Monday to Sunday), and `past`/`last` N
//!   `days`/`weeks`/`months` (a rolling span ending today). A range joins two
//!   (`to`, `through`, `until`, `-`, `between … and …`), and one end's year
//!   is the other's when it names none; `since` and `after` run to today,
//!   `before` and `until` from the start of time.
//! - **What it leaves alone**: a month's name that is also a word ("may",
//!   "march") counts only with a year, a day, or a cue before it (`in`,
//!   `during`, `since`, `last`, …); a bare number is no year without a cue;
//!   nothing in the future (`next week`) is read. Several spans named apart
//!   are read as the one span that holds them all.
//! - **A month without a year** is its latest that has begun: in October
//!   2026, "March" is March 2026 and "December" December 2025.

use crate::wake::{civil_from_days, days_from_civil};

/// A span a question names: `[from_ms, to_ms)`, either end open, and the
/// words that named it, as the question wrote them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct When {
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub said: String,
}

impl When {
    /// Whether `ms` is inside the span.
    pub fn holds(&self, ms: u64) -> bool {
        self.from_ms.is_none_or(|f| ms >= f) && self.to_ms.is_none_or(|t| ms < t)
    }
}

impl From<&When> for theseus_protocol::memory::RecallWhen {
    fn from(w: &When) -> Self {
        Self {
            said: w.said.clone(),
            from_ms: w.from_ms,
            to_ms: w.to_ms,
        }
    }
}

/// The span the turn's own words name: its new messages' texts (`turn_id`'s
/// user messages), never the reply joined to the query, which may name
/// dates of its own.
pub fn of_turn(nodes: &crate::store::Transcript, turn_id: &str, now_ms: u64) -> Option<When> {
    let words: Vec<&str> = nodes
        .iter()
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn_id))
        .filter_map(|(_, n)| match &n.body {
            crate::node::Body::UserMessage { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    read_local(&words.join("\n"), now_ms)
}

/// The index's answer kept to `when` (theseus-w9qv), said in the manifest:
/// the span, and each hit outside it counted as a `when` drop. A tender of
/// this build answers only from inside the span; one of an older build
/// ignores it, and this keeps recall to it either way.
pub fn kept(
    m: &mut theseus_protocol::memory::RecallManifest,
    when: Option<&When>,
    mut r: theseus_protocol::index::IndexQueryResult,
) -> theseus_protocol::index::IndexQueryResult {
    let Some(w) = when else {
        return r;
    };
    m.when = Some(w.into());
    let before = r.hits.len();
    r.hits.retain(|h| w.holds(h.time_ms));
    let out = (before - r.hits.len()) as u64;
    if out > 0 {
        *m.drops.entry("when".into()).or_default() += out;
    }
    r
}

/// The span `text` names, read in the daemon's time zone as of `now_ms`.
pub fn read_local(text: &str, now_ms: u64) -> Option<When> {
    read(text, now_ms, &|ms| crate::wake::local(ms).offset_secs)
}

/// The span `text` names as of `now_ms`, in the time zone whose offset east
/// of UTC, in seconds, at an instant is `offset`.
pub fn read(text: &str, now_ms: u64, offset: &dyn Fn(u64) -> i64) -> Option<When> {
    let toks = tokens(text);
    if toks.is_empty() {
        return None;
    }
    let today = local_day(now_ms, offset);
    let items = items(&toks, today);
    let mut spans: Vec<(Option<i64>, Option<i64>, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let Item::Span(s) = &items[i] else {
            i += 1;
            continue;
        };
        let mut s = s.clone();
        let cue = s.cue.clone();
        // A range: this span, a joiner, and another.
        if let (Some(Item::Join(between_ok)), Some(Item::Span(e))) =
            (items.get(i + 1), items.get(i + 2))
        {
            let joined = !*between_ok || cue.as_deref() == Some("between");
            if joined {
                let mut e = e.clone();
                fill_years(&mut s, &mut e);
                if e.to > s.from {
                    spans.push((Some(s.from), Some(e.to), s.start, e.end));
                    i += 3;
                    continue;
                }
            }
        }
        let (from, to) = match cue.as_deref() {
            Some("since") => (Some(s.from), Some(today + 1)),
            Some("after") => (Some(s.to), Some(today + 1)),
            Some("before" | "until" | "till") => (None, Some(s.from)),
            _ => (Some(s.from), Some(s.to)),
        };
        if from.is_none_or(|f| to.is_none_or(|t| t > f)) {
            spans.push((from, to, s.start, s.end));
        }
        i += 1;
    }
    if spans.is_empty() {
        return None;
    }
    let from = spans
        .iter()
        .map(|s| s.0)
        .reduce(|a, b| a.zip(b).map(|(a, b)| a.min(b)))
        .flatten();
    let to = spans
        .iter()
        .map(|s| s.1)
        .reduce(|a, b| a.zip(b).map(|(a, b)| a.max(b)))
        .flatten();
    let said: Vec<&str> = spans
        .iter()
        .map(|s| text[toks[s.2].at.0..toks[s.3 - 1].at.1].trim())
        .collect();
    Some(When {
        from_ms: from.map(|d| midnight_ms(d, offset)),
        to_ms: to.map(|d| midnight_ms(d, offset)),
        said: said.join(", "),
    })
}

/// A word of the question: lower case, where it was, and whether it began
/// with a capital.
#[derive(Debug, Clone)]
struct Tok {
    w: String,
    at: (usize, usize),
    cap: bool,
}

fn tokens(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let push = |s: usize, e: usize, out: &mut Vec<Tok>| {
        let raw = &text[s..e];
        let iso = raw.len() >= 7
            && raw.as_bytes()[4] == b'-'
            && raw[..4].bytes().all(|b| b.is_ascii_digit());
        if iso {
            out.push(tok(text, s, e));
            return;
        }
        // `March-May`, `mid-March`: words and a joiner.
        let mut at = s;
        for (k, part) in raw.split('-').enumerate() {
            if k > 0 {
                out.push(tok(text, at - 1, at));
            }
            if !part.is_empty() {
                out.push(tok(text, at, at + part.len()));
            }
            at += part.len() + 1;
        }
    };
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() || c == '-' {
            start.get_or_insert(i);
            continue;
        }
        if let Some(s) = start.take() {
            push(s, i, &mut out);
        }
        // An en or em dash joins as a hyphen does.
        if c == '–' || c == '—' {
            out.push(Tok {
                w: "-".into(),
                at: (i, i + c.len_utf8()),
                cap: false,
            });
        }
    }
    if let Some(s) = start {
        push(s, text.len(), &mut out);
    }
    out
}

fn tok(text: &str, s: usize, e: usize) -> Tok {
    let raw = &text[s..e];
    Tok {
        w: raw.to_lowercase(),
        at: (s, e),
        cap: raw.chars().next().is_some_and(char::is_uppercase),
    }
}

/// A span of local days, `[from, to)`, as the words gave it.
#[derive(Debug, Clone)]
struct Span {
    from: i64,
    to: i64,
    /// A month named without a year, which a range may give one: its month
    /// and day (0: the whole month).
    yearless: Option<(u32, u32)>,
    /// The cue before it (`since`, `between`, …).
    cue: Option<String>,
    /// Its tokens, `[start, end)`.
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
enum Item {
    Span(Span),
    /// A range's joiner; `true` for `and`, which joins only after `between`.
    Join(bool),
    Other,
}

/// The words that make what follows a time: never `to`, `and` or `by`,
/// which name people as often ("talk to May"); a range's far end is read by
/// its joiner instead.
const CUES: [&str; 17] = [
    "in", "during", "since", "from", "last", "this", "early", "late", "mid", "of", "through",
    "until", "till", "between", "before", "after", "around",
];

fn month_of(w: &str) -> Option<u32> {
    let w = w.trim_end_matches('.');
    const NAMES: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    if let Some(i) = NAMES.iter().position(|n| *n == w) {
        return Some(i as u32 + 1);
    }
    if w == "sept" {
        return Some(9);
    }
    // Three letters, but never "may" or "mar" (words of their own) alone.
    (w.len() == 3 && w != "may")
        .then(|| NAMES.iter().position(|n| n.starts_with(w)))
        .flatten()
        .map(|i| i as u32 + 1)
}

fn year_of(w: &str) -> Option<i64> {
    (w.len() == 4 && w.bytes().all(|b| b.is_ascii_digit()))
        .then(|| w.parse::<i64>().ok())
        .flatten()
        .filter(|y| (1990..=2100).contains(y))
}

fn day_num(w: &str) -> Option<u32> {
    let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let suffix = &w[digits.len()..];
    if digits.is_empty() || digits.len() > 2 || !["", "st", "nd", "rd", "th"].contains(&suffix) {
        return None;
    }
    digits.parse().ok().filter(|d| (1..=31).contains(d))
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn day(y: i64, m: u32, d: u32) -> i64 {
    days_from_civil(y, m, d)
}

fn month_span(y: i64, m: u32) -> (i64, i64) {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (day(y, m, 1), day(ny, nm, 1))
}

/// A month without a year: its latest that has begun by `today`.
fn latest_year(m: u32, d: u32, today: i64) -> i64 {
    let (y, tm, td) = civil_from_days(today);
    if (m, d.max(1)) <= (tm, td.max(1)) || (d == 0 && m <= tm) {
        y
    } else {
        y - 1
    }
}

fn span_of(y: i64, m: u32, d: u32) -> (i64, i64) {
    match d {
        0 => month_span(y, m),
        d => (day(y, m, d), day(y, m, d) + 1),
    }
}

/// The cue before the word at `i` (`since`, `between`, …), over `early`,
/// `late` and `mid`.
fn cue_at(t: &[Tok], i: usize) -> Option<String> {
    let mut j = i;
    // Over `early`, `late`, `mid` and `the` to the word before them.
    while j > 0 && ["early", "late", "mid", "the"].contains(&t[j - 1].w.as_str()) {
        j -= 1;
    }
    let before = j.checked_sub(1).map(|k| t[k].w.as_str());
    match before {
        Some(w) if CUES.contains(&w) => Some(w.to_string()),
        _ if j < i && t[i - 1].w != "the" => Some(t[i - 1].w.clone()),
        _ => None,
    }
}

/// A month at `i` (`March`, `March 14`, `March 14, 2026`, `March 2026`,
/// `March of 2026`, `14 March`, `the 14th of March`), pushed to `out` as a
/// span, or as no time when it is only a word: the token after it.
fn month_at(t: &[Tok], i: usize, m: u32, today: i64, out: &mut Vec<Item>) -> usize {
    let w = t[i].w.as_str();
    let mut j = i + 1;
    let mut d = 0;
    if let Some(n) = t.get(j).and_then(|x| day_num(&x.w)) {
        d = n;
        j += 1;
    }
    let mut y = t.get(j).and_then(|x| year_of(&x.w));
    if y.is_none() && t.get(j).is_some_and(|x| x.w == "of") {
        y = t.get(j + 1).and_then(|x| year_of(&x.w));
        j += usize::from(y.is_some());
    }
    j += usize::from(y.is_some());
    // `14 March 2026`, `the 14th of March`: the day before it.
    let mut start = i;
    if d == 0 {
        let of = usize::from(i > 0 && t[i - 1].w == "of");
        if let Some(n) = i.checked_sub(1 + of).and_then(|k| day_num(&t[k].w)) {
            d = n;
            start = i - 1 - of;
            for _ in 0..=of {
                if matches!(out.last(), Some(Item::Other)) {
                    out.pop();
                }
            }
        }
    }
    let cue = cue_at(t, start);
    let word = w == "may" || w.starts_with("mar");
    // A range's far end (`March to May`), or its near one (`March–May 2025`).
    let far = matches!(out.as_slice(), [.., Item::Span(_), Item::Join(_)]);
    let near = t
        .get(j)
        .is_some_and(|x| ["-", "to", "through", "until"].contains(&x.w.as_str()))
        && t.get(j + 1)
            .is_some_and(|x| month_of(&x.w).is_some() || iso(&x.w).is_some());
    let plain = y.is_none() && d == 0 && cue.is_none() && !far && !near && (word || !t[i].cap);
    let yr = y.unwrap_or_else(|| match cue.as_deref() {
        Some("last") => latest_year(m, d, today - 1) - i64::from(m == civil_from_days(today).1),
        _ => latest_year(m, d, today),
    });
    if plain || d > days_in_month(yr, m) {
        out.push(Item::Other);
        return i + 1;
    }
    let (from, to) = span_of(yr, m, d);
    out.push(Item::Span(Span {
        from,
        to,
        yearless: y.is_none().then_some((m, d)),
        cue,
        start,
        end: j,
    }));
    j
}

/// The question's spans and joiners, in order.
fn items(t: &[Tok], today: i64) -> Vec<Item> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let w = t[i].w.as_str();
        let span = |from: i64, to: i64, yearless, end: usize| Span {
            from,
            to,
            yearless,
            cue: cue_at(t, i),
            start: i,
            end,
        };
        // 2026-03-14, 2026-03.
        if let Some((from, to)) = iso(w) {
            out.push(Item::Span(span(from, to, None, i + 1)));
            i += 1;
            continue;
        }
        if let Some(m) = month_of(w) {
            i = month_at(t, i, m, today, &mut out);
            continue;
        }
        // `in 2025`: a year after a cue.
        if let Some(y) = year_of(w) {
            if cue_at(t, i).is_some() {
                out.push(Item::Span(span(
                    day(y, 1, 1),
                    day(y + 1, 1, 1),
                    None,
                    i + 1,
                )));
                i += 1;
                continue;
            }
            // The end of a range: `2024 to 2025`.
            if matches!(out.last(), Some(Item::Join(_))) {
                out.push(Item::Span(span(
                    day(y, 1, 1),
                    day(y + 1, 1, 1),
                    None,
                    i + 1,
                )));
                i += 1;
                continue;
            }
        }
        if let Some((from, to, n)) = relative(t, i, today) {
            out.push(Item::Span(span(from, to, None, i + n)));
            i += n;
            continue;
        }
        match w {
            "to" | "through" | "thru" | "until" | "till" | "-" => out.push(Item::Join(false)),
            "and" => out.push(Item::Join(true)),
            _ => out.push(Item::Other),
        }
        i += 1;
    }
    out
}

fn iso(w: &str) -> Option<(i64, i64)> {
    let p: Vec<&str> = w.split('-').collect();
    let y = year_of(p.first()?)?;
    let m: u32 = p.get(1).filter(|s| s.len() == 2)?.parse().ok()?;
    if !(1..=12).contains(&m) {
        return None;
    }
    match p.get(2) {
        None => Some(month_span(y, m)),
        Some(d) if p.len() == 3 && d.len() == 2 => {
            let d: u32 = d.parse().ok()?;
            (1..=days_in_month(y, m))
                .contains(&d)
                .then(|| span_of(y, m, d))
        }
        Some(_) => None,
    }
}

/// `today`, `yesterday`, `this week`, `last month`, `past 3 days`: the span
/// and how many tokens named it.
fn relative(t: &[Tok], i: usize, today: i64) -> Option<(i64, i64, usize)> {
    let w = t[i].w.as_str();
    match w {
        "today" => return Some((today, today + 1, 1)),
        "yesterday" => return Some((today - 1, today, 1)),
        "this" | "last" | "past" | "previous" => {}
        _ => return None,
    }
    let next = t.get(i + 1)?.w.as_str();
    // `past 3 days`, `last two weeks`.
    if let Some(n) = count(next) {
        let unit = t.get(i + 2)?.w.trim_end_matches('s');
        let from = match unit {
            "day" => today - n + 1,
            "week" => today - 7 * n + 1,
            "month" => {
                let (y, m, d) = civil_from_days(today);
                let back = i64::from(m) - 1 - n;
                let (y, m) = (y + back.div_euclid(12), back.rem_euclid(12) as u32 + 1);
                day(y, m, d.min(days_in_month(y, m))) + 1
            }
            _ => return None,
        };
        return (n > 0).then_some((from, today + 1, 3));
    }
    let (y, m, _) = civil_from_days(today);
    // 1970-01-01 was a Thursday: Monday is day 4's weekday.
    let monday = today - (today - 4).rem_euclid(7);
    let span = match (w, next) {
        ("this", "week") => (monday, today + 1),
        ("last" | "previous", "week") => (monday - 7, monday),
        ("past", "week") => (today - 6, today + 1),
        ("this", "month") => (month_span(y, m).0, today + 1),
        ("last" | "previous", "month") => {
            let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
            month_span(py, pm)
        }
        ("past", "month") => (today - 29, today + 1),
        ("this", "year") => (day(y, 1, 1), today + 1),
        ("last" | "previous", "year") => (day(y - 1, 1, 1), day(y, 1, 1)),
        ("past", "year") => (today - 364, today + 1),
        _ => return None,
    };
    Some((span.0, span.1, 2))
}

fn count(w: &str) -> Option<i64> {
    const WORDS: [&str; 12] = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
        "twelve",
    ];
    if let Some(i) = WORDS.iter().position(|n| *n == w) {
        return Some(i as i64 + 1);
    }
    (w.len() <= 3 && w.bytes().all(|b| b.is_ascii_digit()))
        .then(|| w.parse().ok())
        .flatten()
}

/// A range's two ends: one named without a year takes the other's.
fn fill_years(s: &mut Span, e: &mut Span) {
    let year = |x: &Span| civil_from_days(x.from).0;
    match (s.yearless, e.yearless) {
        (Some((m, d)), None) => {
            let em = civil_from_days(e.from).1;
            let y = year(e) - i64::from(m > em);
            (s.from, s.to) = span_of(y, m, d.min(days_in_month(y, m)));
        }
        (None, Some((m, d))) => {
            let sm = civil_from_days(s.from).1;
            let y = year(s) + i64::from(m < sm);
            (e.from, e.to) = span_of(y, m, d.min(days_in_month(y, m)));
        }
        _ => {}
    }
}

/// The local day `ms` falls on, as days since 1970-01-01.
fn local_day(ms: u64, offset: &dyn Fn(u64) -> i64) -> i64 {
    (ms as i64 / 1000 + offset(ms)).div_euclid(86_400)
}

/// The instant a local day begins: its UTC midnight less the offset there,
/// read again at the guess (a change of offset near midnight).
fn midnight_ms(d: i64, offset: &dyn Fn(u64) -> i64) -> u64 {
    let utc = d * 86_400;
    let mut t = utc - offset(utc.max(0) as u64 * 1000);
    t = utc - offset(t.max(0) as u64 * 1000);
    t.max(0) as u64 * 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3600;

    /// Unix ms of a UTC instant.
    fn utc(y: i64, m: u32, d: u32, h: i64) -> u64 {
        ((days_from_civil(y, m, d) * 86_400 + h * H) * 1000) as u64
    }

    /// 2026-10-07, a Wednesday, at noon UTC.
    fn now() -> u64 {
        utc(2026, 10, 7, 12)
    }

    fn at(off: i64) -> impl Fn(u64) -> i64 {
        move |_| off
    }

    fn span(text: &str) -> Option<(u64, u64, String)> {
        read(text, now(), &at(0))
            .map(|w| (w.from_ms.unwrap_or(0), w.to_ms.unwrap_or(u64::MAX), w.said))
    }

    fn days(text: &str) -> Option<(String, String)> {
        let w = read(text, now(), &at(0))?;
        let d = |ms: Option<u64>| {
            ms.map_or("open".to_string(), |ms| {
                let (y, m, d) = civil_from_days(ms as i64 / 86_400_000);
                format!("{y:04}-{m:02}-{d:02}")
            })
        };
        Some((d(w.from_ms), d(w.to_ms)))
    }

    fn is(text: &str, from: &str, to: &str) {
        assert_eq!(days(text), Some((from.into(), to.into())), "{text:?}");
    }

    #[test]
    fn a_named_month_is_that_month() {
        is(
            "remember our coding sessions from March 2026",
            "2026-03-01",
            "2026-04-01",
        );
        is("what did we do in Mar 2026?", "2026-03-01", "2026-04-01");
        is("in march of 2025", "2025-03-01", "2025-04-01");
        is("the 2026-03 sessions", "2026-03-01", "2026-04-01");
        assert_eq!(
            span("remember our coding sessions from March 2026")
                .unwrap()
                .2,
            "March 2026"
        );
    }

    #[test]
    fn a_month_without_a_year_is_its_latest() {
        is("what happened in March?", "2026-03-01", "2026-04-01");
        is("in December", "2025-12-01", "2026-01-01");
        is("during October", "2026-10-01", "2026-11-01");
        is("last March", "2026-03-01", "2026-04-01");
        is("last October", "2025-10-01", "2025-11-01");
    }

    #[test]
    fn days_by_name_and_by_number() {
        is("on 2026-03-14", "2026-03-14", "2026-03-15");
        is("March 14, 2026", "2026-03-14", "2026-03-15");
        is("on the 14th of March", "2026-03-14", "2026-03-15");
        is("14 March 2026", "2026-03-14", "2026-03-15");
        is("Feb 29 2024", "2024-02-29", "2024-03-01");
        assert_eq!(days("Feb 30 2026"), None, "no such day");
        assert_eq!(days("2026-02-30"), None);
        assert_eq!(days("2026-13"), None);
    }

    #[test]
    fn ranges_join_two_ends() {
        is("from March to May 2026", "2026-03-01", "2026-06-01");
        is("between January and March", "2026-01-01", "2026-04-01");
        is("2026-03-01 to 2026-03-15", "2026-03-01", "2026-03-16");
        is("March–May 2025", "2025-03-01", "2025-06-01");
        is("from November 2025 to February", "2025-11-01", "2026-03-01");
        is("since January", "2026-01-01", "2026-10-08");
        is("since 2026-09-30", "2026-09-30", "2026-10-08");
        is("before March 2026", "open", "2026-03-01");
        is("in 2025", "2025-01-01", "2026-01-01");
        is("from 2024 to 2025", "2024-01-01", "2026-01-01");
    }

    #[test]
    fn relative_words_count_from_today() {
        // 2026-10-07 is a Wednesday: this week began Monday the 5th.
        is("what did I do today", "2026-10-07", "2026-10-08");
        is("yesterday's notes", "2026-10-06", "2026-10-07");
        is("this week", "2026-10-05", "2026-10-08");
        is("last week", "2026-09-28", "2026-10-05");
        is("the past week", "2026-10-01", "2026-10-08");
        is("last month", "2026-09-01", "2026-10-01");
        is("this month", "2026-10-01", "2026-10-08");
        is("last year", "2025-01-01", "2026-01-01");
        is("in the past 3 days", "2026-10-05", "2026-10-08");
        is("over the last two weeks", "2026-09-24", "2026-10-08");
    }

    #[test]
    fn words_that_only_look_like_dates_are_no_time() {
        for text in [
            "you may want to march the tests through",
            "May I ask about the parser?",
            "March the build forward",
            "run 2026 iterations",
            "the next week looks busy",
            "fix the 14 failing tests",
            "the sea is calm",
            "",
        ] {
            assert_eq!(days(text), None, "{text:?}");
        }
        // A name with a cue or a year is a month again.
        is("in May", "2026-05-01", "2026-06-01");
        is("May 2026", "2026-05-01", "2026-06-01");
    }

    #[test]
    fn spans_named_apart_are_held_by_one() {
        is("in March or in May", "2026-03-01", "2026-06-01");
    }

    /// The owner's time zone sets each day's edges: a March in Phoenix
    /// (UTC-7, no daylight time) begins at 07:00 UTC, and in Auckland's
    /// summer (UTC+13) at 11:00 UTC the day before; "today" is the owner's.
    #[test]
    fn the_time_zone_sets_the_edges() {
        let phoenix = read("in March 2026", now(), &at(-7 * H)).unwrap();
        assert_eq!(phoenix.from_ms, Some(utc(2026, 3, 1, 7)));
        assert_eq!(phoenix.to_ms, Some(utc(2026, 4, 1, 7)));
        assert!(
            !phoenix.holds(utc(2026, 3, 1, 6)),
            "Feb 28th, 23:00 in Phoenix"
        );
        assert!(phoenix.holds(utc(2026, 3, 1, 7)));
        assert!(
            phoenix.holds(utc(2026, 4, 1, 6)),
            "March 31st, 23:00 in Phoenix"
        );
        assert!(!phoenix.holds(utc(2026, 4, 1, 7)));
        let auckland = read("in March 2026", now(), &at(13 * H)).unwrap();
        assert_eq!(auckland.from_ms, Some(utc(2026, 2, 28, 11)));
        // At 23:30 UTC on the 7th it is already the 8th in Auckland, and
        // still the 7th in Phoenix.
        let late = utc(2026, 10, 7, 23) + 1_800_000;
        let today = |off| read("today", late, &at(off)).unwrap().from_ms.unwrap();
        assert_eq!(today(13 * H), utc(2026, 10, 7, 11));
        assert_eq!(today(-7 * H), utc(2026, 10, 7, 7));
        // An offset that changes at the edge (daylight time begins): the
        // day begins at the offset in force there.
        let dst = |ms: u64| {
            if ms >= utc(2026, 3, 8, 10) {
                -7 * H
            } else {
                -8 * H
            }
        };
        let w = read("2026-03-09", now(), &dst).unwrap();
        assert_eq!(w.from_ms, Some(utc(2026, 3, 9, 7)));
        let w = read("2026-03-08", now(), &dst).unwrap();
        assert_eq!(w.from_ms, Some(utc(2026, 3, 8, 8)));
    }

    /// The parse costs microseconds, never a call: a long turn's words.
    #[test]
    fn a_long_question_reads_in_microseconds() {
        let text = "please remember what we built in March 2026, the parser and the tide tables; "
            .repeat(40);
        let t0 = std::time::Instant::now();
        for _ in 0..100 {
            assert!(read(&text, now(), &at(0)).is_some());
        }
        let each = t0.elapsed() / 100;
        // A debug build, under load: generous, and still far from a call.
        assert!(each < std::time::Duration::from_millis(5), "{each:?}");
    }
}
