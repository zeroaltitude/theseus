//! The floor beneath Jev's `data` answer (theseus-f9qa8): what a restricted
//! model may be sent, for every answer, band, sensitive name and private mark.
//! Only `open` at the act band, naming no sensitive name, on a session
//! without the private mark, is admitted and cleared; no wording of the
//! route pack moves a row of this table, since the code reads the choice and
//! the band, never the words. The sensitive level (`Level::Sensitive`) joins
//! this table with the ops-cal rows.

use theseus_judge::band::Band;

use super::tests_route_data::{choice, judgment};
use crate::data::{admits, clearance, Carried, Carry, Check, Writer};
use crate::judge::inbound::verdict;

/// The confidence that falls in each band under the pack's 0.90 and 0.60.
const BANDS: [(Band, f64); 3] = [
    (Band::Act, 0.95),
    (Band::Confirm, 0.70),
    (Band::Escalate, 0.40),
];

/// Whether a restricted model may see the request the turn's message makes.
fn may_see(answer: &str, confidence: f64, name_hit: bool, private_mark: bool) -> bool {
    let j = judgment(
        vec![
            choice("mode", "chat", 0.95),
            choice("data", answer, confidence),
        ],
        false,
    );
    let data = verdict(&j, "trn_1", name_hit).unwrap().data;
    let check = Check {
        input: "n1",
        before: None,
        answer: data.as_deref(),
        late: false,
        live: true,
        sealed: false,
        attachment: false,
    };
    let Ok(run) = admits(None, &check) else {
        return false;
    };
    let carried = Carried {
        open_view: !private_mark,
        from: Some(1),
        nodes: vec![Carry {
            id: "n1".into(),
            at: 1,
            writer: Writer::Loop,
        }],
    };
    clearance(&run, &carried).is_cleared()
}

#[test]
fn only_open_at_act_with_no_name_and_no_private_mark_reaches_a_restricted_model() {
    let mut seen = 0;
    for answer in ["open", "private", "unclear"] {
        for (band, confidence) in BANDS {
            for name_hit in [false, true] {
                for private_mark in [false, true] {
                    let want = answer == "open" && band == Band::Act && !name_hit && !private_mark;
                    assert_eq!(
                        may_see(answer, confidence, name_hit, private_mark),
                        want,
                        "{answer} at {band:?}, name {name_hit}, private mark {private_mark}"
                    );
                    seen += usize::from(want);
                }
            }
        }
    }
    assert_eq!(seen, 1, "one row of the table is admitted");
}
