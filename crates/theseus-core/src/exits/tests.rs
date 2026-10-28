//! The exits' substitution: a vault value goes, a shape never does, the same
//! request gives the same bytes, and the row names what was met.

use std::time::Instant;

use serde_json::json;

use super::*;
use crate::ledger::LedgerRow;
use crate::provider::{Delta, FakeProvider};
use crate::secrets::Secret;

const KEY: &str = "vault-key-9f3a7c1e5b2d";
const OTHER: &str = "second-vault-value-77";
/// Looks exactly like a provider key, and was never in the vault.
const LOOKALIKE: &str = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";

fn board(values: &[(&str, &str)]) -> Arc<SecretBoard> {
    let b = SecretBoard::new(values.iter().map(|(n, _)| (*n).to_string()), Instant::now());
    b.publish(
        values
            .iter()
            .map(|(n, v)| ((*n).to_string(), Ok(Secret::new((*v).to_string()))))
            .collect(),
        "fake",
    );
    b
}

fn exits(on: bool) -> Arc<Exits> {
    Exits::new(board(&[("anthropic_key", KEY), ("db_password", OTHER)]), on)
}

/// A request that carries `text` in every place a request has text.
fn request(text: &str) -> ProviderRequest {
    let mut r = ProviderRequest::simple("claude-sonnet-5-5", 64, &format!("user says {text}"));
    r.system = vec![json!({"type": "text", "text": format!("system {text}")})];
    r.messages.push(json!({"role": "assistant", "content": [
        {"type": "tool_use", "id": "t1", "name": "proc_run", "input": {"argv": ["echo", text]}}]}));
    r.messages.push(json!({"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": format!("out: {text}")}]}]}));
    r.tools = vec![json!({"name": "proc_run", "description": format!("runs {text}")})];
    r
}

/// The whole request as the wire would carry it.
fn wire(r: &ProviderRequest) -> String {
    serde_json::to_string(&json!({"system": r.system, "messages": r.messages, "tools": r.tools}))
        .unwrap()
}

/// The vault's values are replaced by their names wherever the request has text.
#[test]
fn a_vault_value_is_substituted_by_its_name_everywhere_in_the_request() {
    let x = exits(true);
    let (sent, found) = x.request(&request(&format!("{KEY} and {OTHER}")));
    let sent = wire(&sent.expect("a request that carried a value is substituted"));
    assert!(!sent.contains(KEY) && !sent.contains(OTHER), "{sent}");
    assert!(sent.contains("[redacted:anthropic_key]") && sent.contains("[redacted:db_password]"));
    assert_eq!(found.names(), ["anthropic_key", "db_password"]);
    // Five places carry text (the user's, the system's, a tool's input, a tool's
    // result, a tool's description), each with both values.
    assert_eq!(found.count(), 10, "{found:?}");
}

/// A request with no value is sent as it is: nothing is copied, nothing named.
#[test]
fn a_request_without_a_vault_value_is_left_alone() {
    let x = exits(true);
    let (sent, found) = x.request(&request("nothing secret here"));
    assert!(sent.is_none() && found.is_empty());
}

/// P2's whole point: a string that looks exactly like a key and was never
/// resolved from the vault is content, and goes through untouched.
#[test]
fn a_shape_that_is_not_a_vault_value_goes_through_untouched() {
    let x = exits(true);
    let shapes = [
        LOOKALIKE,
        "AKIAIOSFODNN7EXAMPLE",
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----",
        "postgres://app:hunter2pass%40db.example.com/prod",
        "password = hunter2",
        "ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789",
    ];
    for s in shapes {
        let (sent, found) = x.request(&request(s));
        assert!(
            sent.is_none() && found.is_empty(),
            "{s} was touched: {found:?}"
        );
        let (t, found) = x.text(s);
        assert_eq!((t.as_ref(), found.is_empty()), (s, true), "{s} was touched");
    }
}

