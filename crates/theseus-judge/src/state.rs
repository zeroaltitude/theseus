//! States (design §2.1, §2.4; `refs/jev.md` rule 2, "state is evidence"):
//! a named-field JSON object from a [`StateBuilder`], with priorities,
//! per-field caps, and a total cap by construction.
//!
//! - Every string is scrubbed (the core's `Scrubber` at the wire-in: every
//!   vault value and known token shapes) before it is cut, so a cut never
//!   leaves half a secret behind.
//! - Each field is first cut to its own cap. If the fields together still
//!   pass the total, the lowest-priority field shrinks first, then the next,
//!   and a field that cannot shrink further is dropped. So `build()` never
//!   returns a state over its cap, whatever the inputs.
//! - A cut text says so in place (`…[cut]`), and a list that lost its oldest
//!   items starts with `{"cut": "N earlier left out"}`. The built state lists
//!   every field it cut or dropped, for the record.
//! - Sizes are measured on the serialized JSON, escapes included, so the
//!   cap holds on what Jev receives. Tokens are estimated as bytes / 4,
//!   rounded up.
//! - Work is bounded by the cap, not the input: a text is windowed around
//!   the part that can be kept before it is scrubbed, with a margin wider
//!   than any secret, so a megabyte of input costs what its cap costs.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::client::estimate_tokens;

/// Bytes per estimated token.
const BYTES_PER_TOKEN: usize = 4;
/// Beyond a text's cap, how much of it is kept and scrubbed before cutting:
/// wider than any secret, so a secret that reaches the kept part is whole
/// in the scrubbed window.
const SCRUB_MARGIN: usize = 4096;

const CUT_HEAD: &str = " …[cut]";
const CUT_TAIL: &str = "[cut]… ";
const CUT_BOTH: &str = " …[cut]… ";

/// Takes secrets out of text before it leaves the process. The core's
/// `Scrubber` implements it at the wire-in.
pub trait Scrub: Send + Sync {
    fn scrub(&self, text: &str) -> String;
}

/// For tests and synthetic states only.
pub struct NoScrub;

impl Scrub for NoScrub {
    fn scrub(&self, text: &str) -> String {
        text.to_string()
    }
}

impl<F> Scrub for F
where
    F: Fn(&str) -> String + Send + Sync,
{
    fn scrub(&self, text: &str) -> String {
        self(text)
    }
}

/// Which part of a long text survives its cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    Head,
    Tail,
    /// The start and the end, cut in the middle.
    Both,
}

/// A built state: the JSON Jev receives, and what the record keeps of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuiltState {
    /// The compact JSON object, fields in the builder's order. Exactly the
    /// bytes hashed and sent.
    pub json: String,
    pub sha256: String,
    pub bytes: usize,
    /// Estimated (bytes / 4, rounded up); never over `cap_tokens`.
    pub tokens: u64,
    pub cap_tokens: u64,
    pub builder: String,
    pub builder_version: u32,
    /// The fields present, in order.
    pub fields: Vec<String>,
    /// The fields cut to fit, dropped ones included.
    pub truncated: Vec<String>,
    /// The fields left out entirely to fit the total.
    pub dropped: Vec<String>,
}

impl BuiltState {
    pub fn value(&self) -> Value {
        serde_json::from_str(&self.json).unwrap_or(Value::Null)
    }
}

/// A long text kept as the windows a cut can use, already scrubbed.
#[derive(Debug, Clone)]
struct Windows {
    head: String,
    /// Empty when `head` is the whole text.
    tail: String,
}

impl Windows {
    fn new(text: &str, cap: usize, scrub: &dyn Scrub) -> Self {
        let w = cap + SCRUB_MARGIN;
        if text.len() <= 2 * w {
            return Self {
                head: scrub.scrub(text),
                tail: String::new(),
            };
        }
        let head_end = floor_boundary(text, w);
        let tail_start = ceil_boundary(text, text.len() - w);
        Self {
            head: scrub.scrub(&text[..head_end]),
            tail: scrub.scrub(&text[tail_start..]),
        }
    }

