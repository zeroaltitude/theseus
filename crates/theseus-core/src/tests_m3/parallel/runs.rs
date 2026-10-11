//! A response's writes and programs run together (theseus-d1hi): programs
//! with programs, writes to different paths with each other, at most
//! `[tools] parallel_runs` programs at once, and what depends on an earlier
//! call kept after it.

use super::*;

/// A slowed program: `fs.read`'s plan and output under a `Run` tool's name,
/// so a call names its file (`path`) as a program's argv would.
fn program(name: &'static str, timing: &Arc<Timing>) -> Arc<dyn Tool> {
    slowed(
        name,
        ToolClass::Run,
        Arc::new(theseus_tools::fs::Read),
        timing,
    )
}

fn run(id: &str, path: &str) -> (String, &'static str, Value) {
    (id.into(), "test_run", json!({ "path": path }))
}

fn write(id: &str, path: &str, content: &str) -> (String, &'static str, Value) {
    (
        id.into(),
        "fs_write",
        json!({"path": path, "content": content}),
    )
}

/// At most how many of `runs` were in flight at once.
fn most_at_once(runs: &[(Instant, Instant)]) -> usize {
    let in_flight = |at: Instant| runs.iter().filter(|(a, b)| *a <= at && at < *b).count();
    runs.iter().map(|(a, _)| in_flight(*a)).max().unwrap_or(0)
}

/// The text of the last request's tool results, in order.
fn results_text(r: &Rig) -> Vec<String> {
    let last = r.fake.requests().pop().unwrap();
    last.messages
        .last()
        .unwrap()
        .get("content")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter(|b| b["type"] == "tool_result")
        .map(|b| b["content"].to_string())
        .collect()
}

/// Four programs of one response: each its own call, all four in flight at
/// once (a rendezvous, not a race: a serial dispatch never gets the four
/// there). Their results come back in one message, in call order.
#[tokio::test]
async fn four_programs_of_one_response_run_together() {
    let timing = Arc::<Timing>::default();
    let four: Vec<_> = (1..=4)
        .map(|i| run(&format!("p{i}"), &format!("p{i}.txt")))
        .collect();
    let r = rig_parts(
        vec![calls(&four), Scripted::text("Ran four.")],
        |cfg| {
            cfg.policy.tools.insert("test.run".into(), Posture::Open);
        },
        |p| {
            p.cpu_cores = Some(4);
            p.toollets = vec![program("test.run", &timing)];
        },
    );
    for i in 1..=4 {
        std::fs::write(r.root.join(format!("p{i}.txt")), format!("program {i}\n")).unwrap();
        timing.set(&[(&format!("test.run:p{i}.txt"), 200)]);
    }
    timing.rendezvous(4);
    let res = turn(&r.core, None, "run four programs").await;
    assert_eq!((res.loops, res.tool_calls), (2, 4), "{res:?}");
    let runs: Vec<_> = (1..=4)
        .map(|i| timing.of(&format!("test.run:p{i}.txt")))
        .collect();
    assert_eq!(
        most_at_once(&runs),
        4,
        "the four programs ran one at a time"
    );
    assert_eq!(results_sent(&r), ["p1", "p2", "p3", "p4"].map(String::from));
    let texts = results_text(&r);
    for (i, t) in texts.iter().enumerate() {
        assert!(t.contains(&format!("program {}", i + 1)), "{t}");
    }
}

/// The same with real jobs (`proc.run` of `sleep 1`, through the product's
/// launcher): four end in about one second, not four. The bound is the
/// serial floor less a second, far above a parallel run's one second.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn four_sleeps_of_one_response_end_in_about_one_sleep() {
    let four: Vec<(String, &str, Value)> = (1..=4)
        .map(|i| (format!("s{i}"), "proc_run", json!({"argv": ["sleep", "1"]})))
        .collect();
    let r = rig_with(vec![calls(&four), Scripted::text("Slept.")], |cfg| {
        cfg.policy.tools.insert("proc.run".into(), Posture::Open);
    });
    let t0 = Instant::now();
    let res = turn(&r.core, None, "sleep four times").await;
    let took = t0.elapsed();
    assert_eq!((res.loops, res.tool_calls), (2, 4), "{res:?}");
    assert!(took < Duration::from_secs(3), "four sleeps took {took:?}");
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs.len(), 4);
    for (status, text) in &rs {
        assert_eq!(*status, ResultStatus::Ok, "{text}");
    }
    assert_eq!(results_sent(&r), ["s1", "s2", "s3", "s4"].map(String::from));
}

/// `parallel_runs = 2`: four programs, never more than two at once, two at
/// once at some point (a rendezvous of two), and the results in call order.
#[tokio::test]
async fn parallel_runs_caps_the_programs_at_once() {
    let timing = Arc::<Timing>::default();
    let four: Vec<_> = (1..=4)
        .map(|i| run(&format!("q{i}"), &format!("q{i}.txt")))
        .collect();
    let r = rig_parts(
        vec![calls(&four), Scripted::text("Ran four.")],
        |cfg| {
            cfg.tools.parallel_runs = 2;
            cfg.policy.tools.insert("test.run".into(), Posture::Open);
        },
        |p| {
            p.cpu_cores = Some(4);
            p.toollets = vec![program("test.run", &timing)];
        },
    );
    for i in 1..=4 {
        std::fs::write(r.root.join(format!("q{i}.txt")), "q\n").unwrap();
        timing.set(&[(&format!("test.run:q{i}.txt"), 150)]);
    }
    timing.rendezvous(2);
    let res = turn(&r.core, None, "run four, two at a time").await;
    assert_eq!(res.tool_calls, 4);
    let runs: Vec<_> = (1..=4)
        .map(|i| timing.of(&format!("test.run:q{i}.txt")))
        .collect();
    assert_eq!(most_at_once(&runs), 2, "{runs:?}");
    assert_eq!(results_sent(&r), ["q1", "q2", "q3", "q4"].map(String::from));
}

