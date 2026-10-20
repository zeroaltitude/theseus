use serde_json::json;

use super::*;

struct Rig {
    _d: tempfile::TempDir,
    state: PathBuf,
    store: Store,
}

fn state() -> Rig {
    let d = tempfile::tempdir().unwrap();
    let state = d.path().canonicalize().unwrap();
    for sub in ["self/reports", "self/gates"] {
        std::fs::create_dir_all(state.join(sub)).unwrap();
    }
    let store = Store::open(&state.join("store")).unwrap();
    Rig {
        _d: d,
        state,
        store,
    }
}

fn put(state: &Path, rel: &str, body: &str) {
    std::fs::write(state.join(rel), body).unwrap();
}

impl Rig {
    fn call(&self, input: serde_json::Value) -> Result<String, String> {
        run(
            &self.state,
            &self.store,
            theseus_protocol::now_unix_ms(),
            &input,
        )
        .map(|(t, _)| t)
    }
}

/// An item's earlier reports are listed by name, one is read by name, and
/// the others' are not in the listing.
#[test]
fn an_items_earlier_reports_are_listed_and_read() {
    let r = state();
    let state = r.state.clone();
    put(
        &state,
        "self/reports/self-20261020-tsk-0a1b.md",
        "first run\n",
    );
    put(
        &state,
        "self/reports/self-20261020-tsk-0a1b-2.md",
        "second run\n",
    );
    put(
        &state,
        "self/reports/self-20261020-tsk-0c2d.md",
        "another item\n",
    );
    let said = r
        .call(json!({"kind": "reports", "item": "tsk_0a1b"}))
        .unwrap();
    assert!(said.starts_with("2 reports"), "{said}");
    assert!(said.contains("self-20261020-tsk-0a1b-2.md"), "{said}");
    assert!(!said.contains("0c2d"), "{said}");
    let one = r
        .call(json!({"kind": "reports", "item": "tsk_0a1b", "name": "self-20261020-tsk-0a1b-2.md"}))
        .unwrap();
    assert_eq!(one, "second run\n");
    let none = r
        .call(json!({"kind": "reports", "item": "tsk_9999"}))
        .unwrap();
    assert_eq!(none, "no reports yet");
}

/// The tool never leaves the two folders it reads: a name with a slash, a
/// `..` or a leading dot is refused, and a link out is not followed by name.
#[test]
fn a_name_never_leaves_the_reports_and_gates() {
    let r = state();
    let state = r.state.clone();
    put(&state, "secret.txt", "not for the model\n");
    put(&state, "self/gates/g.sh", "echo gate\n");
    for bad in [
        "../../secret.txt",
        "../secret.txt",
        "/etc/passwd",
        ".hidden",
        "..",
    ] {
        for kind in ["gates", "reports"] {
            let got = r.call(json!({"kind": kind, "item": "x", "name": bad}));
            assert!(got.is_err(), "{kind} {bad}: {got:?}");
        }
    }
    assert_eq!(
        r.call(json!({"kind": "gates", "name": "g.sh"})).unwrap(),
        "echo gate\n"
    );
    let list = r.call(json!({"kind": "gates"})).unwrap();
    assert!(list.contains("g.sh") && !list.contains("secret"), "{list}");
    // The input is checked: a cause needs its key, a report its item.
    assert!(r.call(json!({"kind": "cause"})).is_err());
    assert!(r.call(json!({"kind": "reports"})).is_err());
    assert!(r.call(json!({"kind": "store"})).is_err());
    assert!(r.call(json!({"kind": "gates", "path": "/"})).is_err());
}

/// A brief hands an item that has run before the paths of its reports.
#[test]
fn a_brief_names_the_items_earlier_reports() {
    let r = state();
    let state = &r.state;
    put(
        state,
        "self/reports/self-20261020-tsk-0a1b.md",
        "first run\n",
    );
    put(state, "self/gates/self-20261020-tsk-0a1b.log", "gate log\n");
    let brief = briefing(state, "tsk_0a1b");
    assert!(brief.contains("self.evidence"), "{brief}");
    assert!(
        brief.contains(
            &state
                .join("self/reports/self-20261020-tsk-0a1b.md")
                .display()
                .to_string()
        ),
        "{brief}"
    );
    assert!(brief.contains("Earlier gate logs"), "{brief}");
    // An item that never ran has the pointer and no list.
    let fresh = briefing(state, "tsk_beef");
    assert!(
        fresh.contains("self.evidence") && !fresh.contains("Earlier"),
        "{fresh}"
    );
}
