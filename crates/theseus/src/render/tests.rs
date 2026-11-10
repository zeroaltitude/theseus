use super::*;
use theseus_protocol::notify;

/// `theseus wake` and health's wakes line (DD8).
/// A task's pill takes the place of its state and what it waits on
/// (theseus-in3); a daemon that sends none keeps the old words.
#[test]
fn a_tasks_line_shows_its_attention() {
    let mut t = theseus_protocol::TaskInfo {
        short: "a1b2c3".into(),
        state: "waiting".into(),
        waiting_on: Some("a job".into()),
        limit_usd: 2.0,
        turns: 1,
        created_at_ms: 1_000,
        ..Default::default()
    };
    let line = task_line(&t, 61_000);
    assert!(line
        .text
        .starts_with("a1b2c3\twaiting on a job\t$0.0000 of $2.00"));
    assert_eq!(line.tag, Tag::Plain);
    t.attention = Some(Attention {
        level: Level::Working,
        label: "waiting on 1 call".into(),
        since_ms: 0,
    });
    let line = task_line(&t, 61_000);
    assert!(
        line.text.starts_with("a1b2c3\t◐ waiting on 1 call\t"),
        "{}",
        line.text
    );
    assert_eq!(line.tag, Tag::Level(Level::Working));
    assert_eq!(glyph(Level::NeedsYou), '●');
    assert_eq!(glyph(Level::Idle), '·');
}

/// `theseus health`'s context line (theseus-c48).
#[test]
fn the_context_line_names_the_levels_and_the_persona_in_play() {
    use theseus_protocol::ContextStatus;
    assert_eq!(context_line(&ContextStatus::default()), None);
    let c = ContextStatus {
        system_files: vec!["~/a.md".into(), "~/b.md".into()],
        persona: Some("theseus".into()),
        persona_files: vec!["~/p.md".into()],
        personas: vec!["theseus".into()],
        ..Default::default()
    };
    assert_eq!(
        context_line(&c).unwrap(),
        "context: 2 files at the system level · persona theseus (1 file)"
    );
    let none = ContextStatus {
        persona: None,
        persona_files: vec![],
        ..c
    };
    assert_eq!(
        context_line(&none).unwrap(),
        "context: 2 files at the system level · no persona in play (defined: theseus; set \
         [context] default_persona)"
    );
}

fn tool(name: &str, policy: &str, config: &str, setting: &str) -> theseus_protocol::ToolInfo {
    theseus_protocol::ToolInfo {
        name: name.into(),
        wire_name: name.replace('.', "_"),
        family: String::new(),
        description: String::new(),
        class: "run".into(),
        backend: "job".into(),
        policy: policy.into(),
        setting: setting.into(),
        config_posture: config.into(),
        config_setting: "enforcement = notify".into(),
        tightened: None,
        input_schema: Value::Null,
        calls: 0,
        invalid_json: 0,
    }
}

/// The kernel line counts the job wrappers that linger, and says nothing
/// when none does (theseus-6qy).
#[test]
fn the_kernel_line_counts_lingering_wrappers() {
    assert_eq!(lingering_note(0), "");
    assert_eq!(
        lingering_note(1),
        " · 1 job wrapper lingers for what its command left running"
    );
    assert_eq!(
        lingering_note(3),
        " · 3 job wrappers linger for what their commands left running"
    );
}

/// The broker line names each grant and its uses, never a value
/// (theseus-dcy), and says when there is none.
#[test]
fn the_binary_line_says_only_when_jobs_can_write_it() {
    let b = |state: &str| theseus_protocol::BinaryStatus {
        path: "/home/invented/.local/bin/theseusd".into(),
        state: state.into(),
        detail: "its directory is writable by this daemon's user".into(),
    };
    assert_eq!(binary_line(&b("ok")), None);
    assert_eq!(
        binary_line(&b("jobs_can_write")).unwrap(),
        "binary: JOBS CAN WRITE /home/invented/.local/bin/theseusd: its directory is writable \
         by this daemon's user (run the builder as its own user: theseusd install --separate)"
    );
    assert_eq!(
        binary_line(&theseus_protocol::BinaryStatus::default()),
        None
    );
}

