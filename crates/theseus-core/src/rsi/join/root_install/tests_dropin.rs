//! The first install by the helper did not take (issue 20): the machine's start command was in a drop-in, so the install
//! changed a line that did not win, and the restart's script reached systemd with its newlines in one argument. These
//! replay that layout against the install, with the unit files on a real directory and a stand-in for systemd.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;

use super::answer::restart_script;
use super::machine::restart_argv;
use super::tests::{rooted_with, unit_text, FakeHelper, FakeUnits, Rooted, OLD};
use super::*;
use crate::config::self_improve::SelfInstall;
use crate::ledger::LedgerRow;
use crate::rsi::join::words::{last_join, Install};

const ARGS: &str = "--config /home/ada/theseus.toml --op-token-file /home/ada/tok";
const DROP: &str = "theseusd.service.d";

/// What a drop-in that gives the service its own arguments says, as the documented way does.
fn clears_and_sets(program: &str, args: &str) -> String {
    format!("[Service]\nExecStart=\nExecStart={program} {args}\n")
}

fn auto(units: &Arc<FakeUnits>) -> Rooted {
    rooted_with(
        SelfInstall::Auto,
        None,
        FakeHelper::default(),
        units.clone(),
    )
}

async fn installed(t: &Rooted) -> String {
    let merge = t.join().await;
    t.until("the restart", |t| {
        !t.units.restarts.lock().unwrap().is_empty()
    })
    .await;
    merge
}

fn new_of(merge: &str) -> String {
    program_of(merge).display().to_string()
}

/// KEY, the machine of 2026-11-18: the unit and a drop-in that clears and sets the command both name the old build; the
/// drop-in wins. The install changes the drop-in's one line (its arguments byte for byte), leaves the unit and every
/// other byte alone, and systemd's answer is the new program before it restarts.
#[tokio::test]
async fn a_drop_in_that_sets_the_start_command_is_the_line_changed() {
    let drop_in = format!("{DROP}/start.conf");
    let units = FakeUnits::layout(
        &[
            ("theseusd.service", &unit_text(OLD)),
            (&drop_in, &clears_and_sets(OLD, ARGS)),
        ],
        &[],
        false,
    );
    let before = units.files();
    let t = auto(&units);
    let merge = installed(&t).await;
    let new = new_of(&merge);
    assert_eq!(
        units.own(&drop_in),
        clears_and_sets(&new, ARGS),
        "the drop-in's line, arguments byte for byte"
    );
    assert_eq!(
        units.own("theseusd.service"),
        unit_text(OLD),
        "the unit's own line did not win and is not touched"
    );
    // Every file but that one is what it was.
    let mut after = units.files();
    let key = after
        .keys()
        .find(|k| k.ends_with("start.conf"))
        .cloned()
        .unwrap();
    assert_eq!(after.remove(&key).unwrap(), clears_and_sets(&new, ARGS));
    let mut was = before.clone();
    was.remove(&key);
    assert_eq!(after, was);
    // Asked, after the change and a reload, and only then restarted: once, onto the drop-in's new program.
    assert!(*units.reloads.lock().unwrap() >= 1);
    assert_eq!(units.restarts.lock().unwrap().len(), 1);
    let p = t.r.core.root_pending().unwrap();
    assert_eq!(p.unit.file_name().unwrap(), "start.conf");
    assert_eq!(p.unit_before, clears_and_sets(OLD, ARGS));
    assert!(
        t.why_of("unit").contains(&new) && t.why_of("unit").contains("start.conf"),
        "{}",
        t.why_of("unit")
    );
    // The script restores that file, not the unit, with the drop-in's own text.
    let script = units.restarts.lock().unwrap()[0].clone();
    assert!(
        script.contains("file='") && script.contains("start.conf'"),
        "{script}"
    );
    assert!(script.contains(&format!("want='{new}'")), "{script}");
}

