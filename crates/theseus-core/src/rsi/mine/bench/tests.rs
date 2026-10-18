//! The bench's trials (§2.1): an ATIF trajectory (and the history it is made
//! from) gives the same shapes the ledger gives; a bench motif needs four
//! trials across three tasks; one born with a task's own path is not generic
//! and is rejected.

use serde_json::{json, Value};

use super::*;
use crate::rsi::mine::{mine, Params, Run, SlotType, Vocab};

fn call(id: &str, argv: &[&str]) -> Value {
    json!({"tool_call_id": id, "function_name": "proc_run", "arguments": {"argv": argv}})
}

/// A trajectory of one agent step per call group, results in its observation.
fn trajectory(groups: &[&[(&[&str], &str, bool)]]) -> Value {
    let mut steps = vec![json!({"step_id": 1, "source": "user", "message": "do the task"})];
    for (g, calls) in groups.iter().enumerate() {
        let tc: Vec<Value> = calls
            .iter()
            .enumerate()
            .map(|(i, (a, _, _))| call(&format!("c{g}_{i}"), a))
            .collect();
        let res: Vec<Value> = calls
            .iter()
            .enumerate()
            .map(|(i, (_, out, err))| {
                json!({"source_call_id": format!("c{g}_{i}"), "content": out,
                       "extra": {"tool": "proc.run", "status": "ok", "is_error": err, "duration_ms": 900}})
            })
            .collect();
        steps.push(json!({"step_id": g + 2, "source": "agent", "message": "",
            "tool_calls": tc, "observation": {"results": res},
            "metrics": {"prompt_tokens": 43_000, "completion_tokens": 300, "cached_tokens": 40_000,
                        "cost_usd": 0.02, "extra": {"cache_creation_input_tokens": 2000}}}));
    }
    json!({"schema_version": "ATIF-v1.7", "session_id": "x", "steps": steps})
}

fn env<'a>(
    c: &'a dyn Fn(&str) -> bool,
    s: &'a dyn Fn(&str) -> Option<crate::rsi::mine::read::SessionInfo>,
) -> Env<'a> {
    crate::rsi::mine::fixture::env(c, s)
}

fn trial(t: &Value, suite: &str, name: &str, task: &str, vocab: &mut Vocab) -> Run {
    let clean = |_: &str| true;
    let none = |_: &str| None;
    run_of(
        t,
        suite,
        name,
        task,
        1_700_000_000_000,
        &env(&clean, &none),
        vocab,
    )
    .unwrap()
}

#[test]
fn a_trajectory_gives_the_shapes_the_ledger_gives() {
    let t = trajectory(&[&[
        (&["git", "merge", "--no-ff", "feat/a"], "Merge made", false),
        (
            &["cargo", "nextest", "run", "-p", "harbord"],
            "test result: ok",
            false,
        ),
    ]]);
    let mut v = Vocab::default();
    let r = trial(&t, "tbench", "fix-build__ab12", "fix-build", &mut v);
    let texts: Vec<&str> = r
        .calls
        .iter()
        .map(|c| v.shape(c.shape).unwrap().text.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            "proc.run[git merge --no-ff $REF]",
            "proc.run[cargo nextest run -p $CRATE]"
        ]
    );
    assert_eq!(
        (r.scope.as_str(), r.place.as_str(), r.task.as_str()),
        ("bench:tbench", "bench", "fix-build")
    );
    // The loop's cost, the parts apart: prompt = input + reads + writes.
    assert_eq!(r.loops[0].tokens, [1000, 40_000, 2000, 300]);
    assert_eq!(r.loops[0].usd_micros, 20_000);
    assert_eq!((r.loops[0].calls, r.calls[0].ms), (2, 900));
    // The same call in a ledger run has the same shape id.
    let mut l = crate::rsi::mine::fixture::Ledger::new();
    let lr = l.run("s", "d", "r:x");
    l.model(lr);
    l.argv(lr, &["git", "merge", "--no-ff", "feat/b"], 0, "");
    assert_eq!(l.runs[0].calls[0].shape, r.calls[0].shape);
}

#[test]
fn the_history_a_trajectory_is_made_from_reads_the_same() {
    let h = json!({"nodes": [
        {"kind": "assistant_message", "detail": {"tool_calls": [{"id": "a", "name": "proc_run", "input": {"argv": ["git", "pull", "--ff-only"]}}],
          "usage": {"input_tokens": 1000, "cache_read_input_tokens": 40000, "cache_creation_input_tokens": 2000, "output_tokens": 300}, "cost_usd": 0.02}},
        {"kind": "tool_result", "text": "Already up to date.", "detail": {"tool_use_id": "a", "status": "ok", "is_error": false, "duration_ms": 50}},
    ]});
    let mut v = Vocab::default();
    let r = trial(&h, "tbench", "t__1", "t", &mut v);
    assert_eq!(
        v.shape(r.calls[0].shape).unwrap().text,
        "proc.run[git pull --ff-only]"
    );
    assert_eq!(r.loops[0].tokens, [1000, 40_000, 2000, 300]);
}

