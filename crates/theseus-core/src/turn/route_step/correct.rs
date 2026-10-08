//! The owner's correction at the turn (theseus-q31l; `crate::correction`).
//!
//! - **Words**, read by `turn.submit` and taken here, at the inbound point,
//!   once the message's node is written (its id is the provenance): only in
//!   a private place, and never in a task's turn. The label and the
//!   `route.corrected` row ride the turn's next frame, so a correction adds
//!   no frame of its own; the turn itself runs where the owner said.
//! - **A reaction, a press, the CLI** (`route.correct`): written in their own
//!   frame by the core, and the session's next turn runs where they said.
//! - **The layer**: any other message routing acts on is looked up, in
//!   memory, among the corrected ones; a close one runs where the owner said.

use anyhow::Context as _;
use serde_json::json;
use theseus_protocol::PlaceClass;
use theseus_store::NewRecord;

use super::*;
use crate::correction::{self, Resolved, To, QUESTION};

/// One correction, resolved and checked, before it is written.
#[derive(Debug, Clone)]
pub struct Correcting {
    pub id: String,
    pub label_id: Option<String>,
    /// The label as stored (`{"not": "chat"}`), checked against the pack.
    pub label: Option<serde_json::Value>,
    pub last: RoutedTurn,
    pub resolved: Resolved,
    pub rerun: bool,
    pub who: String,
    pub via: String,
    pub provenance: String,
    pub at_ms: u64,
}

impl Correcting {
    /// Where it steers the session's next turn, or this one.
    pub fn steering(&self, by: By) -> Option<Steering> {
        Some(Steering {
            steer: self.resolved.steer.clone()?,
            follows: self.label_id.clone().unwrap_or_else(|| self.id.clone()),
            by,
        })
    }

    /// Its entry in the layer, when it steers and its message had words.
    pub fn entry(&self) -> Option<correction::Entry> {
        let steer = self.resolved.steer.clone()?;
        (!self.last.words.is_empty()).then(|| correction::Entry {
            id: self.id.clone(),
            label: self.label_id.clone(),
            pack: self.last.pack.clone(),
            session: self.last.session.clone(),
            turn: self.last.turn.clone(),
            words: self.last.words.clone(),
            steer,
            at_ms: self.at_ms,
        })
    }

    fn note(&self) -> String {
        let again = if self.rerun {
            ", and answer it again there"
        } else {
            ""
        };
        format!(
            "the owner's correction {}: it should have run on {}{again} ({} {})",
            self.id, self.resolved.to, self.via, self.provenance
        )
    }

