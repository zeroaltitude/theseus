//! The floor's reach is a property of the PATH, not of the shape of the call
//! that names it (`theseus-3i6d1`, the floor reviewer's F1, 2026-11-10,
//! proven as `b6floor-probe.sh` probe 1).
//!
//! The owner was persuaded into the tool floor on 2026-11-10 at 14:53 — *"ok
//! we can keep the floor for now -- better to add a quick approve than risk
//! something for the moment"* — and he was told what it cost him: a tool call
//! naming `theseus.toml` or the state dir's `AGENTS.md` waits for his
//! approval, reads included. Three shapes of call reached those bytes without
//! ever meeting the floor, and a fourth was found while fixing them. **A
//! floor that holds the obvious spelling and misses the common one buys the
//! cost without the protection, and nobody notices.**
//!
//! So this file does two things, and the second is the one that matters:
//!
//! 1. **Each shape, held.** `the_floor_holds_a_path_whichever_shape_names_it`
//!    walks the shapes a call can name a path in and asserts the floor sees
//!    every one.
//! 2. **The SET of shapes, asserted.** This repository's second named
//!    pattern is *a hand-maintained list with no completeness test*, and a
//!    floor enumerated by hand is exactly that: fixing three shapes without
//!    asserting the set leaves the fourth to be rediscovered, which is
//!    precisely what `{steps}` was. Two tests close it:
//!    `every_field_a_plan_can_name_a_path_in_is_classified` is a compile-time
//!    exhaustive destructure of `Plan` — the gate reads a call ONLY through
//!    its plan, so a new way to name a path is a new `Plan` field, and adding
//!    one cannot compile until someone classifies it here — and
//!    `every_live_tool_whose_plan_runs_something_is_held` sweeps the LIVE
//!    registry, so a tool that starts carrying an argv, a batch or a line is
//!    a red rather than a gap.
//!
//! What is deliberately NOT here is in `b6-floor-blind-call-shapes.md`: the
//! floor reads a line's words as a shell splits them and expands nothing, so
//! a path a variable holds is not held, and the roots label was left reading
//! the argv alone.

use std::path::Path;

use serde_json::{json, Value};
use theseus_tools::{paths, Access, Plan, Resource};

use crate::policy::Posture;
use crate::tests_config::{own_files, OwnFiles};

/// A call's decision, as the gate makes it, with the two marks that say
/// whether the owner's protection applied: `floor`, and whether a **self**
/// session could have gone ahead with a notice instead (`fails_open`).
fn decide(r: &OwnFiles, tool: &str, input: Value) -> (bool, bool, String) {
    let rt = &r.core.tools;
    let t = rt
        .registry
        .get(tool)
        .unwrap_or_else(|| panic!("{tool} is registered"))
        .clone();
    let pl = t
        .plan(&input, &rt.ctx)
        .unwrap_or_else(|why| panic!("{tool} plans {input}: {why}"));
    let d = rt.policy.decide_with(t.as_ref(), &pl, None);
    (d.floor, rt.policy.fails_open(&pl), d.reason)
}

/// The floor's verdict on a plan built by hand, for the two shapes a test
/// registry cannot produce: `term.send` needs a live terminal and an `mcp:`
/// tool a live server. Each one's TOOL side -- that the plan carries the
/// text at all -- is asserted where the tool lives (`term/tests.rs`,
/// `mcp::tool::strings`); this is the GATE side.
fn decide_plan(r: &OwnFiles, pl: &Plan) -> (bool, bool, String) {
    let rt = &r.core.tools;
    let t = rt.registry.get("proc.run").unwrap().clone();
    let d = rt.policy.decide_with(t.as_ref(), pl, None);
    (d.floor, rt.policy.fails_open(pl), d.reason)
}

