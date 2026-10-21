use super::*;

#[test]
fn percentiles_are_nearest_rank() {
    let s: Vec<f64> = (1..=20).map(f64::from).collect();
    assert_eq!(percentile(&s, 50.0), 10.0);
    assert_eq!(percentile(&s, 95.0), 19.0);
    assert_eq!(percentile(&s, 100.0), 20.0);
    assert_eq!(percentile(&[7.0], 95.0), 7.0);
    // Ten runs: p95 is the slowest, so one slow run is never averaged away.
    let ten: Vec<f64> = (1..=10).map(f64::from).collect();
    assert_eq!(percentile(&ten, 95.0), 10.0);
    let s = Summary::of(&[3.0, 1.0, 2.0]).unwrap();
    assert_eq!((s.n, s.p50, s.p95, s.min, s.max), (3, 2.0, 3.0, 1.0, 3.0));
    assert!(Summary::of(&[]).is_none());
}

#[test]
fn budgets_follow_section_nine_and_the_store_size() {
    assert_eq!(budget_ms("cold", 0), Some(50.0));
    assert_eq!(budget_ms("cold", 10_000), Some(250.0));
    assert_eq!(budget_ms("cold", 5_000), Some(150.0));
    assert_eq!(
        budget_ms("cold", 50_000),
        Some(250.0),
        "capped at §9's 10,000"
    );
    assert_eq!(budget_ms("kill", 0), Some(150.0));
    assert_eq!(budget_ms("kill", 10_000), Some(350.0));
    assert_eq!(budget_ms("shutdown", 10_000), Some(100.0));
    // A binary upgrade: under 200 ms without an answer, at any size.
    assert_eq!(budget_ms("swap", 0), Some(200.0));
    // The surfaces' budget is the same at any size (theseus-7bee).
    assert_eq!(budget_ms("surfaces", 0), Some(50.0));
    assert_eq!(budget_ms("surfaces", 200_000), Some(50.0));
    assert_eq!(budget_ms("swap", 10_000), Some(200.0));
    // Restore is measured: §9 names no number yet.
    assert_eq!(budget_ms("restore", 0), None);
    // Since cancel-fast (theseus-kq4n), at any size.
    assert_eq!(budget_ms("cancel", 0), Some(100.0));
    assert_eq!(budget_ms("cancel", 10_000), Some(100.0));
}

/// A run with nothing out of place: no phase judged, every check held.
fn quiet_report() -> Report {
    Report {
        theseusd: "theseusd".into(),
        store: "empty".into(),
        sessions: 0,
        generated: None,
        runs: 10,
        resolver_ms: 0,
        phases: vec![],
        daemon: vec![],
        kernel_steps: vec![],
        secrets_at_first_answer: BTreeMap::new(),
        verdicts: vec![],
        served_before_secrets: true,
        config_at_first_answer: BTreeMap::new(),
        served_from_copy: true,
        driver: None,
        driver_before_token: true,
        vault_confirmed: None,
        vault_first_ms: None,
        swap_job: None,
        swap_lock_wait: None,
        restore: None,
        cancel_frames: None,
        first_message: None,
        samples: BTreeMap::new(),
        warm_up_ms: vec![],
        starts: vec![],
        wall_ms: 1.0,
    }
}

/// theseus-kq4n: a run fails when the cancel that wrote the fewest frames
/// wrote more than two, as every cancel before cancel-fast did (five),
/// and the busy allowance never carries it, a count. Its most is not
/// judged: under load it picks up other work's frames.
#[test]
fn a_run_whose_fewest_frames_a_cancel_are_over_two_fails() {
    assert_eq!(
        CancelFrames::of(&[2, 7, 2, 3]),
        Some(CancelFrames { least: 2, most: 7 })
    );
    assert_eq!(CancelFrames::of(&[]), None);
    let mut r = quiet_report();
    assert!(r.ok(), "no cancel phase: nothing to judge");
    r.cancel_frames = CancelFrames::of(&[2, 2, 7]);
    assert!(r.ok(), "the fewest is 2; the 7 is other work's");
    r.cancel_frames = CancelFrames::of(&[3, 5, 5]);
    assert!(!r.ok(), "the fewest is 3, over 2");
    assert!(!r.ok_with(65), "a count gets no busy allowance");
    r.cancel_frames = CancelFrames::of(&[5, 5, 7]);
    assert!(!r.ok(), "main's before cancel-fast: 5 to 7");
}

