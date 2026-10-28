//! `theseus answer [ID OPTION [WHEN]] [--note WORDS] [--no-wait]` (theseus-ds5os): one verb for
//! every question that waits for the owner. With no ID it lists everything waiting. ID is the
//! question's id, whole or a unique end of at least four characters, or the session's or the
//! execution's it waits in (the daemon resolves it, and an end that names two questions is
//! refused, naming both); OPTION is one of its answers (`approve`, `decline`,
//! `reset`, `accept`, `ack`, a reminder's `done`, or, for a question with options (`owner_ask`),
//! its number from 1: `theseus answer <id> 2`, or `other --note <words>` for none of them). A
//! reminder that fired is snoozed with `theseus answer <id> snooze [WHEN]`, the one spelling
//! (`wake snooze` moves a pending wake). An answer that wakes a turn follows it to its end, as
//! `ask` does, unless `--no-wait` returns once the answer is recorded.

use anyhow::Result;
use theseus_client::Conn;
use theseus_protocol::answer::{WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::method;

use crate::cmd::output;
use crate::print::{Mode, Printer};

/// `theseus answer`'s arguments.
#[derive(clap::Args, Debug)]
pub struct AnswerArgs {
    /// The question: its id, or a unique end of it (four characters at least), or its session's
    /// or execution's. Without one, list everything waiting.
    #[arg(requires = "option")]
    pub question: Option<String>,
    /// The answer: `approve`, `decline`, `reset`, `accept`, `ack`, a
    /// reminder's `done` or `snooze`, or a question with options' number (from 1; `other --note
    /// <words>` for none of them). A wrong one is refused with the question's own.
    #[arg(requires = "question")]
    pub option: Option<String>,
    /// With `snooze`: when (`10m`, `1h`, `tomorrow 9:00`, `fri 9:00`; default 10 minutes).
    pub when: Option<String>,
    /// Words with the answer; the model reads a decline's.
    #[arg(long)]
    pub note: Option<String>,
    /// Return as soon as the answer is recorded instead of following the turn it resumes.
    #[arg(long)]
    pub no_wait: bool,
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

/// `theseus answer`: what waits, or one `work.answer` and the turn it resumes, or a snooze.
pub async fn answer(conn: &mut Conn, json: bool, a: AnswerArgs) -> Result<()> {
    let (Some(question), Some(option)) = (a.question, a.option) else {
        let mut shown = Vec::new();
        let listed = crate::cmd::confirm_list(conn, json, &mut shown).await;
        if listed.is_ok() {
            theseus_client::shown::shown(conn, &shown).await;
        }
        return listed;
    };
    if let Some(spelled) = old_snooze(&option) {
        anyhow::bail!(
            "`{option}` is spelled `theseus answer {question} snooze {spelled}`: one spelling for a reminder's snooze"
        );
    }
    if option == "snooze" {
        return crate::remind::snooze(conn, json, question, a.when).await;
    }
    if a.when.is_some() {
        anyhow::bail!("a time goes with `snooze` alone: `theseus answer ID snooze WHEN`");
    }
    // Subscribed before the answer wakes the turn, so none of its events are missed.
    let params = WorkAnswerParams {
        question_id: question,
        option,
        note: a.note,
        watch: !a.no_wait,
        ..Default::default()
    };
    let mut printer = Printer::new(if json { Mode::Json } else { Mode::Text }, false);
    let v = conn
        .call(
            method::WORK_ANSWER,
            serde_json::to_value(params)?,
            |m, p| printer.on(m, p),
        )
        .await?;
    let resumed = v["resumes"]["session_id"].as_str().map(str::to_string);
    let follow = resumed.clone().filter(|_| !a.no_wait);
    let done = match &follow {
        Some(sid) => {
            let r: WorkAnswerResult = serde_json::from_value(v)?;
            eprintln!(
                "answered {} · {} · following the resumed turn in {sid}",
                r.question_id, r.option
            );
            crate::cmd::follow_resumed(conn, json, &mut printer, sid).await
        }
        None => output(json, v, say),
    };
    // After the output, and after the turn: the seen file's read must not take its events.
    if let (Ok(()), Some(sid)) = (&done, &resumed) {
        theseus_client::shown::shown(conn, std::slice::from_ref(sid)).await;
    }
    done
}

/// An answer, as a line says it.
fn say(r: WorkAnswerResult) -> Result<()> {
    match r.resumes {
        Some(w) => println!(
            "answered {} · {} · {} resumes",
            r.question_id,
            r.option,
            w.session_id.unwrap_or(w.id)
        ),
        None => println!("answered {} · {}", r.question_id, r.option),
    }
    if let Some(detail) = r.detail {
        println!("{detail}");
    }
    Ok(())
}
