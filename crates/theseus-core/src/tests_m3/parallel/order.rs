//! A program of a response never starts before the call of the same response
//! it depends on has ended (theseus-0hufe): a start and a query of one
//! service, a worktree and a build in it. Programs that share nothing still
//! run together, and a held call's result says what it waited for.

use super::*;

/// What the programs did, in order: `begin <program>` and `end <program>`.
#[derive(Default)]
struct Log(Mutex<Vec<String>>);

impl Log {
    fn push(&self, what: String) {
        self.0.lock().unwrap().push(what);
    }

    fn at(&self, what: &str) -> usize {
        let log = self.0.lock().unwrap();
        log.iter()
            .position(|w| w == what)
            .unwrap_or_else(|| panic!("no {what:?} in {log:?}"))
    }
}

/// A program that takes a (virtual) moment: it names its argv as `proc.run`
/// does, and logs when it begins and ends.
struct Prog(Arc<Log>);

impl Tool for Prog {
    fn name(&self) -> &str {
        "test.prog"
    }
    fn description(&self) -> &str {
        "a program"
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"argv": {"type": "array"}}, "required": ["argv"]})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }
    fn backend(&self) -> theseus_tools::Backend {
        theseus_tools::Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let argv: Vec<String> =
            serde_json::from_value(input["argv"].clone()).map_err(|e| e.to_string())?;
        Ok(Plan {
            summary: argv.join(" "),
            argv: Some(argv),
            ..Plan::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        let line = input["argv"].as_array().map_or(String::new(), |a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        });
        let log = self.0.clone();
        Box::pin(async move {
            log.push(format!("begin {line}"));
            tokio::time::sleep(Duration::from_millis(100)).await;
            log.push(format!("end {line}"));
            Ok((
                ToolOutput {
                    text: format!("ran {line}"),
                    meta: Value::Null,
                },
                None,
            ))
        })
    }
}

fn prog(id: &str, line: &str) -> (String, &'static str, Value) {
    let argv: Vec<&str> = line.split(' ').collect();
    (id.into(), "test_prog", json!({ "argv": argv }))
}

