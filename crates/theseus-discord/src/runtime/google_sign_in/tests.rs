//! Google's consent controls and words (issue 59): each press parses to what it names and nothing
//! else; the form has one field; every failure is fixed words and holds nothing said.

use super::*;

fn call_error(code: i64, message: &str) -> CallError {
    CallError {
        code,
        message: message.into(),
        data: Value::Null,
    }
}

#[test]
fn a_press_names_what_it_does_and_nothing_else() {
    assert_eq!(parse("gsi:in:google"), Some(Press::In));
    assert_eq!(parse("gsi:out:google"), Some(Press::Out));
    assert_eq!(
        parse("gsi:paste:abc123:0"),
        Some(Press::Paste("abc123".into()))
    );
    for bad in [
        "gsi:in:other",
        "gsi:begin:google",
        "gsi:paste:a b:0",
        "mcpin:in:docs",
        "gsi:in:",
    ] {
        assert_eq!(parse(bad), None, "{bad}");
    }
}

#[test]
fn the_card_has_connect_and_revoke_and_the_form_one_field() {
    let b = serde_json::to_string(&buttons()).unwrap();
    assert!(
        b.contains("gsi:in:google") && b.contains("Connect Google"),
        "{b}"
    );
    assert!(
        b.contains("gsi:out:google") && b.contains("Revoke Google"),
        "{b}"
    );
    let m = modal("abc123");
    assert_eq!(m["data"]["custom_id"], "gsi:m:abc123:0");
    assert_eq!(m["data"]["components"].as_array().unwrap().len(), 1);
}

#[test]
fn a_failure_is_fixed_words_and_holds_nothing_said() {
    let pasted = "http://127.0.0.1:41999/?code=fake-google-code&state=s";
    let cases = [
        (error_code::REFUSED, format!("refused: {pasted}"), NOT_HERE),
        (
            -32603,
            "no sign-in is open: run `theseus google auth` again".to_string(),
            EXPIRED,
        ),
        (
            -32603,
            format!("the state does not match: {pasted}"),
            NOT_THIS,
        ),
        (
            -32603,
            "that sign-in was for a different account than `[google] account`: nothing was stored"
                .to_string(),
            REFUSED,
        ),
        (
            -32603,
            format!("token endpoint said invalid_grant: {pasted}"),
            GOOGLE_NO,
        ),
    ];
    for (code, message, want) in cases {
        let got = failure(&call_error(code, &message));
        assert_eq!(got, want, "{message}");
        assert!(!got.contains("fake-google-code"), "{got}");
    }
}
