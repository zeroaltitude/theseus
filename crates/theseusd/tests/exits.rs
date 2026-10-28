//! The exits end to end (theseus-gyjyo R7b): a scratch daemon whose vault
//! holds one invented value, and the stand-in Messages API behind it. The
//! owner pastes that value into a message, as one does: the stand-in's request
//! shows `[redacted:<name>]` and never the value, the ledger's
//! `request.redacted` row and `turn.inspect` name the secret, and a string
//! that only looks like a key goes through untouched. With
//! `redact_outbound = false` the value goes as written, and the row still
//! says which secret it was. The record holds what the owner typed either way.
//!
//! The half that is R7a's (a tool's output kept raw in its node, so a job that
//! prints the value leaves it in the node and the spool) is not here: on this
//! branch alone the tool result is still scrubbed at the node.

mod common;

use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Served;
use serde_json::{json, Value};

const NAME: &str = "harbor_token";
const VALUE: &str = "harbor-vault-value-5d2a9c04e7b1";
/// Looks exactly like a provider key; the vault never held it.
const LOOKALIKE: &str = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";

/// `deploy:` and the vault value in base64, as `curl -v -u deploy:$TOKEN` prints
/// it after `Authorization: Basic`.
const BASIC: &str = "ZGVwbG95OmhhcmJvci12YXVsdC12YWx1ZS01ZDJhOWMwNGU3YjE=";

fn daemon(redact: bool) -> (Served, FakeModel) {
    daemon_with(redact, |_| vec![])
}