/// A turn of `calls` through the product's grouping, on a paused clock.
async fn run(
    calls_: &[(String, &'static str, Value)],
    tweak: impl FnOnce(&mut Config),
) -> (Rig, Arc<Log>, Vec<String>) {
    let log = Arc::<Log>::default();
    let p: Arc<dyn Tool> = Arc::new(Prog(log.clone()));
    let r = rig_parts(
        vec![calls(calls_), Scripted::text("Ran.")],
        |cfg| {
            cfg.policy.tools.insert("test.prog".into(), Posture::Open);
            tweak(cfg);
        },
        |parts| {
            parts.cpu_cores = Some(4);
            parts.toollets = vec![p];
        },
    );
    let res = turn(&r.core, None, "run them").await;
    assert_eq!(res.tool_calls as usize, calls_.len(), "{res:?}");
    let texts = results(&r.core, &res.session_id)
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    (r, log, texts)
}

/// A service started and queried by name in one response: the query begins
/// after the start has ended, and its result says what it waited for.
#[tokio::test(start_paused = true)]
async fn a_query_of_a_service_starts_after_its_start_has_ended() {
    let (_r, log, texts) = run(
        &[
            prog("a", "systemctl --user start web"),
            prog("b", "systemctl --user status web"),
        ],
        |_| {},
    )
    .await;
    assert!(
        log.at("end systemctl --user start web") < log.at("begin systemctl --user status web"),
        "{:?}",
        log.0.lock().unwrap()
    );
    assert!(!texts[0].contains("started after"), "{}", texts[0]);
    assert!(
        texts[1].contains("[started after call 1, which names `web` too]"),
        "{}",
        texts[1]
    );
}

/// The same for a container and a worktree, and a call with a program
/// between them that shares nothing runs beside the first.
#[tokio::test(start_paused = true)]
async fn a_container_and_a_worktree_are_used_after_they_are_made() {
    let (_r, log, texts) = run(
        &[
            prog("a", "docker run --name db img"),
            prog("b", "git worktree add ../wt-a"),
            prog("c", "docker exec db psql -c select"),
            prog("d", "make -C ../wt-a"),
        ],
        |_| {},
    )
    .await;
    let (end_a, end_b) = (
        log.at("end docker run --name db img"),
        log.at("end git worktree add ../wt-a"),
    );
    assert!(
        log.at("begin git worktree add ../wt-a") < end_a,
        "a and b run together"
    );
    assert!(end_a.max(end_b) < log.at("begin docker exec db psql -c select"));
    assert!(end_a.max(end_b) < log.at("begin make -C ../wt-a"));
    assert!(
        texts[2].contains("started after call 1, which names `db` too"),
        "{}",
        texts[2]
    );
    assert!(
        texts[3].contains("started after call 2, which names `wt-a` too"),
        "{}",
        texts[3]
    );
}

/// Independent programs keep their concurrency: sixteen `sleep 2`, two
/// pytest files, two URLs, two builds in different directories.
#[tokio::test(start_paused = true)]
async fn programs_that_share_nothing_run_together() {
    let mut v: Vec<_> = (0..4).map(|i| prog(&format!("s{i}"), "sleep 2")).collect();
    v.extend([
        prog("t1", "pytest tests/a.py"),
        prog("t2", "pytest tests/b.py"),
        prog("c1", "fetcher a.example/x"),
        prog("c2", "fetcher b.example/y"),
        prog("m1", "make -C ../one"),
        prog("m2", "make -C ../two"),
    ]);
    let n = v.len();
    let (_r, log, texts) = run(&v, |_| {}).await;
    let events = log.0.lock().unwrap().clone();
    let last_begin = events.iter().rposition(|w| w.starts_with("begin")).unwrap();
    let first_end = events.iter().position(|w| w.starts_with("end")).unwrap();
    assert!(last_begin < first_end, "all {n} began before any ended");
    assert!(
        texts.iter().all(|t| !t.contains("started after")),
        "{texts:?}"
    );
}

impl Log {
    /// The most programs begun and not yet ended at any point of the log.
    fn most_at_once(&self) -> usize {
        let (mut now, mut most) = (0usize, 0usize);
        for w in self.0.lock().unwrap().iter() {
            if w.starts_with("begin") {
                now += 1;
                most = most.max(now);
            } else {
                now -= 1;
            }
        }
        most
    }
}

/// The cap of programs is `[tools] parallel_runs` (16 by default): sixteen
/// `sleep 2` in one response start together in one wave, a seventeenth waits
/// for a slot, and the results come back in call order.
#[tokio::test(start_paused = true)]
async fn sixteen_programs_run_in_one_wave_and_a_seventeenth_waits() {
    let v: Vec<_> = (0..17)
        .map(|i| prog(&format!("p{i:02}"), "sleep 2"))
        .collect();
    let (r, log, _) = run(&v, |_| {}).await;
    assert_eq!(log.most_at_once(), 16);
    let events = log.0.lock().unwrap().clone();
    let (first_end, last_begin) = (
        events.iter().position(|w| w.starts_with("end")).unwrap(),
        events.iter().rposition(|w| w.starts_with("begin")).unwrap(),
    );
    assert_eq!(first_end, 16, "sixteen began before any ended");
    assert!(
        last_begin > first_end,
        "the seventeenth began once a slot was free"
    );
    assert_eq!(
        results_sent(&r),
        (0..17).map(|i| format!("p{i:02}")).collect::<Vec<_>>()
    );
}

/// `[tools] parallel_runs = 3` holds three at once however many there are.
#[tokio::test(start_paused = true)]
async fn the_cap_of_programs_is_the_config() {
    let v: Vec<_> = (0..7).map(|i| prog(&format!("p{i}"), "sleep 2")).collect();
    let (_r, log, _) = run(&v, |cfg| cfg.tools.parallel_runs = 3).await;
    assert_eq!(log.most_at_once(), 3);
}
