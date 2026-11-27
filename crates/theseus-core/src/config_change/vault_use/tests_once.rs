//! `vault.use`'s run form (issue 64), played through the whole core: the owner's own turn names
//! a vault item for one variable of one run of one program, and the next run of that program in
//! the turn has it. No card, no code, no settings written, no restart; one told note; the value
//! in no exit, and the stored result of a run that printed it carries the scrubber's marker. The
//! form asks first after outside text and in a turn the owner's message did not begin, keeps the
//! wall, gives nothing for an item Theseus runs on, and gives each grant to one run only.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use super::*;
use crate::config_change::tests::{rig_with, Rig};
use crate::config_change::tests_look::everything;
use crate::content::OutsideMode;
use crate::node::{Body, Origin, ResultStatus};
use crate::secrets::look::tests::{logged, stub};

pub(super) const VALUE: &str = "INVENTED-FIELD-VALUE-9";
pub(super) const VAR: &str = "LANTERN_TOKEN";
pub(super) const SCRIPT: &str = "call_lantern.sh";
/// What the script does: one line with the variable's length, in `got.txt` beside it.
pub(super) const COUNTS: &str = "printf '%s\\n' \"${#LANTERN_TOKEN}\" >> got.txt\n";
/// What a script that prints its key does, beside counting it.
pub(super) const PRINTS: &str =
    "printf '%s\\n' \"${#LANTERN_TOKEN}\" >> got.txt\necho \"token=$LANTERN_TOKEN\"\n";
/// The fixed words that say an item is a key Theseus itself runs on.
pub(super) const RUNS_ON_WORDS: &str = "a key Theseus itself runs on";

/// A change to the settings a case is played under.
pub(super) type Tweak = Box<dyn Fn(&mut crate::Config) + Send>;

pub(super) struct Once {
    pub(super) r: Rig,
    pub(super) vault: PathBuf,
    pub(super) work: PathBuf,
    pub(super) _d: tempfile::TempDir,
    pub(super) _w: tempfile::TempDir,
}

/// The settings the run form is played under: a work folder that `proc.run` runs in, and
/// `outside`. `tweak` says more.
pub(super) fn settings(
    c: &mut crate::Config,
    work: &Path,
    outside: OutsideMode,
    tweak: &dyn Fn(&mut crate::Config),
) {
    c.content_based_security.outside = outside;
    c.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    c.tools.roots = vec![];
    c.tools.proc_sync_secs = 10;
    c.policy.tools.remove("proc.run");
    tweak(c);
}

/// A daemon with the stub vault and the script `call_lantern.sh` in its work folder, which
/// counts its variable (`body`).
pub(super) fn rig_once_with(
    outside: OutsideMode,
    body: &str,
    tweak: impl Fn(&mut crate::Config) + Send + 'static,
) -> Once {
    let w = tempfile::tempdir().unwrap();
    let work = w.path().canonicalize().unwrap();
    let at = work.clone();
    let r = rig_with(move |c, _| settings(c, &at, outside, &tweak));
    let d = tempfile::tempdir().unwrap();
    r.core.tools.look.set_vault(Arc::new(stub(d.path(), "ok")));
    script(&work, SCRIPT, body);
    // The owner's own typed turns, as `turn.submit` judges them.
    r.owner_types();
    Once {
        r,
        vault: d.path().to_path_buf(),
        work,
        _d: d,
        _w: w,
    }
}

pub(super) fn rig_once() -> Once {
    rig_once_with(OutsideMode::default(), COUNTS, |_| {})
}

/// An executable shell script `name` in `dir`, run by its own first line.
pub(super) fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

pub(super) fn run_call(program: &str) -> Value {
    json!({"vault": "Harbor", "item": "Lantern key", "field": "credential",
           "run": {"program": program, "variable": VAR},
           "reason": "run the lantern script with its key once"})
}

pub(super) fn proc(argv: Value) -> Value {
    json!({ "argv": argv })
}

pub(super) fn script_run() -> (&'static str, Value) {
    ("proc_run", proc(json!(["./call_lantern.sh"])))
}

