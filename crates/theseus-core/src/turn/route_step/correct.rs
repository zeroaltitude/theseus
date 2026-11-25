//! The owner's correction at the turn (theseus-q31l; `crate::correction`).
//!
//! - **A reaction, a press, the CLI** (`route.correct`): written in their own
//!   frame by the core, and the session's next turn runs where they said.
//!   Nothing here reads the message's words.

use anyhow::Context as _;
use serde_json::json;
use theseus_store::NewRecord;

use super::*;
use crate::correction::{self, Resolved, To};

/// One correction, resolved and checked, before it is written.
#[derive(Debug, Clone)]
pub struct Correcting {
    pub id: String,
    pub label_id: Option<String>,
    /// The label as stored (`{"not": "chat"}`), checked against the pack.
    pub label: Option<serde_json::Value>,
    pub last: RoutedTurn,
    pub resolved: Resolved,
    pub who: String,
    pub via: String,
    pub provenance: String,
    pub at_ms: u64,
}

impl Correcting {
    /// Where it steers the session's next turn, or this one.
    pub fn steering(&self) -> Option<Steering> {
        Some(Steering {
            steer: self.resolved.steer.clone()?,
            follows: self.label_id.clone().unwrap_or_else(|| self.id.clone()),
        })
    }

    fn note(&self) -> String {
        format!(
            "the owner's correction {}: it should have run on {} ({} {})",
            self.id, self.resolved.to, self.via, self.provenance
        )
    }

    fn label_fact<'a>(&'a self, note: &'a str) -> Option<fact::judge::JudgeLabel<'a>> {
        Some(fact::judge::JudgeLabel {
            id: self.label_id.as_deref()?,
            judgment: self.last.judgment.as_deref()?,
            pack: &self.last.pack,
            question: Some(self.resolved.question),
            about: None,
            label: self.label.clone()?,
            source: "operator",
            who: &self.who,
            via: &self.via,
            weight: 1.0,
            note,
            correlation_id: None,
            rule: None,
        })
    }

    fn fact(&self) -> fact::route::RouteCorrected<'_> {
        fact::route::RouteCorrected {
            id: &self.id,
            turn: &self.last.turn,
            judgment: self.last.judgment.as_deref(),
            pack: &self.last.pack,
            session: &self.last.session,
            mode: self.last.mode.as_deref(),
            ran: &self.last.profile,
            to: &self.resolved.to,
            profile: self.resolved.profile.as_deref(),
            steer: correction::steer_json(self.resolved.steer.as_ref()),
            label: self.label_id.as_deref(),
            who: &self.who,
            via: &self.via,
            provenance: &self.provenance,
            at_ms: self.at_ms,
        }
    }

    /// Its rows: the owner's label (scoped as its judgment, keyed `lbl_…`),
    /// when the mode was wrong, and the `route.corrected` row (keyed
    /// `rcx_…`), for one frame.
    pub fn records(&self) -> anyhow::Result<Vec<NewRecord>> {
        let session = Some(self.last.session.as_str());
        let note = self.note();
        let mut out = Vec::new();
        if let (Some(f), Some(id)) = (self.label_fact(&note), &self.label_id) {
            let mut r = fact::row(&f, session, None)?;
            r.key = Some(id.clone());
            out.push(r.scoped(&crate::rpc::judge::scope_of(&self.last.pack)));
        }
        let mut r = fact::row(&self.fact(), session, None)?;
        r.key = Some(self.id.clone());
        out.push(r.scoped(correction::SCOPE));
        Ok(out)
    }

    /// Its notifications and sentences, once its rows are written.
    pub fn announce(&self, rec: &fact::Rec<'_>) {
        let note = self.note();
        if let Some(f) = self.label_fact(&note) {
            rec.announce(&f);
        }
        rec.announce(&self.fact());
    }

    /// What `route.correct` answers.
    pub fn result(&self) -> theseus_protocol::route::RouteCorrectResult {
        if let Some(Steer::Persona(p)) = &self.resolved.steer {
            let labeled = self
                .label_id
                .as_ref()
                .map(|l| format!("; labeled {p} on the persona question ({l})"))
                .unwrap_or_default();
            return theseus_protocol::route::RouteCorrectResult {
                correction: self.id.clone(),
                turn_id: self.last.turn.clone(),
                label: self.label_id.clone(),
                judgment: self.last.judgment.clone(),
                profile: None,
                line: format!(
                    "Corrected {}: {p} answers from the next message{labeled}.",
                    self.last.turn
                ),
            };
        }
        let moved = match &self.resolved.profile {
            Some(p) if *p != self.last.profile => {
                format!("the session runs on {p} from its next turn")
            }
            Some(p) => format!("it ran on {p} already"),
            None => format!("there is no {} profile to move to", self.resolved.to),
        };
        let labeled = match (&self.label_id, &self.last.mode) {
            (Some(l), Some(m)) => format!("; labeled not {m} ({l})"),
            _ => String::new(),
        };
        theseus_protocol::route::RouteCorrectResult {
            correction: self.id.clone(),
            turn_id: self.last.turn.clone(),
            label: self.label_id.clone(),
            judgment: self.last.judgment.clone(),
            profile: self.resolved.profile.clone(),
            line: format!("Corrected {}: {}{labeled}.", self.last.turn, moved),
        }
    }
}