#[test]
fn a_p95_past_its_budget_and_margin_misses() {
    let s = |p95: f64| Summary {
        n: 10,
        p50: p95 - 1.0,
        p95,
        min: 1.0,
        max: p95,
    };
    let phases = vec![
        ("cold".to_string(), s(54.0)),
        ("shutdown".to_string(), s(99.0)),
        ("kill".to_string(), s(160.0)),
        ("unbudgeted".to_string(), s(9999.0)),
    ];
    let v = verdicts(&phases, 0, Some(5.0));
    assert_eq!(v.len(), 3, "a phase without a budget is not judged");
    assert!(v[0].ok, "54 ms is inside 50 ms plus a 5 ms margin");
    assert!(v[1].ok);
    assert!(!v[2].ok, "160 ms is past 150 ms plus 5");
    assert_eq!(v[2].budget, 150.0);
    // Each phase's measured margin, when none is given.
    let v = verdicts(&phases, 0, None);
    assert_eq!(
        v.iter().map(|v| v.margin).collect::<Vec<_>>(),
        [7.0, 4.0, 25.0]
    );
    assert!(
        v[0].ok && v[1].ok && v[2].ok,
        "160 ms is inside 150 plus 25"
    );
    // The throwaway 100 ms sleep in cold start: 23 ms becomes 123 ms.
    let slept = verdicts(&[("cold".to_string(), s(123.0))], 0, None);
    assert!(!slept[0].ok);
    // A swap: 200 ms plus its 2 ms margin, whatever the store's size;
    // a restore is measured and never judged.
    let v = verdicts(
        &[
            ("swap".to_string(), s(203.0)),
            ("restore".to_string(), s(9999.0)),
        ],
        10_000,
        None,
    );
    assert_eq!(v.len(), 1);
    assert!(!v[0].ok);
    assert_eq!((v[0].budget, v[0].margin), (200.0, 2.0));
}

#[test]
fn the_seed_and_health_carry_a_margin_over_their_budgets() {
    // theseus-m1r2w: limits of 4 and 8 ms. A p95 inside them passes, and
    // one past them misses.
    let s = |p95: f64| Summary {
        n: 10,
        p50: p95 / 2.0,
        p95,
        min: 0.5,
        max: p95,
    };
    let v = verdicts(
        &[("seed".to_string(), s(3.9)), ("health".to_string(), s(7.9))],
        0,
        None,
    );
    assert_eq!(
        v.iter().map(|v| (v.budget, v.margin)).collect::<Vec<_>>(),
        [(2.0, 2.0), (5.0, 3.0)]
    );
    assert!(v[0].ok && v[1].ok);
    let v = verdicts(
        &[("seed".to_string(), s(4.5)), ("health".to_string(), s(8.5))],
        0,
        None,
    );
    assert!(!v[0].ok && !v[1].ok);
}

#[test]
fn the_keys_from_health_hold_the_imports_sessions() {
    // A daemon that counts the owner's own sessions apart (theseus-revl).
    let h = json!({"sessions": 4, "imported": {"sessions": 4000, "erased": 1000}});
    assert_eq!(health_keys(&h), 5004);
    // An older daemon: no `imported`, and `sessions` is every key.
    assert_eq!(health_keys(&json!({"sessions": 5004})), 5004);
    assert_eq!(health_keys(&json!({})), 0);
}

#[test]
fn a_start_is_read_from_health() {
    let h = json!({
        "secrets": {"state": "resolving"},
        "startup": [
            {"name": "config", "background": false, "start_us": 100, "end_us": 400},
            {"name": "kernel", "background": false, "start_us": 1000, "end_us": 36000,
             "detail": {"steps": [{"name": "store", "us": 7000}, {"name": "load", "us": 6500}]}},
            {"name": "socket", "background": false, "start_us": 37000, "end_us": 37100},
            {"name": "secrets", "background": true, "start_us": 500, "end_us": null},
        ],
    });
    let s = Start::from_health(41.5, &h);
    assert_eq!(s.ms, 41.5);
    assert_eq!(s.secrets, "resolving");
    assert_eq!(
        s.phases,
        vec![
            ("config".to_string(), 0.3),
            ("kernel".to_string(), 35.0),
            ("socket".to_string(), 0.1)
        ]
    );
    assert_eq!(
        s.steps,
        vec![("store".to_string(), 7.0), ("load".to_string(), 6.5)]
    );
    assert_eq!(s.serving_ms, 37.1);
    assert!((s.between_ms - 1.7).abs() < 1e-9, "{}", s.between_ms);
    assert!(!s.index_repaired);
    // The store phase says when the open repaired the index (theseus-7okz9).
    let h = json!({"startup": [{"name": "store", "background": false, "start_us": 0,
        "end_us": 8440000, "detail": {"index_repaired": true, "lock_wait_ms": 0.1}}]});
    let s = Start::from_health(8500.0, &h);
    assert!(s.index_repaired, "{s:?}");
    assert_eq!(s.lock_wait_ms, Some(0.1));
}

