//! What a mail watch does beyond noticing (theseus-batvg.6): the one task a
//! trusted sender's mail may start, and the follow-up at a deadline.
//!
//! - **Stranger text never starts a turn.** A match is a notice. Only when
//!   every matching message's `From` address is in `[google] trusted_senders`,
//!   Gmail authenticated each (`mail_auth`), and the watch said `then: task`, the owner's brief starts a task, with the
//!   mail as its one quote: outside text, so the task's session holds it
//!   before its first turn (its first call that acts asks), and private from
//!   its start. Otherwise the notice says why it is only a notice.
//! - **A follow-up is the owner's.** When a watch with a `follow_up` gives up
//!   at its deadline, a task starts with that brief, not held (the owner wrote
//!   it); it may read the thread and draft, and its `gmail.send` asks like any.
//!   The notice says so in place of "gave up".
//!
//! Either task is started before the frame that writes the news, so the
//! notice can name it; `Entry.started` keeps a retried frame from starting a
//! second one (a crash between the two can).

use theseus_kernel::{ProbeNews, ProbeVerdict};
use theseus_protocol::task_start::TaskStartParams;

use super::mail::Seen;
use super::spec::{Check, Then};
use super::step::Entry;
use crate::approval::{Answerer, Surface};
use crate::rpc::Core;

impl Core {
    /// Whether every sender of a look is on the config's list.
    pub(super) fn mail_listed(&self, m: &Seen) -> bool {
        let list = &self.cfg.google.trusted_senders;
        !m.senders.is_empty()
            && m.senders
                .iter()
                .all(|s| list.iter().any(|t| t.eq_ignore_ascii_case(s)))
    }

    /// Whether a look may start a task: every sender is listed and Gmail
    /// authenticated every message (the `From` alone is the sender's claim).
    /// A look with an unlisted sender keeps no `unverified`: the notice names
    /// that reason, which wins.
    pub(super) fn mail_trust(&self, m: &mut Seen) {
        let listed = self.mail_listed(m);
        m.trusted = listed && m.unverified.is_empty();
        if !listed {
            m.unverified.clear();
        }
    }

    /// Start the task `news` calls for, once, and say so in its text. Returns
    /// the task's short id when one was started (now or by a try before).
    pub(super) fn mail_then(&self, e: &Entry, news: &mut ProbeNews) -> Option<String> {
        let Check::Mail { q } = &e.spec.check else {
            return None;
        };
        let (brief, quote, private) = match &news.verdict {
            ProbeVerdict::Met { .. } => {
                let Then::Task(brief) = &e.spec.then else {
                    return None;
                };
                if news.detail["trusted"] != true {
                    news.text.push_str(if news.detail["unverified"] == true {
                        "\nfrom a trusted address Gmail did not authenticate (no DKIM or DMARC pass): notice only, no task started"
                    } else {
                        "\nfrom a sender not in trusted_senders: notice only, no task started"
                    });
                    return None;
                }
                (brief.clone(), Some(news.text.clone()), true)
            }
            ProbeVerdict::Expired => {
                let brief = e.spec.follow_up.as_deref()?;
                let by = crate::wake::local(e.spec.expires_ms).full();
                let brief = format!(
                    "{brief}\n\n(The watch \"{}\" looked for mail matching `{q}` and found none by {by}.)",
                    e.spec.title
                );
                (brief, None, false)
            }
            _ => return None,
        };
        let title = e.spec.title.trim();
        let started = match &e.started {
            Some(short) => Ok(short.clone()),
            None => self.mail_task(e, &brief, quote, private),
        };
        let expired = matches!(news.verdict, ProbeVerdict::Expired);
        let by = crate::wake::local(e.spec.expires_ms).full();
        match (&started, expired) {
            (Ok(short), true) => {
                news.text =
                    format!("👁 no reply on \"{title}\" by {by}: follow-up task {short} started");
            }
            (Ok(short), false) => news.text.push_str(&format!("\n→ task {short} started")),
            (Err(why), true) => {
                news.text = format!(
                    "👁 no reply on \"{title}\" by {by}: the follow-up task could not start: {why}"
                );
            }
            (Err(why), false) => news
                .text
                .push_str(&format!("\n→ the task could not start: {why}")),
        }
        started.ok()
    }

    /// The task, under the conversation the watch lives in, as the owner's.
    fn mail_task(
        &self,
        e: &Entry,
        brief: &str,
        quote: Option<String>,
        private: bool,
    ) -> Result<String, String> {
        let session = match self.kernel.execution(&e.execution_id) {
            Ok(Some(x)) => x.session_id,
            _ => return Err("its conversation is gone".into()),
        };
        let p = TaskStartParams {
            brief: brief.to_string(),
            title: Some(format!("Mail: {}", e.spec.title.trim())),
            home: Some(session),
            quotes: quote.into_iter().collect(),
            // An email is an outside person's text, whoever set the watch.
            outside_quotes: true,
            ..Default::default()
        };
        // The owner chose this when they set the watch (`Act::Watch`, from a
        // private place), so the task is theirs: the CLI's answer.
        let who = Answerer {
            label: "mail watch".into(),
            surface: Surface::Cli,
            discord: None,
            agent: None,
        };
        self.task_start_as(&p, who, private)
            .map(|r| r.short)
            .map_err(|f| f.message)
    }
}
