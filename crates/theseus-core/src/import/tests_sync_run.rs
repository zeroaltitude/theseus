//! The on-demand sync's guards (theseus-ievo3, S29-1).
//!
//! The two that matter most, in the dispatcher's words, are *"a second request
//! while one runs answers already running"* and *"no argument comes from the
//! model or the message"*. The second is held **structurally** here -- the
//! argv is a constant of literals and the builder has no parameter at all --
//! rather than by trying one malicious string, because a test of one string
//! says nothing about the next one.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::sync::SyncState;
use super::sync_run::{
    already_running_line, command, finished_line, started_line, Asked, OnDemand, Ran, Runner, ARGV,
    BINARY, DEADLINE_MS, KEEP_ENV,
};

/// This file's own source, for the guards that read it.
const SOURCE: &str = include_str!("sync_run.rs");

/// A runner that counts its starts and never forks.
fn counting(n: &Arc<AtomicU64>) -> Runner {
    let n = n.clone();
    Arc::new(move || {
        let n = n.clone();
        Box::pin(async move {
            n.fetch_add(1, Ordering::SeqCst);
            Ran {
                pid: 1,
                code: Some(0),
                ..Ran::default()
            }
        })
    })
}

/// **The argv is a constant of literals.** No model-produced and no
/// message-produced string is in it, because there is no string in it that is
/// not written here. The list is asserted whole, so an argument added later
/// fails this and has to be argued for.
#[test]
fn the_argv_is_a_constant_of_literals() {
    assert_eq!(ARGV, ["import", "sync", "--no-people"]);
    assert_eq!(BINARY, "theseus");
    // The source says it too: an array of literals, nothing computed.
    assert!(
        SOURCE.contains(r#"pub const ARGV: [&str; 3] = ["import", "sync", "--no-people"];"#),
        "ARGV is declared as a constant of literals"
    );
}

/// **The command builder takes no arguments.** This is the structural half of
/// "no argument comes from the model or from the message" (S29-1): a function
/// with an empty parameter list cannot be handed a caller's string, so no
/// review of call sites is needed to know none does. The guard reads the
/// source so that a parameter added later -- which is exactly how this
/// property would be lost -- fails here.
#[test]
fn the_command_builder_takes_no_arguments() {
    let decl = SOURCE
        .lines()
        .find(|l| l.trim_start().starts_with("pub fn command("))
        .expect("`command` is declared in this file");
    assert_eq!(
        decl.trim(),
        "pub fn command() -> io::Result<tokio::process::Command> {",
        "`command` must take NOTHING: a parameter is a way in for a caller's string"
    );
    // And the run that uses it passes nothing either.
    assert!(
        SOURCE.contains("let mut cmd = match command() {"),
        "`run` calls `command()` with no argument"
    );
}

/// The command production builds, read whole: this install's own binary, the
/// fixed argv, and an environment that was cleared and rebuilt from four
/// names. Built and inspected rather than forked, so the production path is
/// read by a test that starts no process.
#[test]
fn the_command_is_the_daemons_own_binary_with_a_fixed_argv_and_environment() {
    std::env::set_var("THESEUS_SYNC_RUN_GUARD", "a value no child should see");
    let cmd = command().expect("the command builds");
    let std = cmd.as_std();

    // The program is beside this test's own image, not whatever PATH finds.
    let program = std::path::PathBuf::from(std.get_program());
    assert_eq!(
        program.file_name().and_then(|n| n.to_str()),
        Some(BINARY),
        "{program:?}"
    );
    assert_eq!(
        program.parent(),
        std::env::current_exe().unwrap().parent(),
        "the binary is beside this process's own, never from PATH"
    );

    // The argv is ARGV, whole and in order.
    let args: Vec<String> = std
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ARGV.to_vec());

    // The environment was cleared, and holds nothing but KEEP_ENV's names.
    assert!(
        std.get_envs().count() > 0,
        "the keep list is applied as explicit assignments"
    );
    for (k, _) in std.get_envs() {
        let k = k.to_string_lossy().into_owned();
        assert!(
            KEEP_ENV.contains(&k.as_str()),
            "the child's environment holds {k}, which is not on the keep list {KEEP_ENV:?}"
        );
    }
    assert!(
        SOURCE.contains(".env_clear()"),
        "the environment is cleared before the keep list is applied"
    );
    // The daemon's own extra variable is not inherited: the clear is real.
    assert!(
        !std.get_envs()
            .any(|(k, _)| k.to_string_lossy() == "THESEUS_SYNC_RUN_GUARD"),
        "a variable of the daemon's that is not on the keep list does not reach the child"
    );
    std::env::remove_var("THESEUS_SYNC_RUN_GUARD");
}

