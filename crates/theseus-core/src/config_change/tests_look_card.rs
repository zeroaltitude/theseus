//! The card for a held shell command (issue 26, line 4): which program or path
//! the floor found, and that a look has a tool, through the real gate.

use serde_json::json;

use super::tests::rig;

/// The card for a held shell command says which program or path the floor found
/// and that a look has a tool, in `plain.held` (what Discord draws) and in
/// `plain.why` (what the terminal's card prints), from the floor's own
/// sentence; a command the floor holds for another reason keeps the old words.
/// Since issue 57 the owner's own `theseusd` command runs in the sandbox and is
/// held only when it says `sandbox: false`
/// ([`the_owners_theseusd_command_is_not_held`]); held so, its card is as it was.
#[tokio::test]
async fn a_held_shell_command_says_which_program_and_where_the_look_is() {
    let cases = [
        ("op whoami", "(`op`)", "`look.vault`", false),
        ("theseusd --version", "(`theseusd`)", "`look.theseus`", true),
        ("FILE", "settings file", "`config.show`", false),
    ];
    for (command, names, tool, at_l0) in cases {
        let r = rig();
        let sid = r.session();
        let command = command.replace("FILE", &format!("cat {}", r.file.display()));
        let mut input = json!({"argv": ["bash", "-c", command]});
        if at_l0 {
            input["sandbox"] = json!(false);
        }
        r.turn_with("proc_run", &sid, input).await;
        let asks = r.asked();
        assert_eq!(asks.len(), 1, "{command}: {asks:?}");
        let p = asks[0].plain.as_ref().expect("plain words");
        assert!(asks[0].floor);
        for said in [&p.held, &p.why] {
            assert!(
                said.contains(names) && said.contains(tool),
                "{command}: {said}"
            );
            assert!(
                !said.contains("/theseus.toml"),
                "no path is repeated: {said}"
            );
        }
        assert!(p.held.contains("always asks"), "{}", p.held);
    }
}

/// Issue 57: the owner's own `theseusd` command, with no `sandbox` key, is not
/// held where L1 was seen to run: it runs in the sandbox, so no card is drawn
/// for it.
#[tokio::test]
async fn the_owners_theseusd_command_is_not_held() {
    let r = rig();
    r.core.tools.sandbox.set_l1_verified(true);
    let sid = r.session();
    r.turn_with(
        "proc_run",
        &sid,
        json!({"argv": ["bash", "-c", "theseusd --version"]}),
    )
    .await;
    assert!(r.asked().is_empty(), "{:?}", r.asked());
}
