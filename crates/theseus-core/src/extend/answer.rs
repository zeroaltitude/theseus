//! The operator's answer to a proposed extension (M7 43a, design §2.7 step
//! 5): `action.confirm` on its `extend.ack` question, judged by the place
//! rule before it gets here (`Core::confirm_action_with`'s `judge_act`). An
//! ack binds the question's confirm and writes `extend.acked`; a decline
//! settles it declined and writes `extend.declined`; so does a question
//! nobody answered in time. Each is one frame: the question, the manifest's
//! new state, and the rows. None wakes the proposing execution, whose turn
//! went on long ago, and nothing loads in this step.

use anyhow::Result;
use theseus_kernel::Action;

use super::{manifest_key, Manifest};
use crate::fact::answer::CallAnswered;
use crate::fact::extend::{ExtendAcked, ExtendDeclined};
use crate::outbox::Closed;
use crate::rpc::Core;
use crate::turn::OPERATOR;

/// How a question ended.
pub(crate) enum Answer<'a> {
    Ack,
    Decline(Option<&'a str>),
    /// Nobody answered by its time.
    Expired(&'a str),
}

impl Core {
    /// The answer to `a`, an `extend.ack` question still waiting, by `by`
    /// through `via`.
    pub(crate) fn answer_extension(
        &self,
        a: &Action,
        answer: Answer<'_>,
        by: &str,
        via: &str,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let args = a
            .proposal
            .as_ref()
            .map(|p| p.args.clone())
            .unwrap_or_default();
        let name = args["name"].as_str().unwrap_or_default().to_string();
        let digest = args["digest"].as_str().unwrap_or_default().to_string();
        let mut m: Option<Manifest> = self.store.get_meta(&manifest_key(&name, &digest))?;
        let approve = matches!(answer, Answer::Ack);
        let note = match answer {
            Answer::Decline(n) => n,
            Answer::Expired(why) => Some(why),
            Answer::Ack => None,
        };
        if let Some(m) = &mut m {
            m.state = if approve { "acked" } else { "declined" }.into();
            m.answered_by = Some(by.into());
            m.answered_at_ms = Some(self.kernel.now_ms());
            m.note = note.map(str::to_string);
        }
        let session = Some(a.session_id.as_str());
        let corr = a.correlation_id.as_str();
        let answered = CallAnswered {
            action: a,
            approve,
            note,
            by,
            via,
            trust: false,
        };
        let acked = ExtendAcked {
            name: &name,
            digest: &digest,
            correlation_id: corr,
            by,
            via,
        };
        let declined = ExtendDeclined {
            name: &name,
            digest: &digest,
            correlation_id: corr,
            by,
            via,
            note,
        };
        let mut rows = vec![crate::fact::row(&answered, session, None)?];
        rows.push(match approve {
            true => crate::fact::row(&acked, session, None)?,
            false => crate::fact::row(&declined, session, None)?,
        });
        if let Some(m) = &m {
            rows.push(m.record()?);
        }
        let proposal = a.proposal.clone().unwrap_or_default();
        self.kernel.frame(&[&a.execution_id], |k| {
            match answer {
                Answer::Ack => {
                    k.bind_confirm(corr, OPERATOR, &proposal)?;
                }
                Answer::Decline(n) => {
                    k.decline_action(corr, by, n.unwrap_or("the operator declined"))?;
                }
                Answer::Expired(why) => {
                    k.decline_action(corr, by, why)?;
                }
            }
            k.stage(&rows)
        })?;
        let rec = self.session_rec(&a.session_id);
        rec.announce(&answered);
        match approve {
            true => rec.announce(&acked),
            false => rec.announce(&declined),
        }
        let closed = match answer {
            Answer::Ack => Closed::new("approved", Some(by)),
            Answer::Decline(_) => Closed::new("declined", Some(by)),
            Answer::Expired(why) => Closed {
                note: Some(why.to_string()),
                ..Closed::new("expired", None)
            },
        };
        self.card_closed(corr, closed);
        Ok(theseus_protocol::ActionConfirmResult {
            correlation_id: corr.into(),
            approved: approve,
            session_id: a.session_id.clone(),
            execution_id: a.execution_id.clone(),
            resumes: false,
        })
    }
}
