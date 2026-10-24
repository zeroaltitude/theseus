//! The floor beneath Jev's `data` answer (theseus-f9qa8): what a restricted
//! model may be sent, for every answer, band, sensitive name and session
//! level. Only `open` at the act band, naming no sensitive name, on a session
//! at `Level::Open`, is admitted and cleared; no wording of the route pack
//! moves a row of this table, since the code reads the choice and the band,
//! never the words. The session's level (`google::private::Level`, ops-cal)
//! is read through the scope the floor reads (`private::scoped`, `in_scope`),
//! here as a request compiled without the open view: `Sensitive` is refused
//! like `Private`. The chokepoint's own refusal is pinned in `tests_data_gate.rs`.

use theseus_judge::band::Band;

use super::tests_route_data::{choice, judgment};
use crate::data::{admits, clearance, Carried, Carry, Check, Writer};
use crate::google::private::{self, Level};
use crate::judge::inbound::verdict;

/// The confidence that falls in each band under the pack's 0.90 and 0.60.
const BANDS: [(Band, f64); 3] = [
    (Band::Act, 0.95),
    (Band::Confirm, 0.70),
    (Band::Escalate, 0.40),
];

/// The levels a session can be at.
const LEVELS: [Level; 3] = [Level::Open, Level::Private, Level::Sensitive];

/// Whether a restricted model may see the request the turn's message makes.
fn may_see(answer: &str, confidence: f64, name_hit: bool, level: Level) -> bool {
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
        open_view: !private::scoped(level, private::in_scope),
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
fn only_open_at_act_with_no_name_on_an_open_session_reaches_a_restricted_model() {
    let mut seen = 0;
    for answer in ["open", "private", "unclear"] {
        for (band, confidence) in BANDS {
            for name_hit in [false, true] {
                for level in LEVELS {
                    let want =
                        answer == "open" && band == Band::Act && !name_hit && level == Level::Open;
                    assert_eq!(
                        may_see(answer, confidence, name_hit, level),
                        want,
                        "{answer} at {band:?}, name {name_hit}, level {level:?}"
                    );
                    seen += usize::from(want);
                }
            }
        }
    }
    assert_eq!(seen, 1, "one row of the table is admitted");
}