#[test]
fn the_broker_line_names_each_grant_and_its_uses() {
    use theseus_protocol::GrantStatus;
    assert_eq!(broker_line(&[]), "broker: no grants");
    let gh = GrantStatus {
        kind: "program".into(),
        to: "gh".into(),
        variable: Some("GH_TOKEN".into()),
        secret: "github_token".into(),
        posture: "notify".into(),
        uses: 3,
    };
    let search = GrantStatus {
        kind: "tool".into(),
        to: "web.search".into(),
        variable: None,
        secret: "brave_api_key".into(),
        posture: "notify".into(),
        uses: 1,
    };
    assert_eq!(
        broker_line(&[gh, search]),
        "broker: gh gets GH_TOKEN (github_token, notify), used 3 times · web.search gets \
         brave_api_key (brave_api_key, notify), used once"
    );
}

/// The children line says what the daemon holds and has reaped, and
/// nothing for a daemon older than theseus-z4b.
#[test]
fn the_disk_line_says_free_space_and_its_limits() {
    use theseus_protocol::DiskStatus;
    assert_eq!(disk_line(&DiskStatus::default()), None, "an older daemon");
    let mut d = DiskStatus {
        path: "/invented/state".into(),
        state: "ok".into(),
        free_mb: 81_920,
        total_mb: 1_006_712,
        warn_mb: 5120,
        floor_mb: 1024,
        error: None,
    };
    assert_eq!(
        disk_line(&d).unwrap(),
        "disk: 81,920 MB free of 1,006,712 MB under /invented/state · health warns below \
         5,120 MB, jobs are refused below 1,024 MB"
    );
    d.state = "low".into();
    d.free_mb = 4000;
    assert!(disk_line(&d)
        .unwrap()
        .starts_with("disk: LOW, 4,000 MB free"));
    d.state = "below_floor".into();
    d.free_mb = 812;
    d.warn_mb = 0;
    assert_eq!(
        disk_line(&d).unwrap(),
        "disk: BELOW THE FLOOR, new jobs are refused: 812 MB free of 1,006,712 MB under \
         /invented/state · jobs are refused below 1,024 MB"
    );
    d.state = "unknown".into();
    d.error = Some("invented failure".into());
    assert_eq!(
        disk_line(&d).unwrap(),
        "disk: unknown under /invented/state: invented failure"
    );
}

#[test]
fn the_spool_line_says_the_last_sweep() {
    use theseus_protocol::{SpoolStatus, SpoolSweep};
    assert_eq!(spool_line(&SpoolStatus::default(), 0), None);
    let sweep = SpoolSweep {
        at_unix_ms: 1_000_000,
        took_ms: 2,
        removed: 3,
        removed_bytes: 1_024,
        kept: 2,
        kept_bytes: 10,
        removed_by: [("absorbed", 1), ("ended", 1), ("unknown", 1)]
            .map(|(k, v)| (k.to_string(), v))
            .into(),
        kept_by: [("pending", 1), ("running", 1)]
            .map(|(k, v)| (k.to_string(), v))
            .into(),
    };
    let s = SpoolStatus {
        last_sweep: Some(sweep),
    };
    assert_eq!(
        spool_line(&s, 1_000_000 + 180_000).unwrap(),
        "spool: swept 3 min ago: removed 3 files (1,024 bytes): 1 absorbed, 1 ended, 1 unknown \
         · kept 2: 1 pending, 1 running"
    );
    assert!(spool_line(&s, 1_005_000)
        .unwrap()
        .starts_with("spool: swept 5 s ago"));
}

