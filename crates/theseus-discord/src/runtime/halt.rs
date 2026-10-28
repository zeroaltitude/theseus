//! "halt self" (theseus-pw1q.2): a message whose first two words are `halt
//! self` (any case; the rest is why) throws self-improvement's kill switch,
//! through `self.halt` as its author with the message's ids, wherever the
//! binding reads it: a halt is anyone's. "resume self" releases it through
//! `self.resume`, the same way, and the core counts it only from an author
//! holding one of the owner's handles (`places::owner_anywhere`, the owner's
//! call of 2026-10-10); anyone else's is refused and ledgered there. The CLI's
//! `theseus self resume` and the web UI are the owner's too.

use theseus_protocol::rsi::{SelfHaltParams, SelfResumeParams, SelfSwitchResult};
use theseus_protocol::DiscordOrigin;
use twilight_model::application::command::Command;
use twilight_model::application::interaction::application_command::{
    CommandDataOption, CommandOptionValue,
};
use twilight_util::builder::command::{BooleanBuilder, CommandBuilder};

use super::{Control, Place};

/// One slash command of the place's list: `/stop` with its option, any other
/// as named (out of runtime.rs, which is at its line ceiling).
pub(super) fn command(name: &str, description: &str) -> Command {
    match name {
        "stop" => stop_command(),
        "tasks" => super::task_cmds::tasks_command(),
        _ => CommandBuilder::new(
            name,
            description,
            twilight_model::application::command::CommandType::ChatInput,
        )
        .build(),
    }
}

/// `/stop`, and its one option (theseus-cdcwr): `all`, default false, also cancels the conversation's tasks.
pub(super) fn stop_command() -> Command {
    CommandBuilder::new(
        "stop",
        "Stop what Theseus is doing here: its turn, jobs, and queued messages. The conversation goes on",
        twilight_model::application::command::CommandType::ChatInput,
    )
    .option(BooleanBuilder::new(
        "all",
        "Also cancel every task this conversation started (an owner's act); default no",
    ))
    .build()
}

/// The control a `/stop` is: `all:true` stops the tasks too, from the presser's place.
pub(super) fn stop_control(
    options: &[CommandDataOption],
    origin: Option<DiscordOrigin>,
) -> Control {
    let all = options
        .iter()
        .any(|o| o.name == "all" && matches!(o.value, CommandOptionValue::Boolean(true)));
    if all {
        Control::StopAll(origin)
    } else {
        Control::Stop
    }
}

/// `execution.stop`'s params for the control: a plain `/stop` sends what it always sent.
pub(super) fn stop_params(
    execution_id: String,
    by: &str,
    cmd: &Control,
) -> theseus_protocol::ExecutionStopParams {
    let (tasks, discord) = match cmd {
        Control::StopAll(origin) => (true, origin.clone()),
        _ => (false, None),
    };
    theseus_protocol::ExecutionStopParams {
        execution_id,
        author: Some(by.to_string()),
        tasks,
        keep_jobs: false,
        discord,
    }
}

/// What `/stop` answers: the plain answer, then the count of tasks a `/stop all:true` cancelled.
pub(super) fn stop_words(r: &theseus_protocol::ExecutionStopResult) -> String {
    let plain = super::stop_answer(r);
    if r.cancelled.is_empty() {
        return plain;
    }
    format!("{plain} {} task(s) cancelled with it.", r.cancelled.len())
}

/// The words a halt is: `Some(why)` when `text` begins `halt self`.
pub(super) fn words(text: &str) -> Option<Option<String>> {
    let mut w = text.split_whitespace();
    let (a, b) = (w.next()?, w.next()?);
    if !(a.eq_ignore_ascii_case("halt") && b.eq_ignore_ascii_case("self")) {
        return None;
    }
    let why = w.collect::<Vec<_>>().join(" ");
    Some((!why.is_empty()).then_some(why))
}

/// Whether `text` is the resume: its first two words `resume self`, any case.
pub(super) fn resume_words(text: &str) -> bool {
    let mut w = text.split_whitespace();
    matches!((w.next(), w.next()), (Some(a), Some(b))
        if a.eq_ignore_ascii_case("resume") && b.eq_ignore_ascii_case("self"))
}

impl Place {
    /// Halt self-improvement as `by`, from where the message came.
    pub(super) async fn halt_self(
        &self,
        why: Option<String>,
        origin: Option<DiscordOrigin>,
        by: &str,
    ) -> String {
        let r = self
            .shared
            .rpc
            .call::<_, SelfSwitchResult>(
                theseus_protocol::method::SELF_HALT,
                SelfHaltParams {
                    why,
                    author: Some(by.to_string()),
                    discord: origin,
                },
            )
            .await;
        match r {
            Ok(r) if r.changed => "🛑 Self-improvement halted: nothing self-directed runs until \
                                   the owner resumes it (`theseus self resume`, or \"resume self\" \
                                   from the owner's own account)."
                .into(),
            Ok(_) => "🛑 Self-improvement was already halted.".into(),
            Err(e) => format!("⚠️ Could not halt self-improvement: {e}."),
        }
    }