/// What each run of the script counted, in order.
pub(super) fn counted(o: &Once) -> Vec<String> {
    std::fs::read_to_string(o.work.join("got.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// The FYI notes the owner was told of a run form.
pub(super) fn notes(r: &Rig) -> Vec<String> {
    r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter_map(|a| {
            crate::outbox::body_of(a)["text"]
                .as_str()
                .map(str::to_string)
        })
        .filter(|t| t.starts_with("ℹ️ FYI") && t.contains("for one run"))
        .collect()
}

/// Every board entry a run form held.
pub(super) fn held(r: &Rig) -> Vec<String> {
    r.core
        .secrets
        .states()
        .keys()
        .filter(|k| k.starts_with("vault-use:"))
        .cloned()
        .collect()
}

/// Every exit the value could reach: each node and ledger row of `sid`, every request the
/// model was sent, every note and notice to the owner, every question, the board's status and
/// the broker's.
pub(super) fn exits(r: &Rig, sid: &str) -> String {
    let requests: Vec<String> = r
        .sent()
        .iter()
        .map(|q| {
            format!(
                "{}\n{}",
                serde_json::to_string(&q.tools).unwrap(),
                serde_json::to_string(&q.messages).unwrap()
            )
        })
        .collect();
    let told: Vec<String> = r
        .core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .map(|a| crate::outbox::body_of(a).to_string())
        .collect();
    format!(
        "{}\n{}\n{}\n{:?}\n{:?}\n{}",
        everything(r, sid),
        requests.join("\n"),
        told.join("\n"),
        r.asked(),
        r.core.tools.broker.status(),
        serde_json::to_string(&r.core.secrets.status()).unwrap_or_default(),
    )
}

/// "Run my lantern script with the key in the vault": one step, the next run of the script has
/// the variable, no card and no code, the settings untouched, one FYI note naming the item, the
/// script and the variable and none of the model's words.
#[tokio::test]
async fn a_run_use_is_one_step_with_no_card_and_no_code() {
    let o = rig_once();
    let r = &o.r;
    let sid = r.session();
    r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let (status, text) = r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains(VAR) && text.contains(SCRIPT), "{text}");
    assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
    assert_eq!(r.file_text(), crate::config_change::tests::FILE);
    let (status, text) = r.result_of(&sid, "p2");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(counted(&o), [VALUE.len().to_string()], "the run had it");
    let told = notes(r);
    assert_eq!(told.len(), 1, "{told:?}");
    let short = crate::narrative::short(&sid);
    for part in [
        "\"Lantern key\"",
        "\"credential\"",
        "\"Harbor\"",
        SCRIPT,
        VAR,
        short.as_str(),
        "ready for one run of",
        "nothing was written to the settings",
        "by name only",
        "reads the vault again",
    ] {
        assert!(told[0].contains(part), "{part} in {}", told[0]);
    }
    assert!(!told[0].contains("run the lantern script"), "{}", told[0]);
    // The daemon's own steps: lists, one get, one read; no second read for the run.
    assert_eq!(
        logged(&o.vault),
        [
            "vault list --format json",
            "item list --format json --vault vaultid00000000000000000001",
            "item get itemid00000000000000000001 --vault vaultid00000000000000000001 --format json",
            "read --no-newline op://Harbor/Lantern key/credential"
        ]
    );
}

/// The run form takes neither a name in the settings nor other settings: those are the card's.
#[tokio::test]
async fn the_run_form_takes_no_name_and_no_also() {
    let o = rig_once();
    let r = &o.r;
    for (key, v) in [("name", json!("lantern_key")), ("also", json!([]))] {
        let sid = r.session();
        let mut call = run_call("./call_lantern.sh");
        call[key] = v;
        r.turn_with("vault_use", &sid, call).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{key}: {text}");
        assert!(text.contains(key), "{key}: {text}");
    }
    assert!(logged(&o.vault).is_empty(), "nothing read");
    assert!(held(r).is_empty());
}

/// A script that prints its key: the value is in no exit, before a restart and after it.
#[tokio::test]
async fn the_value_is_in_no_exit() {
    let tweak = |_: &mut crate::Config| {};
    let o = rig_once_with(OutsideMode::default(), PRINTS, tweak);
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    assert_eq!(counted(&o), [VALUE.len().to_string()], "the run had it");
    let all = exits(&o.r, &sid);
    assert!(!all.contains(VALUE), "the value reached an exit");
    let Once {
        r, work, _d, _w, ..
    } = o;
    let r = r.restart(move |c, _| settings(c, &work, OutsideMode::default(), &tweak));
    let all = exits(&r, &sid);
    assert!(
        !all.contains(VALUE),
        "the value reached an exit after a restart"
    );
}

/// The printed key: the model reads the scrubber's marker, and the stored result holds the
/// marker too, so a restart (an empty board) leaves nothing to mask.
#[tokio::test]
async fn a_printed_key_is_masked_in_what_the_model_reads() {
    let tweak = |_: &mut crate::Config| {};
    let o = rig_once_with(OutsideMode::default(), PRINTS, tweak);
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let stored = |r: &Rig| -> String {
        r.core
            .store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match n.body {
                Body::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } if tool_use_id == "p2" => Some(content),
                _ => None,
            })
            .expect("the run's result")
    };
    let text = stored(&o.r);
    assert!(text.contains("token=[redacted:"), "{text}");
    assert!(!text.contains(VALUE), "{text}");
    let read =
        o.r.sent()
            .iter()
            .map(|q| serde_json::to_string(&q.messages).unwrap())
            .collect::<String>();
    assert!(
        read.contains("token=[redacted:"),
        "the model read the marker"
    );
    assert!(!read.contains(VALUE));
    let Once {
        r, work, _d, _w, ..
    } = o;
    let r = r.restart(move |c, _| settings(c, &work, OutsideMode::default(), &tweak));
    assert!(held(&r).is_empty(), "nothing held after a restart");
    let text = stored(&r);
    assert!(!text.contains(VALUE), "after a restart: {text}");
}

