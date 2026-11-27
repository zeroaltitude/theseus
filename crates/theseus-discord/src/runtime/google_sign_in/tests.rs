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
        parse("gsi:paste:abc123"),
        Some(Press::Paste("abc123".into()))
    );
    for bad in [
        "gsi:in:other",
        "gsi:begin:google",
        "gsi:paste:a b",
        "gsi:paste:abc123:1",
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
    assert_eq!(m["data"]["custom_id"], "gsi:m:abc123");
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
            format!("the redirect's state is not this consent's: nothing was stored: {pasted}"),
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

/// A consent whose keeping could not be made sure of says so, in fixed words; a kept one names the
/// account and what was granted.
#[test]
fn an_uncertain_keep_says_so() {
    let mut done = GoogleAuthFinishResult {
        state: "done".into(),
        account: "owner@example.invalid".into(),
        store: "memory".into(),
        granted: vec!["Gmail".into()],
        ..Default::default()
    };
    assert_eq!(done_words(&done), MAY_BE_KEPT);
    done.store = "file".into();
    let t = done_words(&done);
    assert!(t.starts_with("✅") && t.contains("Gmail"), "{t}");
}

/// A begin that failed (issue 62): each kind's marker is that kind's own fixed words, a refusal
/// keeps its words, and any other message is the one fixed sentence; none holds what the daemon
/// said.
#[test]
fn a_failed_begin_says_its_kind_in_fixed_words() {
    let words = |k: BeginKind| begin_failure(&call_error(-32603, k.marker()));
    let mut seen = std::collections::BTreeSet::new();
    for (kind, want) in [
        (BeginKind::NoAccount, NO_ACCOUNT),
        (BeginKind::Client, NO_CLIENT),
        (BeginKind::Resolving, RESOLVING),
        (BeginKind::Other, NO_BEGIN),
    ] {
        assert_eq!(words(kind), want, "{kind:?}");
        assert!(seen.insert(want), "{kind:?}: two kinds share words");
    }
    assert_eq!(seen.len(), BeginKind::ALL.len());
    let said = "the client secret `google_client` did not resolve: INVENTED-WORDS-2291";
    for (code, message, want) in [
        (
            theseus_protocol::error_code::REFUSED,
            said.to_string(),
            NOT_HERE,
        ),
        (-32603, said.to_string(), NO_BEGIN),
        (
            -32603,
            format!("{} INVENTED-WORDS-2291", BeginKind::Client.marker()),
            NO_BEGIN,
        ),
    ] {
        let got = begin_failure(&call_error(code, &message));
        assert_eq!(got, want, "{message}");
        assert!(!got.contains("INVENTED-WORDS"), "{got}");
    }
}
