//! The bar a promotion is held to (design §2.7, "Moving up"; §2.9's
//! holdouts): a learning report of the version whose frozen holdout has
//! 200 labeled judgments per deciding question and 30 per acting class
//! (`learn::sufficient`), in which the version beats its baseline. A
//! rolled-back version's report must be written after its rollback.
//!
//! An automatic promotion (`Core::promote_automatic`, the learning loop's)
//! is refused one short of the bar, with the numbers. The owner may promote
//! at any time: below the bar his row says `forced`, with the same numbers.
//!
//! "Beats its baseline" is read from the report's holdout as the code can
//! settle it: every acting class (`learning::report::ACTING`: `loop.v1`'s
//! `work_state: progressing`) has a labeled precision above one half, so
//! when the version acts where the baseline would not, it is right more
//! often than wrong. A pack with no acting class listed has nothing the
//! report can set against its baseline, and the sample's minimum is its bar.

use std::collections::BTreeMap;

use theseus_judge::learn::{self, Minimum};
use theseus_protocol::learning::PackReport;
use theseus_protocol::packs::HoldoutBounds;
use theseus_protocol::LedgerKind;

use crate::ledger::LedgerRow;
use crate::store::Store;

/// The days back a promotion looks for the version's latest report.
pub const LOOK_BACK_DAYS: u64 = 14;

/// What the bar found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bar {
    /// The report cited: `rpt_<date>_<pack>`.
    pub report: Option<String>,
    pub holdout: Option<HoldoutBounds>,
    /// What is short, in numbers; none when the bar is cleared.
    pub short: Option<String>,
}

impl Bar {
    pub fn cleared(&self) -> bool {
        self.short.is_none()
    }
}

/// A report row of `pack`, by its key.
fn report_row(store: &Store, key: &str, pack: &str) -> Option<(u64, PackReport)> {
    let r = store.ledger_by_key(key).ok()??;
    let row: LedgerRow = r.decode().ok()?;
    if row.kind != LedgerKind::JudgeReport.as_str() {
        return None;
    }
    let report: PackReport = serde_json::from_value(row.data["report"].clone()).ok()?;
    (report.pack == pack).then_some((row.at_unix_ms, report))
}

/// The bar for `pack` (a version, `loop.v1`), citing `report` or else the
/// version's latest of the last [`LOOK_BACK_DAYS`] days; a rolled-back
/// version's must be written after `rolled_back_at`.
pub fn bar(
    store: &Store,
    pack: &str,
    report: Option<&str>,
    rolled_back_at: Option<u64>,
    now: u64,
) -> Bar {
    let found = match report {
        Some(key) => report_row(store, key, pack).map(|r| (key.to_string(), r)),
        None => (0..=LOOK_BACK_DAYS).find_map(|d| {
            let day = crate::judge::spend::local_day(now.saturating_sub(d * learn::DAY_MS));
            let key = crate::rpc::learning::report_key(&day, pack);
            report_row(store, &key, pack).map(|r| (key, r))
        }),
    };
    let Some((key, (at_ms, r))) = found else {
        return Bar {
            short: Some(match report {
                Some(k) => format!("{k} is no learning report of {pack}"),
                None => format!("no learning report of {pack} in the last {LOOK_BACK_DAYS} days"),
            }),
            ..Bar::default()
        };
    };
    let holdout = Some(HoldoutBounds {
        start_ms: r.holdout.start_ms,
        end_ms: r.holdout.end_ms,
    });
    let short = match rolled_back_at {
        Some(at) if at_ms <= at => Some(format!(
            "{key} was written before the rollback; a move up cites a report written after it"
        )),
        _ => short_of(pack, &r),
    };
    Bar {
        report: Some(key),
        holdout,
        short,
    }
}

/// What a report's holdout leaves short of the bar, in numbers; none when
/// it clears it.
pub fn short_of(pack: &str, r: &PackReport) -> Option<String> {
    let usizes = |m: &BTreeMap<String, u32>| -> BTreeMap<String, usize> {
        m.iter().map(|(k, v)| (k.clone(), *v as usize)).collect()
    };
    let deciding: Vec<String> = theseus_judge::pack::by_name(pack)
        .map(|p| {
            p.questions
                .iter()
                .filter(|q| q.decides)
                .map(|q| q.id.clone())
                .collect()
        })
        .unwrap_or_default();
    let deciding: Vec<&str> = deciding.iter().map(String::as_str).collect();
    let acting: Vec<(&str, &str)> = crate::learning::report::ACTING
        .iter()
        .filter(|(id, _, _)| *id == super::id_of(pack))
        .flat_map(|(_, q, cs)| cs.iter().map(move |c| (*q, *c)))
        .collect();
    let classes: Vec<&str> = acting.iter().map(|(_, c)| *c).collect();
    let h = &r.holdout;
    if let Err(why) = learn::sufficient(
        &usizes(&h.labeled_per_question),
        &usizes(&h.labeled_per_acting_class),
        &deciding,
        &classes,
        Minimum::default(),
    ) {
        return Some(why);
    }
    for (q, c) in acting {
        let precision = h
            .questions
            .iter()
            .find(|x| x.question == q)
            .and_then(|x| x.classes.iter().find(|k| k.class == c))
            .and_then(|k| k.precision);
        match precision {
            Some(p) if p > 0.5 => {}
            Some(p) => {
                return Some(format!(
                    "{q}: {c}'s labeled precision {p:.2} does not beat the baseline (above 0.50)"
                ))
            }
            None => return Some(format!("{q}: {c} has no labeled precision")),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use theseus_protocol::learning::{ClassReport, Holdout, QuestionReport};

    use super::*;

    fn report(labeled: u32, progressing: u32, precision: Option<f64>) -> PackReport {
        PackReport {
            pack: "loop.v1".into(),
            holdout: Holdout {
                labeled_per_question: [
                    ("work_state".to_string(), labeled),
                    ("announced_unfinished".to_string(), labeled),
                ]
                .into(),
                labeled_per_acting_class: [("progressing".to_string(), progressing)].into(),
                questions: vec![QuestionReport {
                    question: "work_state".into(),
                    classes: vec![ClassReport {
                        class: "progressing".into(),
                        precision,
                        ..ClassReport::default()
                    }],
                    ..QuestionReport::default()
                }],
                ..Holdout::default()
            },
            ..PackReport::default()
        }
    }

    /// One short of 200 is refused with the numbers, one short of 30 acting
    /// too, a precision at or below a half does not beat the baseline, and
    /// the full bar clears.
    #[test]
    fn the_bar_says_what_is_short_in_numbers() {
        let short = short_of("loop.v1", &report(199, 30, Some(0.9))).unwrap();
        assert!(short.contains("labeled 199 of 200"), "{short}");
        let short = short_of("loop.v1", &report(200, 29, Some(0.9))).unwrap();
        assert_eq!(short, "progressing: labeled 29 of 30");
        let short = short_of("loop.v1", &report(200, 30, Some(0.5))).unwrap();
        assert!(short.contains("does not beat the baseline"), "{short}");
        assert_eq!(short_of("loop.v1", &report(200, 30, Some(0.51))), None);
    }
}