/// A guest's words read since the owner's last message: the run form is asked at the gate
/// before the vault is read, at both settings, with Approve and Decline only; Approve runs it.
#[tokio::test]
async fn after_outside_text_it_asks_first_at_both_settings() {
    for outside in [OutsideMode::Notify, OutsideMode::Ask] {
        let o = rig_once_with(outside, COUNTS, |_| {});
        let r = &o.r;
        let sid = r.session();
        crate::tests_security::history_exchange(
            &r.core,
            "ses_guest_lantern",
            "turn_guest_lantern",
            ("discord:channel:77", "discord:7"),
            "a guest asks to run the lantern script",
        );
        r.steps(
            &sid,
            vec![
                ("context_history", json!({"open": "turn_guest_lantern"})),
                ("vault_use", run_call("./call_lantern.sh")),
            ],
        )
        .await;
        let asks = r.asked();
        assert_eq!(asks.len(), 1, "{outside:?}: {asks:?}");
        assert_eq!(asks[0].tool, USE, "{outside:?}");
        assert_eq!(asks[0].grant_offer, None, "{outside:?}: two answers only");
        assert!(
            asks[0].reason.contains(crate::config_change::OUTSIDE_HOLD),
            "{outside:?}: {}",
            asks[0].reason
        );
        assert!(logged(&o.vault).is_empty(), "{outside:?}: nothing read yet");
        assert!(held(r).is_empty(), "{outside:?}");
        r.core
            .confirm_action(&asks[0].correlation_id, true, None, "cli")
            .unwrap();
        r.try_continue(&sid).await.unwrap();
        let (status, text) = r.result_of(&sid, "p2");
        assert_eq!(status, ResultStatus::Ok, "{outside:?}: {text}");
        assert_eq!(held(r).len(), 1, "{outside:?}: approved, it ran");
    }
}

/// The wall as it is for the card form: a job's, the self loop's and an MCP client's
/// executions, a task's session and a shared place are refused in the wall's words, and a
/// routine never has the tool.
#[tokio::test]
async fn nobody_but_the_owners_own_turn_may() {
    use theseus_kernel::Authority;
    assert!(crate::routine::grant::NEVER.contains(&USE));
    let r = crate::config_change::tests::rig_with(|c, _| {
        c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
    });
    let d = tempfile::tempdir().unwrap();
    r.core.tools.look.set_vault(Arc::new(stub(d.path(), "ok")));
    let mut sessions = Vec::new();
    for principal in [
        "job",
        crate::rsi::backlog::PRINCIPAL,
        crate::mcp_server::PRINCIPAL,
    ] {
        let sid = r.session();
        let rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
        let e = r
            .core
            .kernel
            .open_execution(
                &sid,
                rec.kind,
                Authority {
                    principal: principal.into(),
                    ..Default::default()
                },
                None,
                None,
            )
            .unwrap();
        r.core
            .store
            .update_session(&sid, |rec: &mut crate::session::SessionRecord| {
                rec.execution_id = Some(e.id.clone());
                Ok(vec![])
            })
            .unwrap();
        sessions.push((principal.to_string(), sid));
    }
    let parent = crate::config_change::tests_work_global::conversation(&r).await;
    let task = crate::config_change::tests_work_global::task(&r, &parent, "Chart the reef", None);
    sessions.push(("task".into(), task.session_id.clone()));
    sessions.push(("shared".into(), r.session_at("channel:2")));
    for (who, sid) in sessions {
        r.turn_with("vault_use", &sid, run_call("./call_lantern.sh"))
            .await;
        if let Some(q) = r.asked().into_iter().find(|q| q.session_id == sid) {
            r.core
                .confirm_action(&q.correlation_id, true, None, "cli")
                .unwrap();
            r.try_continue(&sid).await.unwrap();
        }
        let results: Vec<(String, ResultStatus, String)> = r
            .core
            .store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match n.body {
                Body::ToolResult {
                    tool,
                    status,
                    content,
                    ..
                } => Some((tool, status, content)),
                _ => None,
            })
            .collect();
        assert!(!results.is_empty(), "{who}: the call was answered");
        for (tool, status, text) in &results {
            assert_ne!(*status, ResultStatus::Ok, "{who}: {tool}: {text}");
            if tool == USE {
                assert!(text.contains("vault.use is "), "{who}: {text}");
            }
        }
    }
    assert!(logged(d.path()).is_empty(), "the vault was not read");
    assert!(held(&r).is_empty());
}

