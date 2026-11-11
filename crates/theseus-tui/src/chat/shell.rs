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

/// How the instruction opens and closes, in the bracket every block the harness writes into a
/// request already carries, so a reader can tell it from the conversation's own words
/// (`task_graph::view::HEAD`, `commitments::HEAD`, `project::slice::HEAD`, and
/// `turn::env_tail::HEAD`, which `task_graph::view::is_head` lists). This block needs the closer
/// as much as the opener: a turn attaches its `Date:`/`Git:` block right after the last user
/// text (`turn/env_tail.rs`), so a command left on the message's tail abuts that block with
/// nothing between them and the model has to guess where the command ends -- and in a live run
/// of 2026-11-11 one dispatch in eleven guessed wrong and ran the harness's lines as part of the
/// command. The bracket is the guess removed.
const HEAD: &str = "[Run this exact shell command now with `proc.run`, verbatim and nothing \
    else. The command is the line between this opener and the closing bracket, which is on a \
    line of its own; nothing outside the bracket belongs to the command, and any line after it \
    is the harness's own and not the operator's:";
const TAIL: &str = "]";

/// The one instruction wrapped around the typed command: unambiguous, so the model's only
/// reasonable move is the one call with this exact argv, not a paraphrase and not a question.
/// The command sits on a line of its own inside the bracket, never on the message's tail.
fn directive(cmd: &str) -> String {
    format!("{HEAD}\n{cmd}\n{TAIL}")
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
        assert!(d.contains("\nls -la /tmp\n"), "{d}");
        assert!(d.ends_with(TAIL), "{d}");
    }

    /// The command is bounded, so the harness's own tail block cannot be read as part of it.
    /// `turn/env_tail.rs` attaches `Date:`/`Git:` right after the conversation's last user text,
    /// and in the live run of 2026-11-11 00:3x one dispatch of eleven was
    /// `echo PASTED-BANG-RANDate: Wed 2026-11-11` -- the typed command glued to that block --
    /// because the directive ended on the command. What is rendered here as one text is how a
    /// model reads two adjacent blocks; `theseus-tui` does not depend on `theseus-core`, so the
    /// block is its literal text rather than `turn::env_tail::HEAD`.
    #[test]
    fn the_harness_s_date_and_git_block_cannot_be_read_as_part_of_the_command() {
        const ENV_TAIL: &str = "Date: Wed 2026-11-11\nGit: unknown";
        let cmd = "echo PASTED-BANG-RAN";
        let seen = format!("{}\n{ENV_TAIL}", directive(cmd));

        let (bounded, after) = seen
            .strip_prefix(HEAD)
            .and_then(|rest| rest.rsplit_once(&format!("\n{TAIL}")))
            .unwrap_or_else(|| {
                panic!("the command is unbounded, so the harness's block abuts it: {seen:?}")
            });
        assert_eq!(bounded, format!("\n{cmd}"), "{seen:?}");
        assert!(
            !bounded.contains("Date:") && !bounded.contains("Git:"),
            "the harness's block fell inside the command: {bounded:?}"
        );
        assert_eq!(after.trim_start_matches('\n'), ENV_TAIL, "{seen:?}");
    }
}