    fn whole(&self) -> Option<&str> {
        self.tail.is_empty().then_some(self.head.as_str())
    }
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// A char's length inside a JSON string, as serde_json escapes it.
fn json_char_len(c: char) -> usize {
    match c {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{08}' | '\u{0C}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

fn json_str_len(s: &str) -> usize {
    2 + s.chars().map(json_char_len).sum::<usize>()
}

/// The longest prefix whose escaped length fits `room`.
fn take_head(s: &str, room: usize) -> &str {
    let mut used = 0;
    for (i, c) in s.char_indices() {
        used += json_char_len(c);
        if used > room {
            return &s[..i];
        }
    }
    s
}

/// The longest suffix whose escaped length fits `room`.
fn take_tail(s: &str, room: usize) -> &str {
    let mut used = 0;
    for (i, c) in s.char_indices().rev() {
        used += json_char_len(c);
        if used > room {
            return &s[i + c.len_utf8()..];
        }
    }
    s
}

/// The text cut to fit `cap` bytes as a JSON string (quotes included), and
/// whether it was cut; `None` when not even the marker fits.
fn fit_text(w: &Windows, keep: Keep, cap: usize) -> Option<(String, bool)> {
    if let Some(whole) = w.whole() {
        if json_str_len(whole) <= cap {
            return Some((whole.to_string(), false));
        }
    }
    let marker = match keep {
        Keep::Head => CUT_HEAD,
        Keep::Tail => CUT_TAIL,
        Keep::Both => CUT_BOTH,
    };
    let room = cap.checked_sub(2 + marker.len())?;
    let (head_src, tail_src) = match w.whole() {
        Some(t) => (t, t),
        None => (w.head.as_str(), w.tail.as_str()),
    };
    let out = match keep {
        Keep::Head => format!("{}{marker}", take_head(head_src, room)),
        Keep::Tail => format!("{marker}{}", take_tail(tail_src, room)),
        Keep::Both => {
            let h = take_head(head_src, room / 2);
            let used = json_str_len(h) - 2;
            format!("{h}{marker}{}", take_tail(tail_src, room - used))
        }
    };
    Some((out, true))
}

/// Scrubs every string inside a value.
fn scrub_value(v: Value, scrub: &dyn Scrub) -> Value {
    match v {
        Value::String(s) => Value::String(scrub.scrub(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| scrub_value(x, scrub)).collect()),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, x)| (k, scrub_value(x, scrub)))
                .collect(),
        ),
        other => other,
    }
}

fn json(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// The newest items that fit `cap` bytes as a JSON array, marked when some
/// were left out; `None` when not even the marker fits.
fn fit_list(items: &[String], cap: usize) -> Option<(String, bool)> {
    let whole = 2 + items.iter().map(String::len).sum::<usize>() + items.len().saturating_sub(1);
    if whole <= cap {
        return Some((format!("[{}]", items.join(",")), false));
    }
    let marker = |n: usize| json(&serde_json::json!({ "cut": format!("{n} earlier left out") }));
    // Reserve the marker at its longest (every item left out).
    let room = cap.checked_sub(2 + marker(items.len()).len() + 1)?;
    let mut used = 0;
    let mut kept = 0;
    for item in items.iter().rev() {
        let add = item.len() + usize::from(kept > 0);
        if used + add > room {
            break;
        }
        used += add;
        kept += 1;
    }
    let mut out: Vec<&str> = Vec::with_capacity(kept + 1);
    let m = marker(items.len() - kept);
    out.push(&m);
    out.extend(items[items.len() - kept..].iter().map(String::as_str));
    Some((format!("[{}]", out.join(",")), true))
}

#[derive(Debug, Clone)]
enum Body {
    Scalar(String),
    Text {
        windows: Windows,
        keep: Keep,
    },
    /// Each item's JSON, oldest first.
    List(Vec<String>),
}

#[derive(Debug, Clone)]
struct Field {
    name: String,
    priority: u8,
    body: Body,
    /// The value's JSON as rendered now, and whether it was cut.
    rendered: String,
    cut: bool,
}

impl Field {
    fn cost(&self) -> usize {
        json(&Value::String(self.name.clone())).len() + 1 + self.rendered.len()
    }

    /// Re-render under `cap` bytes; false when it cannot fit at all.
    fn fit(&mut self, cap: usize) -> bool {
        let fitted = match &self.body {
            Body::Scalar(s) => (s.len() <= cap).then(|| (s.clone(), false)),
            Body::Text { windows, keep } => {
                fit_text(windows, *keep, cap).map(|(t, cut)| (json(&Value::String(t)), cut))
            }
            Body::List(items) => fit_list(items, cap),
        };
        match fitted {
            Some((r, cut)) => {
                self.rendered = r;
                self.cut |= cut;
                true
            }
            None => false,
        }
    }
}

/// Builds one state. Fields keep the order they are added in.
pub struct StateBuilder<'s> {
    builder: String,
    version: u32,
    cap_tokens: u64,
    scrub: &'s dyn Scrub,
    fields: Vec<Field>,
}

