//! The approval card's words (theseus-ono8g).

use serde_json::{json, Value};
use theseus_protocol::ConfirmRequest;

use crate::render::{card, Route};

const WALKTHROUGH: &str = "/srv/walkthrough/out/harbour.md";
const DIFF: &str =
    "--- /dev/null\n+++ b/out/harbour.md\n@@ -0,0 +1,2 @@\n+# Harbour\n+Calm water.\n";

fn request(extra: Value) -> ConfirmRequest {
    let mut v = json!({
        "correlation_id": "act_1", "session_id": "s", "execution_id": "e",
        "tool": "fs.write", "input": {"path": WALKTHROUGH, "content": "x".repeat(227)},
        "reason": format!("create {WALKTHROUGH} (227 bytes): fs.write — approve (§3.9)"),
        "by": "operator", "requested_at_ms": 1, "expires_at_ms": 1_790_000_000_000_u64,
        "task": {"task_id": "t1", "short": "490f10", "title": "[walkthrough] harbour summary"},
        "plain": {"who": "[walkthrough] harbour summary",
            "what": "write `~/projects/walkthrough/out/harbour.md`", "why": "",
            "held": "`fs.write` is set to ask before it runs", "risk": "writes",
            "yes": "runs this one call.", "no": "refuses it."},
    });
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(v).unwrap()
}

fn outside_text() -> Value {
    json!({"external_text": {"since_ms": 1_790_000_000_000_u64 - 3_600_000, "tool": "task.start",
        "url": "quoted text", "node_id": ""}})
}

fn with_diff(extra: Value) -> Value {
    let mut v = extra;
    v["preview"] = json!({"kind": "diff", "body": DIFF});
    v
}

/// The owner-approved layout, 2026-10-22 (theseus-ono8g); the Discord task flow is
/// locked: change this golden only with the owner's OK.
#[test]
fn the_approval_card_reads_as_the_owner_approved_it() {
    let req = request(with_diff(outside_text()));
    let c = card(&req, &Route::Here, "with `theseus answer`");
    let expected = "\
✋ **Approve?** task `490f10` · *[walkthrough] harbour summary* wants to **write** `~/projects/walkthrough/out/harbour.md` (new file, 227 bytes)
risk: writes a file · why it asks: the task quoted outside text at <t:1789996400:t>
```diff
b/out/harbour.md:1
@@ -0,0 +1,2 @@
+# Harbour
+Calm water.
```
**Approve** runs this one call · **Approve + trust** also lets its next calls run as usual · **Decline** refuses it; the task is told and carries on
-# expires <t:1790000000:R> · or `theseus answer 490f10 approve`";
    assert_eq!(c.content, expected);
    assert_eq!(
        c.line,
        "task `490f10` · *[walkthrough] harbour summary* wants to **write** `~/projects/walkthrough/out/harbour.md` (new file, 227 bytes)"
    );
}

/// A plain posture ask: no outside text, so no trust button and no trust clause.
#[test]
fn a_posture_ask_has_no_trust_button_clause() {
    let c = card(&request(json!({})), &Route::Here, "");
    let expected = "\
✋ **Approve?** task `490f10` · *[walkthrough] harbour summary* wants to **write** `~/projects/walkthrough/out/harbour.md` (new file, 227 bytes)
risk: writes a file · why it asks: you set this kind of call to ask first
**Approve** runs this one call · **Decline** refuses it; the task is told and carries on
-# expires <t:1790000000:R>";
    assert_eq!(c.content, expected);
}

/// No variant shows a spec clause or the tool's name in its words.
#[test]
fn an_approval_card_names_no_spec_clause() {
    let mut plain = request(json!({})).plain.unwrap();
    plain.held = "it touches Theseus's own state or a secret, which always asks".into();
    let floor = json!({"floor": true, "plain": plain});
    for extra in [json!({}), outside_text(), floor] {
        let c = card(&request(extra), &Route::Here, "with `theseus answer`");
        assert!(
            !c.content.contains('§') && !c.line.contains('§'),
            "{}",
            c.content
        );
        assert!(!c.content.contains("fs.write"), "{}", c.content);
    }
}

/// The floor keeps its lock and its words.
#[test]
fn a_floor_ask_keeps_its_lock_and_its_rule() {
    let mut v = json!({"floor": true});
    v["plain"] = json!({"who": "x", "what": "write `~/.theseus/state`", "why": "",
        "held": "it touches Theseus's own state or a secret, which always asks",
        "risk": "touches secrets", "yes": "", "no": ""});
    let c = card(&request(v), &Route::Here, "");
    assert!(c.content.starts_with("🔒 **Approve?** "), "{}", c.content);
    assert!(
        c.content.contains("risk: touches secrets · why it asks: it touches Theseus's own state or a secret, which always asks"),
        "{}",
        c.content
    );
    assert!(c.line.ends_with("(floor)"), "{}", c.line);
}

/// A request with no plain words (an older daemon) draws the card it always did.
#[test]
fn a_card_without_plain_words_keeps_the_old_shape() {
    let mut req = request(json!({}));
    req.plain = None;
    let c = card(&req, &Route::Here, "");
    assert!(
        c.content
            .starts_with("**Approve?** task `490f10`: `fs.write`"),
        "{}",
        c.content
    );
}
