//! The pure parts of capture: the words, the decision by band, the scope.

use theseus_judge::band::Band;
use theseus_protocol::{StandingKind, StandingRecord, StandingScope};

use super::state::{Candidate, Open, State, WEEK_MS};
use super::*;

fn verdict(heard: Heard, band: Band, confidence: f64) -> Verdict {
    Verdict {
        heard,
        band,
        confidence,
        p_correction: 0.0,
        said: Said::Unsure,
        sure_of_scope: false,
        judgment: "jdg_1".into(),
        pack: PACK.into(),
    }
}

fn record(id: &str, kind: StandingKind, text: &str) -> StandingRecord {
    StandingRecord {
        id: id.into(),
        kind,
        text: text.into(),
        scope: StandingScope::Everywhere,
        since_ms: 1,
        until_ms: None,
        source: "the CLI".into(),
        ends: None,
    }
}

fn plan(v: &Verdict, line: &str, live: &[StandingRecord], state: &State) -> Plan {
    let live: Vec<&StandingRecord> = live.iter().collect();
    decide(v, line, "nod_new", &live, state, 10 * WEEK_MS)
}

#[test]
fn a_line_is_the_owners_words_on_one_line_and_not_too_long() {
    assert_eq!(
        words::line("  no tables\n  with me \n").as_deref(),
        Some("no tables with me")
    );
    assert_eq!(words::line("   \n "), None);
    assert!(words::line(&"word ".repeat(100)).is_none());
    assert!(words::line(&"x".repeat(words::MAX_LINE)).is_some());
}

#[test]
fn two_lines_are_the_same_directive_when_their_content_words_overlap() {
    assert!(words::same(
        "no tables with me",
        "please no tables in answers"
    ));
    assert!(words::same(
        "short answers, code first",
        "Code first, short answers"
    ));
    assert!(!words::same("no tables with me", "I live in Phoenix"));
    // Nothing shared, or nothing to share, is never the same.
    assert!(!words::same("ok", "ok"));
    assert!(!words::same("", ""));
}

#[test]
fn at_the_act_band_a_preference_a_fact_and_a_correction_are_kept() {
    let st = State::default();
    for (heard, kind) in [
        (Heard::Preference, StandingKind::Preference),
        (Heard::Fact, StandingKind::Fact),
        (Heard::Correction, StandingKind::Preference),
    ] {
        let v = verdict(heard, Band::Act, 0.92);
        assert_eq!(
            plan(&v, "no tables with me", &[], &st),
            Plan::Keep(kind),
            "{heard:?}"
        );
    }
}

#[test]
fn an_order_is_asked_at_every_confidence_and_never_kept() {
    let st = State::default();
    for (band, c) in [(Band::Act, 0.99), (Band::Act, 0.8), (Band::Confirm, 0.6)] {
        let v = verdict(Heard::Order, band, c);
        assert_eq!(
            plan(&v, "never push to main", &[], &st),
            Plan::Ask {
                kind: StandingKind::Order,
                why: "order"
            },
            "{band:?} {c}"
        );
    }
}

#[test]
fn between_half_and_four_fifths_it_asks() {
    let st = State::default();
    let v = verdict(Heard::Preference, Band::Confirm, 0.65);
    assert_eq!(
        plan(&v, "keep it short", &[], &st),
        Plan::Ask {
            kind: StandingKind::Preference,
            why: "unsure"
        }
    );
}

#[test]
fn below_half_nothing_is_kept_or_asked() {
    let st = State::default();
    let mut v = verdict(Heard::Preference, Band::Escalate, 0.4);
    v.p_correction = 0.05;
    assert_eq!(plan(&v, "keep it short", &[], &st), Plan::Nothing);
    let none = verdict(Heard::Nothing, Band::Act, 0.97);
    assert_eq!(plan(&none, "what is 2 + 2", &[], &st), Plan::Nothing);
}

#[test]
fn a_correction_below_the_band_is_noted_and_proposed_when_said_twice_in_a_week() {
    let mut st = State::default();
    let mut v = verdict(Heard::Correction, Band::Escalate, 0.4);
    v.p_correction = 0.4;
    assert_eq!(
        plan(&v, "stop using tables for lists", &[], &st),
        Plan::Candidate
    );
    st.candidates.push(Candidate {
        node: "nod_old".into(),
        line: "stop using tables for lists".into(),
        at_ms: 10 * WEEK_MS - 2 * 24 * 3600 * 1000,
    });
    // The same words in another message, inside the week: proposed.
    assert_eq!(
        plan(&v, "again, no tables for lists please", &[], &st),
        Plan::Ask {
            kind: StandingKind::Preference,
            why: "twice"
        }
    );
    // Not its own node, not a different matter, not a stale one.
    assert_eq!(
        decide(
            &v,
            "stop using tables for lists",
            "nod_old",
            &[],
            &st,
            10 * WEEK_MS
        ),
        Plan::Candidate
    );
    assert_eq!(plan(&v, "call me Sam", &[], &st), Plan::Candidate);
    st.candidates[0].at_ms = 10 * WEEK_MS - 8 * 24 * 3600 * 1000;
    assert_eq!(
        plan(&v, "again, no tables for lists please", &[], &st),
        Plan::Candidate
    );
    // A correction too faint to note is nothing.
    let faint = Verdict {
        p_correction: 0.05,
        ..verdict(Heard::Nothing, Band::Act, 0.9)
    };
    assert_eq!(plan(&faint, "thanks", &[], &st), Plan::Nothing);
}

