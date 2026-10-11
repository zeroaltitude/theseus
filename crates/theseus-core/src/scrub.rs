//! Secret scrubbing (spec §3.9): tool output is scrubbed before it reaches the
//! model, the store, or a client. Every secret the vault has given is
//! replaced by `[redacted:<name>]` wherever it appears: verbatim, in base64
//! (standard or URL-safe, at any offset inside a longer encoding, wrapped
//! across lines or not), percent-encoded (review 2's H9), and with any of its
//! characters JSON-escaped (theseus-ubp7, `escaped.rs`). Well-known
//! shapes of secrets never resolved here are replaced by `[redacted:<shape>]`
//! (`shapes.rs`): token prefixes (Anthropic, OpenAI, Stripe, Google, GitHub,
//! Slack and others), AWS access key ids and the secret keys and session
//! tokens beside them, private-key blocks, JWTs, and a connection URL's
//! password; as written, and in the same encodings a value is looked for in
//! (`decoded.rs`, theseus-oyrt).
//!
//! The values are read from the secret board at each scrub, so a value is
//! known here the moment it resolves (theseus-qa0). A turn waits for the
//! board's first round to settle before any tool output passes through here.

use std::ops::Range;
use std::sync::Arc;

use base64::Engine as _;

use crate::secrets::{SecretBoard, SecretState};

#[cfg(test)]
mod corpus;
mod decoded;
mod escaped;
mod shapes;
#[cfg(test)]
mod tests_corpus;
#[cfg(test)]
mod tests_escaped;
#[cfg(test)]
mod tests_fold;
#[cfg(test)]
mod tests_nested;
#[cfg(test)]
mod tests_shapes;

#[derive(Default)]
pub struct Scrubber {
    exact: Vec<(String, String)>,
    board: Option<Arc<SecretBoard>>,
}

/// The shortest value scrubbed: a shorter one would match by chance.
const MIN_VALUE: usize = 8;

/// One replacement: a byte range of the text, and what stands in for it.
type Span = (usize, usize, String);

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
        // Longest first so a secret containing another is replaced whole.
        values.sort_by_key(|e| std::cmp::Reverse(e.0.len()));
        let mut out = text.to_string();
        let mut n = 0u32;
        for (v, name) in &values {
            if out.contains(v) {
                n += out.matches(v).count() as u32;
                out = out.replace(v, &format!("[redacted:{name}]"));
            }
        }
        n += encoded(&mut out, &values);
        drop(values);
        drop(states);
        for find in shapes::FINDERS {
            let spans = find(&out);
            n += splice(&mut out, spans);
        }
        let spans = decoded::spans(&out);
        n += splice(&mut out, spans);
        (out, n)
    }
}

/// Replace each span with its stand-in, in one pass from the end. A span
/// that overlaps an earlier one, or that does not fall on character
/// boundaries, is left alone. How many were replaced.
fn splice(out: &mut String, mut spans: Vec<Span>) -> u32 {
    spans.sort_by_key(|s| (s.0, std::cmp::Reverse(s.1)));
    let mut kept: Vec<Span> = Vec::new();
    for s in spans {
        let clear = kept.last().is_none_or(|k| s.0 >= k.1);
        if clear && s.0 < s.1 && out.is_char_boundary(s.0) && out.is_char_boundary(s.1) {
            kept.push(s);
        }
    }
    let n = kept.len() as u32;
    for (a, b, with) in kept.into_iter().rev() {
        out.replace_range(a..b, &with);
    }
    n
}

// ---------------------------------------------------------------- encoded values

