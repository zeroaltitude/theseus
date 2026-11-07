//! `delay.v1` (theseus-5x8ry): one delayed piece of work, built for the
//! reconsideration point in `theseus-core`'s `turn/delay_step.rs`. The state
//! is what is delayed, how long it has run, what it has cost, and every
//! progress signal the daemon could read beside whether that signal moved
//! since the delay began.
//!
//! Every number is computed here and stated as a field. The pack loader
//! refuses a question that leans on counting or measuring (`COMPUTATION`), and
//! Jev is weak at both, so elapsed, cost and each signal's movement are facts
//! in the state rather than things Jev is asked to work out. A signal the site
//! could not read is left out entirely: an absent signal means *unknown*, and
//! must never read as *not moving*.

use super::*;

const WHAT_CHARS: usize = 200;
const REASON_CHARS: usize = 200;

/// One progress signal, and whether it moved since the delay began.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelaySignal {
    /// What the signal is, in the owner's words ("bytes the job printed").
    pub name: String,
    /// Whether it changed since the delay began.
    pub moved: bool,
}

/// `delay.v1`'s input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelayInput {
    /// Which kind of delay the caller is asking about, as a short word
    /// (`admission_slot`, `admission_turn`, `provider`, `job`, ...). The set
    /// lives in the caller, not here: a new delay site may name a new kind
    /// without a pack version.
    pub kind: String,
    /// What is delayed, in the owner's words.
    pub what: String,
    /// How long it has been delayed.
    pub elapsed_ms: u64,
    /// What the execution has spent so far, in whole cents.
    pub cost_cents: u64,
    /// Every signal the site could read. An empty list is honest: it says
    /// nothing is readable, which is itself what Jev is judging on.
    #[serde(default)]
    pub signals: Vec<DelaySignal>,
    /// The last thing the work was seen doing, where the site knows it.
    #[serde(default)]
    pub last_step: Option<String>,
    /// Why the work was started, where the site knows it.
    #[serde(default)]
    pub reason: Option<String>,
}

pub fn delay(i: &DelayInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("delay", DELAY_VERSION, cap, scrub);
    b.scalar("kind", c.clip(i.kind.trim(), 32));
    b.scalar("what", c.clip(i.what.trim(), WHAT_CHARS))
        .cut_if("what", c.cut());
    // Minutes as well as milliseconds: the question is about whether a wait is
    // long, and a bare millisecond count reads as noise at this scale.
    b.scalar("elapsed_ms", i.elapsed_ms);
    b.scalar("elapsed_minutes", i.elapsed_ms / 60_000);
    b.scalar("spent_cents", i.cost_cents);
    // Counted here, never asked: the loader refuses a question that counts.
    b.scalar("signals_read", i.signals.len());
    b.scalar(
        "signals_that_moved",
        i.signals.iter().filter(|s| s.moved).count(),
    );
    let signals: Vec<Value> = i
        .signals
        .iter()
        .map(|s| {
            json!({
                "signal": c.clip(s.name.trim(), 64),
                "moved_since_the_delay_began": s.moved,
            })
        })
        .collect();
    b.list_after("signals", 10, share(cap, 50), signals, 0)
        .cut_if("signals", c.cut());
    if let Some(s) = i.last_step.as_ref().filter(|s| !s.trim().is_empty()) {
        b.scalar("last_step", c.clip(s.trim(), WHAT_CHARS))
            .cut_if("last_step", c.cut());
    }
    if let Some(r) = i.reason.as_ref().filter(|r| !r.trim().is_empty()) {
        b.scalar("reason", c.clip(r.trim(), REASON_CHARS))
            .cut_if("reason", c.cut());
    }
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::by_name;
    use crate::state::NoScrub;

    fn input() -> DelayInput {
        DelayInput {
            kind: "admission_slot".into(),
            what: "a wake turn on the morning routine".into(),
            elapsed_ms: 660_000,
            cost_cents: 40,
            signals: vec![
                DelaySignal {
                    name: "the admission queue moved".into(),
                    moved: false,
                },
                DelaySignal {
                    name: "bytes a job printed".into(),
                    moved: true,
                },
            ],
            last_step: Some("waiting for a slot".into()),
            reason: Some("the owner's 07:00 routine".into()),
        }
    }

    /// The state states every number the question may not ask for, and the
    /// pack asks its two Nouls and its one Choice.
    #[test]
    fn the_state_states_the_numbers_and_the_pack_asks_three() {
        let p = by_name("delay.v1").unwrap();
        let prepared = prepare(&p, &Input::Delay(input()), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["kind"], "admission_slot");
        assert_eq!(v["elapsed_ms"], 660_000);
        assert_eq!(v["elapsed_minutes"], 11);
        assert_eq!(v["spent_cents"], 40);
        assert_eq!(v["signals_read"], 2);
        assert_eq!(v["signals_that_moved"], 1);
        assert_eq!(v["signals"][1]["moved_since_the_delay_began"], true);
        assert_eq!(v["last_step"], "waiting for a slot");
        let asked = p.ask(&prepared.dynamic);
        assert_eq!(asked.len(), 3);
        let ids: Vec<&str> = asked.iter().map(|a| a.id.as_str()).collect();
        assert!(ids.contains(&"worth_it"));
        assert!(ids.contains(&"progressing"));
        assert!(ids.contains(&"course"));
    }

    /// A site that could read nothing says so: no signals, and the counts are
    /// zero rather than absent. An absent signal must never read as "stalled".
    #[test]
    fn a_site_that_read_no_signal_says_so_and_the_input_round_trips() {
        let mut i = input();
        i.signals.clear();
        i.last_step = None;
        i.reason = None;
        let p = by_name("delay.v1").unwrap();
        let prepared = prepare(&p, &Input::Delay(i.clone()), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["signals_read"], 0);
        assert_eq!(v["signals_that_moved"], 0);
        assert!(v.get("last_step").is_none());
        assert!(v.get("reason").is_none());
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(
            Input::parse(Builder::Delay, &json).unwrap(),
            Input::Delay(i)
        );
    }
}