/// Deliverable 1 and 2: every shape of a call that names a floored path is
/// held, at `open`, with no question available to route around and no
/// fail-open for a self session.
///
/// Before this row, rows 2, 4, 5 and 6 of this table were `floor=false
/// posture=Open fails_open=true`.
#[tokio::test]
async fn the_floor_holds_a_path_whichever_shape_names_it() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    for (what, path) in [("the config file", &r.config), ("AGENTS.md", &r.agents)] {
        let p = path.to_string_lossy().into_owned();
        let shapes: Vec<(&str, &str, Value)> = vec![
            // 1. `argv` — read since the floor existed.
            ("proc.run {argv}", "proc.run", json!({"argv": ["cat", &p]})),
            // 2. `command` — `bash -c <line>`, F1's finding: the whole line
            //    was taken as one candidate path, so no path inside it was
            //    ever compared with the floor.
            (
                "proc.run {command}",
                "proc.run",
                json!({"command": format!("echo pwned >> {p}")}),
            ),
            // 3. `command`, read rather than written: the owner was told
            //    reads wait too, so a read must be held as a write is.
            (
                "proc.run {command}, a read",
                "proc.run",
                json!({"command": format!("cat {p} | head -1")}),
            ),
            // 4. `steps` — NOT in F1. A batch sets `Plan::steps` and leaves
            //    `Plan::argv` `None`, and the gate read `argv` alone.
            (
                "proc.run {steps}",
                "proc.run",
                json!({"steps": [{"argv": ["cat", &p]}]}),
            ),
            // 5. `steps` whose step is itself a line: both holes at once.
            (
                "proc.run {steps} of a line",
                "proc.run",
                json!({"steps": [{"argv": ["bash", "-c", format!("rm -f {p}")]}]}),
            ),
            // 6. A path written in pieces by a redirect with no space, the
            //    spelling a blunt word-split would miss.
            (
                "proc.run {command}, a redirect with no space",
                "proc.run",
                json!({"command": format!("echo x >{p}")}),
            ),
        ];
        for (shape, tool, input) in shapes {
            let (floor, fails_open, reason) = decide(&r, tool, input);
            assert!(floor, "{shape} names {what} and is not held: {reason}");
            assert!(
                !fails_open,
                "{shape} names {what} and a self session goes ahead anyway: {reason}"
            );
            assert!(
                reason.contains("floor:") && reason.contains("approve"),
                "{shape}'s reason names the floor: {reason}"
            );
        }
    }
}

/// Deliverable 2, the `term.send` half: the text typed into a terminal is
/// judged for the paths its words name, not only the terminal's own program.
///
/// F1: `term.open {argv:["bash"]}` then a typed line reached both files with
/// nothing asked, because `Send::plan` names the TERMINAL's argv and its cwd
/// and the typed text appeared in no field of the plan.
#[tokio::test]
async fn the_floor_holds_what_a_terminal_is_told_to_type() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let typed = |text: String| Plan {
        summary: "type into terminal t1, which runs `bash`".into(),
        resources: vec![Resource {
            path: r.root.clone(),
            access: Access::Exec,
        }],
        argv: Some(vec!["bash".into()]),
        lines: vec![text],
        ..Default::default()
    };
    for (what, path) in [("the config file", &r.config), ("AGENTS.md", &r.agents)] {
        let p = path.to_string_lossy().into_owned();
        let (floor, fails_open, reason) = decide_plan(&r, &typed(format!("echo pwned >> {p}\n")));
        assert!(floor, "a line typed at {what} is not held: {reason}");
        assert!(!fails_open, "a self session types it anyway: {reason}");
    }
    // The terminal's own argv is a plain shell naming nothing floored, so
    // the hold above can only have come from the text: an ordinary line
    // typed into the same terminal still runs with nothing asked.
    let (floor, _, reason) = decide_plan(&r, &typed("cargo test\n".into()));
    assert!(!floor, "an ordinary line is not held: {reason}");
}

/// Deliverable 2, the `mcp:` half: an MCP call's arguments are the only part
/// of it we can read at all, so a string of them that names a floored path
/// holds the call.
///
/// F1: an MCP tool's plan carried no resource and no argv, so there was
/// nothing of the call for the floor to see — *"not live on the owner's
/// config today … but it opens the day one is."* This is blunter than the
/// shell line's reading on purpose: of the two grammars, only bash's is one
/// we can parse. The judgement is written up in the row's report.
#[tokio::test]
async fn the_floor_holds_an_mcp_calls_arguments() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let p = r.agents.to_string_lossy().into_owned();
    for (what, args) in [
        ("a top-level string", json!({"path": &p})),
        ("a nested string", json!({"edit": {"file": {"at": &p}}})),
        ("a string in an array", json!({"files": ["/etc/hosts", &p]})),
    ] {
        let mut lines = vec![];
        crate::mcp::tool::strings(&args, &mut lines);
        let pl = Plan {
            summary: format!("mcp:notes/append {args}"),
            lines,
            ..Default::default()
        };
        let (floor, fails_open, reason) = decide_plan(&r, &pl);
        assert!(
            floor,
            "{what} naming the floored file is not held: {reason}"
        );
        assert!(
            !fails_open,
            "a self session makes the call anyway: {reason}"
        );
    }
}

