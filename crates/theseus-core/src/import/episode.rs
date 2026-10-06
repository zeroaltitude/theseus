//! One episode of an episode file (format 1), read and checked: every field
//! the format names, its values from the format's lists, its times, its
//! summary's citations, and its hash.
//!
//! **The hash** is the sha256 of the episode's canonical JSON without
//! `hash`: keys sorted, no spaces, as Python's `json.dumps(sort_keys=True,
//! separators=(",", ":"))` writes it. Both of its string forms are
//! accepted (`ensure_ascii` on, which escapes every non-ASCII character,
//! and off), and a float is written as Python writes one (`1e-05`, `0.91`).

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use theseus_protocol::import::EPISODE_FORMAT;

use super::{AsOf, EpisodeLabels, EpisodePlace, EpisodeTriage, Integrity};

pub const SOURCES: &[&str] = &[
    "openclaw-store",
    "openclaw-sessions",
    "openclaw-snapshot",
    "claude-cli",
    "graph-context",
    "workspace",
    "wiki",
    "report",
    "beads",
    "claude-memory",
    "skill",
    "git",
];

pub const PLACE_KINDS: &[&str] = &[
    "dm",
    "slack-channel",
    "discord-channel",
    "cli",
    "cron",
    "heartbeat",
    "file",
];

pub const SENSITIVITIES: &[&str] = &[
    "personal",
    "company-confidential",
    "partner-confidential",
    "public",
];

pub const BOOKS: &[&str] = &[
    "diary",
    "encyclopedia",
    "cookbook",
    "sop",
    "casebook",
    "register",
    "dictionary",
];

/// What the pipeline writes where it removed a credential.
pub const CREDENTIAL_MARK: &str = "⟦credential redacted⟧";

/// A tag's longest name, and its characters: it names a META key and a scope.
const TAG_MAX: usize = 64;

/// An episode, read and checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    pub import_tag: String,
    pub episode_id: String,
    pub source: String,
    pub agent: Option<String>,
    pub place: EpisodePlace,
    pub as_of: AsOf,
    pub labels: EpisodeLabels,
    pub triage: Option<EpisodeTriage>,
    pub summary: Option<Summary>,
    pub messages: Vec<Message>,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub text: String,
    /// The messages it cites, by their `idx`.
    pub cites: Vec<u32>,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub idx: u32,
    pub at_ms: u64,
    pub author: String,
    pub integrity: Integrity,
    pub text: String,
    pub unit: String,
    pub sha256: String,
}

/// A line that does not read: why, and the episode's id when it got that
/// far.
#[derive(Debug, Clone, PartialEq)]
pub struct Bad {
    pub episode_id: Option<String>,
    pub why: String,
}

#[derive(Deserialize)]
struct RawEpisode {
    format: u64,
    import_tag: String,
    episode_id: String,
    source: String,
    agent: Option<String>,
    place: RawPlace,
    as_of: RawAsOf,
    labels: EpisodeLabels,
    #[serde(default)]
    triage: Option<EpisodeTriage>,
    summary: Option<RawSummary>,
    messages: Vec<RawMessage>,
    hash: String,
}