/// Each value's base64, percent-encoded, and JSON-escaped forms.
fn encoded(out: &mut String, values: &[(&str, &str)]) -> u32 {
    let mut spans = Vec::new();
    let needles: Vec<(String, &str)> = values
        .iter()
        .flat_map(|(v, name)| {
            base64_needles(v.as_bytes())
                .into_iter()
                .map(move |s| (s, *name))
        })
        .collect();
    for (a, b, name) in base64_spans(out, &needles, |_| true) {
        spans.push((a, b, format!("[redacted:{name}]")));
    }
    if out.contains('%') {
        for (v, name) in values {
            for (a, b) in percent_spans(out.as_bytes(), v.as_bytes()) {
                spans.push((a, b, format!("[redacted:{name}]")));
            }
        }
    }
    for (a, b, name) in escaped::spans(out, values, &needles) {
        spans.push((a, b, format!("[redacted:{name}]")));
    }
    splice(out, spans)
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
            for (at, _) in run.match_indices(needle.as_str()) {
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
    'start: while i < t.len() {
        let (mut j, mut encoded) = (i, false);
        for &want in v {
            match t.get(j) {
                Some(&c) if c == want => j += 1,
                Some(b'%') => match (hex(t.get(j + 1)), hex(t.get(j + 2))) {
                    (Some(h), Some(l)) if h * 16 + l == u32::from(want) => {
                        j += 3;
                        encoded = true;
                    }
                    _ => {
                        i += 1;
                        continue 'start;
                    }
                },
                _ => {
                    i += 1;
                    continue 'start;
                }
            }
        }
        if encoded {
            spans.push((i, j));
            i = j;
        } else {
            i += 1;
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_values_and_token_shapes_are_redacted() {
        let s = Scrubber::with_values(vec![("hunter2hunter2".into(), "db_password".into())]);
        let (out, n) = s.scrub("pw=hunter2hunter2 key=sk-ant-api03-abcdefghijklmnop end ghp_short");
        assert_eq!(n, 2, "{out}");
        assert!(out.contains("[redacted:db_password]"));
        assert!(out.contains("[redacted:anthropic_key] end"));
        assert!(out.contains("ghp_short"), "too short to be a token");
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

    /// Review 2's H9: AWS access key ids, the secret key beside one, and a
    /// secret key or session token after its label. All invented.
    #[test]
    fn aws_keys_are_withheld_by_shape() {
        let s = Scrubber::default();
        let id = "AKIAINVENTEDEXAMPLE7";
        let secret = "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";
        assert_eq!(secret.len(), 40);
        let (out, n) = s.scrub(&format!("Access key ID,Secret access key\n{id},{secret}\n"));
        assert_eq!(
            (out.as_str(), n),
            (
                "Access key ID,Secret access key\n[redacted:aws_access_key_id],\
                 [redacted:aws_secret_access_key]\n",
                2
            )
        );
        let sts = format!(
            "{{\"Credentials\": {{\"AccessKeyId\": \"ASIAINVENTEDEXAMPLE2\", \"SecretAccessKey\": \
             \"{secret}\", \"SessionToken\": \"IQoJb3JpZ2luX2VjEINVENTED/token+value==\"}}}}"
        );
        let (out, n) = s.scrub(&sts);
        assert_eq!(n, 3, "{out}");
        assert_eq!(
            out,
            "{\"Credentials\": {\"AccessKeyId\": \"[redacted:aws_access_key_id]\", \
             \"SecretAccessKey\": \"[redacted:aws_secret_access_key]\", \"SessionToken\": \
             \"[redacted:aws_session_token]\"}}"
        );
        for (line, says) in [
            (
                format!("aws_secret_access_key = {secret}"),
                "aws_secret_access_key = [redacted:aws_secret_access_key]",
            ),
            (
                format!("export AWS_SECRET_ACCESS_KEY='{secret}'"),
                "export AWS_SECRET_ACCESS_KEY='[redacted:aws_secret_access_key]'",
            ),
            (
                "AWS_SESSION_TOKEN=FwoGZXIvYXdzEINVENTEDsessiontoken".into(),
                "AWS_SESSION_TOKEN=[redacted:aws_session_token]",
            ),
        ] {
            assert_eq!(s.scrub(&line).0, says);
        }
        // Not keys: a word with the prefix, a hex hash beside an id, a label
        // with no value after it, and the masked key `aws configure list` shows.
        for kept in [
            "AKIAS are not keys; ASIA is a continent",
            "commit 4b825dc642cb6eb9a060e54bf8d69288fbee4904",
            "set aws_secret_access_key in your profile",
            "secret_key     ****************ABCD shared-credentials-file",
        ] {
            assert_eq!(s.scrub(kept), (kept.to_string(), 0), "{kept}");
        }
        let (out, _) = s.scrub(&format!("{id} 4b825dc642cb6eb9a060e54bf8d69288fbee4904"));
        assert_eq!(
            out,
            "[redacted:aws_access_key_id] 4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        );
    }

    /// Review 2's H9: a private-key block, whole, and one cut before its END
    /// line, up to the first line that is not a key's.
    #[test]
    fn a_private_key_block_is_withheld_whole_or_cut() {
        let s = Scrubber::default();
        let block = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ\nINVENTEDINVENTEDINVENTED==\n-----END OPENSSH PRIVATE KEY-----";
        let (out, n) = s.scrub(&format!("cat id:\n{block}\ndone"));
        assert_eq!(
            (out.as_str(), n),
            ("cat id:\n[redacted:private_key]\ndone", 1)
        );
        let cut = "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\n\nMIIEowIBAAKCAQEAinvented\nb3BlbnNzaC1rZXktdjEAAAAA\n[… 4,000 characters not shown …]\ntail";
        let (out, n) = s.scrub(cut);
        assert_eq!(
            (out.as_str(), n),
            (
                "[redacted:private_key]\n[… 4,000 characters not shown …]\ntail",
                1
            )
        );
        let public = "-----BEGIN PUBLIC KEY-----\nMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEinvented\n-----END PUBLIC KEY-----";
        assert_eq!(
            s.scrub(public),
            (public.to_string(), 0),
            "a public key is not secret"
        );
    }

    /// Review 2's H9: a JWT, and a JWE's five parts.
    #[test]
    fn a_jwt_is_withheld() {
        let s = Scrubber::default();
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJpbnZlbnRlZCJ9.c2lnbmF0dXJlLWludmVudGVk";
        let (out, n) = s.scrub(&format!("Authorization: Bearer {jwt}\n"));
        assert_eq!(
            (out.as_str(), n),
            ("Authorization: Bearer [redacted:jwt]\n", 1)
        );
        let unsigned = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJpbnZlbnRlZCJ9.";
        assert_eq!(s.scrub(unsigned).0, "[redacted:jwt]");
        let jwe = "eyJhbGciOiJSU0EtT0FFUCJ9.aW52ZW50ZWRrZXk.aXZpbnZlbnRlZA.Y2lwaGVydGV4dA.dGFnaW52ZW50ZWQ";
        assert_eq!(s.scrub(&format!("{jwe} x")).0, "[redacted:jwt] x");
        for kept in [
            "eyJustAWord.notatoken",
            "version 1.2.3",
            "eyJhbGciOiJIUzI1NiJ9 alone",
        ] {
            assert_eq!(s.scrub(kept), (kept.to_string(), 0), "{kept}");
        }
    }

    /// A prefix too short to be a token no longer stops the scan for that
    /// prefix: a real token after it is still withheld.
    #[test]
    fn a_short_prefix_match_does_not_stop_the_scan() {
        let s = Scrubber::default();
        let (out, n) = s.scrub("ghp_x then ghp_inventedinventedinvented");
        assert_eq!((out.as_str(), n), ("ghp_x then [redacted:github_token]", 1));
    }
}
