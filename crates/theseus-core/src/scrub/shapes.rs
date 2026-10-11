//! The shapes of secrets never resolved here (spec §3.9; review 2's H9;
//! theseus-oyrt): token prefixes, AWS access key ids and the secret keys and
//! session tokens beside them, private-key blocks, JWTs, and a password in a
//! connection URL. Each finder returns the byte ranges of a text it would
//! withhold, with the stand-in for each.
//!
//! A shape with no prefix of its own (Twilio's auth token, a bare hex or
//! base62 secret) is not here: it cannot be told from a hash or an id
//! without false positives.

use std::ops::Range;

use super::Span;

/// The characters a token's body may hold.
#[derive(Clone, Copy)]
enum Body {
    /// Letters, digits, `_`, and `-`.
    Token,
    /// Letters and digits.
    Alnum,
    /// Letters, digits, `_`, `-`, and `.`.
    Dotted,
    /// Hex digits, lower case.
    Hex,
}

impl Body {
    fn holds(self, c: u8) -> bool {
        match self {
            Body::Token => c.is_ascii_alphanumeric() || c == b'_' || c == b'-',
            Body::Alnum => c.is_ascii_alphanumeric(),
            Body::Dotted => c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'),
            Body::Hex => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        }
    }
}

/// What else a body needs to be a token, as a random one has.
#[derive(Clone, Copy, PartialEq)]
enum Needs {
    Nothing,
    /// A capital and a small letter.
    Cases,
    /// A capital, a small letter, and a digit.
    CasesAndDigit,
}

/// A token known by its prefix.
struct Prefix {
    prefix: &'static str,
    shape: &'static str,
    body: Body,
    /// The fewest characters after the prefix, and the most (0: no most).
    min: usize,
    max: usize,
    needs: Needs,
    /// No letter or digit right before the prefix (`disk-` holds no `sk-`).
    alone: bool,
    /// Prefixes that begin with this one and are another shape's.
    not: &'static [&'static str],
}

/// The prefixes the first table held: at least 8 characters after the
/// prefix, of letters, digits, `_`, and `-`.
const fn old(prefix: &'static str, shape: &'static str) -> Prefix {
    Prefix {
        prefix,
        shape,
        body: Body::Token,
        min: 8,
        max: 0,
        needs: Needs::Nothing,
        alone: false,
        not: &[],
    }
}

/// A newer prefix (theseus-oyrt): standing alone, at least `min` characters
/// of `body` after it.
const fn new(prefix: &'static str, shape: &'static str, body: Body, min: usize) -> Prefix {
    Prefix {
        prefix,
        shape,
        body,
        min,
        max: 0,
        needs: Needs::Nothing,
        alone: true,
        not: &[],
    }
}

/// Token prefixes worth catching even when the value was never resolved
/// here. Stripe's `pk_live_` and `pk_test_` keys are left: they are public
/// by design, written into every page that takes a payment.
const PREFIXES: &[Prefix] = &[
    old("sk-ant-", "anthropic_key"),
    old("ghp_", "github_token"),
    old("github_pat_", "github_token"),
    old("gho_", "github_token"),
    old("ops_", "op_service_account"),
    old("xoxb-", "slack_token"),
    old("xoxp-", "slack_token"),
    // OpenAI: `sk-`, `sk-proj-`, `sk-svcacct-`, `sk-admin-`, and a long
    // random body; never Anthropic's `sk-ant-`.
    Prefix {
        needs: Needs::CasesAndDigit,
        not: &["sk-ant-"],
        ..new("sk-", "openai_key", Body::Token, 32)
    },
    new("sk_live_", "stripe_key", Body::Alnum, 16),
    new("sk_test_", "stripe_key", Body::Alnum, 16),
    new("rk_live_", "stripe_key", Body::Alnum, 16),
    new("rk_test_", "stripe_key", Body::Alnum, 16),
    new("whsec_", "stripe_webhook_secret", Body::Alnum, 24),
    // Google: `AIza` and exactly 35 more.
    Prefix {
        max: 35,
        ..new("AIza", "google_api_key", Body::Token, 35)
    },
    new("ghu_", "github_token", Body::Alnum, 30),
    new("ghs_", "github_token", Body::Alnum, 30),
    new("ghr_", "github_token", Body::Alnum, 30),
    new("xoxa-", "slack_token", Body::Token, 8),
    new("xoxr-", "slack_token", Body::Token, 8),
    new("xoxs-", "slack_token", Body::Token, 8),
    new("xoxe-", "slack_token", Body::Token, 8),
    new("xapp-", "slack_token", Body::Token, 8),
    new("glpat-", "gitlab_token", Body::Token, 20),
    new("npm_", "npm_token", Body::Alnum, 36),
    new("pypi-AgEI", "pypi_token", Body::Token, 32),
    // Hugging Face: `hf_` and 34 letters; `hf_hub_download` is a name.
    Prefix {
        needs: Needs::Cases,
        ..new("hf_", "huggingface_token", Body::Alnum, 30)
    },
    // SendGrid: `SG.`, 22 characters, `.`, and 43.
    new("SG.", "sendgrid_key", Body::Dotted, 60),
    new("dop_v1_", "digitalocean_token", Body::Hex, 64),
    new("shpat_", "shopify_token", Body::Hex, 32),
    new("shpss_", "shopify_token", Body::Hex, 32),
    new("gsk_", "groq_key", Body::Alnum, 40),
];