impl TurnRunner {
    /// A correction of `last` to `to`, resolved against the profiles now,
    /// with its label checked against the judgment's pack. Nothing written.
    pub fn correcting(
        &self,
        last: RoutedTurn,
        to: &To,
        who: String,
        via: &str,
        provenance: String,
    ) -> anyhow::Result<Correcting> {
        let resolved = correction::resolve(to, &last, &self.route_profiles());
        let asked = resolved.question != "persona" || last.persona_asked;
        let label = match (&resolved.label, &last.judgment) {
            (Some(words), Some(_)) if asked => {
                let pack = self
                    .judge
                    .pack(&last.pack)
                    .with_context(|| format!("this build has no pack {}", last.pack))?;
                let l =
                    crate::learning::labels::check(&pack, Some(resolved.question), &json!(words))
                        .map_err(anyhow::Error::msg)?;
                Some(l)
            }
            _ => None,
        };
        Ok(Correcting {
            id: crate::new_id("rcx"),
            label_id: label.as_ref().map(|_| crate::new_id("lbl")),
            label,
            last,
            resolved,
            who,
            via: via.to_string(),
            provenance,
            at_ms: theseus_protocol::now_unix_ms(),
        })
    }

    /// A correction written: its label to the ladder's count of the day's
    /// labels, off the turn's path.
    pub fn corrected(&self, c: &Correcting) {
        let Some(label) = c.label.clone() else {
            return;
        };
        let (judge, pack) = (self.judge.clone(), c.last.pack.clone());
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => drop(rt.spawn_blocking(move || judge.land_label(&pack, &label))),
            Err(_) => judge.land_label(&pack, &label),
        }
    }

    /// The inbound point's part: a correction waiting for this session's
    /// next turn (`route.correct`) steers it. A persona's steer needs no
    /// route verdict to apply; a profile's waits for a turn that asked the
    /// route pack (`asked`).
    pub(in crate::turn) fn correction_point(&self, t: &mut Turn<'_>, asked: bool) {
        let sid = t.tc.session_id;
        let corrections = &self.judge.corrections;
        if let Some(s) = corrections.take_pending_if(sid, |s| matches!(s.steer, Steer::Persona(_)))
        {
            Self::set_steer(t, s);
        }
        if !asked {
            return;
        }
        if let Some(s) = corrections.take_pending(sid) {
            Self::set_steer(t, s);
        }
    }

    /// A steer, to where it acts: a persona's to the persona decision, any
    /// other to the route's.
    fn set_steer(t: &mut Turn<'_>, s: Steering) {
        match s.steer {
            Steer::Persona(_) => t.route.persona_steer = Some(s),
            _ => t.route.steer = Some(s),
        }
    }

    /// Where a steered turn runs: a profile by `routing::steer_to`, a mode
    /// as a verdict of that mode at full confidence would place it.
    pub(super) fn decide_steer(
        &self,
        t: &Turn<'_>,
        session: &SessionRecord,
        s: &Steering,
    ) -> Decision {
        let profiles = self.route_profiles();
        let hold = session.routed.as_ref().and_then(|r| r.hold.clone());
        let v = Verdict {
            mode: "correction".into(),
            confidence: 1.0,
            judgment: s.follows.clone(),
            turn: t.tc.turn_id.to_string(),
            effort: None,
            persona: None,
            model: None,
            work: None,
            split: None,
        };
        let a = routing::Ask {
            verdict: &v,
            base: &t.target.profile,
            context_tokens: 0,
            hold: hold.as_ref(),
            images: t.route.images,
        };
        match &s.steer {
            Steer::Profile(p) => routing::steer_to(&self.cfg.routing, &profiles, &a, p, true),
            Steer::Persona(_) => {
                let mut d = routing::decide(&self.cfg.routing, &profiles, &a);
                if d.reason == Reason::Verdict {
                    d.reason = Reason::Correction;
                }
                d
            }
        }
    }

    /// The turn routing decided, kept for a correction to find: its own
    /// verdict's judgment (a late one of the message before is not this
    /// message's), and where it ran. The owner's correction itself is not
    /// kept: a correction after it corrects the turn it corrected.
    pub(super) fn note_routed(
        &self,
        t: &mut Turn<'_>,
        verdict: Option<&Verdict>,
        decision: Option<&Decision>,
    ) {
        if t.route.correcting {
            return;
        }
        let own = verdict.filter(|v| v.turn == t.tc.turn_id);
        self.judge.corrections.noted(RoutedTurn {
            session: t.tc.session_id.to_string(),
            turn: t.tc.turn_id.to_string(),
            judgment: own.map(|v| v.judgment.clone()),
            pack: ROUTE_PACK.to_string(),
            mode: own.map(|v| v.mode.clone()).filter(|m| !m.is_empty()),
            from: t.target.profile.clone(),
            profile: decision.map_or_else(|| t.target.profile.clone(), |d| d.profile.clone()),
            persona_asked: own.is_some_and(|v| v.persona.is_some()),
        });
    }
}
