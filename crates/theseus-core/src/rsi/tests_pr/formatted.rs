//! An attempt formats the tree before it commits, so the gate never sees unformatted code
//! (issue 6, condition 2), through the whole `self.propose_change` call.

use super::*;
use crate::rsi::tests_tree::git;

/// KEY: what the model left unformatted is what the commit holds, formatted, and the commit
/// says it was.
#[tokio::test]
async fn an_unformatted_edit_is_committed_formatted() {
    let r = rig(SelfMode::Act, None);
    let sid = r.session("self");
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("Tree open for trim-the-sails"), "{said}");
    let tree =
        r.f.place
            .trees()
            .canonicalize()
            .unwrap()
            .join("trim-the-sails");
    std::fs::create_dir_all(tree.join("src")).unwrap();
    std::fs::write(
        tree.join("Cargo.toml"),
        "[package]\nname = \"mast\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(tree.join("src/lib.rs"), "pub fn sail( a:u32 )->u32{a}\n").unwrap();
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is green"), "{said}");
    let branch = r.rows()[0]["branch"].as_str().unwrap().to_string();
    let repo = &r.f.place.repo;
    assert_eq!(
        git(repo, &["show", &format!("{branch}:src/lib.rs")]),
        "pub fn sail(a: u32) -> u32 {\n    a\n}"
    );
    let message = git(repo, &["log", "-1", "--format=%B", &branch]);
    assert!(
        message.contains("Formatted before the gate: src/lib.rs."),
        "{message}"
    );
}