/// Prompt caching: the same request and the same values give the same bytes,
/// run after run, and whatever order the values came to the board in.
#[test]
fn the_substituted_request_is_byte_identical_across_runs_and_value_orders() {
    let req = request(&format!("a {KEY} b {OTHER} c {KEY}"));
    let first = wire(&exits(true).request(&req).0.unwrap());
    let second = wire(&exits(true).request(&req).0.unwrap());
    assert_eq!(first, second, "two runs differ");
    // The board's values are published in another order, in two names for one
    // value and with one a prefix of another. (The board keeps them by name, so
    // what the order of publishing could change is what `scrub::tests` shuffles
    // directly: the order `Substitution` is given.)
    let a = Exits::new(
        board(&[
            ("b", "shared-value-aaaa"),
            ("a", "shared-value-aaaa"),
            ("c", "shared-value-aaaa-longer"),
        ]),
        true,
    );
    let b = Exits::new(
        board(&[
            ("c", "shared-value-aaaa-longer"),
            ("a", "shared-value-aaaa"),
            ("b", "shared-value-aaaa"),
        ]),
        true,
    );
    let req = request("x shared-value-aaaa-longer y shared-value-aaaa z");
    let (a, b) = (a.request(&req), b.request(&req));
    assert_eq!(wire(&a.0.unwrap()), wire(&b.0.unwrap()));
    assert_eq!(a.1, b.1);
    // The longer value is replaced whole, not as its shorter prefix.
    assert!(first
        .contains("[redacted:anthropic_key] b [redacted:db_password] c [redacted:anthropic_key]"));
}

/// The request the caller built is not changed: its digest, its cache and its
/// record stay as they were.
#[test]
fn the_callers_request_is_not_mutated() {
    let req = request(KEY);
    let before = wire(&req);
    let _ = exits(true).request(&req);
    assert_eq!(wire(&req), before);
}

/// `redact_outbound = false`: the value goes as written, and the row can still say
/// which secret it was.
#[test]
fn with_redact_outbound_off_the_value_is_sent_as_written_and_still_named() {
    let x = exits(false);
    let (sent, found) = x.request(&request(KEY));
    assert!(sent.is_none(), "nothing is substituted");
    assert_eq!(found.names(), ["anthropic_key"]);
    let said = format!("key {KEY}");
    let (t, found) = x.text(&said);
    assert_eq!(t.as_ref(), said);
    assert_eq!(found.names(), ["anthropic_key"]);
}

/// Only what the vault resolved: a value still resolving, one that failed, and
/// one too short to be told from the text around it are not substituted; one
/// that resolves later is, from then on.
#[test]
fn only_resolved_values_of_a_usable_length_are_substituted() {
    let b = SecretBoard::new(
        [
            "pending".to_string(),
            "failed".to_string(),
            "short".to_string(),
            "late".to_string(),
        ],
        Instant::now(),
    );
    b.publish(
        [
            ("failed".to_string(), Err("op refused".to_string())),
            ("short".to_string(), Ok(Secret::new("abc".into()))),
        ]
        .into(),
        "fake",
    );
    let x = Exits::new(b.clone(), true);
    assert!(x
        .request(&request("pending failed abc late-value-1234"))
        .0
        .is_none());
    b.publish(
        [(
            "late".to_string(),
            Ok(Secret::new("late-value-1234".into())),
        )]
        .into(),
        "fake",
    );
    let (sent, found) = x.request(&request("pending failed abc late-value-1234"));
    assert_eq!(found.names(), ["late"]);
    assert!(wire(&sent.unwrap()).contains("[redacted:late]"));
}

/// The courier's text: a value is substituted, the rest is as it was.
#[test]
fn text_substitutes_a_vault_value_and_nothing_else() {
    let x = exits(true);
    let said = format!("done: token {KEY}, see {LOOKALIKE}");
    let (t, found) = x.text(&said);
    assert_eq!(
        t.as_ref(),
        format!("done: token [redacted:anthropic_key], see {LOOKALIKE}")
    );
    assert_eq!(found.names(), ["anthropic_key"]);
    let (t, _) = x.text("plain");
    assert!(
        matches!(t, Cow::Borrowed(_)),
        "a text with nothing in it is not copied"
    );
}

