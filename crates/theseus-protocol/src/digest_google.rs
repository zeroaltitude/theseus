//! The digest's Google line (theseus-batvg.6): what the owner's Google account
//! did in the window, counted from the daemon's own `google.called` rows, so
//! every surface that prints a digest (the CLI, Discord's `/digest`, the posted
//! digest) says it in the same words with no code of its own.

use serde::{Deserialize, Serialize};

/// Four counts: mail sent, drafts still waiting for a send, mail watches still
/// waiting for a reply, and documents written to Drive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DigestGoogle {
    pub mail_sent: u32,
    pub drafts_waiting: u32,
    pub waiting_on_replies: u32,
    pub docs_written: u32,
}

impl DigestGoogle {
    /// Every count is 0: nothing to say.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// `✉ mail sent 3 · drafts waiting 2 · waiting on 4 replies · docs written 1`,
    /// the counts above 0 alone; none when there is none.
    pub fn line(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.mail_sent > 0 {
            parts.push(format!("mail sent {}", self.mail_sent));
        }
        if self.drafts_waiting > 0 {
            parts.push(format!("drafts waiting {}", self.drafts_waiting));
        }
        if self.waiting_on_replies > 0 {
            let n = self.waiting_on_replies;
            parts.push(format!(
                "waiting on {n} repl{}",
                if n == 1 { "y" } else { "ies" }
            ));
        }
        if self.docs_written > 0 {
            parts.push(format!("docs written {}", self.docs_written));
        }
        (!parts.is_empty()).then(|| format!("✉ {}", parts.join(" · ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{digest_text, WorkDigestResult};

    #[test]
    fn the_line_lists_only_the_counts_above_zero() {
        let all = DigestGoogle {
            mail_sent: 3,
            drafts_waiting: 2,
            waiting_on_replies: 4,
            docs_written: 1,
        };
        assert_eq!(
            all.line().as_deref(),
            Some("✉ mail sent 3 · drafts waiting 2 · waiting on 4 replies · docs written 1")
        );
        let some = DigestGoogle {
            waiting_on_replies: 1,
            docs_written: 2,
            ..Default::default()
        };
        assert_eq!(
            some.line().as_deref(),
            Some("✉ waiting on 1 reply · docs written 2")
        );
        assert_eq!(DigestGoogle::default().line(), None);
        assert!(DigestGoogle::default().is_empty() && !all.is_empty());
    }

    /// The digest's text says it once, under the header, and says nothing of
    /// Google when there is none: the bytes are what they were.
    #[test]
    fn digest_text_carries_the_line_and_without_it_is_unchanged() {
        let hm = |_: u64| "09:00".to_string();
        let mut res = WorkDigestResult {
            from: 0,
            to: 1,
            lines: vec![],
            from_ledger: false,
            now: Default::default(),
            questions: vec![],
            more: None,
            google: None,
        };
        let without = digest_text(&res, 0, &hm, 2000);
        assert_eq!(without, "◆ nothing");
        assert!(!serde_json::to_string(&res).unwrap().contains("google"));
        res.google = Some(DigestGoogle {
            mail_sent: 1,
            ..Default::default()
        });
        assert_eq!(digest_text(&res, 0, &hm, 2000), "◆ nothing\n✉ mail sent 1");
    }
}
