//! `mcp.attach` and a server that signs in (issue 55): the table it writes
//! (`oauth`, and never with `auth_secret`), the trial that never signs in,
//! and the card, which says before the yes how it signs in and what the
//! owner does next, or that the trial was refused for want of a sign-in.

use serde_json::json;

use super::card::{self, Facts, Tried};
use super::{AttachInput, Spec};

fn spec(input: serde_json::Value) -> Result<Spec, String> {
    let i: AttachInput = serde_json::from_value(input).unwrap();
    Spec::of(&i)
}

#[test]
fn an_attach_with_a_sign_in_writes_oauth_and_its_trial_signs_in_never() {
    let s = spec(json!({"name": "docs", "url": "https://docs.example.invalid/mcp", "oauth": true}))
        .unwrap();
    assert_eq!(s.table["oauth"], json!({}));
    assert!(s.wanted.oauth.is_some());
    assert!(s.trial.oauth.is_none(), "the trial never signs in");
    let s = spec(
        json!({"name": "wiki", "url": "https://wiki.example.invalid/mcp",
        "oauth": true, "oauth_client_id": "theseus-1", "oauth_scopes": ["read"]}),
    )
    .unwrap();
    assert_eq!(
        s.table["oauth"],
        json!({"client_id": "theseus-1", "scopes": ["read"]})
    );
    // The table loads as the settings take it.
    let toml = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[mcp.servers.wiki]\n\
                url = \"https://wiki.example.invalid/mcp\"\n\
                oauth = { client_id = \"theseus-1\", scopes = [\"read\"] }\n";
    let (cfg, _) = crate::Config::parse(toml).unwrap();
    assert_eq!(cfg.mcp.servers["wiki"].oauth, s.wanted.oauth);
    // A fixed key and a sign-in are not both; a program does not sign in.
    let e = spec(
        json!({"name": "a", "url": "https://h.invalid/mcp", "oauth": true,
        "auth_secret": "tok"}),
    )
    .err()
    .unwrap();
    assert!(e.contains("not both"), "{e}");
    let e = spec(json!({"name": "a", "command": ["x"], "oauth": true}))
        .err()
        .unwrap();
    assert!(e.contains("over HTTP that signs in"), "{e}");
    // Without it, nothing changes: no `oauth` key.
    let s = spec(json!({"name": "a", "url": "https://h.invalid/mcp"})).unwrap();
    assert!(s.table.get("oauth").is_none());
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

/// The card says, before the yes, that it signs in and the one command; and,
/// for a server asked for with no sign-in whose trial got a 401, that it was
/// refused for want of one and what to ask instead.
#[test]
fn the_card_says_how_it_signs_in_and_what_comes_next() {
    let refused = Tried {
        tools: vec![],
        error: Some(r#"HTTP 401: {"error":"invalid_token"}"#.into()),
    };
    let s = spec(json!({"name": "docs", "url": "https://docs.example.invalid/mcp", "oauth": true}))
        .unwrap();
    let c = card_of(&s, &refused);
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
    assert!(!c.contains("refused for want of a sign-in"), "{c}");
    let plain = spec(json!({"name": "docs", "url": "https://docs.example.invalid/mcp"})).unwrap();
    let c = card_of(&plain, &refused);
    assert!(
        c.contains("The trial was refused for want of a sign-in (HTTP 401)"),
        "{c}"
    );
    assert!(
        c.contains("Ask for it again with a sign-in (`oauth`)"),
        "{c}"
    );
    assert!(c.contains("`theseus mcp login docs`"), "{c}");
    // A server that came up says nothing of a sign-in.
    let ok = Tried {
        tools: vec![],
        error: None,
    };
    let c = card_of(&plain, &ok);
    assert!(!c.contains("sign-in"), "{c}");
    assert!(c.contains("offered from your next message"), "{c}");
}