/// **A second request while one runs answers "already running".** It starts no
/// second child, it queues nothing, and -- the part that would be worst if it
/// were wrong -- it does not quietly report success: the answer is a different
/// variant, which is what the RPC renders as a different sentence.
#[tokio::test]
async fn a_second_request_while_one_runs_starts_nothing_and_says_so() {
    let n = Arc::new(AtomicU64::new(0));
    let od = OnDemand::new(counting(&n));

    // One request takes the seat.
    let first = od.claim(1_000);
    assert_eq!(first, Asked::Started(1_000));

    // A second, and a third, while it is held: told, not served, not queued.
    assert_eq!(od.claim(2_000), Asked::AlreadyRunning(1_000));
    assert_eq!(
        od.claim(300_000),
        Asked::AlreadyRunning(1_000),
        "and the time it names is the RUNNING one's start, not the asker's"
    );
    assert_eq!(
        od.started(),
        1,
        "exactly one run was ever started: {:?}",
        od.flight()
    );

    // The run itself, and the seat comes back.
    let ran = od.run_claimed().await;
    assert!(ran.ok(), "{ran:?}");
    assert_eq!(n.load(Ordering::SeqCst), 1, "one child, not three");
    assert_eq!(od.flight(), None, "the seat is free again");

    // And now a request is served again.
    assert_eq!(od.claim(400_000), Asked::Started(400_000));
    assert_eq!(od.started(), 2);
}

/// The seat comes back even when the run never started. A seat a failure kept
/// would wedge the whole feature until the daemon restarted, and the owner
/// would see "already running" for ever with nothing running.
#[tokio::test]
async fn a_run_that_failed_gives_the_seat_back() {
    let od = OnDemand::new(Arc::new(|| {
        Box::pin(async {
            Ran {
                error: Some("the sync did not start: No such file or directory".into()),
                ..Ran::default()
            }
        })
    }));
    assert_eq!(od.claim(1_000), Asked::Started(1_000));
    let ran = od.run_claimed().await;
    assert!(!ran.ok(), "{ran:?}");
    assert_eq!(od.flight(), None, "the seat is free after a failure too");
    assert_eq!(od.claim(2_000), Asked::Started(2_000));
}

/// **The deadline is the scheduled import-only run's own bound, reused.** The
/// unit that runs the same `import sync --no-people` is bounded by
/// `SYNC_HOURLY_TIMEOUT` in `scripts/user-service.sh`; under the timer design
/// systemd held that bound, and with the daemon starting the run the daemon
/// holds it. This reads the script, so the two cannot drift apart silently.
#[test]
fn the_deadline_is_the_scheduled_import_only_runs_own_bound() {
    let script = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/user-service.sh")
            .canonicalize()
            .expect("the script is beside the crates"),
    )
    .expect("read user-service.sh");
    let line = script
        .lines()
        .find(|l| l.starts_with("SYNC_HOURLY_TIMEOUT="))
        .expect("the import-only unit's bound is in the script");
    let value = line.trim_start_matches("SYNC_HOURLY_TIMEOUT=");
    let mins: u64 = value
        .strip_suffix("min")
        .expect("the bound is written in minutes")
        .parse()
        .expect("a number of minutes");
    assert_eq!(
        DEADLINE_MS,
        mins * 60_000,
        "the on-demand deadline is {DEADLINE_MS} ms and the scheduled run's bound is {value}: \
         reuse it, do not invent a second one"
    );
}

