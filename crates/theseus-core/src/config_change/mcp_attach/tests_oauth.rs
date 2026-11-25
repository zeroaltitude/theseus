//! `mcp.attach` and a server that signs in (issue 55): a server over HTTP
//! asked for with no key, whose trial is refused with HTTP 401, is attached as
//! one that signs in (`oauth = {}`), with no new key in the tool's input; its
//! trial never signs in; and its card says, before the yes, why it signs in
//! and what the owner does next. A server given a key, or one that came up,
//! is written as before.

use serde_json::json;

use super::card::{self, Facts, Tried};
use super::{AttachInput, Spec};

fn spec(input: serde_json::Value) -> Spec {
    let i: AttachInput = serde_json::from_value(input).unwrap();
    Spec::of(&i).unwrap()
}

fn refused() -> Tried {
    Tried {
        tools: vec![],
        error: Some(r#"HTTP 401: {"error":"invalid_token"}"#.into()),
    }
}

fn came_up() -> Tried {
    Tried {
        tools: vec![],
        error: None,
    }
}

#[test]
fn a_401_at_the_trial_makes_it_a_server_that_signs_in() {
    let mut s = spec(json!({"name": "docs", "url": "https://docs.example.invalid/mcp"}));
    assert!(s.table.get("oauth").is_none());
    assert!(s.signs_in_after(&refused()));
    assert_eq!(s.table["oauth"], json!({}));
    assert!(s.wanted.oauth.is_some());
    assert!(s.trial.oauth.is_none(), "the trial never signs in");
    assert!(!s.signs_in_after(&refused()), "once");
    // The table loads as the settings take it.
    let toml = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[mcp.servers.docs]\n\
                url = \"https://docs.example.invalid/mcp\"\noauth = {}\n";
    let (cfg, _) = crate::Config::parse(toml).unwrap();
    assert_eq!(cfg.mcp.servers["docs"].oauth, s.wanted.oauth);
    // A server given a key, a server that came up, a program: as before.
    let mut keyed =
        spec(json!({"name": "k", "url": "https://k.invalid/mcp", "auth_secret": "tok"}));
    assert!(
        !keyed.signs_in_after(&refused()),
        "its trial runs without the key"
    );
    let mut up = spec(json!({"name": "u", "url": "https://u.invalid/mcp"}));
    assert!(!up.signs_in_after(&came_up()));
    let other = Tried {
        tools: vec![],
        error: Some("HTTP 500: down".into()),
    };
    assert!(!up.signs_in_after(&other));
    let mut program = spec(json!({"name": "p", "command": ["x"]}));
    assert!(!program.signs_in_after(&refused()));
    assert!(program.table.get("oauth").is_none());
    // No key for it in the tool's input.
    let e = serde_json::from_value::<AttachInput>(
        json!({"name": "a", "url": "https://h.invalid/", "oauth": true}),
    );
    assert!(e.is_err());
}

fn card_of(s: &Spec, tried: &Tried) -> String {
    card::attach(&Facts {
        name: &s.name,
        server: &s.wanted,
        tried,
        outside_text: false,
        reason: "",
        missing: &[],
        unloaded: &[],
        source: None,
    })
    .unwrap()
}

/// The card says, before the yes, why it signs in, that it signs in in a
/// browser, and the one command; a server that came up says nothing of it.
#[test]
fn the_card_says_how_it_signs_in_and_what_comes_next() {
    let mut s = spec(json!({"name": "docs", "url": "https://docs.example.invalid/mcp"}));
    s.signs_in_after(&refused());
    let c = card_of(&s, &refused());
    assert!(
        c.contains("The trial was refused for want of a sign-in (HTTP 401)"),
        "{c}"
    );
    assert!(
        c.contains("It signs in first, and the trial does not sign in."),
        "{c}"
    );
    assert!(c.contains("Sign-in: it signs in in a browser."), "{c}");
    assert!(
        c.contains("run `theseus mcp login docs` in your own terminal"),
        "{c}"
    );
    assert!(c.contains("offered once you have signed in"), "{c}");
    let plain = spec(json!({"name": "docs", "url": "https://docs.example.invalid/mcp"}));
    let c = card_of(&plain, &came_up());
    assert!(!c.contains("sign-in"), "{c}");
    assert!(c.contains("offered from your next message"), "{c}");
}
