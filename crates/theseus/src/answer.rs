//! `theseus answer ID OPTION [--note WORDS]` (theseus-ds5os): one verb for
//! every question that waits for the owner. ID is the question's id, whole or
//! a unique end of at least four characters (the daemon resolves it, and an
//! end that names two questions is refused, naming both); OPTION is one of
//! its answers (`approve`, `approve_trust`, `decline`, `reset`, `accept`,
//! `ack`, a reminder's `done`, or, for a question with options (`owner_ask`), its number from
//! 1: `theseus answer <id> 2`, or `other --note <words>` for none of them). A reminder that
//! fired is snoozed with `theseus answer <id> snooze [WHEN]`, the one spelling (`wake snooze`
//! moves a pending wake). `theseus confirm` still lists what waits and answers a call with its
//! flags and follows the turn it resumes; both reach the same handlers.

use anyhow::Result;
use theseus_client::Conn;
use theseus_protocol::answer::{WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::method;

use crate::cmd::output;

/// `theseus answer`'s arguments.
#[derive(clap::Args, Debug)]
pub struct AnswerArgs {
    /// The question: its id, or a unique end of it (four characters at least).
    pub question: String,
    /// The answer: `approve`, `approve_trust`, `decline`, `reset`, `accept`, `ack`, a
    /// reminder's `done` or `snooze`, or a question with options' number (from 1; `other --note
    /// <words>` for none of them). A wrong one is refused with the question's own.
    pub option: String,
    /// With `snooze`: when (`10m`, `1h`, `tomorrow 9:00`, `fri 9:00`; default 10 minutes).
    pub when: Option<String>,
    /// Words with the answer; the model reads a decline's.
    #[arg(long)]
    pub note: Option<String>,
}

/// What a retired snooze option now says: `snooze_1h` is `snooze 1h`.
fn old_snooze(option: &str) -> Option<&'static str> {
    match option {
        "snooze_10m" => Some("10m"),
        "snooze_1h" => Some("1h"),
        "tomorrow_9" => Some("tomorrow 9:00"),
        _ => None,
    }
}

/// `theseus answer`: one `work.answer`, or a snooze.
pub async fn answer(conn: &mut Conn, json: bool, a: AnswerArgs) -> Result<()> {
    if let Some(spelled) = old_snooze(&a.option) {
        anyhow::bail!(
            "`{}` is spelled `theseus answer {} snooze {spelled}`: one spelling for a reminder's snooze",
            a.option,
            a.question
        );
    }
    if a.option == "snooze" {
        return crate::remind::snooze(conn, json, a.question, a.when).await;
    }
    if a.when.is_some() {
        anyhow::bail!("a time goes with `snooze` alone: `theseus answer ID snooze WHEN`");
    }
    let v = conn
        .request(
            method::WORK_ANSWER,
            WorkAnswerParams {
                question_id: a.question,
                option: a.option,
                note: a.note,
                ..Default::default()
            },
        )
        .await?;
    if let Some(sid) = v["resumes"]["session_id"].as_str() {
        theseus_client::shown::shown(conn, &[sid.to_string()]).await;
    }
    output(json, v, |r: WorkAnswerResult| {
        match r.resumes {
            Some(w) => println!(
                "answered {} · {} · {} resumes",
                r.question_id,
                r.option,
                w.session_id.unwrap_or(w.id)
            ),
            None => println!("answered {} · {}", r.question_id, r.option),
        }
        Ok(())
    })
}