/// Health's `aws:` line per bound account (row 29, C1): bound with its
/// identity, not bound with why, waiting for its key; and an AWS call's
/// line, its operation, region, account, and resources.
#[test]
fn the_aws_lines_say_each_account_and_each_call() {
    use theseus_protocol::{AwsAccountStatus, AwsPlan, AwsStatus};
    assert!(aws_lines(None, 0).is_empty(), "no account bound");
    let account = |state: &str, error: Option<&str>| AwsAccountStatus {
        account: "111122223333".into(),
        region: "us-west-2".into(),
        regions: vec!["us-west-2".into(), "us-east-1".into()],
        state: state.into(),
        arn: (state == "bound").then(|| "arn:aws:iam::111122223333:user/example".into()),
        checked_at_unix_ms: (state != "waiting").then_some(1_000_000),
        error: error.map(String::from),
        calls: 1_204,
        failed: if state == "bound" { 0 } else { 1 },
        ..Default::default()
    };
    let s = AwsStatus {
        accounts: vec![
            account("bound", None),
            account(
                "failed",
                Some("its key is account 444455556666's, not 111122223333"),
            ),
            account(
                "waiting",
                Some("its secret aws_access_key_id did not resolve"),
            ),
        ],
        ..Default::default()
    };
    assert_eq!(
        aws_lines(Some(&s), 1_012_000),
        [
            "aws: 111122223333 bound (arn:aws:iam::111122223333:user/example) 12 s ago · \
             us-west-2 (may name us-east-1) · 1,204 requests",
            "aws: 111122223333 NOT BOUND 12 s ago: its key is account 444455556666's, not \
             111122223333; its calls fail closed · us-west-2 (may name us-east-1) · 1,204 \
             requests, 1 failed",
            "aws: 111122223333 waiting for its key: its secret aws_access_key_id did not \
             resolve · us-west-2 (may name us-east-1) · 1,204 requests, 1 failed",
        ]
    );
    let call = AwsPlan {
        account: "111122223333".into(),
        region: "us-west-2".into(),
        service: "s3".into(),
        operation: "ListObjectsV2".into(),
        cost_bearing: false,
        resources: vec!["example-bucket".into(), "logs/".into()],
        ..Default::default()
    };
    assert_eq!(
        aws_call_line(&call),
        "s3:ListObjectsV2 · us-west-2 · account 111122223333 · example-bucket, logs/"
    );
}

#[test]
fn the_children_line_counts_wrappers_orphans_and_zombies() {
    use theseus_protocol::ChildrenStatus;
    assert_eq!(children_line(&ChildrenStatus::default()), None);
    let mut c = ChildrenStatus {
        subreaper: true,
        wrappers_running: 2,
        wrappers_lingering: 1,
        orphans: 1,
        zombies: 0,
        owned: 1,
        reaped_wrappers: 160,
        reaped_orphans: 2,
        tenders: vec![],
    };
    assert_eq!(
        children_line(&c).unwrap(),
        "children: 2 job wrappers running, 1 lingering · 1 adopted orphan · 0 zombies · \
         reaped 160 wrappers, 2 orphans"
    );
    // The index tender (row 51): running, then in its backoff.
    c.tenders = vec![theseus_protocol::TenderStatus {
        name: "index".into(),
        state: "running".into(),
        pid: Some(4242),
        restarts: 1,
        ..Default::default()
    }];
    assert!(children_line(&c)
        .unwrap()
        .ends_with("reaped 160 wrappers, 2 orphans · index tender running (pid 4242, 1 restart)"));
    c.tenders[0].state = "backoff".into();
    c.tenders[0].pid = None;
    c.tenders[0].adopted = true;
    c.tenders[0].last_exit = Some("signal 9".into());
    c.tenders[0].backoff_ms = 2000;
    assert!(children_line(&c).unwrap().ends_with(
        " · index tender in backoff (taken over after a restart, signal 9, next wait 2 s, \
         1 restart)"
    ));
    let stdio = ChildrenStatus {
        reaped_wrappers: 1,
        ..ChildrenStatus::default()
    };
    assert_eq!(
        children_line(&stdio).unwrap(),
        "children: 0 job wrappers running · 0 adopted orphans · 0 zombies · reaped 1 \
         wrapper, 0 orphans · not a subreaper: a job that kills its wrapper leaves its \
         orphans to init"
    );
}

/// A call a `/stop` ended reads as stopped, by whom, in `theseus watch`
/// and the history, never as `cancelled`; a cancel's call still reads
/// cancelled, and a decline not run (theseus-4uw).
#[test]
fn a_call_a_stop_ended_reads_stopped_in_watch_and_history() {
    let ended = |v: Value| serde_json::from_value::<ToolEnded>(v).unwrap();
    let stopped = ended(
        serde_json::json!({"tool": "proc.run", "status": "cancelled",
        "duration_ms": 2100, "bytes": 40, "stopped_by": "the CLI"}),
    );
    assert_eq!(
        tool_ended_line(&stopped),
        "  ← proc.run stopped by the CLI · 2100 ms · 40 B"
    );
    let cancelled = ended(
        serde_json::json!({"tool": "proc.run", "status": "cancelled",
        "duration_ms": 5, "bytes": 0}),
    );
    assert_eq!(
        tool_ended_line(&cancelled),
        "  ← proc.run cancelled · 5 ms · 0 B"
    );
    assert_eq!(
        result_word("ok", Some("the CLI")),
        "ok",
        "it finished first"
    );
    assert_eq!(result_word("declined", None), "not run");
}