/// A store, and the substituting provider over a fake, which records what it was sent.
fn rig(on: bool) -> (Substituting, Arc<FakeProvider>, Store, tempfile::TempDir) {
    let dir = crate::scratch::tempdir();
    let store = Store::open(dir.path()).unwrap();
    let fake = Arc::new(FakeProvider::default());
    let p = Substituting::new("fake", fake.clone(), exits(on), Some(store.clone()));
    (p, fake, store, dir)
}

fn rows(store: &Store) -> Vec<LedgerRow> {
    store
        .ledger_tail::<LedgerRow>(100)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == "request.redacted")
        .collect()
}

/// The provider sends the substituted copy, and writes one `request.redacted`
/// row naming the secret, the model, the loop, and that it was substituted.
#[tokio::test]
async fn the_provider_sends_the_substituted_copy_and_writes_the_row() {
    let (p, fake, store, _dir) = rig(true);
    let req = request(&format!("use {KEY}"));
    let call = Call {
        op: None,
        session: Some("ses_a".into()),
        turn: Some("trn_a".into()),
        loop_index: Some(2),
        correlation_id: Some("act_9".into()),
    };
    let mut sink = |_: Delta<'_>| {};
    for_call(call, || p.stream_message(&req, &mut sink))
        .await
        .unwrap();
    let sent = fake.requests();
    assert_eq!(sent.len(), 1);
    assert!(!wire(&sent[0]).contains(KEY) && wire(&sent[0]).contains("[redacted:anthropic_key]"));
    assert!(
        wire(&req).contains(KEY),
        "the caller's request still holds it"
    );
    let rows = rows(&store);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let (r, d) = (&rows[0], &rows[0].data);
    assert_eq!(
        (r.session_id.as_deref(), r.turn_id.as_deref()),
        (Some("ses_a"), Some("trn_a"))
    );
    assert_eq!(d["names"], json!(["anthropic_key"]));
    assert_eq!(
        (d["count"].as_u64(), d["substituted"].as_bool()),
        (Some(5), Some(true))
    );
    assert_eq!(
        (
            d["loop"].as_u64(),
            d["model"].as_str(),
            d["provider"].as_str()
        ),
        (Some(2), Some("claude-sonnet-5-5"), Some("fake"))
    );
    assert_eq!(d["correlation_id"], "act_9");
    // The row names the secret and never carries its value.
    assert!(!serde_json::to_string(d).unwrap().contains(KEY));
}

/// A request with nothing in it writes no row.
#[tokio::test]
async fn a_clean_request_writes_no_row() {
    let (p, fake, store, _dir) = rig(true);
    let mut sink = |_: Delta<'_>| {};
    p.stream_message(&request(LOOKALIKE), &mut sink)
        .await
        .unwrap();
    assert_eq!(wire(&fake.requests()[0]), wire(&request(LOOKALIKE)));
    assert!(rows(&store).is_empty());
}

/// `redact_outbound = false`: the request goes as written and the row says
/// it was not substituted.
#[tokio::test]
async fn with_the_switch_off_the_provider_sends_it_as_written_and_the_row_says_so() {
    let (p, fake, store, _dir) = rig(false);
    let mut sink = |_: Delta<'_>| {};
    p.stream_message(&request(KEY), &mut sink).await.unwrap();
    assert!(wire(&fake.requests()[0]).contains(KEY));
    let rows = rows(&store);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].data["substituted"], json!(false));
    assert_eq!(rows[0].data["names"], json!(["anthropic_key"]));
}

/// The token counter is sent the same copy: the key is not sent for counting either.
#[tokio::test]
async fn the_token_counter_is_sent_the_substituted_copy() {
    let (p, _fake, store, _dir) = rig(true);
    let req = request(KEY);
    assert!(p.count_tokens(&req).await.is_ok());
    assert_eq!(rows(&store).len(), 1);
}

// ------------------------------------------------------------- encoded values
//
// A vault value a tool printed encoded (`curl -v -u user:$TOKEN` prints it in
// base64, a query string percent-encodes it, a Go tool's JSON escapes it) is in
// the record as printed, and leaves substituted all the same: the exits find
// what `Scrubber` finds (review of R7, finding 1).

/// A value that base64 writes with `+` and `/`, percent-encoding writes in
/// pieces, and JSON can escape.
const ENC: &str = "Inv3nted/Value+For~Tests?x=1";