/// The bench config loads as the daemon loads it, with nothing that
/// leaves the machine: no GitHub token, Discord and the web UI off,
/// every secret on the fake vault.
#[test]
fn the_bench_config_loads_and_stays_on_the_machine() {
    let d = tempfile::tempdir().unwrap();
    let text = bench_config(
        "http://127.0.0.1:9",
        &d.path().join("state"),
        &d.path().join("sock"),
        d.path(),
    )
    .unwrap();
    let (cfg, _warnings) = theseus_core::Config::parse(&text).unwrap();
    assert!(!cfg.web.enabled);
    // The binding runs, and all it sends goes to a port nothing listens
    // on (theseus-q4v).
    assert!(cfg.discord.enabled);
    assert_eq!(cfg.discord.rest_proxy.as_deref(), Some("127.0.0.1:9"));
    assert_eq!(
        cfg.discord.gateway_proxy.as_deref(),
        Some("ws://127.0.0.1:9")
    );
    assert!(!cfg.secrets.contains_key(&cfg.github.token_secret));
    assert!(cfg.secrets.values().all(|r| r.starts_with("op://Bench/")));
    assert!(cfg
        .all_providers()
        .values()
        .all(|p| p.api_base == "http://127.0.0.1:9"));
    assert_eq!(cfg.tools.proc_sync_secs, 1);
    // An AWS account is bound, and AWS too is a port nothing listens on
    // (row 29, C1): its key's secrets are the fake vault's.
    let a = &cfg.aws.accounts[BENCH_AWS_ACCOUNT];
    assert_eq!(a.endpoint.as_deref(), Some("http://127.0.0.1:9"));
    assert!(a.durability && a.owner_role.is_some());
    assert!(cfg.secrets.contains_key(&a.credentials.access_key_id));
    assert!(cfg.secrets.contains_key(&a.credentials.secret_access_key));
    // The judge is on, and Jev too is a port nothing listens on (M5 23a).
    assert!(cfg.judge.enabled);
    assert_eq!(cfg.judge.api_base, "http://127.0.0.1:9");
    assert!(cfg.secrets.contains_key(&cfg.judge.key_secret));
    // The index tender runs, and finds no model's files.
    assert!(cfg.index.enabled);
    assert!(!theseus_core::config::expand(&cfg.index.weights_dir).exists());
    // Recall runs in shadow.
    assert!(cfg.memory.on());
}

/// The bench's binding is one the binding takes, and its Discord is the
/// in-process fake, REST and gateway (theseus-l21m): with ids it refused,
/// it failed at load and the driver check checked nothing.
#[test]
fn the_bench_binding_has_discords_ids_and_the_fake_discord() {
    let ids: toml::Table = BENCH_BINDINGS.parse().unwrap();
    let dm = &ids["dm"].as_array().unwrap()[0];
    for id in [&ids["guild_id"], &dm["user"]] {
        let id = id.as_str().unwrap();
        assert!(
            (15..=21).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()),
            "{id:?} is not a Discord id"
        );
    }
    assert_eq!(
        ids["guild_id"].as_str(),
        Some(
            theseus_sim::fake_discord::DEFAULT_GUILD
                .to_string()
                .as_str()
        ),
        "the fake gateway's READY names this guild"
    );
    let fake = theseus_sim::fake_discord::FakeDiscord::start_with_gateway();
    let d = tempfile::tempdir().unwrap();
    let text = bench_config(
        "http://127.0.0.1:9",
        &d.path().join("state"),
        &d.path().join("sock"),
        d.path(),
    )
    .and_then(|t| on_fake_discord(&t, &fake))
    .unwrap();
    let (cfg, _warnings) = theseus_core::Config::parse(&text).unwrap();
    assert!(cfg.discord.enabled);
    assert_eq!(cfg.discord.rest_proxy.as_deref(), Some(fake.addr.as_str()));
    assert_eq!(
        cfg.discord.gateway_proxy,
        Some(fake.gateway().unwrap().url())
    );
    assert!(
        fake.addr.starts_with("127.0.0.1:"),
        "nothing leaves the machine"
    );
}

#[test]
fn the_fake_op_answers_every_reference_after_its_delay() {
    let d = tempfile::tempdir().unwrap();
    let op = d.path().join("op");
    let note = d.path().join("note.toml");
    std::fs::write(&note, "[secrets]\n").unwrap();
    std::fs::write(&op, fake_op(10, &note)).unwrap();
    let read = Command::new("sh")
        .arg(&op)
        .args(["read", "--no-newline", VAULT_REF])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(read.stdout).unwrap(), "[secrets]\n");
    let out = Command::new("sh")
        .arg(&op)
        .arg("inject")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            c.stdin
                .take()
                .unwrap()
                .write_all(b"--b-0\n{{ op://Bench/a/credential }}\n--b-end\n")?;
            c.wait_with_output()
        })
        .unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "--b-0\nbench-secret-value-0000\n--b-end\n"
    );
}
