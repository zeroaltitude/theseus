//! The trusted record and the once-only claim (issue 59): every step after the first needs the
//! record, its user and its stage, before its deadline; an interaction is acted on once; nothing
//! shown is cut.

use super::*;

const OWNER: u64 = 100_000_000_000_000_042;
const SECOND: u64 = 100_000_000_000_000_043;

fn dm(user: u64) -> DiscordOrigin {
    DiscordOrigin {
        user_id: user.to_string(),
        channel_id: "500".into(),
        guild_id: None,
    }
}

fn begun(rs: &Records, kind: &'static str, id: &str, needs_agree: bool) {
    let r = record(
        rs,
        kind,
        id,
        OWNER,
        "docs",
        dm(OWNER),
        needs_agree,
        Duration::from_secs(600),
        "https://auth.example.invalid/authorize",
    );
    rs.begin(r);
}

/// What was agreed is the record's: a sign-in that needs I agree is not ready until its user's own
/// press moves it, and a sent one takes no second form.
#[test]
fn a_form_needs_the_agree_the_record_holds() {
    let rs = Records::default();
    begun(&rs, "mcpin", "s1", true);
    assert_eq!(
        rs.step("mcpin", "s1", Some(OWNER), Record::ready)
            .unwrap_err(),
        Why::Stage,
        "no agree yet"
    );
    rs.set("mcpin", "s1", Stage::Agreed);
    assert!(rs.step("mcpin", "s1", Some(OWNER), Record::ready).is_ok());
    rs.set("mcpin", "s1", Stage::Pasted);
    assert_eq!(
        rs.step("mcpin", "s1", Some(OWNER), Record::ready)
            .unwrap_err(),
        Why::Stage,
        "sent already"
    );
}

/// A second owner, or no presser, goes no further with the first owner's sign-in.
#[test]
fn another_user_cannot_step_a_sign_in() {
    let rs = Records::default();
    begun(&rs, "mcpin", "s1", false);
    assert_eq!(
        rs.step("mcpin", "s1", Some(SECOND), Record::ready)
            .unwrap_err(),
        Why::NotYours
    );
    assert_eq!(
        rs.step("mcpin", "s1", None, Record::ready).unwrap_err(),
        Why::NotYours
    );
    assert!(rs.step("mcpin", "s1", Some(OWNER), Record::ready).is_ok());
}

/// An id this binding never issued, of any kind, is refused.
#[test]
fn an_unissued_id_is_refused() {
    let rs = Records::default();
    begun(&rs, "mcpin", "s1", false);
    assert_eq!(
        rs.step("mcpin", "forged", Some(OWNER), Record::ready)
            .unwrap_err(),
        Why::Missing
    );
    assert_eq!(
        rs.step("gsi", "s1", Some(OWNER), Record::ready)
            .unwrap_err(),
        Why::Missing,
        "another kind's id is not this kind's"
    );
}

/// Past its deadline a record is refused and retired: the clock is moved, nothing sleeps.
#[test]
fn a_record_past_its_deadline_is_refused_and_retired() {
    let rs = Records::default();
    begun(&rs, "mcpin", "s1", false);
    rs.advance(Duration::from_secs(599));
    assert!(rs.step("mcpin", "s1", Some(OWNER), Record::ready).is_ok());
    rs.advance(Duration::from_secs(2));
    assert_eq!(
        rs.step("mcpin", "s1", Some(OWNER), Record::ready)
            .unwrap_err(),
        Why::Stale
    );
    assert_eq!(
        rs.step("mcpin", "s1", Some(OWNER), Record::ready)
            .unwrap_err(),
        Why::Missing
    );
}

/// A newer sign-in of a kind retires the older one, so an old form is refused before it reaches
/// the daemon; another kind's is untouched; a sign-out retires its target's.
#[test]
fn a_newer_sign_in_and_a_sign_out_retire_the_old() {
    let rs = Records::default();
    begun(&rs, "mcpin", "old", false);
    begun(&rs, "gsi", "g1", false);
    begun(&rs, "mcpin", "new", false);
    assert_eq!(
        rs.step("mcpin", "old", Some(OWNER), Record::ready)
            .unwrap_err(),
        Why::Missing
    );
    assert!(rs.step("mcpin", "new", Some(OWNER), Record::ready).is_ok());
    assert!(rs.step("gsi", "g1", Some(OWNER), Record::ready).is_ok());
    rs.retire_target("mcpin", "docs");
    assert_eq!(
        rs.step("mcpin", "new", Some(OWNER), Record::ready)
            .unwrap_err(),
        Why::Missing
    );
    assert_eq!(rs.len(), 1);
}

/// An interaction is acted on once while Discord could deliver it again, and forgotten after.
#[test]
fn an_interaction_is_claimed_once() {
    let rs = Records::default();
    assert!(rs.claim(7));
    assert!(!rs.claim(7), "a second delivery");
    assert!(rs.claim(8));
    rs.advance(CLAIM_FOR + Duration::from_secs(1));
    assert!(rs.claim(7), "past its life it is gone from the list");
}

/// Nothing shown is cut: lines are split over messages whole, an address too long for one message
/// is refused, and a line too long for one is refused.
#[test]
fn nothing_shown_is_cut() {
    let lines: Vec<String> = (0..40)
        .map(|n| {
            format!(
                "Part of the sign-in goes through `host{n}.example.invalid`, padded {}",
                "x".repeat(60)
            )
        })
        .collect();
    let pages = pages(&lines).unwrap();
    assert!(pages.len() > 1);
    assert!(pages.iter().all(|p| p.chars().count() <= SHOWN_MAX));
    let joined = pages.join("\n");
    for l in &lines {
        assert!(joined.contains(l.as_str()), "every line whole");
    }
    assert!(super::pages(&["y".repeat(SHOWN_MAX + 1)]).is_none());
    let long = format!("https://auth.example.invalid/{}", "z".repeat(SHOWN_MAX));
    assert!(address_words(&long).is_none());
    assert!(address_words("https://auth.example.invalid/a").is_some());
}