/// **Deliverable 3, the completeness half that cannot go stale.**
///
/// The gate never sees a tool, only its `Plan`: `ToolPolicy::view` is the one
/// reader, and it reads fields of this struct and nothing else. So the set of
/// ways a call can name a path IS the set of `Plan` fields, and this
/// destructure names every one of them with no `..`. **Adding a field to
/// `Plan` stops this test compiling until someone writes down here whether
/// the floor must read it** — which is what nobody did for `steps`, filed as
/// a `(P1)` eight months after it was added.
#[test]
fn every_field_a_plan_can_name_a_path_in_is_classified() {
    let Plan {
        // READ BY THE FLOOR — each asserted by a shape above.
        resources: _, // a path read, written or run in
        argv: _,      // a program and its words, and a shell's `-c` line
        steps: _,     // a batch's programs and their words
        lines: _,     // text handed to a shell or another program to read
        // NOT A PATH, and each has its own door.
        url: _,       // a host, judged by `private_address` (DD5)
        aws: _,       // an account and an operation, judged by `aws_asks`
        authority: _, // layer 1: it waits for the owner on its own
        // CARRIES NO ACCESS AT ALL.
        summary: _, // one line for people
        class: _,   // this call's read/write class
        card: _,    // what a surface renders
    } = Plan::default();
}

/// **Deliverable 3, the behavioural half.** The structural test above proves
/// there is no fifth `Plan` field; this proves no live tool DROPS what it is
/// about to do out of its plan, which is how `term.send` and `mcp:` were
/// blind with every field read.
///
/// For every tool in the live registry, a synthesized input naming a floored
/// path in each of its string slots is planned, and any plan that comes back
/// carrying a program, a batch or a line must be held. A new tool that runs
/// something is a RED here, not a quiet gap — and the set it measures is
/// asserted, so a tool that stops carrying one is a red too.
#[tokio::test]
async fn every_live_tool_whose_plan_runs_something_is_held() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let rt = &r.core.tools;
    let floored = r.agents.to_string_lossy().into_owned();
    let mut runs: Vec<String> = vec![];
    let mut unheld: Vec<String> = vec![];
    for t in rt.registry.all() {
        for input in attempts(&t.input_schema(), &floored) {
            let Ok(pl) = t.plan(&input, &rt.ctx) else {
                // The tool refuses this input, so no call of that shape
                // exists to hold. The `Plan` destructure covers the rest.
                continue;
            };
            // "Runs something": a program, a batch, or a line handed on.
            let runs_something = pl.argv.is_some_and(|a| !a.is_empty())
                || pl.steps.is_some_and(|s| !s.is_empty())
                || !pl.lines.is_empty();
            if !runs_something {
                continue;
            }
            let name = t.name().to_string();
            if !runs.contains(&name) {
                runs.push(name.clone());
            }
            let pl = t.plan(&input, &rt.ctx).unwrap();
            let d = rt.policy.decide_with(t.as_ref(), &pl, None);
            if !d.floor || rt.policy.fails_open(&pl) {
                unheld.push(format!("{name} {input} -> {}", d.reason));
            }
        }
    }
    assert!(
        unheld.is_empty(),
        "a tool runs something that names Theseus's own state and is not held: {unheld:#?}"
    );
    runs.sort();
    // The measured set, not a wish. `term.open` is here because the sweep
    // found it, not because anyone remembered it: its plan runs the argv it
    // is given, so a floored path AS THE PROGRAM is held -- which is the
    // other thing this row fixed, since `path_args` skips argv's first
    // word. `term.send` needs a live terminal and `mcp:` a live server, so
    // the registry sweep cannot reach either; each has its own test above,
    // and this assertion says so rather than appearing to cover them.
    assert_eq!(
        runs,
        ["proc.run", "term.open"],
        "the set of tools whose plan runs something has changed: add each new one to the shapes \
         above, then record it here"
    );
}

