//! `theseus self revert <merge>` (theseus-pw1q.10): the owner's veto of a
//! self-join. It sends the shell's `THESEUS_SESSION` as `from_job`, so the
//! daemon refuses a job's veto and ledgers the refusal; it waits for the
//! whole revert (main's gate runs on it), which can take minutes.

use anyhow::Result;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::rsi::{SelfRevertParams, SelfRevertResult};

use crate::cmd::output;

pub async fn run(conn: &mut Conn, json: bool, merge: String) -> Result<()> {
    let p = SelfRevertParams {
        merge,
        from_job: theseus_client::client::job_session(),
        ..Default::default()
    };
    let v = conn
        .request(method::SELF_REVERT, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: SelfRevertResult| {
        println!("{}", lines(&r));
        Ok(())
    })
}

/// The answer in words.
pub fn lines(r: &SelfRevertResult) -> String {
    let short = |c: &str| c.chars().take(12).collect::<String>();
    match r.changed {
        true => format!(
            "Reverted self-join {} as {}, signed and pushed to main.{}",
            short(&r.merge),
            short(&r.revert),
            r.item
                .as_deref()
                .map(|i| format!(" Its backlog item {i} is dropped and never filed again."))
                .unwrap_or_default()
        ),
        false => format!(
            "Self-join {} was reverted already, as {}.",
            short(&r.merge),
            short(&r.revert)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_says_the_revert_or_that_it_was_there() {
        let mut r = SelfRevertResult {
            merge: "aaaaaaaaaaaaaaaaaaaa".into(),
            revert: "bbbbbbbbbbbbbbbbbbbb".into(),
            item: Some("tsk_1".into()),
            changed: true,
        };
        assert_eq!(
            lines(&r),
            "Reverted self-join aaaaaaaaaaaa as bbbbbbbbbbbb, signed and pushed to main. \
             Its backlog item tsk_1 is dropped and never filed again."
        );
        r.changed = false;
        assert_eq!(
            lines(&r),
            "Self-join aaaaaaaaaaaa was reverted already, as bbbbbbbbbbbb."
        );
    }
}