/// The way back, in the script: a restart that is not confirmed puts the DROP-IN back byte for byte, with its mode, and
/// restarts once more; the unit file is never written.
#[tokio::test]
async fn the_way_back_restores_the_drop_in_exactly() {
    let drop_in = format!("{DROP}/start.conf");
    let original = clears_and_sets(OLD, ARGS);
    let units = FakeUnits::layout(
        &[("theseusd.service", &unit_text(OLD)), (&drop_in, &original)],
        &[],
        false,
    );
    let at = units.root.path().join("config/systemd/user").join(&drop_in);
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let t = auto(&units);
    let merge = installed(&t).await;
    // The write kept the mode it found.
    assert_eq!(mode_of(&at), 0o600);
    let p = t.r.core.root_pending().unwrap();
    // Run the script the install made (its confirmation wait cut to a second): the new daemon never confirms.
    let script = restart_script(
        &t.r.core.tools.root_install.signal(),
        &p.unit,
        &p.unit_before,
        "theseusd.service",
        &merge,
        &p.new_program,
        1,
    );
    let calls = run_delivered(&script, &new_of(&merge));
    assert_eq!(
        calls.iter().filter(|c| c.contains("restart")).count(),
        2,
        "onto the build, and back: {calls:?}"
    );
    assert_eq!(units.own(&drop_in), original, "the drop-in is as it was");
    assert_eq!(mode_of(&at), 0o600, "with its mode");
    assert_eq!(units.own("theseusd.service"), unit_text(OLD));
}

fn mode_of(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
}

/// The way back, in the daemon: a restart that cannot start puts the drop-in back exactly and ends the install.
#[tokio::test]
async fn a_restart_that_cannot_start_puts_the_drop_in_back_exactly() {
    let drop_in = format!("{DROP}/start.conf");
    let units = FakeUnits::layout(
        &[
            ("theseusd.service", &unit_text(OLD)),
            (&drop_in, &clears_and_sets(OLD, ARGS)),
        ],
        &[],
        true,
    );
    let before = units.files();
    let t = auto(&units);
    t.join().await;
    t.until("the failure", |t| t.steps().contains(&"failed".to_string()))
        .await;
    assert_eq!(units.files(), before, "every file as it was");
    let p = t.r.core.root_pending().unwrap();
    assert_eq!(p.stage, Stage::RolledBack);
    assert!(
        t.why_of("failed").contains("start.conf"),
        "{}",
        t.why_of("failed")
    );
}

/// A layout with no drop-in is as before: the unit's one line, its arguments, and nothing else.
#[tokio::test]
async fn a_unit_with_no_drop_in_is_changed_as_before() {
    let units = FakeUnits::new(OLD, false);
    let before = units.files();
    let t = auto(&units);
    let merge = installed(&t).await;
    let want = unit_text(OLD).replace(OLD, &new_of(&merge));
    assert_eq!(units.own("theseusd.service"), want);
    assert_eq!(units.files().len(), before.len());
}

/// A drop-in that sets other keys and no start command is left alone: the unit's line is the one changed.
#[tokio::test]
async fn a_drop_in_with_no_start_command_is_left_alone() {
    let env = format!("{DROP}/env.conf");
    let body = "[Service]\nEnvironment=\"A=1\"\nRestart=always\n";
    let units = FakeUnits::layout(
        &[("theseusd.service", &unit_text(OLD)), (&env, body)],
        &[],
        false,
    );
    let t = auto(&units);
    let merge = installed(&t).await;
    assert_eq!(units.own(&env), body, "the drop-in is byte for byte");
    assert_eq!(
        units.own("theseusd.service"),
        unit_text(OLD).replace(OLD, &new_of(&merge))
    );
}

/// Two drop-ins that both set it: the last in name order is the one that wins, and the one changed; the other and the
/// unit are untouched.
#[tokio::test]
async fn of_two_drop_ins_the_last_in_name_order_is_the_one_changed() {
    let (a, b) = (
        format!("{DROP}/10-first.conf"),
        format!("{DROP}/20-second.conf"),
    );
    let (first, second) = (clears_and_sets(OLD, "--a"), clears_and_sets(OLD, ARGS));
    let units = FakeUnits::layout(
        &[
            ("theseusd.service", &unit_text(OLD)),
            // Written in the other order than their names, so a directory listing's order is not the answer.
            (&b, &second),
            (&a, &first),
        ],
        &[],
        false,
    );
    let t = auto(&units);
    let merge = installed(&t).await;
    assert_eq!(units.own(&a), first);
    assert_eq!(units.own(&b), clears_and_sets(&new_of(&merge), ARGS));
    assert_eq!(units.own("theseusd.service"), unit_text(OLD));
}