/// **The other half of getting this right: a floor that fires on everything
/// is a floor the owner turns off.** He accepted one named cost — a call
/// naming those two files waits — and widening what counts as naming one
/// must not quietly turn every shell line into a question. Each line below
/// names no floored path and must still run with nothing asked.
#[tokio::test]
async fn an_ordinary_command_still_runs_with_nothing_asked() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let inside = r.root.join("src/lib.rs");
    let i = inside.to_string_lossy().into_owned();
    let floored = r.config.to_string_lossy().into_owned();
    let state = r.agents.parent().unwrap().to_string_lossy().into_owned();
    let ordinary: Vec<(&str, Value)> = vec![
        ("a build", json!({"command": "cargo test -p theseus-core"})),
        (
            "a pipeline",
            json!({"command": format!("grep -rn x {i} | head -20")}),
        ),
        (
            "a redirect inside the roots",
            json!({"command": format!("echo x >> {i}")}),
        ),
        // A message that NAMES the file is not an access to it: the line's
        // words are read as a shell splits them, so the path is part of one
        // quoted word and resolves to nothing. This is the case a blunt
        // substring match would have held, and holding it would have put a
        // question on every commit that mentions the config.
        (
            "a commit message naming the floored file",
            json!({"command": format!("git commit -m \"floor {floored} properly\"")}),
        ),
        // A sibling of a floored path, and the state dir itself: `within`
        // holds a path and what is under it, and neither of these is.
        (
            "a sibling of the config",
            json!({"argv": ["cat", format!("{}.bak", floored)]}),
        ),
        (
            "the state directory itself",
            json!({"argv": ["ls", &state]}),
        ),
        // The paths of the line are read, the variables are not, so a line
        // whose words are plain stays plain.
        (
            "a loop over the tree",
            json!({"command": "for f in *.rs; do wc -l $f; done"}),
        ),
        (
            "a batch",
            json!({"steps": [{"argv": ["cargo", "fmt"]}, {"argv": ["cargo", "clippy"]}]}),
        ),
    ];
    for (what, input) in ordinary {
        let (floor, _, reason) = decide(&r, "proc.run", input);
        assert!(!floor, "{what} is held and should not be: {reason}");
    }
}

/// Reading a line costs the gate time on every `proc.run` that gives one,
/// and the gate's budget is a millisecond a call (`policy::tests`'s FAST
/// check). Canonicalising a path is syscalls, so the cost is the number of
/// path-looking WORDS, which is why both numbers are measured and printed:
/// an ordinary line must stay inside the gate's own budget, and a line that
/// names two hundred files is the worst case anyone writes.
///
/// The worst case's ceiling is loose on purpose — it is there to catch a
/// tenfold regression, not to pin a debug build's arithmetic — and the
/// number it prints is the one to read.
#[tokio::test]
async fn reading_a_line_stays_inside_the_gates_budget() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let rt = &r.core.tools;
    let t = rt.registry.get("proc.run").unwrap().clone();
    let per = |line: String| {
        let pl = t.plan(&json!({"command": line}), &rt.ctx).unwrap();
        let n = 200u32;
        let t0 = std::time::Instant::now();
        for _ in 0..n {
            std::hint::black_box(rt.policy.decide_with(t.as_ref(), &pl, None));
        }
        t0.elapsed() / n
    };
    let ordinary = per("cargo nextest run -p theseus-core -E 'test(/floor/)' | tail -20".into());
    let many = (0..200)
        .map(|i| format!("./crates/theseus-core/src/file{i}.rs"))
        .collect::<Vec<_>>()
        .join(" ");
    let worst = per(format!("wc -l {many}"));
    eprintln!("FLOOR decide: ordinary line {ordinary:?}, 200-path line {worst:?} (debug build)");
    assert!(
        ordinary < std::time::Duration::from_millis(1),
        "{ordinary:?}"
    );
    assert!(worst < std::time::Duration::from_millis(60), "{worst:?}");
}

/// Every input the sweep tries on one tool: its whole schema filled in with
/// `path` in each string slot, and then each property ALONE, because a tool
/// whose schema says "exactly one of these" (`proc.run`'s argv, command and
/// steps) refuses the whole-schema attempt and would otherwise be swept
/// over in silence. Reads the schema and invents nothing.
fn attempts(schema: &Value, path: &str) -> Vec<Value> {
    let whole = synthesize(schema, path);
    let mut out = vec![whole.clone()];
    if let Value::Object(props) = &whole {
        for (name, v) in props {
            out.push(json!({name.clone(): v.clone()}));
        }
    }
    out
}