/// The two lines the thread gets, rendered. The sharpest lesson of the last
/// two shifts is that a green suite can be pinned to a lie, so these are the
/// strings themselves and not a shape.
#[test]
fn the_thread_is_told_it_started_and_then_what_it_imported() {
    assert_eq!(
        started_line(),
        "The OpenClaw sync started. I will say what it imported when it is done."
    );

    let before = SyncState {
        at_ms: 1_000,
        sessions_new: 7,
        sessions_appended: 2,
        messages: 90,
        ..SyncState::default()
    };
    let after = SyncState {
        at_ms: 2_000,
        sessions_new: 9,
        sessions_appended: 3,
        messages: 134,
        ..SyncState::default()
    };
    let ok = Ran {
        pid: 11,
        code: Some(0),
        ..Ran::default()
    };
    assert_eq!(
        finished_line(&ok, &before, &after),
        "The OpenClaw sync finished: 2 new sessions, 1 session that grew, 44 messages. \
         The people step waits for tonight's full run."
    );

    // Nothing to do is said as nothing to do, never as a count of zero. The
    // run DID happen -- `at_ms` moved -- it just found nothing.
    let empty = SyncState {
        at_ms: 3_000,
        ..after.clone()
    };
    assert_eq!(
        finished_line(&ok, &after, &empty),
        "The OpenClaw sync finished: nothing new to import."
    );
    // A child that exited 0 without recording a run at all is said as that,
    // and not as "nothing new": the two are different failures to look into.
    assert_eq!(
        finished_line(&ok, &after, &after.clone()),
        "The OpenClaw sync finished and recorded no run: nothing was imported."
    );

    // A failure is said as a failure, and says nothing was imported.
    let dead = Ran {
        error: Some("the sync did not start: No such file or directory".into()),
        ..Ran::default()
    };
    assert_eq!(
        finished_line(&dead, &before, &before.clone()),
        "The OpenClaw sync failed: the sync did not start: No such file or directory. \
         Nothing was imported."
    );
    let late = Ran {
        pid: 11,
        timed_out: true,
        ..Ran::default()
    };
    assert_eq!(
        finished_line(&late, &before, &before.clone()),
        "The OpenClaw sync failed: it ran past 45 minutes and was stopped. Nothing was imported."
    );
    // A run the child recorded as failed is a failure too, even if the child
    // itself exited 0: the record is what says whether anything was imported.
    let recorded = SyncState {
        error: Some("python3 did not start".into()),
        ..before.clone()
    };
    assert_eq!(
        finished_line(&ok, &before, &recorded),
        "The OpenClaw sync failed: python3 did not start. Nothing was imported."
    );
}

/// The "already running" sentence, rendered at the ages it is really read at.
#[test]
fn the_already_running_line_says_how_long_it_has_been_going() {
    assert_eq!(
        already_running_line(1_000, 1_000),
        "An OpenClaw sync is already running (started just now); nothing new was started. \
         I will say what it imported when it is done."
    );
    assert_eq!(
        already_running_line(1_000, 31_000),
        "An OpenClaw sync is already running (started 30 seconds ago); nothing new was started. \
         I will say what it imported when it is done."
    );
    // The hand-over from seconds to minutes reaches ONE minute rather than
    // jumping over it: 45 s rounds to a minute, it does not read "45 seconds"
    // and then "2 minutes".
    assert_eq!(
        already_running_line(1_000, 46_000),
        "An OpenClaw sync is already running (started 1 minute ago); nothing new was started. \
         I will say what it imported when it is done."
    );
    assert_eq!(
        already_running_line(1_000, 61_000),
        "An OpenClaw sync is already running (started 1 minute ago); nothing new was started. \
         I will say what it imported when it is done."
    );
    assert_eq!(
        already_running_line(1_000, 601_000),
        "An OpenClaw sync is already running (started 10 minutes ago); nothing new was started. \
         I will say what it imported when it is done."
    );
}

/// There is no timer left to install, and no default that would switch one on.
/// A default left behind in a script or an example is exactly the kind of
/// residue that ships, so this reads the script rather than trusting the diff.
#[test]
fn no_on_demand_timer_remains_to_be_installed() {
    let script = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/user-service.sh")
            .canonicalize()
            .expect("the script is beside the crates"),
    )
    .expect("read user-service.sh");
    for residue in [
        "--on-demand-mins",
        "SYNC_ONDEMAND",
        "ONDEMAND_TIMEOUT",
        "ONDEMAND_ACCURACY",
        "--if-requested",
    ] {
        assert!(
            !script.contains(residue),
            "`{residue}` is still in scripts/user-service.sh: the on-demand TIMER is not shipped \
             (S29-1), so nothing of it may be left to switch on"
        );
    }
    // The unit itself is never WRITTEN. The `sync-ondemand` name does survive,
    // on purpose and in one direction only: the install and the uninstall both
    // REMOVE a pair an earlier build of this script wrote, whose command no
    // longer exists. So the name may appear beside `rm`/`disable`, never
    // beside `write_sync_unit`.
    assert!(
        !script.contains("write_sync_unit \"$osvc\"")
            && !script.contains("write_sync_unit \"$otim\""),
        "scripts/user-service.sh still writes an on-demand unit"
    );
    for line in script.lines().filter(|l| l.contains("sync-ondemand")) {
        assert!(
            line.contains("obase=") || line.contains("disable") || line.contains("list-timers"),
            "`sync-ondemand` may only be named to remove or report a pair an earlier build \
             wrote, not to install one: {line}"
        );
    }
}