    fn label_fact<'a>(&'a self, note: &'a str) -> Option<fact::judge::JudgeLabel<'a>> {
        Some(fact::judge::JudgeLabel {
            id: self.label_id.as_deref()?,
            judgment: self.last.judgment.as_deref()?,
            pack: &self.last.pack,
            question: Some(QUESTION),
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
            rerun: self.rerun,
            who: &self.who,
            via: &self.via,
            provenance: &self.provenance,
            words: &self.last.words,
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
        rerun: bool,
        who: String,
        via: &str,
        provenance: String,
    ) -> anyhow::Result<Correcting> {
        let resolved = correction::resolve(to, &last, &self.cfg.routing, &self.route_profiles());
        let label = match (&resolved.label, &last.judgment) {
            (Some(words), Some(_)) => {
                let pack = self
                    .judge
                    .pack(&last.pack)
                    .with_context(|| format!("this build has no pack {}", last.pack))?;
                let l = crate::learning::labels::check(&pack, Some(QUESTION), &json!(words))
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
            rerun,
            who,
            via: via.to_string(),
            provenance,
            at_ms: theseus_protocol::now_unix_ms(),
        })
    }

    /// A correction written: its entry into the layer, and its label to the
    /// ladder's count of the day's labels, off the turn's path.
    pub fn corrected(&self, c: &Correcting) {
        if let Some(e) = c.entry() {
            self.judge.corrections.add(e, &self.cfg.routing.corrections);
        }
        let Some(label) = c.label.clone() else {
            return;
        };
        let (judge, pack) = (self.judge.clone(), c.last.pack.clone());
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => drop(rt.spawn_blocking(move || judge.land_label(&pack, &label))),
            Err(_) => judge.land_label(&pack, &label),
        }
    }

    /// The inbound point's part: the owner's words, read by `turn.submit`,
    /// taken and acted on in a private place; else, for a turn that asked the
    /// route pack (`asked`), a correction waiting for this session's next
    /// turn; else, while routing acts, the layer's entry closest to the
    /// message.
    pub(in crate::turn) fn correction_point(
        &self,
        t: &mut Turn<'_>,
        node_id: &str,
        text: &str,
        author: &str,
        route: PackMode,
        asked: bool,
    ) {
        let sid = t.tc.session_id;
        let cfg = &self.cfg.routing.corrections;
        let corrections = &self.judge.corrections;
        t.route.words = correction::layer::words(text);
        let said = corrections.take_said(sid, text);
        let owners = t.tc.class == PlaceClass::Private && t.tc.task.is_none();
        if let Some(said) = said.filter(|_| owners && cfg.enabled) {
            if self.correct_in_words(t, &said, node_id, author) {
                return;
            }
            return self.layer_point(t, route, asked);
        }
        if !asked {
            return;
        }
        if let Some(s) = corrections.take_pending(sid) {
            t.route.steer = Some(s);
            return;
        }
        self.layer_point(t, route, asked)
    }

    /// The owner's words, acted on: the label and the row ride the turn's
    /// next frame, and the turn runs where the owner said. False when there
    /// is no routed turn to correct, or the correction does not resolve: the
    /// words are a message.
    fn correct_in_words(
        &self,
        t: &mut Turn<'_>,
        said: &correction::Said,
        node_id: &str,
        author: &str,
    ) -> bool {
        let sid = t.tc.session_id;
        let Some(last) = self.judge.corrections.last_of(sid) else {
            tracing::debug!(session_id = %sid, "a correction with no routed turn to correct: a message");
            return false;
        };
        let who = author.to_string();
        let c = match self.correcting(last, &said.to, said.rerun, who, "message", node_id.into()) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "the owner's correction did not resolve: a message");
                return false;
            }
        };
        Self::defer_rows(t, &c);
        c.announce(&t.tc.rec());
        self.corrected(&c);
        t.route.correcting = true;
        t.route.steer = c.steering(By::Owner);
        true
    }

    /// A correction's rows, into the turn's next frame.
    fn defer_rows(t: &Turn<'_>, c: &Correcting) {
        let written = c
            .records()
            .and_then(|rows| rows.into_iter().try_for_each(|r| t.tc.store.defer(r)));
        if let Err(e) = written {
            tracing::warn!(error = %format!("{e:#}"), "a correction's rows were not written");
        }
    }

    /// The layer's entry closest to the message, while routing acts.
    fn layer_point(&self, t: &mut Turn<'_>, route: PackMode, asked: bool) {
        if !asked || route < PackMode::Canary {
            return;
        }
        let cfg = &self.cfg.routing.corrections;
        if let Some((e, _)) = self
            .judge
            .corrections
            .nearest(&t.route.words, ROUTE_PACK, cfg)
        {
            t.route.steer = Some(Steering {
                steer: e.steer,
                follows: e.label.unwrap_or(e.id),
                by: By::Layer,
            });
        }
    }

    /// Where a steered turn runs: a profile by `routing::steer_to`, a mode
    /// as a verdict of that mode at full confidence would place it.
    pub(super) fn decide_steer(
        &self,
        t: &Turn<'_>,
        session: &SessionRecord,
        s: &Steering,
        est_tokens: u64,
    ) -> Decision {
        let profiles = self.route_profiles();
        let cap =
            t.tc.ceiling
                .and_then(|c| c.profile.as_ref())
                .and_then(|p| profiles.get(p))
                .and_then(|p| p.short_cost);
        let hold = session.routed.as_ref().and_then(|r| r.hold.clone());
        let at_once = s.by == By::Owner;
        let v = Verdict {
            mode: match &s.steer {
                Steer::Mode(m) => m.clone(),
                Steer::Profile(_) => "correction".into(),
            },
            confidence: 1.0,
            judgment: s.follows.clone(),
            turn: t.tc.turn_id.to_string(),
        };
        let a = routing::Ask {
            verdict: &v,
            base: &t.target.profile,
            est_tokens: if at_once { 0 } else { est_tokens },
            hold: hold.as_ref(),
            images: t.route.images,
            cap,
        };
        match &s.steer {
            Steer::Profile(p) => routing::steer_to(&self.cfg.routing, &profiles, &a, p, at_once),
            Steer::Mode(_) => {
                let mut d = routing::decide(&self.cfg.routing, &profiles, &a);
                if matches!(d.reason, Reason::Verdict | Reason::Detour) {
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
            mode: own.map(|v| v.mode.clone()),
            from: t.target.profile.clone(),
            profile: decision.map_or_else(|| t.target.profile.clone(), |d| d.profile.clone()),
            words: std::mem::take(&mut t.route.words),
        });
    }
}
