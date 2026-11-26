use super::*;

fn call_error(code: i64, message: &str) -> CallError {
    CallError {
        code,
        message: message.to_string(),
        data: Value::Null,
    }
}

#[test]
fn the_card_is_one_button_and_the_form_one_field() {
    let b = serde_json::to_string(&buttons()).unwrap();
    assert!(
        b.contains("awsc:in:alerts") && b.contains("Confirm alerts"),
        "{b}"
    );
    assert!(parse("awsc:in:alerts"));
    assert!(!parse("awsc:in:other") && !parse("gsi:in:google"));
    let m = modal("1a2b");
    assert_eq!(m["data"]["custom_id"], "awsc:m:1a2b");
    assert!(m["data"]["custom_id"].as_str().unwrap().starts_with(MODAL));
    assert_eq!(m["data"]["components"].as_array().unwrap().len(), 1);
}

#[test]
fn an_alerts_failure_is_fixed_words_and_holds_nothing_said() {
    let token = "5f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
    let cases = [
        (error_code::REFUSED, format!("refused: {token}"), NOT_HERE),
        (
            -32602,
            format!("that is not a confirmation token: {token}"),
            NOT_THIS,
        ),
        (-32603, format!("InvalidParameter: Token {token}"), AWS_NO),
    ];
    for (code, message, want) in cases {
        let got = failure(&call_error(code, &message));
        assert_eq!(got, want, "{message}");
        assert!(!got.contains(token), "{got}");
    }
}
