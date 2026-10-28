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
    // The board's values arrive in another order, in two names for one value
    // and with one a prefix of another.
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
