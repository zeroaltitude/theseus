//! Credential requests on Discord (M4 18d; design `m4-boundaries` §2.11):
//! a request an L1 job made that was granted at notify posts its notice in
//! the job's place, "🔑 job a1b2c3 (`cargo publish`, L1) asked for
//! `crates_io_token`: granted". One granted at open is quiet, as an open call
//! is; one that waits is a card, which the outbox posts; one refused at once
//! (a name the broker may not hand out) is in the ledger and the narrative.

use theseus_protocol::cred::asked;
use theseus_protocol::SecretRequested;

use crate::render::{NoticeCard, Op, AMBER};

/// What a `secret.requested` posts: its notice, for a grant at notify.
pub fn ops(r: &SecretRequested, embeds: bool) -> Vec<Op> {
    if !embeds || r.outcome != "granted" || r.posture != "notify" {
        return vec![];
    }
    vec![Op::Notice {
        key: format!("cred:{}", r.correlation_id),
        card: notice(r),
    }]
}

/// The notice: who asked for what, and the setting that let it go.
pub fn notice(r: &SecretRequested) -> NoticeCard {
    NoticeCard {
        title: "🔑 A job asked for a secret".into(),
        color: AMBER,
        description: format!("{}: granted", asked(&r.short, &r.command, &r.secret)),
        fields: vec![("Posture".into(), format!("`notify` ({})", r.setting))],
        ask: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grant at notify posts its notice; one at open, one that waits, and
    /// one refused post nothing here.
    #[test]
    fn a_notify_grant_posts_its_notice_and_nothing_else_does() {
        let r = SecretRequested {
            correlation_id: "act_req".into(),
            short: "a1b2c3".into(),
            command: "cargo publish".into(),
            secret: "crates_io_token".into(),
            posture: "notify".into(),
            setting: "proc.run ran at notify".into(),
            outcome: "granted".into(),
            ..Default::default()
        };
        let posted = ops(&r, true);
        let [Op::Notice { key, card }] = posted.as_slice() else {
            panic!("{posted:?}")
        };
        assert_eq!(key, "cred:act_req");
        assert_eq!(
            card.description,
            "job a1b2c3 (`cargo publish`, L1) asked for `crates_io_token`: granted"
        );
        for quiet in [
            SecretRequested {
                posture: "open".into(),
                ..r.clone()
            },
            SecretRequested {
                posture: "approve".into(),
                outcome: "waiting".into(),
                ..r.clone()
            },
            SecretRequested {
                outcome: "declined".into(),
                ..r.clone()
            },
        ] {
            assert!(ops(&quiet, true).is_empty(), "{quiet:?}");
        }
        assert!(ops(&r, false).is_empty(), "no embeds, no notice");
    }
}