/// `theseus policy list` says what set each posture, and marks a
/// tightening with who, when, and the call; `tighten` and `untighten`
/// say what changed (theseus-sgh).
#[test]
fn policy_list_and_its_lines_say_what_set_each_posture() {
    let t = theseus_protocol::Tightening {
        tool: "proc.run".into(),
        posture: "approve".into(),
        by: "discord:zeroaltitude".into(),
        at_ms: 3_600_000,
        correlation_id: Some("act_1".into()),
        ..Default::default()
    };
    let mut run = tool(
        "proc.run",
        "approve",
        "notify",
        "tightened by discord:zeroaltitude",
    );
    run.tightened = Some(t.clone());
    let l = ToolListResult {
        tools: vec![
            run,
            tool("fs.write", "notify", "notify", "enforcement = notify"),
        ],
        roots: vec![],
        shell_fallback_ratio: 0.0,
        calls_total: 0,
    };
    let gone = theseus_protocol::Tightening {
        tool: "mcp:x/y".into(),
        ..t.clone()
    };
    let out = policy_list(&l, &[t.clone(), gone]);
    assert!(
        out.contains(
            "proc.run     approve  tightened by discord:zeroaltitude at 18:00:00.000, from act_1 \
             (the config says notify, enforcement = notify)"
        ),
        "{out}"
    );
    assert!(
        out.contains("fs.write     notify   enforcement = notify"),
        "{out}"
    );
    assert!(
        out.contains("mcp:x/y      approve  tightened by discord:zeroaltitude"),
        "{out}"
    );
    assert!(out.ends_with("2 tightenings · undo one with `theseus policy untighten <tool>`\n"));
    let r = theseus_protocol::TightenResult {
        tool: "proc.run".into(),
        by: "sock#3".into(),
        tightening: t,
        posture: "notify".into(),
        setting: "enforcement = notify".into(),
        config_posture: "notify".into(),
        config_setting: "enforcement = notify".into(),
        changed: true,
        already: false,
    };
    assert_eq!(
        tightened_line(&r, false),
        "proc.run is back to what the config says: notify (enforcement = notify); the \
         tightening by discord:zeroaltitude is undone"
    );
    assert!(tightened_line(&r, true).starts_with("proc.run now asks first: tightened by sock#3"));
}