#[derive(Deserialize)]
struct RawPlace {
    kind: String,
    /// As the source records it, or null: the pipeline names no place for a
    /// CLI, a heartbeat, and many channels.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize)]
struct RawAsOf {
    start: String,
    end: String,
}

#[derive(Deserialize)]
struct RawSummary {
    text: String,
    cites: Vec<u32>,
    model: String,
}

#[derive(Deserialize)]
struct RawMessage {
    idx: u32,
    time: String,
    author: String,
    integrity: Integrity,
    text: String,
    unit: String,
    sha256: String,
}

fn hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn time(s: &str, what: &str) -> Result<u64, String> {
    crate::wake::parse_at(s).map_err(|_| format!("{what} is not an RFC 3339 time: {s:?}"))
}

/// A message's author: the operator by name, `agent:<name>`,
/// `person:<name>`, `tool`, or `outside`.
fn author_ok(a: &str) -> bool {
    let named = |p: &str| a.strip_prefix(p).is_some_and(|n| !n.trim().is_empty());
    named("agent:")
        || named("person:")
        || a == "tool"
        || a == "outside"
        || (!a.trim().is_empty() && !a.contains(':'))
}

/// Read one line of an episode file.
pub fn parse(line: &str) -> Result<Episode, Bad> {
    let v: Value = serde_json::from_str(line).map_err(|e| Bad {
        episode_id: None,
        why: format!("not JSON: {e}"),
    })?;
    let id = v
        .get("episode_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let bad = |why: String| Bad {
        episode_id: id.clone(),
        why,
    };
    match v.get("format").and_then(Value::as_u64) {
        Some(EPISODE_FORMAT) => {}
        Some(f) => {
            return Err(bad(format!(
                "format {f}: this build reads format {EPISODE_FORMAT}"
            )))
        }
        None => return Err(bad("no format".into())),
    }
    let raw: RawEpisode =
        serde_json::from_value(v.clone()).map_err(|e| bad(format!("not an episode: {e}")))?;
    check(raw, &v).map_err(bad)
}

fn check(r: RawEpisode, v: &Value) -> Result<Episode, String> {
    debug_assert_eq!(r.format, EPISODE_FORMAT);
    check_names(&r)?;
    let as_of = AsOf {
        start_ms: time(&r.as_of.start, "as_of.start")?,
        end_ms: time(&r.as_of.end, "as_of.end")?,
    };
    if as_of.end_ms < as_of.start_ms {
        return Err("as_of.end is before as_of.start".into());
    }
    let messages = messages_of(r.messages)?;
    let summary = match r.summary {
        Some(s) => {
            if let Some(c) = s
                .cites
                .iter()
                .find(|c| !messages.iter().any(|m| m.idx == **c))
            {
                return Err(format!(
                    "summary cites message {c}, which the episode has not"
                ));
            }
            Some(Summary {
                text: s.text,
                cites: s.cites,
                model: s.model,
            })
        }
        None => None,
    };
    if messages.is_empty() && summary.as_ref().is_none_or(|s| s.text.trim().is_empty()) {
        return Err("no messages and no summary: nothing to import".into());
    }
    if !hash_matches(v, &r.hash) {
        return Err("hash does not match the episode's canonical JSON".into());
    }
    Ok(Episode {
        import_tag: r.import_tag,
        episode_id: r.episode_id,
        source: r.source,
        agent: r.agent,
        place: EpisodePlace {
            kind: r.place.kind,
            name: r.place.name,
            id: r.place.id,
        },
        as_of,
        labels: r.labels,
        triage: r.triage,
        summary,
        messages,
        hash: r.hash,
    })
}

/// The episode's names and labels, each from the format's lists.
fn check_names(r: &RawEpisode) -> Result<(), String> {
    let tag_ok = !r.import_tag.is_empty()
        && r.import_tag.len() <= TAG_MAX
        && r.import_tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if !tag_ok {
        return Err(format!(
            "import_tag {:?}: 1 to {TAG_MAX} letters, digits, `.`, `_` or `-`",
            r.import_tag
        ));
    }
    if !r.episode_id.strip_prefix("ep_").is_some_and(hex64) {
        return Err(format!(
            "episode_id {:?} is not ep_ and 64 hex digits",
            r.episode_id
        ));
    }
    if !hex64(&r.hash) {
        return Err(format!("hash {:?} is not 64 hex digits", r.hash));
    }
    if !SOURCES.contains(&r.source.as_str()) {
        return Err(format!("source {:?} is not one the format names", r.source));
    }
    if !PLACE_KINDS.contains(&r.place.kind.as_str()) {
        return Err(format!(
            "place.kind {:?} is not one the format names",
            r.place.kind
        ));
    }
    let l = &r.labels;
    if !SENSITIVITIES.contains(&l.sensitivity.as_str()) {
        return Err(format!(
            "labels.sensitivity {:?} is not one the format names",
            l.sensitivity
        ));
    }
    if let Some(p) = &l.partner {
        if !p
            .strip_prefix("partner-candidate:")
            .is_some_and(|c| !c.is_empty())
        {
            return Err(format!(
                "labels.partner {p:?} is not partner-candidate:<codename>"
            ));
        }
    }
    if let Some(b) = &l.book_hint {
        if !BOOKS.contains(&b.as_str()) {
            return Err(format!("labels.book_hint {b:?} is not one of the books"));
        }
    }
    if let Some(t) = &r.triage {
        if !(0.0..=1.0).contains(&t.keep) {
            return Err(format!("triage.keep {} is not between 0 and 1", t.keep));
        }
    }
    Ok(())
}

/// The messages, each checked: its author, its digest, its idx once, and
/// its time.
fn messages_of(raw: Vec<RawMessage>) -> Result<Vec<Message>, String> {
    let mut messages: Vec<Message> = Vec::with_capacity(raw.len());
    for m in raw {
        if !author_ok(&m.author) {
            return Err(format!("message {}: author {:?}", m.idx, m.author));
        }
        if !hex64(&m.sha256) {
            return Err(format!("message {}: sha256 is not 64 hex digits", m.idx));
        }
        if messages.iter().any(|x| x.idx == m.idx) {
            return Err(format!("message {}: its idx twice", m.idx));
        }
        messages.push(Message {
            idx: m.idx,
            at_ms: time(&m.time, &format!("message {}'s time", m.idx))?,
            author: m.author,
            integrity: m.integrity,
            text: m.text,
            unit: m.unit,
            sha256: m.sha256,
        });
    }
    Ok(messages)
}

/// Whether `hash` is the sha256 of `v`'s canonical JSON without `hash`,
/// in either string form.
pub fn hash_matches(v: &Value, hash: &str) -> bool {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("hash");
    }
    [true, false].into_iter().any(|ascii| {
        let mut out = String::new();
        canonical(&v, ascii, &mut out);
        hex::encode(Sha256::digest(out.as_bytes())) == hash
    })
}

