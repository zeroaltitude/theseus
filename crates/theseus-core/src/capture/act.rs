//! Capture's writes: a verdict's plan carried out (theseus-slvl.5). One frame
//! each: the record (if any), its rows, the state, and the card posted to the
//! owner's DM (if the words came from Discord), so a crash loses all of it or
//! none. Off the turn's path: the inbound point's task calls it once the
//! batch's judgments are in, and it reads the store on the blocking pool.

use anyhow::{bail, Result};
use serde_json::json;
use theseus_judge::Judgment;
use theseus_protocol::{StandingAsk, StandingKind, StandingRecord, StandingScope};

use super::state::{Candidate, Kept, Open, State};
use super::{
    ask_line, decide, kept_line, owner_alone, scope_of, surface_name, words, Plan, Verdict,
};
use crate::fact;
use crate::rpc::Core;
use crate::standing::{self, Scope};

/// The message a verdict is about.
#[derive(Debug, Clone)]
pub struct Message {
    pub session: String,
    pub node: String,
    pub text: String,
    /// The inbound state's `place_kind` (`cli (private)`).
    pub place_kind: String,
}

/// What a message came to, for the tests and the log.
#[derive(Debug, Clone, PartialEq)]
pub enum Done {
    Nothing,
    Kept(String),
    Asked(String),
    Noted,
    Repeated(String),
}

/// An id short enough to type, as a standing record's is.
pub(super) fn short(prefix: &str) -> String {
    let u = crate::new_id(prefix);
    format!("{prefix}_{}", &u[u.len() - 8..])
}

/// What one message needs to be acted on.
struct Ctx<'a> {
    m: &'a Message,
    v: &'a Verdict,
    now: u64,
    line: String,
    place: Option<String>,
    surface: &'static str,
    /// Where a record or an ask from this message holds.
    at: StandingScope,
    all: &'a [StandingRecord],
    live: &'a [&'a StandingRecord],
}