fn b64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn encoded_exits(on: bool) -> Arc<Exits> {
    Exits::new(board(&[("demo_secret", ENC)]), on)
}

/// `text` leaves both exits without `leaked` in it and with the name, and each
/// reports the secret by name.
fn assert_encoded_goes(text: &str, leaked: &str) {
    let x = encoded_exits(true);
    let (t, found) = x.text(text);
    assert!(!t.contains(leaked), "text still holds {leaked:?}: {t}");
    assert!(t.contains("[redacted:demo_secret]"), "{t}");
    assert_eq!(found.names(), ["demo_secret"], "text: {text}");
    // The request carries the text inside JSON, so a backslash is doubled there.
    let (sent, found) = x.request(&request(text));
    let sent = wire(&sent.expect("a request that carried an encoded value is substituted"));
    let quoted = serde_json::to_string(leaked).unwrap();
    assert!(
        !sent.contains(&quoted[1..quoted.len() - 1]),
        "request still holds {leaked:?}: {sent}"
    );
    assert!(sent.contains("[redacted:demo_secret]"), "{sent}");
    assert_eq!(found.names(), ["demo_secret"], "request: {text}");
}

/// Base64 at each of the three offsets a value can start at inside a longer
/// encoding (`curl -v -u user:value` prints the header), in the standard
/// alphabet and the URL-safe one.
#[test]
fn a_value_in_base64_leaves_by_name_at_every_offset() {
    for before in ["", "u:", "us:", "user:"] {
        let blob = b64(format!("{before}{ENC} and more").as_bytes());
        assert_encoded_goes(&format!("Authorization: Basic {blob}\nok"), &blob);
        let url = blob.replace('+', "-").replace('/', "_").replace('=', "");
        assert_encoded_goes(&format!("?t={url}&"), &url);
    }
}

/// Base64 wrapped across lines, as `base64` writes it at 76 columns: the
/// lines that hold the value go, a line of the filler only stays.
#[test]
fn a_value_in_wrapped_base64_leaves_by_name() {
    let long = b64(format!("{}{ENC}{}", "x".repeat(100), "y".repeat(100)).as_bytes());
    let lines: Vec<&str> = long
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    let wrapped = lines.join("\n");
    let x = encoded_exits(true);
    let said = format!("{wrapped}\nend");
    let (t, found) = x.text(&said);
    assert!(t.contains("[redacted:demo_secret]"), "{t}");
    assert!(
        t.starts_with(lines[0]),
        "a line of filler only is kept: {t}"
    );
    assert_eq!(found.names(), ["demo_secret"]);
    let (sent, found) = x.request(&request(&format!("{wrapped}\nend")));
    assert!(wire(&sent.unwrap()).contains("[redacted:demo_secret]"));
    assert_eq!(found.names(), ["demo_secret"]);
}

/// Percent-encoded, wholly or in part, in either case of hex.
#[test]
fn a_percent_encoded_value_leaves_by_name() {
    let upper = "Inv3nted%2FValue%2BFor~Tests%3Fx%3D1";
    let lower = "Inv3nted%2fValue%2bFor~Tests%3fx%3d1";
    let every: String = ENC.bytes().map(|c| format!("%{c:02X}")).collect();
    for enc in [upper.to_string(), lower.to_string(), every] {
        assert_encoded_goes(&format!("GET /cb?token={enc}&next=1"), &enc);
    }
}

/// JSON-escaped, as PHP (`\/`), Python's ASCII mode and Go (`\u0026`) write it.
#[test]
fn a_json_escaped_value_leaves_by_name() {
    for enc in [
        r"Inv3nted\/Value+For~Tests?x=1",
        r"Inv3nted\u002fValue\u002BFor~Tests\u003fx=1",
    ] {
        assert_encoded_goes(&format!("{{\"token\": \"{enc}\"}}"), enc);
    }
}

/// With the switch off the encoded value goes as it was, and the row still
/// names it: "what key did we send?" has an answer for an encoded key too.
#[test]
fn an_encoded_value_is_named_even_with_the_switch_off() {
    let x = encoded_exits(false);
    let blob = b64(format!("user:{ENC}").as_bytes());
    let text = format!("Authorization: Basic {blob}");
    let (t, found) = x.text(&text);
    assert_eq!(t.as_ref(), text);
    assert_eq!(found.names(), ["demo_secret"]);
    let (sent, found) = x.request(&request(&text));
    assert!(sent.is_none());
    assert_eq!(found.names(), ["demo_secret"]);
}

