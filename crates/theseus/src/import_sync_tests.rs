use super::*;
use theseus_protocol::import_sync::HealthSync;

fn plan() -> ImportSyncPlan {
    ImportSyncPlan {
        configured: true,
        state_dir: Some("~/.reef".into()),
        owner_ids: vec!["U-WREN".into(), "D-WREN".into()],
        agents: vec!["main".into()],
        learning_hour: 3,
        ..ImportSyncPlan::default()
    }
}

/// The exporter is run on the plan's directory, owner ids and agents, and a
/// first run names its start, a later one the cursor.
#[test]
fn the_exporter_is_run_with_the_plans_words() {
    let a = SyncArgs::default();
    let v = script_args(
        &plan(),
        &a,
        Path::new("/s"),
        Some(5),
        None,
        Path::new("/r.json"),
        9,
    );
    assert_eq!(
        v,
        [
            "--from",
            "/s",
            "--report",
            "/r.json",
            "--now-ms",
            "9",
            "--since-ms",
            "5",
            "--owner-id",
            "U-WREN",
            "--owner-id",
            "D-WREN",
            "--agents",
            "main"
        ]
    );
    let a = SyncArgs {
        agents: Some("a,b".into()),
        tag: Some("t".into()),
        dry_run: true,
        ..SyncArgs::default()
    };
    let v = script_args(
        &plan(),
        &a,
        Path::new("/s"),
        None,
        Some(Path::new("/c.json")),
        Path::new("/r"),
        9,
    );
    assert!(v.windows(2).any(|w| w == ["--cursor-in", "/c.json"]));
    assert!(v.windows(2).any(|w| w == ["--agents", "a,b"]));
    assert!(v.windows(2).any(|w| w == ["--tag", "t"]));
    assert!(v.contains(&"--dry-run".to_string()) && !v.contains(&"--since-ms".to_string()));
}

#[test]
fn a_home_directory_is_expanded() {
    std::env::set_var("HOME", "/home/reef");
    assert_eq!(expand("~/.reef"), PathBuf::from("/home/reef/.reef"));
    assert_eq!(expand("/var/reef"), PathBuf::from("/var/reef"));
}

/// A run's end in words, its refusals and its people step named.
#[test]
fn a_runs_end_is_worded_with_its_refusals_and_its_people() {
    theseus_client::render::time::pin_for_tests();
    let r = ImportSyncResult {
        sync: HealthSync {
            at_ms: 1,
            sessions_new: 2,
            sessions_appended: 3,
            messages: 40,
            rejected: 1,
            next_ms: 1_790_000_000_000,
            ..HealthSync::default()
        },
        people: Some(SyncPeople {
            tags: vec!["openclaw-sync-2026-10".into()],
            declared: 2,
            joined: 5,
            swept: 4,
            candidates: 6,
            judged: 3,
            spent_usd: 0.1234,
            left: 2,
            stopped: Some("at the day's cap".into()),
        }),
        ms: 1.0,
    };
    let rec = ImportSyncRecord {
        episodes: 6,
        messages: 40,
        skipped: 1,
        ..ImportSyncRecord::default()
    };
    let rejected = vec![ImportRejected {
        line: 3,
        episode_id: Some("ep_ab".into()),
        why: "message 7 changed".into(),
    }];
    let out = lines(&r, &rec, &rejected);
    assert!(out.starts_with("sync: 6 sessions read (40 messages): 2 new, 3 appended to, 1 already held, 1 refused; next run "), "{out}");
    assert!(
        out.contains("  refused ep_ab: message 7 changed\n"),
        "{out}"
    );
    assert!(out.contains("people: 1 tags' handles (2 declared, 5 sessions joined); 4 sessions read for people, 6 candidates, 3 judged, $0.1234 spent; stopped: at the day's cap (2 left for the next run)\n"), "{out}");
}

#[test]
fn a_dry_run_says_it_wrote_nothing() {
    let v = serde_json::json!({"episodes": 2, "messages": 9,
        "agents": {"main": {"sessions": 2, "messages": 9, "archives": 1}}});
    assert_eq!(
        dry_lines(&v),
        "dry run: 2 sessions with 9 messages would be taken; nothing was written or recorded\n  main: 2 sessions, 9 messages, 1 from the archive\n"
    );
}

/// The gate on the people step (theseus-ievo3, S29-1): the people step is
/// where the minutes and the dollars go and it stays nightly, so the daemon's
/// own on-demand run passes `--no-people` in its fixed argv
/// (`theseus_core::import::sync_run::ARGV`) and arrives here as an ordinary
/// `--no-people` run. Nothing else sets it, which is the point: there is one
/// way to skip the people step and it is the flag.
#[test]
fn an_on_demand_run_is_import_only_because_its_argv_says_so() {
    assert!(!no_people(&SyncArgs::default()));
    assert!(no_people(&SyncArgs {
        no_people: true,
        ..SyncArgs::default()
    }));
}

/// What `--request` prints (theseus-ievo3, S29-1): the daemon's own sentence,
/// unchanged, so the terminal and the thread read the same words. The CLI does
/// not word this itself -- it cannot, since it is the daemon that knows
/// whether a run started or one was already going.
#[test]
fn a_request_prints_the_daemons_own_sentence() {
    assert_eq!(
        request_line(&ImportSyncStarted {
            started: true,
            at_ms: 1_790_000_000_000,
            say: "The OpenClaw sync started. I will say what it imported when it is done.".into(),
        }),
        "The OpenClaw sync started. I will say what it imported when it is done.\n"
    );
    assert_eq!(
        request_line(&ImportSyncStarted {
            started: false,
            at_ms: 1_790_000_000_000,
            say: "An OpenClaw sync is already running (started 2 minutes ago); nothing new was \
                  started. I will say what it imported when it is done."
                .into(),
        }),
        "An OpenClaw sync is already running (started 2 minutes ago); nothing new was started. \
         I will say what it imported when it is done.\n"
    );
}