/// Writes to different paths run together; two to one path run in order,
/// and the second's content is the file's. Then a program after a write
/// starts once the write has ended (a class change is a barrier), and a
/// program that names a file an earlier program of its group names waits for
/// it, while the next one, naming another, runs beside it.
#[tokio::test]
async fn what_depends_on_an_earlier_call_runs_after_it() {
    let timing = Arc::<Timing>::default();
    let r = rig_full(
        vec![
            calls(&[
                write("w1", "a.txt", "first\n"),
                write("w2", "b.txt", "b\n"),
                write("w3", "a.txt", "second\n"),
            ]),
            Scripted::text("Written."),
            calls(&[
                write("x1", "d.txt", "d\n"),
                run("x2", "d.txt"),
                run("x3", "d.txt"),
                run("x4", "c.txt"),
            ]),
            Scripted::text("Ran."),
        ],
        |cfg| {
            cfg.policy.tools.insert("fs.write".into(), Posture::Open);
            cfg.policy.tools.insert("test.run".into(), Posture::Open);
        },
        vec![
            slowed(
                "fs.write",
                ToolClass::Write,
                Arc::new(theseus_tools::fs::WriteFile),
                &timing,
            ),
            program("test.run", &timing),
        ],
    );
    std::fs::write(r.root.join("c.txt"), "c\n").unwrap();
    timing.set(&[
        ("fs.write:a.txt", 150),
        ("fs.write:b.txt", 150),
        ("fs.write:d.txt", 150),
        ("test.run:d.txt", 150),
        ("test.run:c.txt", 150),
    ]);
    let all = |key: &str| -> Vec<(Instant, Instant)> {
        let runs = timing.runs.lock().unwrap();
        runs.iter()
            .filter(|(k, _, _)| k == key)
            .map(|(_, a, b)| (*a, *b))
            .collect()
    };

    // w1 and w2 wait for each other once started; w3 goes alone, after w1.
    timing.rendezvous(2);
    let res = turn(&r.core, None, "write a and b, then a again").await;
    assert_eq!(res.tool_calls, 3);
    let (w1, w3) = (all("fs.write:a.txt")[0], all("fs.write:a.txt")[1]);
    let w2 = timing.of("fs.write:b.txt");
    assert!(w1.0 < w2.1 && w2.0 < w1.1, "the two paths ran together");
    assert!(
        w3.0 >= w1.1,
        "the second write to a.txt waited for the first"
    );
    assert_eq!(
        std::fs::read_to_string(r.root.join("a.txt")).unwrap(),
        "second\n"
    );

    // x1 and x2 go alone, one after the other; x3 and x4 wait for each other.
    timing.rendezvous_after(2, 2);
    let again = turn(&r.core, None, "write d, run on it twice, and on c").await;
    assert_eq!(again.tool_calls, 4);
    let x1 = timing.of("fs.write:d.txt");
    let (x2, x3) = (all("test.run:d.txt")[0], all("test.run:d.txt")[1]);
    let x4 = timing.of("test.run:c.txt");
    assert!(x2.0 >= x1.1, "the program waited for the write before it");
    assert!(
        x3.0 >= x2.1,
        "the program naming d.txt again waited for the first"
    );
    assert!(
        x3.0 < x4.1 && x4.0 < x3.1,
        "the one naming c.txt ran beside it"
    );
    let rs = results(&r.core, &again.session_id);
    assert!(
        rs[1].1.contains("d\n"),
        "the program read what the write wrote: {rs:?}"
    );
}

/// A cancel during a group of programs that `parallel_runs` holds to two:
/// the two running are each answered once, and the third, which never got a
/// slot, is never planned and is answered as not run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancel_during_a_group_of_programs_starts_none_that_waits() {
    let three: Vec<(String, &str, Value)> = (1..=3)
        .map(|i| {
            (
                format!("c{i}"),
                "proc_run",
                json!({"argv": ["sleep", "30"]}),
            )
        })
        .collect();
    let r = rig_with(vec![calls(&three)], |cfg| {
        cfg.tools.parallel_runs = 2;
        cfg.policy.tools.insert("proc.run".into(), Posture::Open);
    });
    let (sid, exec, running) = start_and_cancel_when_dispatched(&r, "sleep three times", 2).await;
    tokio::time::timeout(Duration::from_secs(20), running)
        .await
        .expect("the turn ended")
        .unwrap()
        .expect_err("the next provider call is refused");
    let (planned, mut got) = answers(&r.core, &sid);
    assert_eq!(planned, ["c1", "c2"], "the third was never planned");
    got.sort_by(|a, b| a.0.cmp(&b.0));
    let ids: Vec<&str> = got.iter().map(|g| g.0.as_str()).collect();
    assert_eq!(ids, ["c1", "c2", "c3"], "one result each: {got:?}");
    for (id, status, text, _) in &got {
        assert_eq!(*status, ResultStatus::Cancelled, "{id}: {text}");
    }
    assert_eq!(got[2].2, "Not run: the execution was cancelled by test.");
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state.as_str(), "cancelled");
}
