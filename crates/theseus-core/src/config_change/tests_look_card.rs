//! The card for a held shell command (issue 26, line 4): which program or path
//! the floor found, and that a look has a tool, through the real gate.

use serde_json::json;

use super::tests::rig;

/// The card for a held shell command says which program or path the floor found
/// and that a look has a tool, in `plain.held` (what Discord draws) and in
/// `plain.why` (what the terminal's card prints), from the floor's own
/// sentence; a command the floor holds for another reason keeps the old words.
#[tokio::test]
async fn a_held_shell_command_says_which_program_and_where_the_look_is() {
    let cases = [
        ("op whoami", "(`op`)", "`look.vault`"),
        ("theseusd --version", "(`theseusd`)", "`look.theseus`"),
        ("FILE", "settings file", "`config.show`"),
    ];
    for (command, names, tool) in cases {
        let r = rig();
        let sid = r.session();
        let command = command.replace("FILE", &format!("cat {}", r.file.display()));
        r.turn_with("proc_run", &sid, json!({"argv": ["bash", "-c", command]}))
            .await;
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
