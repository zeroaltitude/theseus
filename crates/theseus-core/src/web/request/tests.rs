//! `http.request`'s input rules and its placeholders, with nothing sent.

use serde_json::{json, Value};

use super::send::{fill, Measured};
use super::{check, PUBLIC_ONLY};

fn call(more: Value) -> Value {
    let mut c = json!({
        "method": "POST",
        "url": "https://lantern.example.test/v1/things",
        "key": {"vault": "Harbor", "item": "Lantern key", "field": "credential", "as": {"bearer": {}}},
        "reason": "send it"
    });
    if let (Some(c), Value::Object(m)) = (c.as_object_mut(), more) {
        c.extend(m);
    }
    c
}

#[test]
fn a_key_goes_only_to_https_at_a_public_host() {
    for url in [
        "http://lantern.example.test/v1",
        "https://10.1.2.3/v1",
        "https://127.0.0.1/v1",
        "https://[::1]/v1",
        "https://localhost/v1",
        "https://169.254.169.254/latest",
    ] {
        let e = check(&call(json!({"url": url})), false).unwrap_err();
        assert_eq!(e, PUBLIC_ONLY, "{url}");
    }
    for url in [
        "https://a:b@lantern.example.test/v1",
        "https://lantern.example.test/v1#x",
    ] {
        assert!(check(&call(json!({"url": url})), false).is_err(), "{url}");
    }
    let ok = check(&call(json!({})), false).unwrap();
    assert_eq!(ok.host(), "lantern.example.test");
    assert_eq!(ok.marker(), "https://lantern.example.test/v1/things");
}

#[test]
fn the_key_has_one_place_and_the_model_writes_none_of_it() {
    for headers in [
        json!({"Authorization": "x"}),
        json!({"cookie": "x"}),
        json!({"Content-Type": "x"}),
        json!({"X-Ok": "two\r\nlines"}),
    ] {
        assert!(
            check(&call(json!({"headers": headers})), false).is_err(),
            "{headers}"
        );
    }
    let own = json!({"key": {"vault": "Harbor", "item": "Lantern key", "field": "credential",
        "as": {"header": {"name": "X-Lantern-Key"}}}, "headers": {"x-lantern-key": "x"}});
    assert!(check(&call(own), false).is_err());
    let query = json!({"key": {"vault": "Harbor", "item": "Lantern key", "field": "credential",
        "as": {"query": {"name": "k"}}}});
    assert!(check(&call(query), false).is_err());
    assert!(check(&call(json!({"method": "GET", "json": {}})), false).is_err());
    assert!(check(&call(json!({"json": {}, "text": "x"})), false).is_err());
}

#[test]
fn placeholders_are_a_files_size_and_digest() {
    let one = [Measured::nothing(None)];
    assert_eq!(fill("n={{file.bytes}}", &one).unwrap(), "n=0");
    assert_eq!(
        fill("{{file.sha256}}", &one).unwrap(),
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
    assert_eq!(fill("{{other}} stays", &one).unwrap(), "{{other}} stays");
    assert!(fill("{{file.md5}}", &one).unwrap_err().contains("no MD5"));
    let two = [Measured::nothing(Some("a")), Measured::nothing(Some("b"))];
    assert!(fill("{{file.bytes}}", &two)
        .unwrap_err()
        .contains("name its part"));
    assert_eq!(fill("{{file:b.bytes}}", &two).unwrap(), "0");
    assert!(fill("{{file:c.bytes}}", &two).is_err());
    assert!(
        check(&call(json!({"json": {"n": "{{file.bytes}}"}})), false)
            .unwrap_err()
            .contains("sends no file")
    );
}
