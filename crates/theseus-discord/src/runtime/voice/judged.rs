//! The call's judge of short or unclear transcripts (theseus-s3dwj): the
//! engine's `Judge`, answered by the core's `heard.v1` (Jev, live). Nothing
//! here decides: it maps the engine's case onto the pack's input and Jev's
//! answer back, and the core records each call as a `judge.call` row. The
//! core is held by `Weak`, so a call outliving the daemon asks nothing.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};

use theseus_core::judge::heard::{HeardInput, HeardOverlap, HeardVerdict};
use theseus_core::Core;
use theseus_protocol::PlaceClass;
use theseus_voice::{Began, Case, Engine, Judge, Said, Verdict};

use super::super::Shared;

/// `engine`, given a judge when `[voice] judge_unclear` is on (the default);
/// off, nothing is asked and its events and times are the rules' alone.
pub(super) fn attach(engine: Engine, shared: &Shared, session: &str, target: &str) -> Engine {
    if !shared.voice.cfg.judge_unclear {
        return engine;
    }
    let core = &shared.core;
    let private = core.runner.place_rule.class(&core.cfg, Some(target)) == PlaceClass::Private;
    engine.with_judge(Arc::new(CoreJudge::new(core, session, private)))
}

pub(super) struct CoreJudge {
    core: Weak<Core>,
    /// The voice place's session: the ladder's arm and the row's context.
    session: String,
    /// `private` or `shared`: the place's class.
    place: &'static str,
}

impl CoreJudge {
    pub(super) fn new(core: &Arc<Core>, session: &str, private: bool) -> Self {
        Self {
            core: Arc::downgrade(core),
            session: session.to_string(),
            place: if private { "private" } else { "shared" },
        }
    }
}

/// The pack's input for `case`.
fn input(case: Case, place: &str) -> HeardInput {
    let ms = |d: std::time::Duration| d.as_millis() as u64;
    HeardInput {
        transcript: case.text,
        said: case.said,
        overlap: match case.began {
            Began::During => HeardOverlap::During,
            Began::Tail => HeardOverlap::Tail,
            Began::Join => HeardOverlap::Join,
        },
        after_start_ms: case.timing.after_start.map(ms),
        after_end_ms: case.timing.after_end.map(ms),
        length_ms: ms(case.length),
        confidence: case.confidence.map(f64::from),
        echo_prone: case.echo_prone,
        place: place.to_string(),
    }
}

/// Jev's answer as the engine reads it; an option the pack does not offer is none.
fn verdict(v: HeardVerdict) -> Option<Verdict> {
    let said = match v.choice.as_str() {
        "addressed" => Said::Addressed,
        "echo" => Said::Echo,
        "other" => Said::Other,
        _ => return None,
    };
    Some(Verdict {
        said,
        probability: v.p,
        acts: v.acts,
        judgment: v.judgment,
    })
}

impl Judge for CoreJudge {
    fn judge(&self, case: Case) -> Pin<Box<dyn Future<Output = Option<Verdict>> + Send>> {
        let core = self.core.upgrade();
        let (session, input) = (self.session.clone(), input(case, self.place));
        Box::pin(async move {
            let core = core?;
            verdict(core.runner.judge.judge_heard(&session, input).await?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pack's act band is the one the engine takes as acting: the core
    /// reads the band from the pack, so it is held here against the 0.80 the
    /// task and the engine's docs name.
    #[test]
    fn the_packs_act_band_is_point_eight() {
        let pack = theseus_judge::pack::by_name("heard.v1").unwrap();
        let q = pack.questions.iter().find(|q| q.id == "heard").unwrap();
        assert_eq!(q.thresholds.act, 0.80);
        assert_eq!(q.thresholds.confirm, 0.60);
    }

    #[test]
    fn a_case_maps_onto_the_packs_input_and_an_unknown_option_is_none() {
        let case = Case {
            text: "yeah".into(),
            said: vec!["Shall I go on?".into()],
            began: Began::Tail,
            timing: theseus_voice::Timing {
                after_start: None,
                after_end: Some(std::time::Duration::from_millis(300)),
            },
            length: std::time::Duration::from_millis(700),
            confidence: Some(0.5),
            echo_prone: true,
        };
        let i = input(case, "private");
        assert_eq!(
            (i.overlap, i.after_end_ms, i.length_ms, i.echo_prone),
            (HeardOverlap::Tail, Some(300), 700, true)
        );
        let said = |choice: &str| {
            verdict(HeardVerdict {
                judgment: "jdg_x".into(),
                choice: choice.into(),
                p: 0.9,
                acts: true,
            })
        };
        assert_eq!(said("echo").map(|v| v.said), Some(Said::Echo));
        assert!(said("elsewhere").is_none());
    }
}