impl<'s> StateBuilder<'s> {
    pub fn new(builder: &str, version: u32, cap_tokens: u64, scrub: &'s dyn Scrub) -> Self {
        Self {
            builder: builder.to_string(),
            version,
            cap_tokens,
            scrub,
            fields: Vec::new(),
        }
    }

    fn push(&mut self, name: &str, priority: u8, body: Body, cap: usize) -> &mut Self {
        let mut f = Field {
            name: name.to_string(),
            priority,
            body,
            rendered: String::new(),
            cut: false,
        };
        if !f.fit(cap) {
            // Not even its marker fits its own cap: `build` drops it.
            f.rendered.clear();
        }
        self.fields.push(f);
        self
    }

    /// A small value: a number, a flag, a short enum or id. Kept until
    /// everything else is gone (priority 255), and never cut, only dropped.
    pub fn scalar(&mut self, name: &str, value: impl Into<Value>) -> &mut Self {
        let v = scrub_value(value.into(), self.scrub);
        let s = json(&v);
        let cap = s.len();
        self.push(name, u8::MAX, Body::Scalar(s), cap)
    }

    /// A text, cut to `cap_tokens` (keeping `keep`), then shrunk by priority
    /// if the state passes its total.
    pub fn text(
        &mut self,
        name: &str,
        priority: u8,
        cap_tokens: u64,
        keep: Keep,
        text: &str,
    ) -> &mut Self {
        let cap = cap_tokens as usize * BYTES_PER_TOKEN;
        let windows = Windows::new(text, cap, self.scrub);
        self.push(name, priority, Body::Text { windows, keep }, cap)
    }

    /// A text when there is one.
    pub fn opt_text(
        &mut self,
        name: &str,
        priority: u8,
        cap_tokens: u64,
        keep: Keep,
        text: Option<&str>,
    ) -> &mut Self {
        match text {
            Some(t) => self.text(name, priority, cap_tokens, keep, t),
            None => self,
        }
    }

    /// A list, oldest first; the newest items that fit are kept.
    pub fn list(
        &mut self,
        name: &str,
        priority: u8,
        cap_tokens: u64,
        items: Vec<Value>,
    ) -> &mut Self {
        let cap = cap_tokens as usize * BYTES_PER_TOKEN;
        let items = items
            .into_iter()
            .map(|v| json(&scrub_value(v, self.scrub)))
            .collect();
        self.push(name, priority, Body::List(items), cap)
    }

    /// A short string for a list item or a scalar: scrubbed, then cut to
    /// `max_chars` characters (marked). Scrubbing first means a cut never
    /// splits a secret.
    pub fn clip(&self, text: &str, max_chars: usize) -> String {
        clip_with(self.scrub, text, max_chars)
    }

