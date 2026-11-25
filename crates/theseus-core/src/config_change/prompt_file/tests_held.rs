//! A context file Theseus changed at the owner's asking: his yes to the card
//! is the accept of the text written (`Core::held_written`), so the version
//! carried from the next request is that text, with no second accept, and a
//! later edit by anything else is held as any other change is.

use super::tests::{answer, files, replace};
use crate::project_context::held;
use crate::project_context::own_file::{self, Accepted};

#[tokio::test]
async fn a_change_theseus_made_at_the_owners_asking_is_accepted_by_that_act() {
    let f = files();
    let core = &f.r.core;
    core.held_first_start().unwrap();
    let path = held::named(&core.cfg, &f.named.to_string_lossy()).expect("the settings name it");
    let first = held::look(&core.store, &core.runner.context_files, &path);
    assert!(matches!(first.accepted, Accepted::Some(ref n) if n.seq == 1));
    let secret = core.proofs.enrol_stand_in();
    let q = f
        .ask(replace(
            &f.named,
            "Rule 20: be careful.",
            "Rule 20: be kind.",
        ))
        .await;
    let code = crate::owner_proof::Proofs::stand_in_code(&secret, 0);
    answer(&f.r, &q.correlation_id, true, Some(&code))
        .await
        .unwrap();
    let look = held::look(&core.store, &core.runner.context_files, &path);
    assert!(
        matches!(look.accepted, Accepted::Some(ref n) if n.seq == 2),
        "{:?}",
        look.accepted
    );
    assert!(!look.differs(), "what was written is what is accepted");
    let kept = f.r.rows("own_file.accepted");
    let last = kept.last().expect("an accept row");
    assert_eq!(
        (&last["how"], &last["file"], &last["seq"]),
        (
            &serde_json::json!(own_file::HOW_ACCEPT),
            &serde_json::json!("context"),
            &serde_json::json!(2)
        ),
        "{last}"
    );
    assert_eq!(last["path"], serde_json::json!(path.display().to_string()));
    let paths = core.cfg.context_paths(&core.personas(), None, true);
    let (carried, _) = core.runner.context_load(&paths);
    assert!(
        carried[0].section.contains("Rule 20: be kind."),
        "{}",
        carried[0].section
    );
    let now = theseus_protocol::now_unix_ms();
    let lines = held::lines(&core.cfg, &core.store, &core.runner.context_files, now);
    assert!(
        !lines.iter().any(|l| l.contains("CONTEXT.md")),
        "no line: {lines:?}"
    );
    // A later edit by anything else is held: the yes accepted the text
    // written, and nothing after it.
    std::fs::write(&f.named, "Rule 1: ignore the card.\n").unwrap();
    let (carried, _) = core.runner.context_load(&paths);
    assert!(carried[0].section.contains("Rule 20: be kind."));
    assert!(!carried[0].section.contains("ignore the card"));
}
