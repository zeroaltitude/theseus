//! `! shell` (theseus-vjzt, phase 3): a leading `!` runs the rest of the line as a shell command;
//! `/shell` names the same call. **Never a client-side spawn**: the line becomes one `turn.submit`
//! whose one instruction is to run it, verbatim, with `proc.run` -- so it waits behind the gate's
//! posture, the floor's path checks and the session's mode exactly as a model-proposed `proc.run`
//! would, and the job that runs it carries `THESEUS_SESSION` as every job's does (`job.rs`). The
//! chat minds the same variable the other way round: refused when THIS chat is a Theseus job's own
//! (`job_session` set, `refuse_bang_shell_in_a_job`) -- a job using the operator's own shell-escape
//! fast path is the confused-deputy shape `refuse_stop_all_in_a_job` already blocks for `/stop
//! --tasks`.

use super::state::{Chat, Effect};
use super::style::Sty;

/// The one instruction wrapped around the typed command: unambiguous, so the model's only
/// reasonable move is the one call with this exact argv, not a paraphrase and not a question.
fn directive(cmd: &str) -> String {
    format!("Run this exact shell command now with `proc.run`, verbatim and nothing else: {cmd}")
}

/// `! CMD` or `/shell CMD`: refuse an empty command, refuse inside a job, refuse with no
/// conversation open yet; otherwise the one `turn.submit`, exactly as a typed message sends one.
pub(super) fn shell(chat: &mut Chat, args: &str) -> Vec<Effect> {
    let cmd = args.trim();
    if cmd.is_empty() {
        chat.flash("! needs a command, e.g. !ls");
        return Vec::new();
    }
    if let Err(e) = theseus_client::client::refuse_bang_shell_in_a_job(chat.job_session.as_deref())
    {
        chat.say_words(Sty::Red, &format!("✗ {e}"));
        return Vec::new();
    }
    let Some(sid) = chat.session.clone() else {
        chat.flash("the conversation is not open yet");
        return Vec::new();
    };
    vec![chat.submit_call(&sid, directive(cmd), &[])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directive_names_proc_run_and_carries_the_command_verbatim() {
        let d = directive("ls -la /tmp");
        assert!(d.contains("proc.run"), "{d}");
        assert!(d.ends_with("ls -la /tmp"), "{d}");
    }
}