/// Every shape in `s`, each finder's spans in turn. Where two overlap, the
/// splice keeps the one that starts first.
pub(super) fn all(s: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    for find in FINDERS {
        spans.extend(find(s));
    }
    spans
}

/// The finders, in the order the scrub applies them to the output.
pub(super) const FINDERS: [fn(&str) -> Vec<Span>; 5] =
    [private_keys, jwts, aws, prefixed, url_passwords];

/// A prefixed token: its prefix and a body its row allows. A body too short
/// or too long, or missing what its row needs, is skipped, and the scan goes
/// on past its prefix. One pass over the text: only a byte that begins some
/// prefix is looked at further.
fn prefixed(s: &str) -> Vec<Span> {
    let b = s.as_bytes();
    let mut spans = Vec::new();
    // Where each row may match next: past its last match.
    let mut next = [0usize; PREFIXES.len()];
    for i in 0..b.len() {
        let mut rows = STARTS[usize::from(b[i])];
        while rows != 0 {
            let k = rows.trailing_zeros() as usize;
            rows &= rows - 1;
            let p = &PREFIXES[k];
            if i < next[k] || !b[i..].starts_with(p.prefix.as_bytes()) {
                continue;
            }
            let body_at = i + p.prefix.len();
            let end = b[body_at..]
                .iter()
                .position(|c| !p.body.holds(*c))
                .map_or(b.len(), |e| body_at + e);
            let body = &b[body_at..end];
            let fits = body.len() >= p.min
                && (p.max == 0 || body.len() <= p.max)
                && (!p.alone || i == 0 || !b[i - 1].is_ascii_alphanumeric())
                && !p.not.iter().any(|n| s[i..].starts_with(n))
                && match p.needs {
                    Needs::Nothing => true,
                    Needs::Cases => cases(body),
                    Needs::CasesAndDigit => cases(body) && body.iter().any(u8::is_ascii_digit),
                };
            if fits {
                spans.push((i, end, format!("[redacted:{}]", p.shape)));
                next[k] = end;
            }
        }
    }
    spans
}

/// For each byte, the rows whose prefix begins with it, a bit each.
const STARTS: [u64; 256] = {
    assert!(PREFIXES.len() <= 64);
    let mut starts = [0u64; 256];
    let mut k = 0;
    while k < PREFIXES.len() {
        starts[PREFIXES[k].prefix.as_bytes()[0] as usize] |= 1 << k;
        k += 1;
    }
    starts
};

fn cases(body: &[u8]) -> bool {
    body.iter().any(u8::is_ascii_uppercase) && body.iter().any(u8::is_ascii_lowercase)
}

