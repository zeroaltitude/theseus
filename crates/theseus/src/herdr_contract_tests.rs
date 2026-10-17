//! herdr's wire contract (theseus-6ifz1): every request this crate builds, checked against a
//! snapshot of herdr 0.9.1's own `herdr api schema --json`
//! (`tests/fixtures/herdr-0.9.1-api-schema.json`, checked by `tests/support/herdr_schema.rs`).
//! `layout.apply` once took an invented multi-tab shape that the tests' fake herdr accepted and the
//! real one refused ("missing field root"), so the follower, `up`, the board tab and `main` never
//! worked live. The fakes of `tests/herdr_*.rs` use the same checker, so a request any of those
//! tests sends is checked too; this file holds the builders and the method names.
//!
//! To follow a new herdr: `herdr api schema --json > tests/fixtures/herdr-<version>-api-schema.json`,
//! point `support/herdr_schema.rs` at it, and fix what these tests then name.

use std::path::Path;

use serde_json::{json, Value};

use crate::herdr::{self, Meta};
use crate::herdr_up::wire::{self, TabSpec, Where};

#[path = "../tests/support/herdr_schema.rs"]
mod herdr_schema;
use herdr_schema::Schema;

fn exe() -> &'static Path {
    Path::new("/opt/tide/bin/theseus")
}

fn valid(s: &Schema, method: &str, params: &Value) {
    if let Err(e) = s.check_request(method, params) {
        panic!("{method} does not match herdr's schema: {e}\n{params:#}");
    }
}

fn session_tab() -> TabSpec {
    TabSpec {
        sid: "ses_q7f3k2".into(),
        label: "Tide notes".into(),
        kind: "task",
        pinned: true,
    }
}

#[test]
fn the_tab_builders_make_herdrs_layout_apply() {
    let s = Schema::load();
    for t in [session_tab(), TabSpec::main(), TabSpec::board()] {
        for at in [Where::Workspace("w1"), Where::Replacing("w1:t1")] {
            for focus in [false, true] {
                let p = wire::tab_params(&at, exe(), "/run/t.sock", &t, focus);
                valid(&s, "layout.apply", &p);
                assert_eq!(p["root"]["type"], "pane");
                assert_eq!(p["tab_label"], t.label.as_str());
            }
        }
    }
    let p = wire::tab_params(
        &Where::Replacing("w1:t1"),
        exe(),
        "/s",
        &TabSpec::main(),
        true,
    );
    assert!(
        p.get("workspace_id").is_none(),
        "tab_id and workspace_id are exclusive: {p}"
    );
    assert_eq!(
        p["root"]["command"],
        json!([exe(), "--socket", "/s", "chat"])
    );
    let p = wire::tab_params(
        &Where::Workspace("w1"),
        exe(),
        "/s",
        &TabSpec::board(),
        false,
    );
    assert_eq!(
        p["root"]["command"],
        json!([exe(), "--socket", "/s", "herdr", "up", "--follow", "--here"])
    );
    let p = wire::tab_params(&Where::Workspace("w1"), exe(), "/s", &session_tab(), false);
    assert_eq!(
        p["root"]["command"],
        json!([
            exe(),
            "--socket",
            "/s",
            "watch",
            "ses_q7f3k2",
            "--interactive"
        ])
    );
}