/// The hash a pipeline writes for `v` (ASCII form): for fixtures.
pub fn hash_of(v: &Value) -> String {
    let mut v = v.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("hash");
    }
    let mut out = String::new();
    canonical(&v, true, &mut out);
    hex::encode(Sha256::digest(out.as_bytes()))
}

/// `v` as Python's `json.dumps(v, sort_keys=True, separators=(",", ":"),
/// ensure_ascii=ascii)` writes it.
pub fn canonical(v: &Value, ascii: bool, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => out.push_str(&i.to_string()),
            (_, Some(u), _) => out.push_str(&u.to_string()),
            (_, _, Some(f)) => out.push_str(&py_float(f)),
            _ => out.push_str(&n.to_string()),
        },
        Value::String(s) => string(s, ascii, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(x, ascii, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(k, ascii, out);
                out.push(':');
                canonical(&o[k], ascii, out);
            }
            out.push('}');
        }
    }
}

fn string(s: &str, ascii: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (ascii && (c as u32) == 0x7f) => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if ascii && !c.is_ascii() => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A float as Python's `repr` writes it: the shortest digits that read
/// back, positional from 1e-4 to below 1e16, else `1e-05`, `1.5e+16`.
pub fn py_float(f: f64) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // Rust's `{:e}` writes the shortest digits that read back.
    let e = format!("{:e}", f.abs());
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let exp: i64 = exp.parse().unwrap_or(0);
    let decpt = exp + 1;
    let sign = if f < 0.0 { "-" } else { "" };
    let n = digits.len() as i64;
    if -4 < decpt && decpt <= 16 {
        let body = if decpt <= 0 {
            format!("0.{}{digits}", "0".repeat((-decpt) as usize))
        } else if decpt >= n {
            format!("{digits}{}.0", "0".repeat((decpt - n) as usize))
        } else {
            let (a, b) = digits.split_at(decpt as usize);
            format!("{a}.{b}")
        };
        format!("{sign}{body}")
    } else {
        let (first, rest) = digits.split_at(1);
        let frac = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let es = if exp < 0 { "-" } else { "+" };
        format!("{sign}{first}{frac}e{es}{:02}", exp.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Python's own reprs, from `python3 -c 'print(repr(x))'`.
    #[test]
    fn floats_are_written_as_python_writes_them() {
        for (f, want) in [
            (0.91, "0.91"),
            (1.0, "1.0"),
            (0.5, "0.5"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (123.456, "123.456"),
            (-2.5e-7, "-2.5e-07"),
            (0.0, "0.0"),
            (1.5e300, "1.5e+300"),
        ] {
            assert_eq!(py_float(f), want, "{f}");
        }
    }

    /// A place the pipeline names `null` (every heartbeat, most CLIs and
    /// many channels of the real files) reads, with no name.
    #[test]
    fn a_place_with_no_name_reads() {
        let line = include_str!("python-ascii.jsonl").trim_end();
        let mut v: Value = serde_json::from_str(line).unwrap();
        v["place"] = serde_json::json!({"kind": "heartbeat", "name": null, "id": null});
        v["hash"] = serde_json::json!(hash_of(&v));
        let e = parse(&v.to_string()).unwrap_or_else(|b| panic!("{}", b.why));
        assert_eq!(e.place.kind, "heartbeat");
        assert_eq!(e.place.name, None);
    }

    /// Two lines a Python pipeline wrote (`json.dumps`, its hash over
    /// `json.dumps(sort_keys=True, separators=(",", ":"))`): one hashed with
    /// `ensure_ascii` on, one with it off and non-ASCII text, a credential
    /// marker and a float Python writes `1e-05`. Both read, hash and all;
    /// a byte changed in either does not.
    #[test]
    fn lines_a_python_pipeline_wrote_read_with_their_hashes() {
        for line in [
            include_str!("python-ascii.jsonl"),
            include_str!("python-unicode.jsonl"),
        ] {
            let e = parse(line.trim_end()).unwrap_or_else(|b| panic!("{}", b.why));
            assert_eq!(e.messages.len(), 3);
            let changed = line.replacen("tide log", "tide book", 1);
            let b = parse(changed.trim_end()).unwrap_err();
            assert!(b.why.contains("hash does not match"), "{}", b.why);
        }
        let uni = parse(include_str!("python-unicode.jsonl").trim_end()).unwrap();
        assert!(uni.messages[1].text.contains(CREDENTIAL_MARK));
        assert_eq!(uni.triage.unwrap().keep, 1e-5);
    }

    /// `json.dumps({"b": "\u00e9\u2028\U0001F600\n\x7f", "a": [1, 0.5, None,
    /// True]}, sort_keys=True, separators=(",", ":"))`, and the same with
    /// `ensure_ascii=False`, as Python 3 writes them.
    #[test]
    fn the_canonical_form_is_pythons_in_both_string_forms() {
        let v: Value =
            serde_json::json!({"b": "é\u{2028}\u{1F600}\n\u{7f}", "a": [1, 0.5, null, true]});
        let mut ascii = String::new();
        canonical(&v, true, &mut ascii);
        assert_eq!(
            ascii,
            r#"{"a":[1,0.5,null,true],"b":"\u00e9\u2028\ud83d\ude00\n\u007f"}"#
        );
        let mut raw = String::new();
        canonical(&v, false, &mut raw);
        assert_eq!(
            raw,
            "{\"a\":[1,0.5,null,true],\"b\":\"é\u{2028}\u{1F600}\\n\u{7f}\"}"
        );
    }
}