    /// The state, under its total cap whatever the inputs.
    pub fn build(self) -> BuiltState {
        let cap = (self.cap_tokens as usize).saturating_mul(BYTES_PER_TOKEN);
        let mut dropped: Vec<String> = Vec::new();
        let mut fields: Vec<Field> = Vec::new();
        for f in self.fields {
            if f.rendered.is_empty() {
                dropped.push(f.name);
            } else {
                fields.push(f);
            }
        }
        let total = |fs: &[Field]| {
            2 + fs.iter().map(Field::cost).sum::<usize>() + fs.len().saturating_sub(1)
        };
        loop {
            let t = total(&fields);
            if t <= cap {
                break;
            }
            let excess = t - cap;
            // The lowest priority first; among equals, the one added last.
            let Some(i) = (0..fields.len()).min_by_key(|&i| (fields[i].priority, usize::MAX - i))
            else {
                break;
            };
            let f = &mut fields[i];
            let before = f.rendered.len();
            let shrunk = matches!(f.body, Body::Text { .. } | Body::List(_))
                && before > excess
                && f.fit(before - excess)
                && f.rendered.len() < before;
            if !shrunk {
                dropped.push(fields.remove(i).name);
            }
        }
        let body: Vec<String> = fields
            .iter()
            .map(|f| format!("{}:{}", json(&Value::String(f.name.clone())), f.rendered))
            .collect();
        let out = format!("{{{}}}", body.join(","));
        let mut truncated: Vec<String> = fields
            .iter()
            .filter(|f| f.cut)
            .map(|f| f.name.clone())
            .collect();
        truncated.extend(dropped.iter().cloned());
        BuiltState {
            sha256: hex::encode(Sha256::digest(out.as_bytes())),
            bytes: out.len(),
            tokens: estimate_tokens(out.len()),
            cap_tokens: self.cap_tokens,
            builder: self.builder,
            builder_version: self.version,
            fields: fields.iter().map(|f| f.name.clone()).collect(),
            truncated,
            dropped,
            json: out,
        }
    }
}

