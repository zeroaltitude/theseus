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

fn with_diff(extra: Value) -> Value {
    let mut v = extra;
    v["preview"] = json!({"kind": "diff", "body": DIFF});
    v
}

/// The owner-approved layout, 2026-10-22 (theseus-ono8g); the Discord task flow is
/// locked: change this golden only with the owner's OK. The variant it was approved
/// with asked because the task quoted outside text, and said "Approve + trust" for it;
/// the hold behind both is gone (R6, theseus-gyjyo.8), so the same layout now shows
/// the posture ask, and only those two words differ from what the owner approved.
#[test]
fn the_approval_card_reads_as_the_owner_approved_it() {
    let req = request(with_diff(json!({})));
    let c = card(&req, &Route::Here, "with `theseus answer`");
    let expected = "\
✋ **Approve?** task `490f10` · *[walkthrough] harbour summary* wants to **write** `~/projects/walkthrough/out/harbour.md` (new file, 227 bytes)
risk: writes a file · why it asks: you set this kind of call to ask first
```diff
b/out/harbour.md:1
@@ -0,0 +1,2 @@
+# Harbour
+Calm water.
```
**Approve** runs this one call · **Decline** refuses it; the task is told and carries on
-# held since <t:0:R> · still waiting · or `theseus answer 490f10 approve`";
    assert_eq!(c.content, expected);
    assert_eq!(
        c.line,
        "task `490f10` · *[walkthrough] harbour summary* wants to **write** `~/projects/walkthrough/out/harbour.md` (new file, 227 bytes)"
    );
}

/// A plain posture ask, without a diff.
#[test]
fn a_posture_ask_has_no_trust_button_clause() {
    let c = card(&request(json!({})), &Route::Here, "");
    let expected = "\
✋ **Approve?** task `490f10` · *[walkthrough] harbour summary* wants to **write** `~/projects/walkthrough/out/harbour.md` (new file, 227 bytes)
risk: writes a file · why it asks: you set this kind of call to ask first
**Approve** runs this one call · **Decline** refuses it; the task is told and carries on
-# held since <t:0:R> · still waiting";
    assert_eq!(c.content, expected);
}

/// No variant shows a spec clause or the tool's name in its words.
#[test]
fn an_approval_card_names_no_spec_clause() {
    let mut plain = request(json!({})).plain.unwrap();
    plain.held = "it touches Theseus's own state or a secret, which always asks".into();
    let floor = json!({"floor": true, "plain": plain});
    let private = json!({"tool": "http.fetch", "input": {"url": "http://10.0.0.5/notes"},
        "reason": "fetch http://10.0.0.5/notes: http.fetch — approve (§4.2: 10.0.0.5 is a private address, \
                   and a private address waits for approval; this is a shared place, so the page joins a \
                   conversation others can read)"});
    for extra in [json!({}), floor, private] {
        let c = card(&request(extra), &Route::Here, "with `theseus answer`");
        assert!(
            !c.content.contains('§') && !c.line.contains('§'),
            "{}",
            c.content
        );
        assert!(!c.content.contains("fs.write"), "{}", c.content);
    }
}

/// A hold that is not the owner's posture says its own rule, not the posture's words: a private
/// address fetched from a shared place keeps the notice of where the page goes.
#[test]
fn a_private_address_card_says_its_own_rule() {
    let c = card(
        &request(
            json!({"tool": "http.fetch", "input": {"url": "http://10.0.0.5/notes"},
            "reason": "fetch http://10.0.0.5/notes: http.fetch — approve (10.0.0.5 is a private \
                       address, and a private address waits for approval; this is a shared place, \
                       so the page joins a conversation others can read)"}),
        ),
        &Route::Here,
        "",
    );
    assert!(
        c.content.contains(
            "why it asks: 10.0.0.5 is a private address, and a private address waits for approval; \
             this is a shared place, so the page joins a conversation others can read"
        ),
        "{}",
        c.content
    );
    assert!(
        !c.content.contains("you set this kind of call to ask first"),
        "{}",
        c.content
    );
    for posture in [
        "enforcement = approve",
        "[policy.tools] \"http.fetch\" = approve",
        "§3.9",
    ] {
        let c = card(
            &request(json!({"reason": format!("the call: fs.write — approve ({posture})")})),
            &Route::Here,
            "",
        );
        assert!(
            c.content
                .contains("why it asks: you set this kind of call to ask first"),
            "{posture}: {}",
            c.content
        );
    }
}

