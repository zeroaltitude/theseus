//! The owner's verdict on what Theseus said first (theseus-slvl.9):
//! `initiative.rate`, from a 👍 or 👎 reaction on Discord or `theseus
//! notices rate`. One `initiative.rated` row names the message's id (a
//! turn's id for a wake's or a report's reply, a voiced note's own), judged
//! as `route.correct` is: the owner, from a private place. A message that was
//! not sent first is no one to rate, and says so.

use anyhow::{bail, Result};
use theseus_protocol::initiative::{InitiativeRateParams, InitiativeRateResult};
use theseus_protocol::LedgerKind;

use crate::approval::Answerer;
use crate::fact::initiative::InitiativeRated;
use crate::rpc::{Act, Core};

const DAY_MS: u64 = 86_400_000;
/// How far back a verdict finds its message.
const WINDOW_DAYS: u64 = 30;

impl Core {
    pub fn initiative_rate(
        &self,
        p: &InitiativeRateParams,
        who: impl Into<Answerer>,
    ) -> Result<InitiativeRateResult> {
        let who = who.into();
        let up = match p.rating.as_str() {
            "up" => true,
            "down" => false,
            other => bail!("{other:?} is no verdict: say `up` or `down`"),
        };
        let since = theseus_protocol::now_unix_ms().saturating_sub(WINDOW_DAYS * DAY_MS);
        let spoke = self.synthesis_rows(LedgerKind::InitiativeSpoke, Some(since))?;
        // The message the id names, else the latest one (the session's when
        // it is named).
        let found = spoke
            .iter()
            .rev()
            .find(|r| match (&p.turn_id, &p.session_id) {
                (Some(t), _) => r.data["id"].as_str() == Some(t),
                (None, Some(s)) => r.session_id.as_deref() == Some(s),
                (None, None) => true,
            });
        let Some(found) = found else {
            match &p.turn_id {
                Some(t) => bail!(
                    "{t} is not a message Theseus sent first (a wake's or a task's reply, or a \
                     note it wrote itself): nothing to rate"
                ),
                None => bail!(
                    "Theseus has sent nothing first in the last {WINDOW_DAYS} days: nothing to rate"
                ),
            }
        };
        let id = found.data["id"].as_str().unwrap_or_default().to_string();
        // A turn's reply belongs to its session: a verdict from another one is not its.
        if let (Some(said), Some(from)) = (&found.session_id, &p.session_id) {
            if said != from {
                bail!("{id} was said in another session: rate it from there");
            }
        }
        let what = format!("{} on {id}", p.rating);
        self.judge_act(
            &who,
            Act::Session {
                method: theseus_protocol::method::INITIATIVE_RATE,
                what: &what,
            },
        )?;
        let via = p.via.clone().unwrap_or_else(|| who.via());
        let rated = InitiativeRated {
            id: &id,
            rating: &p.rating,
            via: &via,
            by: &who.who(),
        };
        self.store.append(&[crate::fact::row(
            &rated,
            found.session_id.as_deref(),
            found.turn_id.as_deref(),
        )?])?;
        Ok(InitiativeRateResult {
            id: id.clone(),
            turn_id: id,
            rating: if up { "up" } else { "down" }.into(),
        })
    }
}