/// `theseus health` says where the secrets stand and names a failure
/// with its reason; the startup line splits the start path from what
/// follows it (theseus-qa0).
#[test]
fn health_says_resolving_ready_or_failed_and_times_the_start() {
    use theseus_protocol::{SecretFailed, SecretsStatus, StartupPhase};
    let resolving = SecretsStatus {
        state: "resolving".into(),
        resolving: vec!["a".into(), "b".into()],
        ..Default::default()
    };
    assert_eq!(
        secrets_line(&resolving, &[]),
        "secrets: resolving · 0 of 2 ready so far"
    );
    let failed = SecretsStatus {
        state: "failed".into(),
        ready: vec!["a".into()],
        failed: vec![SecretFailed {
            name: "b".into(),
            error: "could not find item".into(),
        }],
        method: Some("inject, then read".into()),
        settled_ms: Some(1720),
        retry_in_ms: Some(4000),
        ..Default::default()
    };
    assert_eq!(
        secrets_line(&failed, &[]),
        "secrets: failed b · 1 ready 1720 ms after start (inject, then read) [a]\n  b did not \
         resolve: could not find item\n  fetched again in 4 s"
    );
    assert_eq!(
        secrets_line(&SecretsStatus::default(), &["a".into()]),
        "secrets [a]",
        "an older daemon"
    );
    // theseus-n88g.1: the secrets from outside the vault, with their sources.
    let local = SecretsStatus {
        state: "ready".into(),
        ready: vec!["anthropic_api_key".into(), "db".into()],
        method: Some("local".into()),
        settled_ms: Some(2),
        outside_vault: vec![
            theseus_protocol::SecretSource {
                name: "anthropic_api_key".into(),
                kind: "env".into(),
            },
            theseus_protocol::SecretSource {
                name: "db".into(),
                kind: "file".into(),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        secrets_line(&local, &[]),
        "secrets: ready · 2 ready 2 ms after start (local) [anthropic_api_key, db]\n  outside \
         the vault: anthropic_api_key (env), db (file)"
    );
    let phase = |name: &str, bg: bool, start: u64, end: Option<u64>| StartupPhase {
        name: name.into(),
        background: bg,
        start_us: start,
        end_us: end,
        detail: Value::Null,
    };
    let line = startup_line(&[
        phase("config", false, 100, Some(900)),
        phase("socket", false, 11_000, Some(11_400)),
        phase("secrets", true, 1000, None),
    ])
    .unwrap();
    // No phase names the 10.1 ms from config's end to the socket's
    // start, nor the 0.1 ms before config: 10.2 ms in all.
    assert_eq!(
        line,
        "startup: serving at 11.4 ms (config 800 µs · socket 400 µs · 10.2 ms between phases) \
         · after: secrets running"
    );
    assert!(startup_line(&[]).is_none());
}

/// `store: CORRUPT …` in `theseus health` when the history check after
/// serving (theseus-8ni) found a frame that does not check; nothing while
/// it runs or when the history is whole.
#[test]
fn health_is_loud_when_the_store_history_is_corrupt() {
    let verify = |detail: Value, end: Option<u64>| theseus_protocol::StartupPhase {
        name: "store.verify".into(),
        background: true,
        start_us: 1000,
        end_us: end,
        detail,
    };
    assert!(store_history_line(&[verify(Value::Null, None)]).is_none());
    assert!(store_history_line(&[verify(serde_json::json!({"outcome": "ok"}), Some(9))]).is_none());
    let line = store_history_line(&[verify(
        serde_json::json!({"outcome": "corrupt", "error": "corrupt frame in segment 1 at offset 0: crc mismatch"}),
        Some(9),
    )])
    .unwrap();
    assert!(
        line.starts_with("store: CORRUPT history, corrupt frame in segment 1 at offset 0"),
        "{line}"
    );
}

/// `config:` in `theseus health` (theseus-2fo): a file, a read before
/// serving, confirming, confirmed from the copy, and held after a restart
/// onto a changed note, acting on the copy (theseus-zmgb).
#[test]
fn health_says_where_the_config_came_from_and_what_the_vault_said() {
    use theseus_protocol::{ConfigRestart, ConfigStatus};
    let vault = |state: &str, from: &str| ConfigStatus {
        source: "vault".into(),
        reference: "op://V/c/notesPlain".into(),
        state: state.into(),
        started_from: from.into(),
        ..Default::default()
    };
    assert!(
        config_line(&ConfigStatus::default()).is_none(),
        "an older daemon"
    );
    let file = ConfigStatus {
        source: "file".into(),
        reference: "/x/c.toml".into(),
        state: "confirmed".into(),
        started_from: "file".into(),
        ..Default::default()
    };
    assert_eq!(config_line(&file).unwrap(), "config: file /x/c.toml");
    let first = ConfigStatus {
        detail: Some("there was no copy yet; the copy is kept for the next start".into()),
        confirmed_ms: Some(1012),
        ..vault("confirmed", "vault")
    };
    assert_eq!(
        config_line(&first).unwrap(),
        "config: vault (read before serving, in 1012 ms: there was no copy yet; the copy is \
         kept for the next start)"
    );
    assert_eq!(
        config_line(&vault("confirming", "copy")).unwrap(),
        "config: confirming · acting on the copy of op://V/c/notesPlain, which the daemon \
         wrote; the vault is being read"
    );
    let ok = ConfigStatus {
        confirmed_ms: Some(1034),
        detail: Some("the same text as the copy".into()),
        ..vault("confirmed", "copy")
    };
    assert_eq!(
        config_line(&ok).unwrap(),
        "config: vault (confirmed in 1034 ms)"
    );
    let held = ConfigStatus {
        detail: Some("the vault's note changed again since the restart; restart to apply".into()),
        restarted: Some(ConfigRestart {
            reference: "op://V/c/notesPlain".into(),
            at_unix_ms: 3_600_000,
            tables: vec!["kernel".into()],
            ..Default::default()
        }),
        ..vault("held", "copy")
    };
    assert_eq!(
        config_line(&held).unwrap(),
        "config: held: the vault's note changed again since the restart; restart to apply\n  \
         acting on the copy this start served from; the next start reads the vault again\n  \
         restarted at 18:00:00.000 onto the vault's changed note; changed since the copy: \
         kernel"
    );
}

// The lines' tags, which the TUI reads (theseus-7yx, step 10a). The
// goldens hold the CLI's bytes; these hold what each line is.

const ALL: Show = Show {
    reply: true,
    thinking: true,
    turns: true,
};

fn tagged(lines: Vec<Line>) -> Vec<(Tag, String)> {
    lines.into_iter().map(|l| (l.tag, l.text)).collect()
}

/// `event`'s lines for one notification.
fn shown(method: &str, params: Value, show: Show) -> Vec<(Tag, String)> {
    let e = Event::from_notification(method, &params)
        .expect("its params decode")
        .expect("a method this build knows");
    tagged(event(&e, show))
}

fn question() -> ConfirmRequest {
    serde_json::from_value(serde_json::json!({"correlation_id": "act_q",
        "session_id": "ses_a", "execution_id": "exe_a", "tool": "proc.run",
        "input": {"argv": ["tide"]}, "reason": "proc.run — approve", "by": "operator",
        "requested_at_ms": 1, "expires_at_ms": 2}))
    .unwrap()
}

/// A call's start and its end are `Tool` lines, and so are a call and
/// its result in a history.
#[test]
fn a_calls_lines_are_tool_lines() {
    let started = serde_json::json!({"session_id": "ses_a", "turn_id": "turn_a",
        "tool_use_id": "tu_1", "tool": "proc.run", "correlation_id": "act_1",
        "backend": "job", "pid": 77, "argv": ["tide", "--port", "lantern"]});
    assert_eq!(
        shown(notify::TOOL_STARTED, started, ALL),
        [(
            Tag::Tool,
            "  → proc.run [tide --port lantern] pid 77".to_string()
        )]
    );
    let ended = serde_json::json!({"tool": "proc.run", "status": "ok", "duration_ms": 40,
        "bytes": 12, "exit_code": 0});
    assert_eq!(
        shown(notify::TOOL_ENDED, ended, ALL),
        [(
            Tag::Tool,
            "  ← proc.run ok · exit 0 · 40 ms · 12 B".to_string()
        )]
    );
    let node = |kind: &str, detail: Value| -> NodeInfo {
        serde_json::from_value(serde_json::json!({"node_id": "nod_1", "kind": kind,
            "session_id": "ses_a", "position": 1, "at_unix_ms": 1_000,
            "text": "low water 14:10", "bytes": 15, "detail": detail}))
        .unwrap()
    };
    let call = node(
        "tool_call",
        serde_json::json!({"tool": "proc.run", "input": {"argv": ["tide"]},
            "decision": {"posture": "notify", "reason": "proc.run — notify"}}),
    );
    assert_eq!(
        tagged(node_lines(&call, false)),
        [(
            Tag::Tool,
            "      ⚙ proc.run {\"argv\":[\"tide\"]} [notify: proc.run — notify]".to_string()
        )]
    );
    let result = node(
        "tool_result",
        serde_json::json!({"tool": "proc.run", "status": "ok", "duration_ms": 40}),
    );
    assert_eq!(
        tagged(node_lines(&result, false)),
        [(
            Tag::Tool,
            "      ← proc.run ok · 40 ms · 15 B: low water 14:10".to_string()
        )]
    );
}

/// A row that carries a pill is tagged with its level, and one from a
/// daemon that sends none is `Plain`.
#[test]
fn a_pill_tags_its_row_with_its_level() {
    let mut s: SessionInfo = serde_json::from_value(serde_json::json!({"session_id": "ses_a",
        "kind": "conversation", "label": "harbour", "created_at_unix_ms": 1_000,
        "turns": 2}))
    .unwrap();
    assert_eq!(session_row(&s).tag, Tag::Plain);
    s.attention = Some(Attention {
        level: Level::NeedsYou,
        label: "confirm proc.run: tide".into(),
        since_ms: 1,
    });
    let row = session_row(&s);
    assert_eq!(row.tag, Tag::Level(Level::NeedsYou));
    assert!(
        row.text.contains("\t● confirm proc.run: tide\t"),
        "{}",
        row.text
    );
    let view: theseus_protocol::ExecutionView =
        serde_json::from_value(serde_json::json!({"position": 7, "at_ms": 1,
            "execution_id": "exe_a", "session_id": "ses_a", "kind": "conversation",
            "state": "running", "pending": [], "turns": 2, "spent_usd": 0.5,
            "limit_usd": 100.0,
            "attention": {"level": "working", "label": "turn 2", "since_ms": 1}}))
        .unwrap();
    assert_eq!(
        view_line(&view),
        Line::new(
            Tag::Level(Level::Working),
            "7\tses_a\tconversation\trunning\t◐ turn 2\t$0.5000"
        )
    );
    let waited: theseus_protocol::SessionWaitResult =
        serde_json::from_value(serde_json::json!({"reached": "settled", "already": true,
            "execution": view, "confirms": []}))
        .unwrap();
    let line = waited_line(&waited);
    assert_eq!(line.tag, Tag::Level(Level::Working));
    assert!(
        line.text.starts_with("settled (already)\t7\tses_a\t"),
        "{}",
        line.text
    );
}

/// A question's first line is `Ask` and how to answer it `Dim`, as it
/// arrives and in a list, and a budget question's too. A reason with a
/// newline is two rows, and the same text.
#[test]
fn a_question_is_ask_then_how_to_answer_it() {
    let q = question();
    assert_eq!(
        tagged(event(&Event::ConfirmRequested(q.clone()), Show::default())),
        [
            (
                Tag::Ask,
                "  ? proc.run needs your confirmation: proc.run — approve".to_string()
            ),
            (Tag::Dim, "      input: {\"argv\":[\"tide\"]}".to_string()),
            (
                Tag::Dim,
                "      approve: theseus answer act_q approve".to_string()
            ),
            (
                Tag::Dim,
                "      decline: theseus answer act_q decline".to_string()
            ),
        ]
    );
    let tags = |lines: &[Line]| lines.iter().map(|l| l.tag).collect::<Vec<_>>();
    assert_eq!(
        tags(&confirm_lines(&q)),
        [Tag::Ask, Tag::Dim, Tag::Dim, Tag::Dim]
    );
    let mut budget = q.clone();
    budget.budget = Some(
        serde_json::from_value(serde_json::json!({"spent_usd": 1.0, "limit_usd": 1.0,
            "needed_usd": 0.02, "lifetime_usd": 3.5}))
        .unwrap(),
    );
    assert_eq!(
        tags(&confirm_lines(&budget)),
        [Tag::Ask, Tag::Dim, Tag::Dim]
    );
    let mut two = q;
    two.reason = "the harbour's table\nand the river's".into();
    let lines = confirm_lines(&two);
    assert_eq!(tags(&lines[..2]), [Tag::Ask, Tag::Ask]);
    assert_eq!(lines[1].text, "and the river's");
    let joined: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        joined.join("\n"),
        "  ? proc.run waits for you in ses_a: the harbour's table\nand the river's\n      \
         input: {\"argv\":[\"tide\"]}\n      approve: theseus answer act_q approve\n      decline: \
         theseus answer act_q decline"
    );
}

/// A failed turn and a refused answer are `Bad`; an answer that approved
/// is `Ok`, and one that did not is `Warn`.
#[test]
fn an_error_is_bad_and_an_answer_ok_or_warn() {
    assert_eq!(
        shown(
            notify::TURN_FAILED,
            serde_json::json!({"session_id": "ses_a", "class": "auth",
                "error": "the key was refused"}),
            ALL
        ),
        [(
            Tag::Bad,
            "  ✗ turn failed (auth): the key was refused".to_string()
        )]
    );
    let answered = |approved: bool, superseded: bool| {
        let v = serde_json::json!({"session_id": "ses_a", "correlation_id": "act_q",
            "approved": approved, "superseded": superseded, "by": "the CLI"});
        shown(notify::CONFIRM_RESOLVED, v, ALL)
    };
    assert_eq!(
        answered(true, false),
        [(Tag::Ok, "  ✓ approved act_q (by the CLI)".to_string())]
    );
    assert_eq!(answered(false, false)[0].0, Tag::Warn);
    assert_eq!(answered(false, true)[0].0, Tag::Warn);
}

/// The reply and the thinking are pieces of their streams, as they came:
/// never split, and none when `Show` leaves them out. A turn's start is
/// `Dim`, and only under `turns`.
#[test]
fn the_reply_and_the_thinking_are_pieces_of_their_streams() {
    let delta =
        serde_json::json!({"turn_id": "turn_a", "loop_index": 0, "text": "Low water\nat 14:"});
    assert_eq!(
        shown(notify::MODEL_DELTA, delta.clone(), ALL),
        [(Tag::Reply, "Low water\nat 14:".to_string())]
    );
    let no_reply = Show {
        reply: false,
        ..ALL
    };
    assert!(shown(notify::MODEL_DELTA, delta.clone(), no_reply).is_empty());
    assert_eq!(
        shown(notify::MODEL_THINKING, delta.clone(), ALL),
        [(Tag::Thinking, "Low water\nat 14:".to_string())]
    );
    let no_thinking = Show {
        thinking: false,
        ..ALL
    };
    assert!(shown(notify::MODEL_THINKING, delta, no_thinking).is_empty());
    let started =
        serde_json::json!({"session_id": "ses_a", "turn_id": "turn_a", "continuation": false});
    assert_eq!(
        shown(notify::TURN_STARTED, started.clone(), ALL),
        [(Tag::Dim, "── turn turn_a".to_string())]
    );
    let no_turns = Show {
        turns: false,
        ..ALL
    };
    assert!(shown(notify::TURN_STARTED, started, no_turns).is_empty());
}

/// A turn a refusal's fallback answered says so above its status line, as
/// a `Warn` line (theseus-7gir.18): `theseus watch`, and the TUI's pane.
#[test]
fn a_fallbacks_turn_says_so_above_its_status_line() {
    let ended = serde_json::json!({"session_id": "ses_a", "turn_id": "turn_a", "loops": 2,
        "output": "", "stop_reason": "no_tool_calls", "provider_stop_reason": "end_turn",
        "model": "claude-sonnet-5", "provider": "anthropic", "profile": "sonnet",
        "usage": {"input_tokens": 1, "output_tokens": 2}, "elapsed_ms": 900,
        "fallback": {"from": "claude-sonnet-5-5", "to": "claude-sonnet-5", "category": "cyber",
            "answered": true}});
    let lines = shown(notify::TURN_ENDED, ended, ALL);
    let line = "Sonnet 5.5 declined (cyber); Sonnet 5 answered.";
    assert_eq!(lines[0], (Tag::Warn, line.to_string()));
    assert_eq!((lines.len(), lines[1].0), (2, Tag::Dim), "{lines:?}");
    assert!(lines[1]
        .1
        .starts_with("[sonnet → anthropic/claude-sonnet-5 · 2 loop(s)"));
}

/// The status line says the effort Jev set when it applied (route.v3,
/// theseus-qe3v), after the mode, and nothing when it did not.
#[test]
fn the_status_line_says_the_effort_jev_set() {
    let mut r: TurnSubmitResult = serde_json::from_value(serde_json::json!({"session_id": "ses_a",
        "turn_id": "turn_a", "loops": 1, "output": "", "stop_reason": "no_tool_calls",
        "model": "claude-opus-5-5", "provider": "anthropic", "profile": "opus",
        "usage": {"input_tokens": 1, "output_tokens": 2}, "elapsed_ms": 900,
        "route": {"mode": "sophisticated", "reason": "verdict", "from": "sonnet",
            "effort": "max", "effort_reason": "applied", "effort_applied": "max"}}))
    .unwrap();
    assert!(status_line(&r).contains(" · sophisticated (verdict) · effort max · "));
    r.route.as_mut().unwrap().effort_applied = None;
    assert!(status_line(&r).contains(" · sophisticated (verdict) · 1 loop(s)"));
    // Who answered, and why when shown (route.v5, theseus-c1uz).
    r.route.as_mut().unwrap().persona = Some(theseus_protocol::route::RoutePersona {
        key: Some("birch".into()),
        name: "Birch".into(),
        how: "jev_shown".into(),
        why: "\"CVE\" \u{b7} 0.74".into(),
    });
    assert!(status_line(&r).contains(" · Birch · \"CVE\" · 0.74 · sophisticated (verdict) · "));
}