/// A turn a wake began in the owner's own private place is asked first, before the vault is
/// read: the driver runs the due wake's turn, whose note the turn's catch-up writes as a harness
/// message, and the owner's own message did not begin it.
#[tokio::test]
async fn a_turn_a_wake_began_is_asked_first() {
    use theseus_kernel::{NewWake, Purpose, WakeRun};
    let o = rig_once();
    let r = &o.r;
    let sid = r.session();
    // The owner's own turn first: it gives the session its execution, and asks nothing.
    r.run_turn(&sid).await;
    let exec = r
        .core
        .store
        .get_session::<crate::session::SessionRecord>(&sid)
        .unwrap()
        .unwrap()
        .execution_id
        .unwrap();
    r.core
        .kernel
        .create_wake(
            &exec,
            None,
            NewWake {
                due_at_ms: r.core.kernel.now_ms() + 100,
                note: "run the lantern script".into(),
                target: None,
                repeat: None,
                purpose: Purpose::Check,
                run: WakeRun::Turn,
            },
            "the CLI",
        )
        .unwrap();
    r.call_next("vault_use", run_call("./call_lantern.sh"));
    r.drive();
    let asked = || -> Vec<theseus_protocol::ConfirmRequest> {
        r.asked()
            .into_iter()
            .filter(|q| q.session_id == sid)
            .collect()
    };
    for _ in 0..200 {
        if !asked().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let asks = asked();
    assert_eq!(asks.len(), 1, "the wake's use is asked: {asks:?}");
    assert_eq!(asks[0].tool, USE);
    assert_eq!(asks[0].grant_offer, None);
    assert!(
        asks[0].reason.contains(once::NOT_BEGUN),
        "{}",
        asks[0].reason
    );
    let wake = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .any(|(_, n)| {
            n.origin == Origin::Harness
                && n.author.as_deref().is_some_and(|a| a.starts_with("wake:"))
        });
    assert!(wake, "the wake's note is a harness message");
    assert!(logged(&o.vault).is_empty(), "the vault was not read");
    assert!(held(r).is_empty());
}

/// A read that fails: an error by class, nothing held, no grant, no note.
#[tokio::test]
async fn nothing_is_held_or_granted_when_the_vault_cannot_read_the_item() {
    let o = rig_once();
    let r = &o.r;
    r.core
        .tools
        .look
        .set_vault(Arc::new(stub(&o.vault, "fail-read")));
    let sid = r.session();
    r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let (status, text) = r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("refused the daemon's token"), "{text}");
    assert!(held(r).is_empty());
    assert!(notes(r).is_empty());
    assert!(r.rows("secret.granted").is_empty());
    assert_eq!(counted(&o), ["0"], "the run had nothing");
}

/// One grant is one run of one program in one turn: a shell, a wrapper, an interpreter-style
/// run, a call that sets the environment and another file of the same name get nothing and
/// leave it; the script's own run takes it; a second run and a later turn get nothing; and a
/// run in the background is not given it, the row saying why.
#[tokio::test]
async fn the_grant_is_one_run_of_one_program() {
    let o = rig_once();
    let r = &o.r;
    std::fs::create_dir_all(o.work.join("other")).unwrap();
    script(&o.work.join("other"), SCRIPT, COUNTS);
    let sid = r.session();
    r.steps(
        &sid,
        vec![
            ("vault_use", run_call("./call_lantern.sh")),
            ("proc_run", proc(json!(["bash", "-c", "./call_lantern.sh"]))),
            ("proc_run", proc(json!(["sh", "./call_lantern.sh"]))),
            ("proc_run", proc(json!(["env", "A=1", "./call_lantern.sh"]))),
            // A call may not set PATH at all: the run is refused, and gets nothing.
            (
                "proc_run",
                json!({"argv": ["./call_lantern.sh"], "env": {"PATH": "/usr/bin:/bin"}}),
            ),
            (
                "proc_run",
                json!({"argv": ["./call_lantern.sh"], "env": {"LANG": "C"}}),
            ),
            ("proc_run", proc(json!(["./other/call_lantern.sh"]))),
            script_run(),
            script_run(),
        ],
    )
    .await;
    let (status, text) = r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let n = VALUE.len().to_string();
    assert_eq!(counted(&o), ["0", "0", "0", "0", "0", n.as_str(), "0"]);
    r.steps(&sid, vec![script_run()]).await;
    assert_eq!(
        counted(&o).last().map(String::as_str),
        Some("0"),
        "a later turn"
    );
    assert_eq!(r.rows("secret.granted").len(), 1);
    // In the background: not given, and the row says why.
    let sid = r.session();
    r.steps(
        &sid,
        vec![
            ("vault_use", run_call("./call_lantern.sh")),
            (
                "proc_run",
                json!({"argv": ["./call_lantern.sh"], "background": true}),
            ),
        ],
    )
    .await;
    let withheld = r.rows("secret.withheld");
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    assert!(
        withheld[0]["why"]
            .as_str()
            .is_some_and(|w| w.contains("background")),
        "{withheld:?}"
    );
    assert_eq!(r.rows("secret.granted").len(), 1, "no second grant");
}

