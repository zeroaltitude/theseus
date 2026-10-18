//! The spec's rules, without a core: what a spec may name, how it is
//! bounded, how it digests, and how its steps bind.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::spec::{bind, repo_id, InputType, Spec, StepOut, ALLOWED_TOOLS, MAX_STEPS};

/// A `repo_land`-shaped spec of invented names, scoped to the repository
/// `repo`.
pub(super) fn land(repo: &str) -> Value {
    json!({
        "v": 1, "kind": "flow", "name": "repo_land",
        "description": "Land a branch: test, merge, resolve what rerere knows, test, push.",
        "scope": {"repos": [repo], "places": ["cli", "task"]},
        "inputs": {
            "branch": {"type": "ref"},
            "test": {"type": "argv", "default": ["true"]}
        },
        "steps": [
            {"id": "test", "tool": "proc.run", "args": {"argv": "$test"},
             "on": {"err": "exit:tests-red"}},
            {"id": "merge", "tool": "proc.run",
             "args": {"argv": ["git", "merge", "--no-ff", "$branch"]},
             "on": {"conflict": "goto:rerere", "ok": "goto:retest"}},
            {"id": "rerere", "tool": "proc.run", "args": {"argv": ["git", "rerere", "remaining"]},
             "on": {"rerere-left": "handback:conflicts-left"}},
            {"id": "stage", "tool": "proc.run", "args": {"argv": ["git", "add", "-u"]}},
            {"id": "commit", "tool": "proc.run", "args": {"argv": ["git", "commit", "--no-edit"]}},
            {"id": "retest", "tool": "proc.run", "args": {"argv": "$test"},
             "on": {"err": "exit:tests-red"}},
            {"id": "push", "tool": "proc.run", "args": {"argv": ["git", "push"]}}
        ],
        "exits": {
            "ok": "landed $branch",
            "tests-red": "the tests are red: nothing of $branch is pushed",
            "handback:conflicts-left": {"state": ["conflict_files", "rerere_resolved", "output_tail"]}
        },
        "evidence": {"occurrences": 11, "sessions": 5, "days": 6, "usd_per_occ": 0.21}
    })
}

fn parse(v: &Value) -> Result<Spec, String> {
    Spec::parse(v.to_string().as_bytes())
}

fn repo() -> String {
    repo_id(std::path::Path::new("/invented/harbor"))
}

#[test]
fn the_example_spec_reads_and_seals() {
    let s = parse(&land(&repo())).unwrap();
    assert_eq!(s.steps.len(), 7);
    assert_eq!(s.inputs["test"].ty, InputType::Argv);
    let sealed = s.clone().sealed();
    let again = Spec::parse(&sealed.frozen_bytes()).unwrap();
    assert_eq!(again.digest, sealed.digest);
    assert_eq!(again.digest_hex(), s.digest_hex());
}

/// The digest follows every byte of the content, and not the key order.
#[test]
fn the_digest_follows_the_content_and_not_the_spelling() {
    let a = parse(&land(&repo())).unwrap();
    let mut v = land(&repo());
    v["steps"][5]["args"]["argv"] = json!("$test");
    assert_eq!(
        parse(&v).unwrap().digest_hex(),
        a.digest_hex(),
        "the same spec"
    );
    v["exits"]["ok"] = json!("landed $branch.");
    assert_ne!(parse(&v).unwrap().digest_hex(), a.digest_hex(), "one byte");
    // A sealed spec whose content changed no longer digests as it says.
    let mut sealed: Value = serde_json::from_slice(&a.frozen_bytes()).unwrap();
    sealed["exits"]["ok"] = json!("landed anything");
    let e = parse(&sealed).unwrap_err();
    assert!(e.contains("changed after it was sealed"), "{e}");
}

/// §5.2: a flow names only the allow-list. The message, extension, self,
/// memory-write, approval, AWS, web and file-write tools fail by name, and so
/// does a sandbox override.
#[test]
fn a_disallowed_tool_cannot_be_named() {
    for tool in [
        "channel.post",
        "extend.propose",
        "self.propose_change",
        "thread.note",
        "owner.ask",
        "aws.call",
        "aws.hands.run",
        "http.fetch",
        "web.search",
        "fs.write",
        "fs.edit",
        "fs.patch",
        "wake.at",
        "task.create",
        "routine.create",
        "term.open",
        "flow:repo_land",
    ] {
        let mut v = land(&repo());
        v["steps"][0]["tool"] = json!(tool);
        let e = parse(&v).unwrap_err();
        assert!(e.contains(tool), "{tool}: {e}");
    }
    for t in ALLOWED_TOOLS {
        let mut v = land(&repo());
        v["steps"][0]["tool"] = json!(t);
        v["steps"][0]["args"] = json!({"argv": ["true"]});
        assert!(parse(&v).is_ok(), "{t} is allowed");
    }
    for (key, val) in [
        ("sandbox", json!(true)),
        ("sandbox", json!({"egress": ["example.invalid"]})),
        ("env", json!({"TOKEN": "x"})),
        ("background", json!(true)),
        ("command", json!("echo hi")),
        ("steps", json!([{"argv": ["true"]}])),
    ] {
        let mut v = land(&repo());
        v["steps"][0]["args"] = json!({"argv": ["true"]});
        v["steps"][0]["args"][key] = val;
        let e = parse(&v).unwrap_err();
        assert!(e.contains(key), "{key}: {e}");
    }
}