/// A winner the user cannot write (the administrator's drop-in): refused before any card, build, helper or restart,
/// with the reason and the step by hand, and nothing changed.
#[tokio::test]
async fn a_winner_the_user_cannot_write_is_refused_before_anything_changes() {
    let units = FakeUnits::layout(
        &[("theseusd.service", &unit_text(OLD))],
        &[(
            &format!("{DROP}/20-admin.conf"),
            &clears_and_sets(OLD, ARGS),
        )],
        false,
    );
    let before = units.files();
    let t = auto(&units);
    t.join().await;
    assert_eq!(units.files(), before, "nothing written");
    assert!(units.restarts.lock().unwrap().is_empty());
    assert!(t
        .helper
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|c| c == "presence"));
    assert!(
        t.steps().contains(&"refused".to_string()),
        "{:?}",
        t.steps()
    );
    assert!(t.why_of("refused").contains("belongs to the administrator"));
    let said = t.notices().join("\n");
    assert!(
        said.contains("is not what the service runs")
            && said.contains("20-admin.conf")
            && said.contains("To install it by hand"),
        "{said}"
    );
}

/// The same winner appearing after the card was made: the yes is refused at the plan, before the helper is asked.
#[tokio::test]
async fn a_winner_that_became_unwritable_since_the_card_is_refused_at_the_yes() {
    let units = FakeUnits::new(OLD, false);
    let t = rooted_with(
        SelfInstall::Ask,
        Some(true),
        FakeHelper::default(),
        units.clone(),
    );
    t.join().await;
    let q = t.cards()[0].correlation_id.clone();
    let admin = units.root.path().join("etc/systemd/user").join(DROP);
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::write(admin.join("zz.conf"), clears_and_sets(OLD, ARGS)).unwrap();
    let before = units.files();
    t.yes(&q);
    t.until("the failure", |t| t.steps().contains(&"failed".to_string()))
        .await;
    assert!(t.why_of("failed").contains("belongs to the administrator"));
    assert_eq!(units.files(), before);
    assert!(units.restarts.lock().unwrap().is_empty());
    assert!(t.helper.installed.lock().unwrap().is_empty());
}

/// Systemd says it will run something else: nothing is restarted, the file goes back, and the words say what systemd
/// said instead.
#[tokio::test]
async fn a_start_command_systemd_would_not_run_is_never_restarted_onto() {
    let drop_in = format!("{DROP}/start.conf");
    let units = FakeUnits::layout(
        &[
            ("theseusd.service", &unit_text(OLD)),
            (&drop_in, &clears_and_sets(OLD, ARGS)),
        ],
        &[],
        false,
    );
    *units.says.lock().unwrap() = Some(vec![OLD.to_string()]);
    let before = units.files();
    let t = auto(&units);
    t.join().await;
    t.until("the failure", |t| t.steps().contains(&"failed".to_string()))
        .await;
    let why = t.why_of("failed");
    assert!(
        why.contains(&format!("systemd says it will run {OLD}")) && why.contains("not restarted"),
        "{why}"
    );
    assert!(units.restarts.lock().unwrap().is_empty(), "no restart");
    assert_eq!(units.files(), before, "the file is as it was");
    assert_eq!(t.r.core.root_pending().unwrap().stage, Stage::RolledBack);
    // Another install can follow: nothing is left "under way".
    assert!(t
        .helper
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|c| c == "install"));
}

/// What the helper installed is checked before the service is pointed at it.
#[tokio::test]
async fn a_program_that_is_not_the_helpers_is_not_pointed_at() {
    let units = FakeUnits::new(OLD, false);
    let before = units.files();
    let helper = FakeHelper {
        not_root: Some("is not root's".into()),
        ..Default::default()
    };
    let t = rooted_with(SelfInstall::Auto, None, helper, units.clone());
    t.join().await;
    t.until("the failure", |t| t.steps().contains(&"failed".to_string()))
        .await;
    assert!(
        t.why_of("failed").contains("not as it should be"),
        "{}",
        t.why_of("failed")
    );
    assert_eq!(units.files(), before);
    assert!(units.restarts.lock().unwrap().is_empty());
    assert!(t.r.core.root_pending().is_none());
}