#[test]
fn a_message_that_says_a_live_record_again_is_counted_not_kept() {
    let st = State::default();
    let live = [
        record("std_a", StandingKind::Preference, "no tables with me"),
        record("std_b", StandingKind::Fact, "I live in Phoenix"),
    ];
    let v = verdict(Heard::Preference, Band::Act, 0.9);
    assert_eq!(
        plan(&v, "I said no tables with me", &live, &st),
        Plan::Repeat("std_a".into())
    );
    // The same words as a record of another kind are not a repeat of it.
    let fact = verdict(Heard::Fact, Band::Act, 0.9);
    assert_eq!(
        plan(&fact, "no tables with me", &live, &st),
        Plan::Keep(StandingKind::Fact)
    );
    // An order said again is a repeat too, never a second ask.
    let order = [record("std_c", StandingKind::Order, "never push to main")];
    let o = verdict(Heard::Order, Band::Confirm, 0.7);
    assert_eq!(
        plan(&o, "never push to main, I told you", &order, &st),
        Plan::Repeat("std_c".into())
    );
}

#[test]
fn an_ask_already_open_is_not_asked_again() {
    let mut st = State::default();
    st.asks.push(Open {
        ask: theseus_protocol::StandingAsk {
            id: "ask_1".into(),
            kind: StandingKind::Order,
            text: "never push to main".into(),
            scope: StandingScope::Everywhere,
            why: "order".into(),
            node: "nod_1".into(),
            at_ms: 10 * WEEK_MS,
        },
        judgment: "jdg_0".into(),
        session: "ses_1".into(),
    });
    let v = verdict(Heard::Order, Band::Act, 0.95);
    assert_eq!(plan(&v, "never push to main", &[], &st), Plan::Nothing);
}

#[test]
fn the_scope_is_jevs_when_it_is_sure_and_else_where_the_words_were_said() {
    let mut v = verdict(Heard::Preference, Band::Act, 0.9);
    let dm = Some("discord:dm:42");
    assert_eq!(
        scope_of(&v, "ses_1", dm),
        StandingScope::Place("discord:dm:42".into())
    );
    assert_eq!(
        scope_of(&v, "ses_1", None),
        StandingScope::Session("ses_1".into())
    );
    // Not sure of the scope: where it was said, whatever it said.
    v.said = Said::Everywhere;
    assert_eq!(
        scope_of(&v, "ses_1", None),
        StandingScope::Session("ses_1".into())
    );
    v.sure_of_scope = true;
    assert_eq!(scope_of(&v, "ses_1", dm), StandingScope::Everywhere);
    v.said = Said::Session;
    assert_eq!(
        scope_of(&v, "ses_1", dm),
        StandingScope::Session("ses_1".into())
    );
    v.said = Said::Place;
    assert_eq!(
        scope_of(&v, "ses_1", dm),
        StandingScope::Place("discord:dm:42".into())
    );
}

#[test]
fn the_state_forgets_what_lapsed_and_counts_the_weeks_repeats() {
    let now = 40 * 24 * 3600 * 1000;
    let day = 24 * 3600 * 1000;
    let mut st = State::default();
    st.repeats.insert(
        "std_a".into(),
        vec![now - 3 * day, now - 10 * day, now - 35 * day],
    );
    st.candidates.push(Candidate {
        node: "n".into(),
        line: "x".into(),
        at_ms: now - 8 * day,
    });
    st.trim(now);
    assert!(st.candidates.is_empty());
    assert_eq!(st.repeats["std_a"].len(), 2, "a month's, not older");
    assert_eq!(st.week_of("std_a", now), 1);
    assert_eq!(st.week_of("std_zzz", now), 0);
}

#[test]
fn the_notice_names_what_was_kept_where_and_from_which_surface() {
    assert_eq!(
        kept_line(
            StandingKind::Preference,
            &StandingScope::Everywhere,
            "terminal",
            "short answers, code first"
        ),
        "📌 Kept as a standing preference everywhere (terminal): short answers, code first"
    );
    assert_eq!(surface_name("cli (private)"), "terminal");
    assert_eq!(surface_name("web (private)"), "web");
    assert_eq!(surface_name("discord_dm (private)"), "Discord");
    assert!(ask_line(
        StandingKind::Order,
        &StandingScope::Everywhere,
        "order",
        "never push to main"
    )
    .contains("always ask"));
}

#[test]
fn only_a_place_the_owner_alone_writes_in_is_theirs() {
    use crate::places::PlaceClass::{Private, Shared};
    assert!(owner_alone(Private, None));
    assert!(owner_alone(Private, Some("discord:dm:42")));
    assert!(!owner_alone(Private, Some("discord:channel:7")));
    assert!(!owner_alone(Shared, None));
    assert!(!owner_alone(Shared, Some("discord:dm:42")));
}