/// An item Theseus itself runs on is not given to a program for one run: by what reads its name
/// (the main model's key, Discord's by its default name) and by value (the same value kept under
/// another item). The answer says, in fixed words, that it goes by the card. A key only a
/// program's grant reads is not Theseus's own, and is given.
#[tokio::test]
async fn an_item_theseus_runs_on_keeps_its_card() {
    const OWN: &str = "op://Harbor/Lantern key/credential";
    let cases: Vec<(&str, Tweak)> = vec![
        (
            "the main model's key",
            Box::new(|c: &mut crate::Config| {
                c.secrets.insert("lantern_key".into(), OWN.into());
                c.model.api_key_secret = "lantern_key".into();
            }),
        ),
        (
            "discord's default name",
            Box::new(|c: &mut crate::Config| {
                let name = c.discord.token_secret.clone();
                c.secrets.insert(name, OWN.into());
            }),
        ),
    ];
    for (what, tweak) in cases {
        let o = rig_once_with(OutsideMode::default(), COUNTS, tweak);
        let sid = o.r.session();
        o.r.steps(
            &sid,
            vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
        )
        .await;
        let (status, text) = o.r.result_of(&sid, "p1");
        assert_eq!(status, ResultStatus::Error, "{what}: {text}");
        assert!(text.contains(RUNS_ON_WORDS), "{what}: {text}");
        assert!(text.contains("`vault.use` with `name`"), "{what}: {text}");
        assert!(held(&o.r).is_empty(), "{what}");
        assert!(notes(&o.r).is_empty(), "{what}");
        assert_eq!(counted(&o), ["0"], "{what}");
    }
    // By value: the main model's key, read from another item, has the same value.
    let o = rig_once_with(OutsideMode::default(), COUNTS, |c| {
        c.secrets
            .insert("model_key".into(), "op://Dock/Other/credential".into());
        c.model.api_key_secret = "model_key".into();
    });
    o.r.core
        .secrets
        .hold("model_key", crate::secrets::Secret::new(VALUE.into()));
    let sid = o.r.session();
    o.r.turn_with("vault_use", &sid, run_call("./call_lantern.sh"))
        .await;
    let (status, text) = o.r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains(RUNS_ON_WORDS), "{text}");
    // A vendor's key that only a program's grant reads is given.
    let o = rig_once_with(OutsideMode::default(), COUNTS, |c| {
        c.secrets.insert("vendor_key".into(), OWN.into());
        c.broker.programs.insert(
            "lantern-cli".into(),
            crate::config::ProgramGrant {
                env: [("TOKEN".to_string(), "vendor_key".to_string())].into(),
                aws_account: None,
            },
        );
    });
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let (status, text) = o.r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(counted(&o), [VALUE.len().to_string()]);
}

/// The grant's row names the item as the vault listed it and marks the grant as one run's;
/// never the value.
#[tokio::test]
async fn the_granted_row_names_the_item_and_never_the_value() {
    let o = rig_once();
    let r = &o.r;
    let sid = r.session();
    r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let rows = r.rows("secret.granted");
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(row["once"], json!(true), "{row}");
    assert_eq!(
        row["vault"],
        json!({"vault": "Harbor", "item": "Lantern key", "field": "credential"}),
        "{row}"
    );
    assert_eq!(row["variable"], json!(VAR));
    assert_eq!(row["program"], json!(SCRIPT));
    assert!(!row.to_string().contains(VALUE));
}