#[test]
fn tool_names_on_the_wire_are_made_canonical() {
    assert_eq!(canonical("proc_run"), "proc.run");
    assert_eq!(canonical("fs_read"), "fs.read");
    assert_eq!(canonical("proc.run"), "proc.run");
}

fn trials(suite: &str, tasks: usize, n: usize, extra: &[&str]) -> (Vec<Run>, Vocab) {
    let mut v = Vocab::default();
    let mut runs = vec![];
    for i in 0..n {
        let mut c: Vec<(&[&str], &str, bool)> = vec![
            (&["git", "status", "--short"], "", false),
            (&["cargo", "build", "--release"], "ok", false),
            (&["cargo", "test", "--workspace"], "ok", false),
        ];
        if !extra.is_empty() {
            c.push((extra, "ok", false));
        }
        let t = trajectory(&[&c]);
        runs.push(trial(
            &t,
            suite,
            &format!("task{}__{i}", i % tasks),
            &format!("task{}", i % tasks),
            &mut v,
        ));
    }
    (runs, v)
}

#[test]
fn a_bench_motif_needs_four_trials_across_three_tasks() {
    let p = Params::default();
    let (r, v) = trials("tbench", 3, 4, &[]);
    let m = mine(&r, &v, &p, 1_700_000_100_000);
    assert_eq!(m.motifs.len(), 1);
    assert!(m.motifs[0].bench_born && m.motifs[0].scope == "bench:tbench");
    assert_eq!(
        (m.motifs[0].support.occurrences, m.motifs[0].support.tasks),
        (4, 3)
    );
    let (r, v) = trials("tbench", 3, 3, &[]);
    assert!(mine(&r, &v, &p, 1).motifs.is_empty(), "3 trials");
    let (r, v) = trials("tbench", 2, 6, &[]);
    assert!(mine(&r, &v, &p, 1).motifs.is_empty(), "2 tasks");
}

#[test]
fn a_bench_motif_with_a_task_path_constant_is_not_generic() {
    let p = Params::default();
    let (r, v) = trials("tbench", 3, 6, &["python", "tests/test_outputs.py"]);
    assert!(
        mine(&r, &v, &p, 1).motifs.is_empty(),
        "a task's own path is a constant"
    );
    // The same flow whose path varies is generic: an input.
    let mut v = Vocab::default();
    let mut runs = vec![];
    for i in 0..6 {
        let path = format!("work/task{i}/check.py");
        let t = trajectory(&[&[
            (&["git", "status", "--short"], "", false),
            (&["cargo", "build", "--release"], "ok", false),
            (&["cargo", "test", "--workspace"], "ok", false),
            (&["python", &path], "ok", false),
        ]]);
        runs.push(trial(
            &t,
            "tbench",
            &format!("task{}__{i}", i % 3),
            &format!("task{}", i % 3),
            &mut v,
        ));
    }
    let m = mine(&runs, &v, &p, 1);
    assert_eq!(m.motifs.len(), 1);
    assert!(m.motifs[0].inputs.iter().any(|i| i.ty == SlotType::Path));
}

#[test]
fn bench_trials_are_never_mixed_with_a_repository() {
    let (mut r, v) = trials("tbench", 3, 4, &[]);
    r.truncate(3);
    let mut l = crate::rsi::mine::fixture::Ledger::new();
    l.vocab.merge(&v);
    // A fourth trial under a private repository's key does not make four.
    let mut odd = r[0].clone();
    odd.scope = "r:0000000000feed01".into();
    odd.task = "task9".into();
    r.push(odd);
    assert!(mine(&r, &v, &Params::default(), 1).motifs.is_empty());
}

#[test]
fn a_directory_of_trials_is_read_newest_first_by_the_trial_name() {
    let d = std::env::temp_dir().join(format!("theseus-bench-{}", crate::new_id("b")));
    for (i, task) in ["alpha", "beta"].iter().enumerate() {
        let a = d.join(format!("job1/{task}__x{i}/agent"));
        std::fs::create_dir_all(&a).unwrap();
        let t = trajectory(&[&[(&["git", "status"], "", false)]]);
        std::fs::write(a.join("trajectory.json"), t.to_string()).unwrap();
    }
    let clean = |_: &str| true;
    let none = |_: &str| None;
    let mut v = Vocab::default();
    let runs = read_dir(&d.join("job1"), 0, &env(&clean, &none), &mut v);
    let mut tasks: Vec<_> = runs.iter().map(|r| r.task.clone()).collect();
    tasks.sort();
    assert_eq!(tasks, ["alpha", "beta"]);
    assert!(runs.iter().all(|r| r.scope == "bench:job1"));
}
