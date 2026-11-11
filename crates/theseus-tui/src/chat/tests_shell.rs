//! `! shell` (theseus-vjzt, phase 3): the bang escape is registered as a `COMMANDS` entry (reachable
//! both as `!` and by name as `/shell`), sends one `turn.submit` whose instruction names `proc.run`
//! with the command verbatim (never a local spawn), and is refused -- sending nothing -- when this
//! chat is itself a Theseus job's own.

use crossterm::event::KeyCode;

use super::tests::{script, Rig, SID};

async fn ask(r: &mut Rig, line: &str) {
    r.ready().await;
    r.type_text(line).await;
    r.press(KeyCode::Enter).await;
}

/// The command is registered, reachable through either the bang or its name.
#[tokio::test(start_paused = true)]
async fn the_bang_and_the_name_both_reach_the_one_shell_command() {
    let mut r = Rig::with(100, 30, script()).await;
    ask(&mut r, "!echo one").await;
    r.asked("turn.submit", 1).await;
    ask(&mut r, "/shell echo two").await;
    r.asked("turn.submit", 2).await;
    let sent = r.daemon.asked("turn.submit");
    assert_eq!(sent.len(), 2, "{sent:?}");
    // Bounded, never left on the message's tail: `shell::directive` puts the command on a line
    // of its own inside the bracket, so the turn's `Date:`/`Git:` block cannot join it.
    assert!(
        sent[0]["input"].as_str().unwrap().contains("\necho one\n"),
        "{sent:?}"
    );
    assert!(
        sent[1]["input"].as_str().unwrap().contains("\necho two\n"),
        "{sent:?}"
    );
}

/// The gated call itself: a `turn.submit` whose text names `proc.run` and carries the command
/// verbatim. Asserted on the call the daemon received, never on a rendered output string.
#[tokio::test(start_paused = true)]
async fn the_submitted_turn_names_proc_run_with_the_command_verbatim() {
    let mut r = Rig::with(100, 30, script()).await;
    ask(&mut r, "!ls -la /tmp").await;
    r.asked("turn.submit", 1).await;
    let sent = r.daemon.asked("turn.submit");
    let input = sent[0]["input"].as_str().unwrap();
    assert!(input.contains("proc.run"), "{input}");
    // Delimited and bounded, so a future edit cannot put the command back on the message's tail
    // where `turn/env_tail.rs`'s appended block would abut it (`shell::directive`'s own test
    // pins the glued case).
    assert!(input.contains("\nls -la /tmp\n"), "{input}");
    assert!(input.ends_with(']'), "{input}");
    assert_eq!(sent[0]["session_id"], SID);
}

/// `THESEUS_SESSION` honoured: a chat that is itself a job's own refuses the bang and sends
/// nothing, the same speed bump `/stop --tasks` already has.
#[tokio::test(start_paused = true)]
async fn inside_a_job_the_bang_is_refused_and_sends_nothing() {
    let mut r = Rig::with(100, 30, script()).await;
    r.ready().await;
    r.runner.chat.job_session = Some("ses_job0123456789".into());
    r.type_text("!rm -rf /").await;
    r.press(KeyCode::Enter).await;
    r.shows("! refused").await;
    r.settle().await;
    assert!(r.daemon.asked("turn.submit").is_empty());
}

/// The negative: `shell.rs` itself never reaches for a client-side spawn. A revert of this
/// assertion (planting `std::process::Command::new("sh")` in `shell.rs`) must fail it.
#[test]
fn the_modules_own_source_names_no_client_side_spawn() {
    let src = include_str!("shell.rs");
    let code = &src[..src.find("#[cfg(test)]").unwrap()];
    assert!(
        !code.contains("process::Command"),
        "shell.rs must never spawn a process client-side; the command must go through the gate"
    );
}