/// The encoded scan runs on every string of every request: it stays bounded
/// on a transcript of ordinary output, finds nothing in it, and changes
/// nothing. (Measured: see the row's report; this is the guard.)
#[test]
fn a_200_kb_transcript_of_ordinary_output_is_scanned_in_bounded_time() {
    let x = Exits::new(
        board(&[
            ("a", "vault-value-aaaaaaaa"),
            ("b", "vault-value-bbbbbbbb"),
            ("c", "vault-value-cccccccc"),
            ("d", "vault-value-dddddddd"),
            ("e", "vault-value-eeeeeeee"),
            ("f", "vault-value-ffffffff"),
            ("g", "vault-value-gggggggg"),
            ("h", "vault-value-hhhhhhhh"),
            ("i", "vault-value-iiiiiiii"),
            ("j", "vault-value-jjjjjjjj"),
        ]),
        true,
    );
    let line = "   Compiling theseus-core v0.1.0 (/work/src/crates/theseus-core)\n\
                warning: unused import: `std::fmt` at src/lib.rs:42:5 100% done\n\
                {\"level\":\"info\",\"msg\":\"GET /v1/items?id=42&page=3\",\"ms\":12}\n\
                aGVsbG8gd29ybGQ= and dGVzdA== then a path C:\\Users\\x\\file\n";
    let mut req = ProviderRequest::simple("m", 64, "start");
    for i in 0..100 {
        let text = line.repeat(2_000 / line.len() + 1);
        req.messages.push(json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": format!("t{i}"),
             "content": [{"type": "text", "text": text}]}]}));
    }
    assert!(wire(&req).len() > 200_000);
    // What the exits did before the encodings: a `contains` of each value in
    // each string. Printed beside the encoded scan for the report's cost line.
    let texts: Vec<String> = req
        .messages
        .iter()
        .filter_map(|m| m["content"][0]["content"][0]["text"].as_str())
        .map(str::to_string)
        .collect();
    let values: Vec<String> = ('a'..='j')
        .map(|c| format!("vault-value-{}", c.to_string().repeat(8)))
        .collect();
    let began = std::time::Instant::now();
    let verbatim = texts
        .iter()
        .filter(|t| values.iter().any(|v| t.contains(v.as_str())))
        .count();
    let verbatim_took = began.elapsed();
    assert_eq!(verbatim, 0);
    let began = std::time::Instant::now();
    let (sent, found) = x.request(&req);
    let took = began.elapsed();
    assert!(sent.is_none() && found.is_empty(), "{found:?}");
    eprintln!(
        "exits: 200 KB request, ten values, one request: {took:?} (the verbatim scan alone: {verbatim_took:?})"
    );
    assert!(
        took < std::time::Duration::from_secs(5),
        "the encoded scan took {took:?}"
    );
    // The worst inputs for each pass, 200 KB in one string: base64 wrapped at 76
    // columns (every run read, every needle tried), percent escapes end to end,
    // and a JSON string of escapes (decoded twice).
    let blob = b64(&(0..150_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect::<Vec<u8>>());
    let wrapped: Vec<&str> = blob
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    for (what, text) in [
        ("base64 wrapped at 76", wrapped.join("\n")),
        ("percent escapes", "%2F%41%7e".repeat(25_000)),
        ("JSON escapes", "\\n\\\"\\u00e9\\/".repeat(30_000)),
    ] {
        let hard = request(&text);
        let began = std::time::Instant::now();
        let (sent, found) = x.request(&hard);
        let took = began.elapsed();
        assert!(sent.is_none() && found.is_empty(), "{what}: {found:?}");
        eprintln!(
            "exits: {what}, {} KB in each of five places, ten values: {took:?}",
            text.len() / 1000
        );
        assert!(
            took < std::time::Duration::from_secs(5),
            "{what} took {took:?}"
        );
    }
}