fn daemon_with(
    redact: bool,
    calls: impl Fn(&str) -> Vec<(&'static str, Value)> + Send + Sync + 'static,
) -> (Served, FakeModel) {
    let model = FakeModel::start(calls);
    let base = model.base.clone();
    let dir = std::cell::RefCell::new(std::path::PathBuf::new());
    let d = Served::start(
        |d| {
            use std::os::unix::fs::PermissionsExt;
            for (f, v) in [
                ("provider-key", "tv-provider-key-invented"),
                ("harbor", VALUE),
            ] {
                std::fs::write(d.join(f), v).unwrap();
                std::fs::set_permissions(d.join(f), std::fs::Permissions::from_mode(0o600))
                    .unwrap();
            }
            *dir.borrow_mut() = d.to_path_buf();
        },
        |t| {
            let table = |t: &mut toml::Table, key: &str| -> toml::Table {
                t.get(key)
                    .and_then(|v| v.as_table())
                    .cloned()
                    .unwrap_or_default()
            };
            let mut m = table(t, "model");
            m.insert("api_base".into(), base.clone().into());
            t.insert("model".into(), m.into());
            let mut providers = table(t, "providers");
            for (_, p) in providers.iter_mut() {
                p.as_table_mut()
                    .unwrap()
                    .insert("api_base".into(), base.clone().into());
            }
            t.insert("providers".into(), providers.into());
            // Every configured secret is the provider's invented key, and one
            // more is the vault value under test.
            let mut secrets = table(t, "secrets");
            let file = |f: &str| format!("file:{}", dir.borrow().join(f).display());
            for (_, v) in secrets.iter_mut() {
                *v = file("provider-key").into();
            }
            secrets.insert(NAME.into(), file("harbor").into());
            t.insert("secrets".into(), secrets.into());
            let mut privacy = table(t, "privacy");
            privacy.insert("redact_outbound".into(), redact.into());
            t.insert("privacy".into(), privacy.into());
            let mut policy = table(t, "policy");
            policy.insert("enforcement".into(), "notify".into());
            t.insert("policy".into(), policy.into());
        },
    );
    (d, model)
}

/// A turn that says `input`: its session, and its turn id once it has ended.
fn say(d: &Served, model: &FakeModel, input: &str) -> (Value, String) {
    let s = d.call("session.open", json!({"label": "exits"})).unwrap();
    let session = s["session_id"].clone();
    d.call(
        "turn.submit",
        json!({"session_id": session, "input": input, "author": "test", "attachments": []}),
    )
    .unwrap();
    let t0 = Instant::now();
    while model.requests().is_empty()
        || d.call("session.list", Value::Null)
            .unwrap()
            .to_string()
            .contains("\"busy\":true")
    {
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "the turn never ended:\n{}",
            d.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let tail = d
        .call("ledger.tail", json!({"n": 5000}))
        .unwrap()
        .to_string();
    let turn = d
        .call("ledger.tail", json!({"n": 5000, "kind": "turn.started"}))
        .unwrap()["rows"][0]["turn_id"]
        .as_str()
        .unwrap_or_else(|| panic!("no turn.started row in {tail}"))
        .to_string();
    (session, turn)
}

fn redacted_rows(d: &Served) -> Vec<Value> {
    d.call(
        "ledger.tail",
        json!({"n": 5000, "kind": "request.redacted"}),
    )
    .unwrap()["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn the_request_carries_the_name_the_row_and_inspect_say_which_and_a_shape_is_left_alone() {
    let (d, model) = daemon(true);
    let (session, turn) = say(
        &d,
        &model,
        &format!("use {VALUE} to log in; my other key is {LOOKALIKE}"),
    );
    let body: String = model
        .raw_requests()
        .iter()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    assert!(
        !body.contains(VALUE),
        "the vault value reached the model:\n{body}"
    );
    assert!(body.contains(&format!("[redacted:{NAME}]")), "{body}");
    assert!(
        body.contains(LOOKALIKE),
        "a shape that was never in the vault is content:\n{body}"
    );

    let rows = redacted_rows(&d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["data"]["names"], json!([NAME]));
    assert_eq!(rows[0]["data"]["substituted"], json!(true));
    assert_eq!(rows[0]["turn_id"], json!(turn));
    assert!(
        !rows[0].to_string().contains(VALUE),
        "the row names the secret, never its value"
    );

    // `theseus inspect` reads this: the answer's loop names the secret.
    let r = d
        .call(
            "turn.inspect",
            json!({"session_id": session, "turn_id": turn}),
        )
        .unwrap();
    let red = &r["loops"][0]["answer"]["redacted"];
    assert_eq!(red[0]["names"], json!([NAME]), "{red}");
    assert_eq!(red[0]["substituted"], json!(true));

    // The record is the owner's own words, as he typed them.
    let history = d
        .call("session.history", json!({"session_id": session}))
        .unwrap()
        .to_string();
    assert!(history.contains(VALUE), "the stored message is raw");
}

#[test]
fn with_redact_outbound_off_the_value_goes_as_written_and_the_row_still_says_which() {
    let (d, model) = daemon(false);
    let (session, turn) = say(&d, &model, &format!("use {VALUE}"));
    let body: String = model
        .raw_requests()
        .iter()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    assert!(body.contains(VALUE), "the operator chose to send it");
    assert!(!body.contains(&format!("[redacted:{NAME}]")));
    let rows = redacted_rows(&d);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["data"]["names"], json!([NAME]));
    assert_eq!(rows[0]["data"]["substituted"], json!(false));
    let r = d
        .call(
            "turn.inspect",
            json!({"session_id": session, "turn_id": turn}),
        )
        .unwrap();
    assert_eq!(
        r["loops"][0]["answer"]["redacted"][0]["substituted"],
        json!(false)
    );
}

/// A tool that prints a vault value encoded: the record holds it as printed,
/// and the next request carries the name, not the base64, and the row says
/// which secret. Before the encodings were applied at the exit, the request
/// carried the base64 and no row was written.
#[test]
fn a_value_a_tool_printed_in_base64_leaves_by_name_and_the_row_says_which() {
    let (d, model) = daemon_with(true, |prompt| {
        if prompt.contains("show the header") {
            let script = format!("echo 'Authorization: Basic {BASIC}'");
            vec![("proc_run", json!({"argv": ["sh", "-c", script]}))]
        } else {
            vec![]
        }
    });
    let (session, turn) = say(&d, &model, "show the header");
    let t0 = Instant::now();
    while model.requests().len() < 2 {
        assert!(t0.elapsed() < Duration::from_secs(60), "{}", d.log());
        std::thread::sleep(Duration::from_millis(20));
    }
    let bodies: Vec<String> = model
        .raw_requests()
        .iter()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    let last = bodies.last().unwrap();
    assert!(
        !last.contains(BASIC) && !last.contains(VALUE),
        "the encoded vault value reached the model:\n{last}"
    );
    assert!(last.contains(&format!("[redacted:{NAME}]")), "{last}");
    let rows = redacted_rows(&d);
    assert!(!rows.is_empty(), "no request.redacted row for an encoded value");
    assert_eq!(rows[0]["data"]["names"], json!([NAME]));
    assert_eq!(rows[0]["turn_id"], json!(turn));
    let history = d
        .call("session.history", json!({"session_id": session}))
        .unwrap()
        .to_string();
    assert!(history.contains(BASIC), "the record holds what the tool printed");
}