#[test]
fn the_workspace_and_pane_builders_match() {
    let s = Schema::load();
    valid(&s, "workspace.create", &wire::workspace_params());
    for t in [session_tab(), TabSpec::main(), TabSpec::board()] {
        valid(
            &s,
            "pane.report_metadata",
            &wire::pane_tokens_params("w1:p2", &t),
        );
    }
    let tokens = wire::pane_tokens_params("w1:p2", &session_tab())["tokens"].clone();
    assert_eq!(
        tokens,
        json!({"kind": "task", "session": "ses_q7f3k2", "pinned": "1"})
    );
    let tokens = wire::pane_tokens_params("w1:p2", &TabSpec::main())["tokens"].clone();
    assert_eq!(tokens, json!({"kind": "main"}));
    valid(&s, "agent.view.set", &wire::view_params());
    valid(&s, "events.subscribe", &wire::subscribe_params());
    valid(
        &s,
        "workspace.report_metadata",
        &wire::counts_params("w1", &Default::default()),
    );
    valid(&s, "tab.close", &json!({"tab_id": "w1:t2"}));
    valid(&s, "pane.close", &json!({"pane_id": "w1:p2"}));
    // The keys' and the card's reads and focuses (herdr_keys.rs, herdr_answer.rs, herdr_main.rs).
    valid(&s, "pane.get", &json!({"pane_id": "w1:p2"}));
    valid(&s, "pane.focus", &json!({"pane_id": "w1:p2"}));
    valid(&s, "agent.focus", &json!({"target": "w1:p2"}));
    valid(&s, "pane.list", &json!({}));
    valid(&s, "workspace.list", &json!({}));
}

#[test]
fn the_reporters_requests_match() {
    let s = Schema::load();
    valid(
        &s,
        "pane.report_agent",
        &herdr::report_params("w1:p2", "tide-notes", "working", "x", 3),
    );
    valid(
        &s,
        "pane.release_agent",
        &herdr::release_params("w1:p2", "tide-notes", 4),
    );
    valid(
        &s,
        "notification.show",
        &herdr::notice_params("Theseus", "daemon is back", false),
    );
    valid(
        &s,
        "notification.show",
        &herdr::notice_params("Theseus", "asks", true),
    );
    valid(
        &s,
        "agent.rename",
        &json!({"target": "w1:p2", "name": "theseus-q7f3k2-tide"}),
    );
    let mut tokens = serde_json::Map::new();
    tokens.insert("session".into(), json!("ses_q7f3k2"));
    tokens.insert("confirm".into(), Value::Null);
    let meta = Meta {
        title: Some("Tide notes".into()),
        agent: "tide-notes".into(),
        tokens,
        labels: json!({"working": "writing"}),
    };
    valid(&s, "pane.report_metadata", &meta.params("w1:p2", 5));
}

/// Every dotted name the herdr modules send is a method herdr has. Event names (what the follower
/// hears, not sends) and a file name are listed; any other dotted literal fails until it is a
/// method or is listed.
#[test]
fn every_method_the_code_names_is_in_herdrs_schema() {
    let s = Schema::load();
    let heard = [
        "pane.exited",
        "tab.focused",
        "pane.focused",
        "events.lost",
        "herdr.sock",
    ];
    let files = [
        include_str!("herdr.rs"),
        include_str!("herdr_answer.rs"),
        include_str!("herdr_keys.rs"),
        include_str!("herdr_main.rs"),
        include_str!("herdr_sync.rs"),
        include_str!("herdr_up.rs"),
        include_str!("herdr_up/wire.rs"),
    ];
    let mut named = std::collections::BTreeSet::new();
    for text in files {
        let code = text.split("#[cfg(test)]").next().unwrap();
        for lit in code.split('"').skip(1).step_by(2) {
            let dotted = lit.contains('.')
                && lit.len() < 40
                && lit.split('.').all(|w| {
                    !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                });
            // `theseus.answer` and the rest are the plugin's actions, herdr's names for our keys.
            if dotted && !heard.contains(&lit) && !lit.starts_with("theseus.") {
                named.insert(lit.to_string());
            }
        }
    }
    // The daemon's own methods are constants, never literals here; what is left is herdr's.
    for m in &named {
        assert!(
            s.knows(m),
            "`{m}` is not a method of herdr's API (or an event: list it)"
        );
    }
    for m in [
        "layout.apply",
        "workspace.create",
        "pane.report_metadata",
        "events.subscribe",
    ] {
        assert!(named.contains(m), "the scan missed `{m}`: {named:?}");
    }
}