/// A password in a URL's authority, `scheme://user:password@host`, for any
/// scheme: the password alone is withheld, so the URL stays readable. A URL
/// with no password, an empty one, or one that stands for a value given
/// elsewhere (`${DB_PASSWORD}`, `$PW`, `<password>`, `{{ pw }}`, `%(pw)s`,
/// `****`) is left as it is. The authority ends at the first `/`, `?`, `#`,
/// space, quote, `<`, `>`, or backslash, and its last `@` ends the user's
/// part, so an `@` in a password written raw is read as the password's.
fn url_passwords(s: &str) -> Vec<Span> {
    let b = s.as_bytes();
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(rel) = s[from..].find("://") {
        let at = from + rel;
        from = at + 3;
        let scheme = b[..at]
            .iter()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-'))
            .count();
        if scheme == 0 || !b[at - scheme..at].iter().any(u8::is_ascii_alphabetic) {
            continue;
        }
        let auth = at + 3;
        let auth_end = b[auth..]
            .iter()
            .position(|c| {
                c.is_ascii_whitespace()
                    || matches!(
                        c,
                        b'/' | b'?' | b'#' | b'"' | b'\'' | b'`' | b'<' | b'>' | b'\\'
                    )
            })
            .map_or(b.len(), |e| auth + e);
        let Some(host) = s[auth..auth_end].rfind('@').map(|e| auth + e) else {
            continue;
        };
        let Some(colon) = s[auth..host].find(':').map(|e| auth + e) else {
            continue;
        };
        let pw = &s[colon + 1..host];
        if host + 1 < auth_end && !pw.is_empty() && !stands_for_a_value(pw) {
            spans.push((colon + 1, host, "[redacted:url_password]".into()));
        }
        from = auth_end.max(from);
    }
    spans
}

/// A password that names a value given elsewhere, or one already withheld.
fn stands_for_a_value(pw: &str) -> bool {
    pw.starts_with('$')
        || pw.starts_with("{{")
        || pw.starts_with('<')
        || pw.starts_with("%(")
        || pw.starts_with("[redacted:")
        || pw.bytes().all(|c| c == b'*' || c == b'x' || c == b'X')
}

/// Private-key blocks: `-----BEGIN … PRIVATE KEY-----` to its END line. When
/// the END line is missing, as in a cut result, the block runs to the end of
/// the BEGIN line and over the lines after it that look like a key's.
fn private_keys(s: &str) -> Vec<Span> {
    const BEGIN: &str = "-----BEGIN ";
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(rel) = s[from..].find(BEGIN) {
        let start = from + rel;
        let label_at = start + BEGIN.len();
        from = label_at;
        let line_end = s[start..].find('\n').map_or(s.len(), |e| start + e);
        let Some(len) = s[label_at..line_end].find("-----") else {
            continue;
        };
        let label = &s[label_at..label_at + len];
        if !label.contains("PRIVATE KEY") {
            continue;
        }
        let end_line = format!("-----END {label}-----");
        let end = match s[label_at..].find(&end_line) {
            Some(e) => label_at + e + end_line.len(),
            None => key_body_end(s, line_end),
        };
        spans.push((start, end, "[redacted:private_key]".into()));
        from = end;
    }
    spans
}

/// The end of the lines after `at` (a line's end) that look like a key's
/// body: base64, a `Name: value` header, or blank.
fn key_body_end(s: &str, mut at: usize) -> usize {
    while at < s.len() {
        let next = at + 1;
        let end = s[next..].find('\n').map_or(s.len(), |e| next + e);
        let line = s[next..end].trim_end_matches('\r');
        let body = line
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'='))
            || line.contains(": ");
        if !body {
            break;
        }
        at = end;
    }
    at
}

/// JWTs: three dot-joined base64url parts, the first `eyJ` (`{"`), the first
/// two at least 10 characters, and the signature possibly empty; a JWE's two
/// further parts go with it.
fn jwts(s: &str) -> Vec<Span> {
    let b = s.as_bytes();
    let url = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'_';
    let run = |mut j: usize| {
        while j < b.len() && url(b[j]) {
            j += 1;
        }
        j
    };
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(rel) = s[from..].find("eyJ") {
        let i = from + rel;
        from = i + 3;
        if i > 0 && url(b[i - 1]) {
            continue;
        }
        let header = run(i);
        if header - i < 10 || b.get(header) != Some(&b'.') {
            continue;
        }
        let payload = run(header + 1);
        if payload - (header + 1) < 10 || b.get(payload) != Some(&b'.') {
            continue;
        }
        let mut end = run(payload + 1);
        for _ in 0..2 {
            if b.get(end) == Some(&b'.') && run(end + 1) > end + 1 {
                end = run(end + 1);
            } else {
                break;
            }
        }
        spans.push((i, end, "[redacted:jwt]".into()));
        from = end;
    }
    spans
}