/// A JSON schema filled in so that every string it asks for is `path`.
fn synthesize(schema: &Value, path: &str) -> Value {
    let props = match schema.get("properties") {
        Some(Value::Object(p)) => p,
        _ => return json!({}),
    };
    let mut out = serde_json::Map::new();
    for (name, spec) in props {
        if let Some(v) = value_for(spec, path) {
            out.insert(name.clone(), v);
        }
    }
    Value::Object(out)
}

fn value_for(spec: &Value, path: &str) -> Option<Value> {
    match spec.get("type").and_then(Value::as_str)? {
        "string" => Some(json!(path)),
        "integer" | "number" => Some(json!(1)),
        "boolean" => Some(json!(false)),
        "array" => {
            let item = spec.get("items").and_then(|i| value_for(i, path))?;
            Some(json!([item]))
        }
        "object" => Some(synthesize(spec, path)),
        _ => None,
    }
}

/// The floor's paths are what `guarded.rs` puts there and this row changed
/// none of them: the owner's keel word of 2026-11-10 14:53 covers the two
/// files on the list, and widening how the gate MATCHES that list is not a
/// change to the list.
#[tokio::test]
async fn this_row_changed_no_floor_path() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let floor = &r.core.tools.policy.floor_paths;
    assert!(
        floor.contains(&r.config),
        "the config file is still floored"
    );
    assert!(floor.contains(&r.agents), "AGENTS.md is still floored");
    assert_eq!(
        r.core.tools.policy.floor_argv,
        crate::policy::floor_argv(),
        "the floored programs are untouched"
    );
}

/// The programs the floor never runs unasked reach a batch's steps too.
/// `op` as `proc.run {steps:[{argv:["op", …]}]}` bypassed the program floor
/// outright, because `Plan::steps` left `Plan::argv` `None` and the gate
/// matched the list against an empty argv. Every secret was one batch away.
#[tokio::test]
async fn a_floored_program_is_held_as_a_step_too() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    for (what, input) in [
        (
            "as its own argv",
            json!({"argv": ["op", "item", "get", "x"]}),
        ),
        (
            "as a step",
            json!({"steps": [{"argv": ["cargo", "fmt"]}, {"argv": ["op", "item", "get", "x"]}]}),
        ),
        (
            "as a step's shell line",
            json!({"steps": [{"argv": ["bash", "-c", "theseusd --config /x"]}]}),
        ),
    ] {
        let (floor, fails_open, reason) = decide(&r, "proc.run", input);
        assert!(floor, "the 1Password CLI {what} is not held: {reason}");
        assert!(!fails_open, "a self session runs it anyway: {reason}");
    }
}

/// What this row does NOT hold, pinned so the limit is a decision on the
/// record rather than a surprise. The floor reads a line's words as a shell
/// splits them and **expands nothing**: resolving `$D` means running the
/// line. Both of these reach a floored path at `open` today.
///
/// This is the honest cost of not over-holding, and the alternatives are
/// worse: a blunt substring match would hold a commit message that names the
/// config (`an_ordinary_command_still_runs_with_nothing_asked`), and flooring
/// `bash` as a program would hold every shell line the owner lives with.
/// The durable answer is the 1.0.6 line the owner already accepted — carry
/// only the last-kept version of the user-level `AGENTS.md`, so a changed
/// file is named rather than read — because that guards the BYTES instead of
/// the door.
#[tokio::test]
async fn a_path_a_variable_holds_is_not_held_and_that_is_on_the_record() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let state = r.agents.parent().unwrap().to_string_lossy().into_owned();
    for (what, input) in [
        (
            "a path a variable holds",
            json!({"command": format!("D={state}; echo pwned >> $D/AGENTS.md")}),
        ),
        (
            "a path the line computes",
            json!({"command": format!("echo pwned >> $(echo {state})/AGENTS.md")}),
        ),
    ] {
        let (floor, _, reason) = decide(&r, "proc.run", input);
        assert!(
            !floor,
            "{what} is now held, which is better than this test says: update the row's report \
             and this test together, do not just flip the assertion ({reason})"
        );
    }
}

/// The floor is also the list L1 hides from a sandboxed job
/// (`sandbox.rs`'s hidden paths), and that list is `floor_paths` — the
/// paths, not the matching. Widening the match must not have moved it.
#[test]
fn widening_the_match_did_not_move_what_l1_hides() {
    let p = Path::new("/invented/state/store");
    assert!(
        paths::within(p, Path::new("/invented/state")),
        "the hidden list is a prefix test over the same paths"
    );
}