// ---- the restart's script reaches systemd in a form it reads whole --------------------------------

/// Run the command a `systemd-run` of `script` would run, with a stand-in `systemctl` that answers a start-command
/// question with `shows`, logs the other calls, and returns them.
fn run_delivered(script: &str, shows: &str) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    let (bin, log) = (d.path().join("bin"), d.path().join("log"));
    std::fs::create_dir(&bin).unwrap();
    let ctl = bin.join("systemctl");
    std::fs::write(
        &ctl,
        format!(
            "#!/bin/bash\nif [ \"$2\" = show ]; then echo 'ExecStart={{ path={shows} ; argv[]=x ; ignore_errors=no }}'; exit 0; fi\necho \"$@\" >> {}\n",
            log.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let argv = restart_argv(script, "bbbbbbbb");
    let out = std::process::Command::new("/bin/bash")
        .args(["-c", argv.last().unwrap()])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(out.status.code().is_some(), "{out:?}");
    std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// A script as the install writes it, over a drop-in in a directory of the test's own.
fn script_over(dir: &Path, wait: u64) -> (String, PathBuf, String) {
    let file = dir.join("start.conf");
    let before = clears_and_sets(OLD, ARGS);
    std::fs::write(&file, &before).unwrap();
    let want = "/usr/local/lib/theseus/bbbbbbbb/theseusd";
    // A multi-line, quote-bearing text is what the way back embeds.
    let script = restart_script(
        &dir.join("confirmed"),
        &file,
        &before,
        "theseusd.service",
        &"b".repeat(40),
        want,
        wait,
    );
    (script, file, want.into())
}

/// No argument of the `systemd-run` has a newline (systemd writes its command to a file and reads it back line by
/// line), and the form is the same script, decoded: it runs to its end.
#[test]
fn the_restart_reaches_systemd_in_one_line_and_runs_whole() {
    let d = tempfile::tempdir().unwrap();
    let (script, file, want) = script_over(d.path(), 1);
    assert!(script.contains('\n'), "premise: the script has many lines");
    let argv = restart_argv(&script, "bbbbbbbb");
    assert!(no_line_break(&argv), "{argv:?}");
    // Nothing systemd would read as a variable, a specifier or a separator either.
    for a in &argv {
        assert!(!a.contains(['$', '%', ';']), "{a}");
    }
    assert_eq!(argv[0..2], ["systemd-run", "--user"]);
    // It decodes to the script, whole.
    let command = argv.last().unwrap();
    let word = command.split(' ').nth(2).unwrap();
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(word)
        .unwrap();
    assert_eq!(String::from_utf8(decoded).unwrap(), script);
    // Run: the new build is not confirmed in a second, so it goes back: the whole script ran to its last lines.
    let calls = run_delivered(&script, &want);
    assert_eq!(
        calls.iter().filter(|c| c.contains("restart")).count(),
        2,
        "{calls:?}"
    );
    assert_eq!(
        std::fs::read_to_string(file).unwrap(),
        clears_and_sets(OLD, ARGS)
    );
}

fn no_line_break(argv: &[String]) -> bool {
    argv.iter().all(|a| !a.contains(['\n', '\r']))
}

/// The check can tell: the script as it was passed before (as the one argument) has newlines in it.
#[test]
fn passing_the_script_as_it_is_would_put_newlines_in_front_of_systemd() {
    let d = tempfile::tempdir().unwrap();
    let (script, ..) = script_over(d.path(), 1);
    let old_way: Vec<String> = ["/bin/bash", "-c", &script].map(String::from).to_vec();
    assert!(!no_line_break(&old_way));
}

/// A start command that is not what the script was told to expect is never restarted onto: the script puts the file
/// back, and restarts once (onto what it was), not onto the other program.
#[test]
fn the_script_does_not_restart_onto_a_program_systemd_will_not_run() {
    let d = tempfile::tempdir().unwrap();
    let (script, file, _) = script_over(d.path(), 1);
    std::fs::write(
        &file,
        clears_and_sets("/usr/local/lib/theseus/bbbbbbbb/theseusd", ARGS),
    )
    .unwrap();
    // Systemd would still start the old program.
    let calls = run_delivered(&script, OLD);
    assert_eq!(
        calls.iter().filter(|c| c.contains("restart")).count(),
        1,
        "only the way back's restart: {calls:?}"
    );
    assert_eq!(
        std::fs::read_to_string(file).unwrap(),
        clears_and_sets(OLD, ARGS)
    );
}

// ---- the words ---------------------------------------------------------------------------------------

fn ledger(kind: &str, at_ms: u64, data: serde_json::Value) -> LedgerRow {
    let mut r = LedgerRow::named(kind, None, None, data);
    r.at_unix_ms = at_ms;
    r
}

/// "Not confirmed running" becomes: not what runs, since when, why (the install's own last row) and the one command.
#[test]
fn the_pipelines_words_say_why_the_install_did_not_take_and_the_step() {
    let a = "a".repeat(40);
    let at = 1_795_000_000_000; // a fixed instant
    let joined = ledger("self.joined", at - 10_000, json!({"merge": a}));
    let rolled = ledger(
        "self.root_install",
        at,
        json!({"merge": a, "step": "rolled_back", "what": "the install did not take: the service runs the build before",
               "why": "the new build is not what is running", "numbers": {"step": "sudo install x"}}),
    );
    let said = last_join(&[joined.clone(), rolled.clone()]).unwrap();
    assert_eq!(said.install, Install::DidNotTake);
    let when = format!(
        "{} {} UTC",
        crate::recall::render::utc_day(at),
        theseus_protocol::utc_hm(at)
    );
    assert_eq!(
        said.line,
        format!(
            "joined at {}; the service is not running it: the install of {when} did not take \
             (the new build is not what is running). To install it by hand, run: sudo install x",
            &a[..12]
        )
    );
    // No step on the row: where the steps are written.
    let mut bare = rolled.clone();
    bare.data["numbers"] = json!(null);
    assert!(last_join(&[joined.clone(), bare])
        .unwrap()
        .line
        .contains("docs/setup.md"));
    // A row of an install still under way says nothing of failure; a later install that took says it runs.
    let under_way = ledger(
        "self.root_install",
        at + 1,
        json!({"merge": a, "step": "started"}),
    );
    assert_eq!(
        last_join(&[joined.clone(), rolled.clone(), under_way])
            .unwrap()
            .install,
        Install::Unconfirmed
    );
    let took = ledger("self.installed", at + 2, json!({"merge": a}));
    assert_eq!(
        last_join(&[joined, rolled, took]).unwrap().install,
        Install::Running
    );
}

/// The install that went back says the same to the owner, and the pipeline's words follow from its rows.
#[tokio::test]
async fn the_notice_of_a_failed_install_and_the_pipeline_say_the_same() {
    let t = auto(&FakeUnits::new(OLD, false));
    let merge = installed(&t).await;
    // The old daemon again: the install did not take.
    t.r.core.clone().root_install_after_serving_as(
        Some("0".repeat(40).as_str()),
        PathBuf::from("/nonexistent.sock"),
    );
    let said = t.notices().join("\n");
    assert!(
        said.contains("is not what the service runs")
            && said.contains("the install did not take")
            && said.contains("To install it by hand"),
        "{said}"
    );
    let line = t.r.core.self_last_join().unwrap();
    assert_eq!(line.install, Install::DidNotTake, "{}", line.line);
    assert!(
        line.line
            .starts_with(&format!("joined at {}", &merge[..12]))
            && line.line.contains("did not take")
            && line.line.contains("To install it by hand"),
        "{}",
        line.line
    );
    assert_eq!(t.r.core.self_joined_not_running(), Some(merge));
}

// ---- the real thing ----------------------------------------------------------------------------------

/// `systemctl --user <args>` on the box's own user manager: its output.
fn ctl(args: &[&str]) -> String {
    let o = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// The program a scratch service is running now, by its main process.
fn running_program(service: &str) -> String {
    let pid = ctl(&["show", "-p", "MainPID", "--value", service]);
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// The restart, for real: the script in its one-line form through `systemd-run --user` on a real user manager, over a
/// scratch service whose start command is in a drop-in. Confirmed, the service ends on the new program; not confirmed,
/// the drop-in is back as it was and the service runs the old one. It writes only a scratch unit and a scratch
/// transient unit, in the runtime directory, and removes them. Off unless `THESEUS_REAL_SYSTEMD_PROOF` is set, since a
/// machine without a user manager (a container, CI) has nothing to ask: the fleet's boxes have one.
#[test]
fn the_restart_through_a_real_user_manager_runs_to_its_end() {
    if std::env::var_os("THESEUS_REAL_SYSTEMD_PROOF").is_none() {
        eprintln!(
            "skipped: THESEUS_REAL_SYSTEMD_PROOF is not set (it needs a systemd user manager)"
        );
        return;
    }
    let runtime =
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").expect("a user manager's runtime dir"));
    let tag = format!("{}", std::process::id());
    let service = format!("theseus-proof-{tag}.service");
    let units = runtime.join("systemd/user");
    let drop_in = units.join(format!("{service}.d/start.conf"));
    let d = tempfile::tempdir().unwrap();
    let (old, new) = (d.path().join("old-sleep"), d.path().join("new-sleep"));
    for p in [&old, &new] {
        std::fs::copy("/usr/bin/sleep", p).unwrap();
    }
    let (old, new) = (old.display().to_string(), new.display().to_string());
    let args = "3000";
    let exercise = |confirm: bool| -> (String, String, String) {
        std::fs::create_dir_all(drop_in.parent().unwrap()).unwrap();
        std::fs::write(
            units.join(&service),
            "[Service]\nType=simple\nExecStart=/bin/false\n",
        )
        .unwrap();
        let before = clears_and_sets(&old, args);
        std::fs::write(&drop_in, &before).unwrap();
        ctl(&["daemon-reload"]);
        ctl(&["start", &service]);
        assert_eq!(
            running_program(&service),
            old,
            "premise: the old program runs"
        );
        // The install's own edit, then the script exactly as the install makes it.
        std::fs::write(&drop_in, repoint_text(&before, &old, &new)).unwrap();
        let signal = d.path().join("confirmed");
        let _ = std::fs::remove_file(&signal);
        if confirm {
            std::fs::write(&signal, "x").unwrap();
        }
        let script = restart_script(
            &signal,
            &drop_in,
            &before,
            &service,
            &"b".repeat(40),
            &new,
            4,
        );
        let argv = restart_argv(&script, &format!("proof{tag}"));
        let o = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        // What systemd was given, read back from its own transient file while the unit lives.
        let transient = std::fs::read_to_string(runtime.join(format!(
            "systemd/transient/theseus-root-install-proof{tag}.service"
        )))
        .unwrap_or_default();
        for _ in 0..120 {
            if ctl(&[
                "is-active",
                &format!("theseus-root-install-proof{tag}.service"),
            ]) != "active"
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        let ended = (
            running_program(&service),
            std::fs::read_to_string(&drop_in).unwrap(),
            transient,
        );
        ctl(&["stop", &service]);
        ended
    };
    let (ran, file, transient) = exercise(true);
    assert_eq!(ran, new, "confirmed: the service runs the new program");
    assert_eq!(file, clears_and_sets(&new, args));
    // Every line of the transient unit systemd wrote is a section, a setting or a comment: none cut.
    assert!(transient.contains("base64 -d"), "{transient}");
    for l in transient.lines().filter(|l| !l.is_empty()) {
        assert!(
            l.starts_with('[')
                || l.starts_with('#')
                || l.split_once('=')
                    .is_some_and(|(k, _)| k.chars().all(|c| c.is_ascii_alphabetic())),
            "a line systemd would not read: {l:?}"
        );
    }
    let (ran, file, _) = exercise(false);
    assert_eq!(ran, old, "not confirmed: the way back runs the old program");
    assert_eq!(
        file,
        clears_and_sets(&old, args),
        "and the drop-in is as it was"
    );
    let _ = std::fs::remove_dir_all(drop_in.parent().unwrap());
    let _ = std::fs::remove_file(units.join(&service));
    ctl(&["daemon-reload"]);
}

fn repoint_text(text: &str, old: &str, new: &str) -> String {
    super::unit::repoint(text, old, new).unwrap()
}
