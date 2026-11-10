use super::*;

/// A card's rule (theseus-50p): a card written while its lane runs, for a
/// session a place here renders, waits for its place to show its call,
/// for `CARD_WAIT` at most, and then goes all the same; once the place
/// has shown it, it goes at once; a card from before this process, or for
/// a session no place here renders, never waits.
#[test]
fn a_card_waits_for_its_call_at_most_card_wait() {
    let now = tokio::time::Instant::from_std(std::time::Instant::now());
    let later = now + CARD_WAIT / 2;
    // A fresh card for a session rendered here starts its wait.
    assert_eq!(
        card_hold(false, true, true, None, now),
        CardHold::Until(now + CARD_WAIT)
    );
    // It waits on, until its wait is over, never longer.
    let until = now + CARD_WAIT;
    assert_eq!(
        card_hold(false, true, true, Some(until), later),
        CardHold::Until(until)
    );
    assert_eq!(
        card_hold(false, true, true, Some(until), until),
        CardHold::GaveUp
    );
    assert_eq!(
        card_hold(false, true, true, Some(until), until + CARD_WAIT),
        CardHold::GaveUp
    );
    // The place showed the call: it goes, waiting or not.
    assert_eq!(card_hold(true, true, true, None, now), CardHold::Go);
    assert_eq!(
        card_hold(true, true, true, Some(until), later),
        CardHold::Go
    );
    // A card from before this process, or one no place here shows.
    assert_eq!(card_hold(false, false, true, None, now), CardHold::Go);
    assert_eq!(card_hold(false, true, false, None, now), CardHold::Go);
    assert_eq!(
        card_hold(false, true, false, Some(until), later),
        CardHold::Go
    );
}

#[test]
fn a_nonce_is_stable_per_message_and_fits_discord() {
    let a = nonce("turn_1:L0:p0");
    assert_eq!(a, nonce("turn_1:L0:p0"));
    assert_ne!(a, nonce("turn_1:L0:p1"));
    assert!(
        a.len() <= 25 && a.chars().all(|c| c.is_ascii_digit()),
        "{a}"
    );
}

/// A card's answerers (theseus-9j9): the place's users who are owners,
/// whose answers alone count (the place rule, theseus-zmgb); nobody when
/// none of them is.
#[test]
fn a_cards_answerers_are_the_places_users_who_are_owners() {
    // Invented ids, of a Discord id's length.
    let (a, b, c) = (
        100_000_000_000_000_101,
        200_000_000_000_000_202,
        300_000_000_000_000_303,
    );
    let place = [a, b, c];
    assert!(answerers(&place, &BTreeSet::new()).is_empty());
    let owners: BTreeSet<String> =
        [format!("discord:{b}"), "discord:400000000000000404".into()].into();
    assert_eq!(answerers(&place, &owners), [b]);
    assert!(answerers(&[], &owners).is_empty());
    assert_eq!(
        mentioning(&[202, 101], "**Approve?** `fs.write` a.txt".into()),
        "<@202> <@101> **Approve?** `fs.write` a.txt"
    );
    assert_eq!(mentioning(&[], "plain".into()), "plain");
}