    /// Ask the core to release the switch as `by`; it counts only the
    /// owner's.
    pub(super) async fn resume_self(&self, origin: Option<DiscordOrigin>, by: &str) -> String {
        self.shared.resume_self_words(origin, by).await
    }
}

impl super::Shared {
    /// The one resume, the typed words' and the halt note's button's (theseus-6s7ll).
    pub(super) async fn resume_self_words(
        &self,
        origin: Option<DiscordOrigin>,
        by: &str,
    ) -> String {
        let r = self
            .rpc
            .call::<_, SelfSwitchResult>(
                theseus_protocol::method::SELF_RESUME,
                SelfResumeParams {
                    author: Some(by.to_string()),
                    discord: origin,
                    from_job: None,
                },
            )
            .await;
        match r {
            Ok(r) if r.changed => "▶️ Self-improvement resumed.".into(),
            Ok(_) => "▶️ Self-improvement was not halted.".into(),
            Err(e) => format!("🛑 {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resume_words, stop_command, stop_control, stop_params, stop_words, words};
    use crate::runtime::Control;
    use theseus_protocol::DiscordOrigin;
    use twilight_model::application::interaction::application_command::{
        CommandDataOption, CommandOptionValue,
    };

    fn all(v: bool) -> Vec<CommandDataOption> {
        vec![CommandDataOption {
            name: "all".into(),
            value: CommandOptionValue::Boolean(v),
        }]
    }

    /// `/stop` has one optional boolean, `all`; only `all:true` reaches the tasks, and says where it was said
    /// (theseus-cdcwr). The plain `/stop` sends what it always sent.
    #[test]
    fn stop_all_is_an_added_option_that_defaults_to_no() {
        let cmd = stop_command();
        assert_eq!(cmd.name, "stop");
        assert_eq!(cmd.options.len(), 1);
        assert_eq!(cmd.options[0].name, "all");
        assert_ne!(cmd.options[0].required, Some(true));
        let here = Some(DiscordOrigin {
            user_id: "1".into(),
            channel_id: "2".into(),
            guild_id: None,
        });
        assert_eq!(stop_control(&[], here.clone()), Control::Stop);
        assert_eq!(stop_control(&all(false), here.clone()), Control::Stop);
        assert_eq!(
            stop_control(&all(true), here.clone()),
            Control::StopAll(here.clone())
        );
        let plain = stop_params("exe_1".into(), "discord:u", &Control::Stop);
        assert!(!plain.tasks && plain.discord.is_none());
        let bare = serde_json::to_value(&plain).unwrap();
        assert_eq!(
            bare,
            serde_json::json!({"execution_id": "exe_1", "author": "discord:u"})
        );
        let more = stop_params("exe_1".into(), "discord:u", &Control::StopAll(here.clone()));
        assert!(more.tasks);
        assert_eq!(more.discord, here);
    }

    /// The answer says how many tasks went with the stop; a plain stop's answer is unchanged.
    #[test]
    fn the_stops_answer_counts_the_cancelled_tasks() {
        let mut r = theseus_protocol::ExecutionStopResult {
            execution: serde_json::from_value(serde_json::json!({
                "execution_id": "exe_1", "session_id": "ses_1", "kind": "conversation",
                "state": "waiting", "turns": 1, "interrupted": 0, "outstanding": 0,
                "queued_results": 0,
                "budget": {"limit_usd": 1.0, "spent_usd": 0.0, "reserved_usd": 0.0,
                           "held_unknown_usd": 0.0, "available_usd": 1.0},
                "created_at_ms": 0, "updated_at_ms": 0,
            }))
            .unwrap(),
            stopped: true,
            stopped_actions: vec![],
            verdicts: vec![],
            declined: vec![],
            turn_running: false,
            tasks_running: 0,
            wakes_pending: 0,
            cancelled: vec![],
        };
        let plain = stop_words(&r);
        assert_eq!(plain, crate::runtime::stop_answer(&r));
        r.cancelled = vec!["exe_a".into(), "exe_b".into(), "exe_c".into()];
        assert_eq!(
            stop_words(&r),
            format!("{plain} 3 task(s) cancelled with it.")
        );
    }

    #[test]
    fn halt_self_is_the_first_two_words_and_the_rest_is_why() {
        assert_eq!(words("halt self"), Some(None));
        assert_eq!(
            words("Halt SELF it broke the gate"),
            Some(Some("it broke the gate".into()))
        );
        assert_eq!(words("halt"), None);
        assert_eq!(words("please halt self"), None);
        assert_eq!(words("halt selfish"), None);
    }

    #[test]
    fn resume_self_is_the_first_two_words() {
        assert!(resume_words("resume self"));
        assert!(resume_words("Resume SELF now"));
        assert!(!resume_words("resume"));
        assert!(!resume_words("please resume self"));
        assert!(!resume_words("resume selfish"));
        assert!(!resume_words("halt self"));
    }
}