/// AWS: access key ids (`AKIA` or `ASIA`, and 16 capitals and digits); a
/// 40-character secret key on the same line as one, as the console's CSV and
/// a pasted pair put them; and a secret key or session token after a label
/// that names one (`aws_secret_access_key = …`, `"SecretAccessKey": "…"`,
/// `AWS_SESSION_TOKEN=…`).
fn aws(s: &str) -> Vec<Span> {
    let b = s.as_bytes();
    let mut spans = Vec::new();
    for prefix in ["AKIA", "ASIA"] {
        let mut from = 0;
        while let Some(rel) = s[from..].find(prefix) {
            let i = from + rel;
            from = i + prefix.len();
            let end = i + 20;
            let id = end <= b.len()
                && b[i + 4..end]
                    .iter()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && (i == 0 || !b[i - 1].is_ascii_alphanumeric())
                && !b.get(end).is_some_and(u8::is_ascii_alphanumeric);
            if !id {
                continue;
            }
            spans.push((i, end, "[redacted:aws_access_key_id]".into()));
            let line = s[..i].rfind('\n').map_or(0, |p| p + 1)
                ..s[end..].find('\n').map_or(s.len(), |p| end + p);
            spans.extend(bare_secret_keys(b, line));
        }
    }
    spans.extend(labeled(s));
    spans
}

/// 40-character tokens of the secret keys' alphabet within `line`, each with
/// a capital, `/`, or `+` (so a 40-digit hex hash is not one).
fn bare_secret_keys(b: &[u8], line: Range<usize>) -> Vec<Span> {
    let key = |c: u8| c.is_ascii_alphanumeric() || c == b'/' || c == b'+';
    let mut spans = Vec::new();
    let mut i = line.start;
    while i < line.end {
        if !key(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < line.end && key(b[i]) {
            i += 1;
        }
        let token = &b[start..i];
        let alone = b.get(i) != Some(&b'=');
        let mixed = token
            .iter()
            .any(|c| c.is_ascii_uppercase() || matches!(c, b'/' | b'+'));
        if token.len() == 40 && alone && mixed {
            spans.push((start, i, "[redacted:aws_secret_access_key]".into()));
        }
    }
    spans
}

/// `word`, its underscores dropped and its case folded, ends with `tail`
/// (lower case, no underscores).
fn ends_folded(word: &[u8], tail: &str) -> bool {
    let mut w = word
        .iter()
        .rev()
        .filter(|c| **c != b'_')
        .map(u8::to_ascii_lowercase);
    tail.bytes().rev().all(|t| w.next() == Some(t))
}

/// A secret key or session token after the label that names it: the value
/// (16 or more characters of base64) after at most 6 of ` \t"':=`.
fn labeled(s: &str) -> Vec<Span> {
    let b = s.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let value = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'/' | b'+' | b'=');
    let mut spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !word(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && word(b[i]) {
            i += 1;
        }
        let w = &b[start..i];
        let shape = if ends_folded(w, "secretaccesskey") {
            "aws_secret_access_key"
        } else if ends_folded(w, "sessiontoken") || ends_folded(w, "securitytoken") {
            "aws_session_token"
        } else {
            continue;
        };
        let mut j = i;
        while j < b.len() && j - i < 6 && matches!(b[j], b' ' | b'\t' | b'"' | b'\'' | b':' | b'=')
        {
            j += 1;
        }
        let v = j;
        while j < b.len() && value(b[j]) {
            j += 1;
        }
        if j - v >= 16 {
            spans.push((v, j, format!("[redacted:{shape}]")));
            i = j;
        }
    }
    spans
}
