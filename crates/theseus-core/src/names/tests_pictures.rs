//! The picture labels as `names::pictures` reads them from one judgment: the
//! pack's subjects, the act band, and the gate shared with topics.

use theseus_judge::band::band;
use theseus_judge::client::Answer;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::{Judgment, Mode, Thresholds};

use super::{acting, roots, PREFIX, SCREEN, SUBJECT};
use crate::exchange::{touch, Root};
use crate::judge::onto::ONTO_PACK;
use crate::tests_route_data::{choice, judgment};

const ACT: f64 = 0.80;

fn noul(question: &str, p: f64) -> AnswerRecord {
    let answer = Answer::Noul { noul: p };
    AnswerRecord {
        question: question.into(),
        def: question.into(),
        about: None,
        band: band(
            &answer,
            Thresholds {
                act: ACT,
                confirm: 0.55,
            },
        ),
        no_match: None,
        answer,
    }
}

/// The wired onto pack's judgment, live, with `answers`.
fn onto(answers: Vec<AnswerRecord>) -> Judgment {
    let mut j = judgment(answers, false);
    j.pack = ONTO_PACK.into();
    j.mode = Mode::Live;
    j
}

/// KEY: every subject the wired pack offers is a label a touch takes, and the
/// reader is pinned to the pack's own question ids.
#[test]
fn every_subject_the_pack_offers_is_a_root_a_touch_takes() {
    let pack = theseus_judge::pack::by_name(ONTO_PACK).expect("the wired onto pack");
    let q = pack.question(SUBJECT).expect("the pack asks the subject");
    assert!(pack.question(SCREEN).is_some(), "the pack asks the screen");
    assert!(!q.options.is_empty());
    for o in &q.options {
        let j = onto(vec![choice(SUBJECT, &o.id, 0.95)]);
        let want = format!("{PREFIX}{}", o.id);
        assert_eq!(roots(&j, ACT), vec![want.clone()], "{}", o.id);
        let t = touch(Root::Thing(&want), "trn_1", "ses_1", 1);
        assert!(t.is_some(), "{want} is a root a touch takes");
    }
}

/// Only an answer at the act band is a label, and a no to the screen
/// question is none.
#[test]
fn only_answers_at_the_act_band_are_labels_and_a_no_is_none() {
    let both = onto(vec![choice(SUBJECT, "document", 0.95), noul(SCREEN, 0.9)]);
    let want = ["picture:document", "picture:shows_screen_or_document"];
    assert_eq!(roots(&both, ACT), want);
    let no = onto(vec![choice(SUBJECT, "photo", 0.80), noul(SCREEN, 0.05)]);
    assert_eq!(roots(&no, ACT), ["picture:photo"]);
    let unsure = onto(vec![choice(SUBJECT, "photo", 0.79), noul(SCREEN, 0.79)]);
    assert!(roots(&unsure, ACT).is_empty());
    assert!(roots(&onto(vec![]), ACT).is_empty(), "no picture, no label");
}

/// KEY: only the wired onto pack's judgment, answered by its own model in a
/// mode that acts, is read: never a learned version's, a shadow one's, a
/// drifted one's or another pack's.
#[test]
fn only_the_wired_acting_onto_judgment_is_read() {
    let answers = || vec![choice(SUBJECT, "document", 0.95), noul(SCREEN, 0.9)];
    let live = onto(answers());
    let mut learned = onto(answers());
    learned.pack = "onto.v101".into();
    let mut shadow = onto(answers());
    shadow.mode = Mode::Shadow;
    let mut drifted = judgment(answers(), true);
    drifted.pack = ONTO_PACK.into();
    let mut other = onto(answers());
    other.pack = crate::judge::inbound::CLASSIFY_PACK.into();
    let batch = vec![live, learned, shadow, drifted, other];
    let read: Vec<&str> = acting(&batch).iter().map(|j| j.pack.as_str()).collect();
    assert_eq!(read, vec![ONTO_PACK]);
    assert_eq!(acting(&batch)[0].mode, Mode::Live);
}
