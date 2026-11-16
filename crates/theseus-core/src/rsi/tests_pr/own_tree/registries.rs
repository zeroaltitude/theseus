//! A self builder's command may reach the registries (issue 20, "The builder cannot try a
//! library the machine does not already hold"): its `sandbox.started` row says so, and no
//! other session's command is given them.

use super::*;

/// The egress list of every `sandbox.started` row, oldest first.
fn started(r: &Rig) -> Vec<Vec<String>> {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == "sandbox.started")
        .map(|(_, row)| {
            row.data["egress"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| h.as_str().unwrap().to_string())
                .collect()
        })
        .collect()
}

/// KEY: the builder's own L1 command is given the registries' hosts and a package cache of its
/// own; the same command from a session that is not a self builder's has no network, as before.
#[tokio::test]
async fn a_self_builders_command_may_reach_the_registries_and_no_other_session_gets_them() {
    let r = held(|_| {});
    let (sid, tree) = task(&r, "sails");
    std::fs::create_dir(tree.join(".git")).unwrap();
    // The test rig has no L1 to start, and says so in the result; the row is written before.
    r.turn(
        &sid,
        "proc_run",
        json!({"argv": ["echo", "tried"], "cwd": tree, "sandbox": true}),
    )
    .await;
    let other = r.session("someone");
    r.turn(
        &other,
        "proc_run",
        json!({"argv": ["echo", "no"], "cwd": tree, "sandbox": true}),
    )
    .await;
    let lists = started(&r);
    assert_eq!(lists.len(), 2, "{lists:?}");
    assert_eq!(
        lists[0],
        [
            "index.crates.io:443",
            "static.crates.io:443",
            "registry.npmjs.org:443"
        ]
    );
    assert!(
        lists[1].is_empty(),
        "no other session reaches them: {:?}",
        lists[1]
    );
    let state = r.f.place.state.canonicalize().unwrap();
    assert!(
        state.join("self/builder/sails/.packages").is_dir(),
        "its cache is inside the item's build folder"
    );
    // A task whose tree is not open is no builder yet: nothing is made for it.
    let (closed, tides) = task(&r, "tides");
    r.turn(
        &closed,
        "proc_run",
        json!({"argv": ["echo", "no"], "cwd": tides, "sandbox": true}),
    )
    .await;
    assert!(started(&r)[2].is_empty());
    assert!(!state.join("self/builder/tides").exists());
}