impl Core {
    /// `directive.v1`'s judgment of a message, acted on. A judgment that is
    /// not live, or that names nothing, does nothing. A failure is logged and
    /// changes no turn. Blocking: it runs in the inbound point's task, off
    /// every turn's path.
    pub fn capture_directive(&self, m: &Message, j: &Judgment) {
        let Some(v) = Verdict::of(j) else { return };
        let done = theseus_store::blocking(|| self.capture_now(m, &v));
        match done {
            Ok(Done::Nothing) => {}
            Ok(d) => tracing::debug!(?d, node = %m.node, "capture: a directive was heard"),
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "capture: nothing was written"),
        }
    }

    /// The plan for `v`, carried out. Blocking: call it from a blocking
    /// context.
    pub fn capture_now(&self, m: &Message, v: &Verdict) -> Result<Done> {
        if !self.cfg.standing.capture {
            return Ok(Done::Nothing);
        }
        let place = self.runner.target_of(&m.session);
        // The owner's words only: the belt for the braces at the inbound
        // point, which does not ask the pack elsewhere.
        if !owner_alone(self.runner.class_of(&m.session), place.as_deref()) {
            return Ok(Done::Nothing);
        }
        let Some(line) = words::line(&m.text) else {
            return Ok(Done::Nothing);
        };
        let now = theseus_protocol::now_unix_ms();
        let all = self.runner.standing.all(&self.store)?;
        let scope = Scope {
            session: &m.session,
            place: place.as_deref(),
        };
        let live = standing::live(&all, Some(scope), now);
        let mut held = self.runner.standing.capture.lock(&self.store)?;
        let mut next = (*held).clone();
        next.trim(now);
        let plan = decide(v, &line, &m.node, &live, &next, now);
        let c = Ctx {
            m,
            v,
            now,
            at: scope_of(v, &m.session, place.as_deref()),
            surface: surface_name(&m.place_kind),
            line,
            place,
            all: &all,
            live: &live,
        };
        let (done, frame) = match plan {
            Plan::Nothing => return Ok(Done::Nothing),
            Plan::Candidate => Self::note_correction(&c, &mut next)?,
            Plan::Repeat(record) => Self::note_repeat(&c, &mut next, record)?,
            Plan::Keep(kind) => self.keep(&c, &mut next, kind)?,
            Plan::Ask { kind, why } => self.ask(&c, &mut next, kind, why)?,
        };
        let mut rows = frame.rows;
        rows.push(next.row()?);
        if let Some((_, staged)) = &frame.post {
            rows.extend(staged.iter().cloned());
        }
        let records = frame.records;
        self.runner
            .standing
            .write(&self.store, |_| Ok(records), |_, _| Ok(rows))?;
        held.set(next);
        drop(held);
        if let Some((action, _)) = &frame.post {
            self.outbox.posted(action);
        }
        self.say(&m.session, frame.say);
        Ok(done)
    }

    /// A correction below the band, noted to wait for a second.
    fn note_correction(c: &Ctx<'_>, next: &mut State) -> Result<(Done, Frame)> {
        next.candidates.push(Candidate {
            node: c.m.node.clone(),
            line: c.line.clone(),
            at_ms: c.now,
        });
        let f = fact::capture::StandingCandidate {
            node: &c.m.node,
            judgment: &c.v.judgment,
            strength: c.v.p_correction.max(match c.v.heard {
                super::Heard::Correction => c.v.confidence,
                _ => 0.0,
            }),
        };
        let row = fact::row(&f, Some(&c.m.session), None)?;
        Ok((Done::Noted, Frame::rows(vec![row])))
    }

    /// A live record said again: counted, no record.
    fn note_repeat(c: &Ctx<'_>, next: &mut State, record: String) -> Result<(Done, Frame)> {
        next.repeats.entry(record.clone()).or_default().push(c.now);
        let week = next.week_of(&record, c.now);
        let f = fact::capture::StandingRepeated {
            record: &record,
            node: &c.m.node,
            judgment: &c.v.judgment,
            week,
        };
        let row = fact::row(&f, Some(&c.m.session), None)?;
        let say = Say::Repeated(record.clone(), c.m.node.clone(), c.v.judgment.clone(), week);
        Ok((
            Done::Repeated(record),
            Frame {
                say,
                ..Frame::rows(vec![row])
            },
        ))
    }

    /// A record kept, with its notice.
    fn keep(&self, c: &Ctx<'_>, next: &mut State, kind: StandingKind) -> Result<(Done, Frame)> {
        refuse_if_full(c.live, &c.at)?;
        let (m, v) = (c.m, c.v);
        let rec = StandingRecord {
            id: standing::mint(c.all),
            kind,
            text: c.line.clone(),
            scope: c.at.clone(),
            since_ms: c.now,
            until_ms: None,
            source: format!("capture:{}", m.node),
            ends: None,
        };
        next.kept.push(Kept {
            record: rec.id.clone(),
            judgment: v.judgment.clone(),
            at_ms: c.now,
        });
        let notice = kept_line(kind, &c.at, c.surface, &c.line);
        let added = fact::standing::StandingAdded {
            record: &rec,
            who: "capture",
            via: c.surface,
        };
        let captured = fact::capture::StandingCaptured {
            record: &rec.id,
            kind,
            scope: &c.at,
            node: &m.node,
            judgment: &v.judgment,
            pack: &v.pack,
            confidence: v.confidence,
            line: &notice,
        };
        let mut captured_row = fact::row(&captured, Some(&m.session), None)?;
        captured_row.key = Some(super::captured_key(&v.judgment));
        let rows = vec![fact::row(&added, Some(&m.session), None)?, captured_row];
        let body = json!({"kind": "standing_card", "text": notice,
            "card": {"mode": "kept", "id": rec.id}});
        let post = self.card_for(&m.session, c.place.as_deref(), body)?;
        let done = Done::Kept(rec.id.clone());
        let say = Say::Kept(rec.clone(), v.judgment.clone(), v.confidence, notice);
        let records = vec![rec];
        Ok((
            done,
            Frame {
                records,
                rows,
                post,
                say,
            },
        ))
    }

    /// An ask for the owner.
    fn ask(
        &self,
        c: &Ctx<'_>,
        next: &mut State,
        kind: StandingKind,
        why: &'static str,
    ) -> Result<(Done, Frame)> {
        let (m, v) = (c.m, c.v);
        let ask = StandingAsk {
            id: short("ask"),
            kind,
            text: c.line.clone(),
            scope: c.at.clone(),
            why: why.into(),
            node: m.node.clone(),
            at_ms: c.now,
        };
        let question = ask_line(kind, &c.at, why, &c.line);
        let f = fact::capture::StandingAsked {
            ask: &ask.id,
            kind,
            scope: &c.at,
            why,
            node: &m.node,
            judgment: &v.judgment,
            confidence: v.confidence,
            line: &question,
        };
        let row = fact::row(&f, Some(&m.session), None)?;
        let body = json!({"kind": "standing_card", "text": question,
            "card": {"mode": "ask", "id": ask.id, "everywhere": c.at != StandingScope::Everywhere}});
        let post = self.card_for(&m.session, c.place.as_deref(), body)?;
        // Matching candidates are spent by the proposal they made.
        next.candidates.retain(|k| !words::same(&k.line, &c.line));
        next.asks.push(Open {
            ask: ask.clone(),
            judgment: v.judgment.clone(),
            session: m.session.clone(),
        });
        let done = Done::Asked(ask.id.clone());
        let say = Say::Asked(ask, v.judgment.clone(), v.confidence, question);
        Ok((
            done,
            Frame {
                post,
                say,
                ..Frame::rows(vec![row])
            },
        ))
    }

    /// A card for the owner's DM, staged for this frame, when the words came
    /// from Discord; the CLI and the web UI hear the narrative line instead.
    fn card_for(
        &self,
        session: &str,
        place: Option<&str>,
        body: serde_json::Value,
    ) -> Result<Option<(theseus_kernel::Action, Vec<theseus_store::NewRecord>)>> {
        if !place.is_some_and(|p| p.starts_with("discord:dm:")) {
            return Ok(None);
        }
        Ok(Some(self.outbox.stage_to_operator(Some(session), body)?))
    }

    /// The sentences of a frame that was written.
    fn say(&self, session: &str, say: Say) {
        let rec = self.rec(Some(session));
        match say {
            Say::None => {}
            Say::Kept(r, judgment, confidence, line) => {
                rec.announce(&fact::capture::StandingCaptured {
                    record: &r.id,
                    kind: r.kind,
                    scope: &r.scope,
                    node: r.source.strip_prefix("capture:").unwrap_or(&r.source),
                    judgment: &judgment,
                    pack: "",
                    confidence,
                    line: &line,
                })
            }
            Say::Asked(a, judgment, confidence, line) => {
                rec.announce(&fact::capture::StandingAsked {
                    ask: &a.id,
                    kind: a.kind,
                    scope: &a.scope,
                    why: &a.why,
                    node: &a.node,
                    judgment: &judgment,
                    confidence,
                    line: &line,
                })
            }
            Say::Repeated(record, node, judgment, week) => {
                rec.announce(&fact::capture::StandingRepeated {
                    record: &record,
                    node: &node,
                    judgment: &judgment,
                    week,
                })
            }
        }
    }
}

/// One plan's frame, before it is written.
struct Frame {
    records: Vec<StandingRecord>,
    rows: Vec<theseus_store::NewRecord>,
    post: Option<(theseus_kernel::Action, Vec<theseus_store::NewRecord>)>,
    say: Say,
}

impl Frame {
    /// A frame of rows alone: no record, no card, nothing to say.
    fn rows(rows: Vec<theseus_store::NewRecord>) -> Frame {
        Frame {
            records: vec![],
            rows,
            post: None,
            say: Say::None,
        }
    }
}

/// What to say once the frame is written.
enum Say {
    None,
    Kept(StandingRecord, String, f64, String),
    Asked(StandingAsk, String, f64, String),
    Repeated(String, String, String, usize),
}

/// A scope already holding [`standing::MAX_LIVE`] records takes no more.
pub(crate) fn refuse_if_full(live: &[&StandingRecord], scope: &StandingScope) -> Result<()> {
    let held = live.iter().filter(|r| &r.scope == scope).count();
    if held >= standing::MAX_LIVE {
        bail!("{held} records already hold {}", scope.words());
    }
    Ok(())
}