#[test]
fn a_spec_is_bounded_and_every_goto_names_a_step() {
    let mut v = land(&repo());
    let many: Vec<Value> = (0..=MAX_STEPS)
        .map(|i| json!({"id": format!("s{i}"), "tool": "proc.run", "args": {"argv": ["true"]}}))
        .collect();
    v["steps"] = json!(many);
    v["inputs"] = json!({});
    v["exits"] = json!({});
    assert!(parse(&v).unwrap_err().contains("25 steps"));
    let mut v = land(&repo());
    v["steps"][1]["on"]["conflict"] = json!("goto:nowhere");
    assert!(parse(&v)
        .unwrap_err()
        .contains("goto:nowhere names no step"));
    let mut v = land(&repo());
    v["steps"][1]["on"]["conflict"] = json!("jump:rerere");
    assert!(parse(&v).is_err());
    let mut v = land(&repo());
    v["steps"][1]["on"]["flaky"] = json!("exit:ok");
    assert!(parse(&v).unwrap_err().contains("not a result class"));
}

#[test]
fn a_cycle_needs_a_bound_on_every_step_in_it() {
    let mut v = land(&repo());
    v["steps"][5]["on"] = json!({"err": "goto:merge"});
    let e = parse(&v).unwrap_err();
    assert!(e.contains("on a cycle"), "{e}");
    for i in 1..=5 {
        v["steps"][i]["max"] = json!(3);
    }
    assert!(parse(&v).is_ok(), "{:?}", parse(&v));
    v["steps"][2]["max"] = json!(99);
    assert!(parse(&v).unwrap_err().contains("max must be"));
}

#[test]
fn constants_are_not_absolute_paths_and_bindings_name_something() {
    let mut v = land(&repo());
    v["steps"][0]["args"] = json!({"argv": ["/usr/bin/true"]});
    assert!(parse(&v).unwrap_err().contains("make it an input"));
    let mut v = land(&repo());
    v["steps"][0]["args"] = json!({"argv": ["echo", "$nothing"]});
    assert!(parse(&v).unwrap_err().contains("names no input or step"));
    let mut v = land(&repo());
    v["steps"][0]["args"] = json!({"argv": ["echo", "$$"]});
    assert!(parse(&v).unwrap_err().contains("not a binding"));
    let mut v = land(&repo());
    v["steps"][1]["args"] = json!({"argv": ["echo", "$test.out"]});
    assert!(parse(&v).is_ok(), "an earlier step's output binds");
}

/// A flow is offered only where its motif was seen: it names repositories.
#[test]
fn a_scope_names_a_repository_and_real_kinds_of_place() {
    let mut v = land(&repo());
    v["scope"]["repos"] = json!([]);
    assert!(parse(&v).unwrap_err().contains("at least one repository"));
    v["scope"]["repos"] = json!(["/home/x/harbor"]);
    assert!(parse(&v).unwrap_err().contains("not a repository id"));
    let mut v = land(&repo());
    v["scope"]["places"] = json!(["moon"]);
    assert!(parse(&v).unwrap_err().contains("not a kind of place"));
}

#[test]
fn inputs_are_typed_and_never_read_as_options() {
    let s = parse(&land(&repo())).unwrap();
    let ok = s.resolve_inputs(&json!({"branch": "topic/tide"})).unwrap();
    assert_eq!(ok["test"], json!(["true"]), "the default");
    for bad in ["-rf", "a b", "", "x\0y"] {
        assert!(
            s.resolve_inputs(&json!({"branch": bad})).is_err(),
            "{bad:?}"
        );
    }
    assert!(s
        .resolve_inputs(&json!({"branch": "a", "other": 1}))
        .is_err());
    assert!(s
        .resolve_inputs(&json!({}))
        .unwrap_err()
        .contains("required"));
    assert!(s
        .resolve_inputs(&json!({"branch": "a", "test": "true"}))
        .is_err());
}

#[test]
fn bindings_fill_inputs_and_earlier_results() {
    let inputs: BTreeMap<String, Value> = [
        ("branch".to_string(), json!("topic")),
        ("test".to_string(), json!(["cargo", "nextest", "run"])),
        ("n".to_string(), json!(3)),
    ]
    .into();
    let outs: BTreeMap<String, StepOut> = [
        (
            "head".to_string(),
            StepOut {
                out: "abc123\n".into(),
                exit: Some(0),
            },
        ),
        (
            "bad".to_string(),
            StepOut {
                out: "-x".into(),
                exit: Some(1),
            },
        ),
    ]
    .into();
    let b = |v: Value| bind(&v, &inputs, &outs);
    assert_eq!(
        b(json!("$test")).unwrap(),
        json!(["cargo", "nextest", "run"])
    );
    assert_eq!(
        b(json!(["git", "merge", "$branch", "$n", "$head.out"])).unwrap(),
        json!(["git", "merge", "topic", "3", "abc123"])
    );
    assert_eq!(
        b(json!(["$test", "--x"])).unwrap(),
        json!(["cargo", "nextest", "run", "--x"])
    );
    assert_eq!(
        b(json!({"timeout_secs": "$n"})).unwrap(),
        json!({"timeout_secs": 3})
    );
    assert_eq!(b(json!("$head.exit")).unwrap(), json!(0));
    assert!(b(json!(["$bad.out"]))
        .unwrap_err()
        .contains("begins with '-'"));
    assert!(b(json!(["$later.out"]))
        .unwrap_err()
        .contains("has not run"));
}