/// Scrubs, then cuts to `max_chars` characters, marked with `…`.
pub fn clip_with(scrub: &dyn Scrub, text: &str, max_chars: usize) -> String {
    let window = floor_boundary(text, text.len().min(max_chars * 4 + SCRUB_MARGIN));
    let s = scrub.scrub(&text[..window]);
    if s.chars().count() <= max_chars && window == text.len() {
        return s;
    }
    let keep: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{keep}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn under_cap(s: &BuiltState) {
        assert!(
            s.tokens <= s.cap_tokens,
            "{} tokens over a cap of {}",
            s.tokens,
            s.cap_tokens
        );
        assert_eq!(s.bytes, s.json.len());
        assert!(s.value().is_object(), "valid JSON: {}", s.json);
    }

    #[test]
    fn fields_keep_their_order_and_a_small_state_is_untouched() {
        let mut b = StateBuilder::new("t", 1, 100, &NoScrub);
        b.text("ask", 9, 50, Keep::Head, "Run the tests.")
            .scalar("loops", 3)
            .list("calls", 5, 50, vec![json!({"tool": "proc.run"})]);
        let s = b.build();
        assert_eq!(
            s.json,
            r#"{"ask":"Run the tests.","loops":3,"calls":[{"tool":"proc.run"}]}"#
        );
        assert_eq!(s.fields, vec!["ask", "loops", "calls"]);
        assert!(s.truncated.is_empty() && s.dropped.is_empty());
        assert_eq!(s.sha256, hex::encode(Sha256::digest(s.json.as_bytes())));
        under_cap(&s);
    }

    #[test]
    fn a_text_past_its_own_cap_is_cut_and_marked_by_its_keep() {
        let long = "a".repeat(1000) + &"z".repeat(1000);
        for (keep, starts, ends) in [
            (Keep::Head, "aaa", "…[cut]"),
            (Keep::Tail, "[cut]…", "zzz"),
            (Keep::Both, "aaa", "zzz"),
        ] {
            let mut b = StateBuilder::new("t", 1, 1000, &NoScrub);
            b.text("x", 1, 25, keep, &long);
            let s = b.build();
            let x = s.value()["x"].as_str().unwrap().to_string();
            assert!(x.starts_with(starts) && x.ends_with(ends), "{keep:?}: {x}");
            assert!(json_str_len(&x) <= 100);
            assert_eq!(s.truncated, vec!["x"]);
            under_cap(&s);
        }
    }

    #[test]
    fn escapes_count_against_the_cap() {
        // 400 quotes are 800 bytes as JSON: a 100-token field holds half.
        let mut b = StateBuilder::new("t", 1, 1000, &NoScrub);
        b.text("q", 1, 100, Keep::Head, &"\"".repeat(400));
        let s = b.build();
        let q = &s.json;
        assert!(q.len() <= 2 + 3 + 1 + 400, "{}", q.len());
        under_cap(&s);
        let mut b = StateBuilder::new("t", 1, 10, &NoScrub);
        b.text("ctl", 1, 10, Keep::Tail, &"\u{1}".repeat(100));
        under_cap(&b.build());
    }

    #[test]
    fn the_total_is_kept_by_shrinking_the_lowest_priority_first() {
        let mut b = StateBuilder::new("t", 1, 100, &NoScrub);
        b.text("important", 9, 100, Keep::Head, &"i".repeat(300))
            .text("minor", 1, 100, Keep::Head, &"m".repeat(300))
            .scalar("loops", 7);
        let s = b.build();
        under_cap(&s);
        let v = s.value();
        let important = v["important"].as_str().unwrap().len();
        let minor = v.get("minor").and_then(Value::as_str).map_or(0, str::len);
        assert!(important > minor, "important {important}, minor {minor}");
        assert_eq!(v["loops"], 7);
        assert!(s.truncated.contains(&"minor".to_string()));
    }

    #[test]
    fn a_list_keeps_its_newest_items_and_says_how_many_it_left_out() {
        let items: Vec<Value> = (0..100).map(|i| json!({"n": i})).collect();
        let mut b = StateBuilder::new("t", 1, 1000, &NoScrub);
        b.list("calls", 1, 20, items);
        let s = b.build();
        under_cap(&s);
        let calls = s.value()["calls"].as_array().unwrap().clone();
        assert!(calls[0]["cut"]
            .as_str()
            .unwrap()
            .ends_with("earlier left out"));
        assert_eq!(calls.last().unwrap()["n"], 99);
        let kept = calls.len() - 1;
        assert_eq!(
            calls[0]["cut"].as_str().unwrap(),
            format!("{} earlier left out", 100 - kept)
        );
        assert_eq!(s.truncated, vec!["calls"]);
    }

    #[test]
    fn huge_inputs_stay_under_any_cap() {
        let huge = "word ".repeat(400_000);
        for cap in [1u64, 2, 5, 50, 4000] {
            let mut b = StateBuilder::new("t", 1, cap, &NoScrub);
            b.text("a", 3, 4000, Keep::Both, &huge)
                .text("b", 2, 4000, Keep::Tail, &huge)
                .list(
                    "c",
                    1,
                    4000,
                    (0..5000)
                        .map(|i| json!({"i": i, "t": "x".repeat(50)}))
                        .collect(),
                )
                .scalar("n", 123_456);
            let s = b.build();
            under_cap(&s);
            if cap <= 2 {
                assert!(!s.dropped.is_empty());
            }
        }
    }

    #[test]
    fn strings_are_scrubbed_before_they_are_cut() {
        // An exact-value scrubber, as the core's is: half a secret would not
        // match it, so a cut made before the scrub would leak the half.
        let secret = "apik".to_string() + &"S".repeat(104);
        let value = secret.clone();
        let scrub = move |t: &str| t.replace(value.as_str(), "[redacted:jev]");
        let text = "x".repeat(10) + &secret + &" tail".repeat(40);
        let mut b = StateBuilder::new("t", 1, 1000, &scrub);
        b.text("arg", 1, 25, Keep::Head, &text)
            .scalar("s", secret.clone())
            .list("l", 1, 100, vec![json!({ "arg": secret })]);
        let s = b.build();
        assert!(
            !s.json.contains("SSSS") && !s.json.contains("apik"),
            "{}",
            s.json
        );
        assert!(s.json.contains("[redacted:jev]"));
        assert_eq!(s.truncated, vec!["arg"]);
        let clipped = clip_with(&scrub, &text, 20);
        assert!(
            !clipped.contains("SSSS") && !clipped.contains("apik"),
            "{clipped}"
        );
        assert!(clipped.ends_with('…'));
        assert_eq!(clipped.chars().count(), 20);
        assert_eq!(clip_with(&NoScrub, "short", 10), "short");
    }
}