/// The card's `why it asks` is the clause the core reads from the reason, the
/// one the TUI and `theseus answer` show as the plain `why` (theseus-zcn80).
#[test]
fn the_cards_clause_is_the_cores_one() {
    let reason = "run gh: proc.run — approve (gh is on the approve list (§3.9))";
    let said = theseus_core::needs_you::rule_clause(reason).unwrap();
    let c = card(&request(json!({ "reason": reason })), &Route::Here, "");
    assert!(
        c.content.contains(&format!("why it asks: {said}\n")),
        "{}",
        c.content
    );
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

/// A question that needs the owner's second proof says so on its card (issue 22),
/// before the press: with the daemon's words for the place it is answered in
/// (whole), that Approve goes through on a proof given earlier or opens a form
/// for a code. One that asks for a fresh code every time says that; one that
/// asks for none adds nothing.
#[test]
fn a_card_that_needs_the_second_proof_says_so() {
    let ordinary = card(&request(json!({})), &Route::Here, "").content;
    assert!(!ordinary.contains("Second proof"), "{ordinary}");
    let said = |v| card(&request(v), &Route::Here, "").content;
    // The place's standing is not known: both are said.
    let unknown = said(json!({"needs_proof": true}));
    assert!(
        unknown.contains(
            "\n🔐 **Second proof:** **Approve** goes through if a proof you gave here earlier \
             still covers it, and otherwise opens a form for the code from your authenticator app"
        ),
        "{unknown}"
    );
    // An open window covers it: the daemon's own words, whole, and Approve goes through.
    let line = "second proof: given here 3 minutes ago, good for 23 hours more";
    let covered = said(json!({"needs_proof": true,
        "proof": {"code_needed": false, "line": line}}));
    assert!(
        covered.contains(&format!("\n🔐 {line}; **Approve** goes through")),
        "{covered}"
    );
    assert!(!covered.contains("opens a form"), "{covered}");
    // No window: a code is asked.
    let needed = said(json!({"needs_proof": true,
        "proof": {"code_needed": true, "line": "second proof: needed"}}));
    assert!(
        needed.contains("**Approve** opens a form for the code from your authenticator app"),
        "{needed}"
    );
    assert!(!needed.contains("goes through"), "{needed}");
    // Nothing enrolled: the daemon's words, whole, with no promise of a form.
    let line = "second proof: needed, and no authenticator app is enrolled yet: run `theseus owner proof enroll` in your own terminal";
    let none = said(json!({"needs_proof": true,
        "proof": {"code_needed": true, "not_enrolled": true, "line": line}}));
    assert!(none.contains(&format!("\n🔐 {line}")), "{none}");
    assert!(!none.contains("opens a form"), "{none}");
    // A fresh code every time, whatever window is open.
    let fresh = said(json!({"needs_proof": true, "needs_fresh_proof": true,
        "proof": {"code_needed": false, "line": line}}));
    assert!(fresh.contains("**Second proof, every time:**"), "{fresh}");
    assert!(!fresh.contains("goes through"), "{fresh}");
}

/// A shell command the floor holds because it names the vault's program or
/// Theseus's own (issue 26) is drawn with which program it names and that a
/// look has a tool: the daemon's own words, from the floor's own reason, as the
/// card draws them.
#[test]
fn a_floor_hold_names_its_program_and_says_where_the_look_is() {
    let gloss = theseus_core::needs_you::Gloss {
        state: std::path::PathBuf::from("/state"),
        who: "the session".into(),
        own_task: None,
        model_why: Some("Checking whether the key is in the vault.".into()),
    };
    for (program, tool) in [("op", "look.vault"), ("theseusd", "look.theseus")] {
        let mut req = request(json!({
            "floor": true, "task": null, "tool": "proc.run",
            "input": {"argv": ["bash", "-c", format!("{program} whoami")]},
            "reason": format!("run `bash -c {program} whoami` in /w: proc.run — approve (floor: `{program}` is Theseus's own binary or the 1Password CLI)"),
        }));
        req.task = None;
        req.plain = Some(theseus_core::needs_you::plain(&req, &gloss));
        let c = card(&req, &Route::Here, "");
        assert!(c.content.starts_with("🔒 **Approve?** "), "{}", c.content);
        assert!(
            c.content.contains(&format!("(`{program}`)"))
                && c.content.contains(&format!("`{tool}`")),
            "{}",
            c.content
        );
        assert!(
            !c.content
                .contains("touches Theseus's own state or a secret"),
            "{}",
            c.content
        );
    }
}

/// A held call's card says its cause whole (the Discord card cuts `why it asks` at
/// 200 characters), with no "layer 1:".
#[test]
fn a_held_call_says_its_cause_whole() {
    use theseus_core::config_change::{flag_way_in, hold_words};
    let session = "ses_01a1593f784974cf92a4a4666c968b67";
    let what = "hand another model profile a piece of work";
    for place in [Some("discord:dm:1"), None] {
        let way = flag_way_in(place, session);
        for (outside, steered) in [(true, false), (false, true)] {
            let said = hold_words(what, outside, steered, &way).unwrap();
            let reason = format!("hand over: profile.run — approve (layer 1: {said})");
            let c = card(&request(json!({ "reason": reason })), &Route::Here, "");
            assert!(
                c.content.contains(&format!("why it asks: {said}\n")),
                "{}",
                c.content
            );
            assert!(!c.content.contains("layer 1"), "{}", c.content);
        }
    }
}

/// A held look's card that offers "Approve and clear" (issue 54) says what that answer does in the
/// line of answers, and its reason no longer tells how to clear the flag: the answer does it.
#[test]
fn a_cleared_holds_card_says_what_its_third_answer_does() {
    use theseus_core::config_change::{flag_way_in, hold_words, FLAG_HOLD};
    use theseus_protocol::grants::GrantOffer;
    let way = flag_way_in(Some("discord:dm:1"), "ses_01a1593f784974cf92a4a4666c968b67");
    let said = hold_words("look at what Theseus runs as", false, true, &way).unwrap();
    let reason = format!("look: look.theseus — approve (layer 1: {said})");
    let mut req = request(json!({ "reason": reason, "tool": "look.theseus" }));
    req.grant_offer = Some(GrantOffer::clear(true));
    let c = card(&req, &Route::Here, "");
    assert!(
        c.content.contains(&format!(
            "why it asks: look at what Theseus runs as: {FLAG_HOLD}\n"
        )),
        "{}",
        c.content
    );
    assert!(!c.content.contains("/trust"), "{}", c.content);
    assert!(
        c.content.contains("**Approve** runs this one call · **Approve and clear** runs it and \
                            clears the flag (what it flagged stays here) · **Decline** refuses it"),
        "{}",
        c.content
    );
    // Without the offer, the way stays.
    let c = card(&request(json!({ "reason": reason })), &Route::Here, "");
    assert!(c.content.contains("/trust"), "{}", c.content);
}
