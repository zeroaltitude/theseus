//! The authenticator's file and the gate: a call that names it, in any shape
//! the floor knows, waits for the owner at every posture and is never one a
//! self session may go ahead with (`fails_open`), exactly as a call naming
//! the config file does (`tests_floor_shapes`). At L1 the file is not in the
//! job's view at all (`toolrun::guarded`'s own test).

use serde_json::{json, Value};

use crate::policy::Posture;
use crate::tests_config::{own_files, OwnFiles};

/// A call's decision, as the gate makes it: whether the floor held it, whether
/// a self session could have gone ahead anyway, and the reason.
fn decide(r: &OwnFiles, tool: &str, input: Value) -> (bool, bool, String) {
    let rt = &r.core.tools;
    let t = rt.registry.get(tool).unwrap().clone();
    let pl = t.plan(&input, &rt.ctx).unwrap();
    let d = rt.policy.decide_with(t.as_ref(), &pl, None);
    (d.floor, rt.policy.fails_open(&pl), d.reason)
}

/// `fs.read`, `fs.write` and `proc.run` in each shape that names a path are
/// held at the floor, at `open`, with nothing a self session can slip past.
#[tokio::test]
async fn a_call_naming_the_authenticators_file_is_held_in_every_shape() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let state = r.agents.parent().unwrap().to_path_buf();
    for name in ["owner-proof.json", "owner-proof.json.new"] {
        let path = state.join(name);
        std::fs::write(&path, "not a secret: a stand-in\n").unwrap();
        let p = path.to_string_lossy().into_owned();
        let shapes: Vec<(&str, &str, Value)> = vec![
            ("fs.read", "fs.read", json!({"path": &p})),
            ("fs.write", "fs.write", json!({"path": &p, "content": "x"})),
            ("proc.run {argv}", "proc.run", json!({"argv": ["cat", &p]})),
            (
                "proc.run {command}",
                "proc.run",
                json!({"command": format!("cat {p} | head -1")}),
            ),
            (
                "proc.run {steps}",
                "proc.run",
                json!({"steps": [{"argv": ["cat", &p]}]}),
            ),
        ];
        for (shape, tool, input) in shapes {
            let (floor, fails_open, reason) = decide(&r, tool, input);
            assert!(floor, "{shape} names {name} and is not held: {reason}");
            assert!(
                !fails_open,
                "{shape} names {name} and a self session goes ahead: {reason}"
            );
        }
    }
    // The control: a file beside it that is not on the floor is not held, so
    // the test above does not pass because every call in the state dir waits.
    let other = state.join("notes.txt").to_string_lossy().into_owned();
    std::fs::write(&other, "a quiet harbour\n").unwrap();
    let (floor, _, reason) = decide(&r, "fs.read", json!({"path": other}));
    assert!(
        !floor,
        "an ordinary file in the state dir is held: {reason}"
    );
}
