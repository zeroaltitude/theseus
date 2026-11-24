//! The line a `proc_run` of the daemon's own CLI search ends with (`native::hint`): it names the history
//! tool's search of this place by the same words (issue 46). Moved here from `memory.lookup`'s tests when that
//! tool went (D6 step 6): the hint is the history tool's, and stays.

use serde_json::json;

use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{
    answers_a_call, last_user, offered, result_of, rig_scripted, session, turn,
};

/// The model of the owner's DM is offered the history tool, and a proc_run
/// of the daemon's own CLI search by words ends with the line naming the
/// history tool's search of this place by those words, after the cap (issue
/// 46). The March question's first wrong turn, a search whose words name a
/// time, gets no line: the history tool's words cannot say a date (D5
/// review §4).
#[tokio::test]
async fn a_proc_run_of_the_cli_in_the_owners_dm_ends_with_the_native_call() {
    use crate::tests_places::OWNER;
    let r = rig_scripted(
        |c| c.tools.result_max_chars = 2_000,
        |req: &ProviderRequest| {
            if answers_a_call(req) {
                return Scripted::text("Done.");
            }
            let last = last_user(req);
            let argv = if last.contains("long") {
                json!([
                    "./theseus",
                    "memory",
                    "index",
                    "search",
                    "--long",
                    "ferry timetable"
                ])
            } else if last.contains("March") {
                json!([
                    "bash",
                    "-c",
                    "./theseus memory index search -k 20 --json 'March 2026' | head -5"
                ])
            } else {
                json!([
                    "bash",
                    "-c",
                    "./theseus memory index search -k 20 --json 'ferry timetable' | head -5"
                ])
            };
            Scripted::tools("Searching.", &[("s1", "proc_run", json!({ "argv": argv }))])
        },
    );
    let cli = r.root.join("theseus");
    std::fs::write(
        &cli,
        "#!/bin/sh\nif [ \"$4\" = \"--long\" ]; then seq 1 3000; fi\necho \"searched: $*\"\n",
    )
    .unwrap();
    let mut p = std::fs::metadata(&cli).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut p, 0o755);
    std::fs::set_permissions(&cli, p).unwrap();
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    assert_eq!(
        r.core.runner.class_of(&dm),
        crate::places::PlaceClass::Private
    );
    turn(&r.core, &dm, "the ferry timetable, from before?").await;
    assert!(offered(&r.requests()[0]).contains(&"context_history".to_string()));
    let want = r#"[native: context_history {"words":"ferry timetable"} searches every place's history by those words"#;
    let got = result_of(&r.core, &dm, "s1");
    assert!(
        got.contains("searched: memory index search -k 20 --json ferry timetable"),
        "{got}"
    );
    assert!(
        got.trim_end().lines().last().unwrap().starts_with(want),
        "{got}"
    );
    // A long output is cut, and the line is still its last.
    let long = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &long, "the long one").await;
    let got = result_of(&r.core, &long, "s1");
    assert!(got.contains("3000\nsearched:"), "{got}");
    assert!(got.len() < 4_000, "cut: {}", got.len());
    assert!(
        got.trim_end().lines().last().unwrap().starts_with(want),
        "{got}"
    );
    // The March question: a search whose words name a time ends with no line.
    let march = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &march, "do you remember March 2026?").await;
    let got = result_of(&r.core, &march, "s1");
    assert!(
        got.contains("searched: memory index search -k 20 --json March 2026"),
        "{got}"
    );
    assert!(!got.contains("[native:"), "{got}");
}
