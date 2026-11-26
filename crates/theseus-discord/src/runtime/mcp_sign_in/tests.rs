//! The sign-in's controls and words (issue 59): each press parses to what it
//! names and nothing else; the form has one field; every failure is fixed words
//! and holds no part of what the daemon or the presser said.

use super::*;

fn call_error(code: i64, message: &str) -> CallError {
    CallError {
        code,
        message: message.into(),
        data: Value::Null,
    }
}

#[test]
fn a_press_names_its_server_or_sign_in_and_nothing_else() {
    assert_eq!(parse("mcpin:in:docs"), Some(Press::In("docs".into())));
    assert_eq!(parse("mcpin:out:docs"), Some(Press::Out("docs".into())));
    assert_eq!(
        parse("mcpin:agree:abc123"),
        Some(Press::Agree("abc123".into()))
    );
    assert_eq!(
        parse("mcpin:paste:abc123"),
        Some(Press::Paste("abc123".into()))
    );
    // No id carries what was agreed: one that tries is no press of this module.
    for bad in [
        "mcpin:paste:abc123:1",
        "mcpin:paste:abc123:0",
        "mcpin:in:",
        "mcpin:in:a b",
        "mcpin:begin:docs",
        "psoul:write:x",
        "mcpin:in:../x",
    ] {
        assert_eq!(parse(bad), None, "{bad}");
    }
}

#[test]
fn the_card_has_sign_in_and_sign_out_and_the_form_one_field() {
    let b = serde_json::to_string(&buttons("docs")).unwrap();
    assert!(b.contains("mcpin:in:docs") && b.contains("Sign in"), "{b}");
    assert!(
        b.contains("mcpin:out:docs") && b.contains("Sign out"),
        "{b}"
    );
    assert!(buttons("not a name").is_empty());
    let m = modal("abc123");
    assert_eq!(m["data"]["custom_id"], "mcpin:m:abc123");
    let about = m["data"]["components"][0]["description"].as_str().unwrap();
    assert!(about.contains("passes through Discord"), "{about}");
    assert!(about.contains("4,000"), "{about}");
    let fields = m["data"]["components"].as_array().unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0]["component"]["custom_id"], FIELD);
}

/// Every failure is one of the fixed answers, and none holds the daemon's
/// message or anything pasted.
#[test]
fn a_failure_is_fixed_words_and_holds_nothing_said() {
    let pasted = "http://127.0.0.1:1/callback?code=fake-code-secret&state=s";
    let cases = [
        (
            theseus_protocol::error_code::REFUSED,
            format!("refused: {pasted}"),
            NOT_HERE,
        ),
        (
            -32602,
            "no sign-in is open: run `theseus mcp login` again".to_string(),
            EXPIRED,
        ),
        (
            -32602,
            format!("the state is not this sign-in's: {pasted}"),
            NOT_THIS,
        ),
        (
            -32602,
            format!("the sign-in was not completed: {pasted}"),
            SERVER_NO,
        ),
        (
            -32603,
            format!("the sign-in was not made sure to last (disk full): {pasted}"),
            MAY_BE_KEPT,
        ),
        (-32602, format!("something else: {pasted}"), REFUSED),
    ];
    for (code, message, want) in cases {
        let got = failure(&call_error(code, &message));
        assert_eq!(got, want, "{message}");
        assert!(
            !got.contains("fake-code") && !got.contains("callback"),
            "{got}"
        );
    }
}

#[test]
fn the_host_and_every_other_place_are_shown_with_the_agree_words() {
    let b = McpLoginBeginResult {
        login_id: "abc".into(),
        name: "docs".into(),
        host: "auth.example.invalid".into(),
        url: "https://auth.example.invalid/authorize".into(),
        elsewhere: vec!["https://tokens.example.invalid".into()],
        ..Default::default()
    };
    let t = host_lines(&b).join("\n");
    assert!(t.contains("auth.example.invalid"), "{t}");
    assert!(t.contains("tokens.example.invalid"), "{t}");
    assert!(t.contains("I agree"), "{t}");
    assert!(
        !t.contains("/authorize"),
        "the address comes after the agree: {t}"
    );
}

/// A sign-out says what happened: forgotten, still kept on disk, or nothing kept.
#[test]
fn a_sign_out_says_whether_its_kept_file_remains() {
    let out = |forgot, kept_remains| McpLogoutResult {
        name: "docs".into(),
        forgot,
        kept_remains,
        ..Default::default()
    };
    assert_eq!(signed_out(&out(true, false)), SIGNED_OUT);
    assert_eq!(signed_out(&out(false, true)), KEPT_REMAINS);
    assert_eq!(signed_out(&out(false, false)), NOTHING_KEPT);
}
