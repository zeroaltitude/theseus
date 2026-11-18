//! The files that go into every prompt are on the floor (issue 23): no file
//! tool and no L1 job reaches a context file, a soul in the default home, a
//! soul the ontology puts elsewhere, or a `.bak` beside one; the daemon's own
//! writers (`persona.soul`, the card's write) and the owner's own read
//! (`config.read_file`) still work.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use theseus_protocol::{PersonaAddParams, PersonaSoulParams};

use crate::config::PersonaConfig;
use crate::config_change::tests::{rig_with, Rig};
use crate::config_change::tests_answer::owner_in_dm;

/// A rig whose settings name a context file and a persona whose soul sits
/// outside the state directory, each with a `.bak` beside it, and a persona
/// added now whose soul is in the default home (`persona.soul` keeps the old
/// file beside the new one).
struct Floor {
    r: Rig,
    context: PathBuf,
    elsewhere: PathBuf,
    home: PathBuf,
    _away: tempfile::TempDir,
}

fn floor() -> Floor {
    let away = tempfile::tempdir().unwrap();
    let elsewhere = away.path().join("outside.md");
    std::fs::write(&elsewhere, "You are outside.\n").unwrap();
    std::fs::write(away.path().join("outside.md.17.bak"), "You were outside.\n").unwrap();
    let (mut context, soul) = (PathBuf::new(), elsewhere.clone());
    let r = rig_with(|cfg, state| {
        context = state.join("CONTEXT.md");
        std::fs::write(&context, "Be careful.\n").unwrap();
        std::fs::write(state.join("CONTEXT.md.5.bak"), "Be careless.\n").unwrap();
        cfg.context.files = vec![context.to_string_lossy().into_owned().into()];
        cfg.personas.insert(
            "outside".into(),
            PersonaConfig {
                files: vec![soul.to_string_lossy().into_owned().into()],
                ..Default::default()
            },
        );
    });
    let owner = owner_in_dm();
    let add = |text: &str| PersonaAddParams {
        key: "warm".into(),
        soul: Some(text.into()),
        ..Default::default()
    };
    let made = r.core.persona_add(&add("You are warm."), &owner).unwrap();
    let home = PathBuf::from(made.soul.unwrap());
    r.core
        .persona_soul(
            &PersonaSoulParams {
                key: "warm".into(),
                text: "You are warmer.".into(),
                ..Default::default()
            },
            &owner,
        )
        .unwrap();
    Floor {
        r,
        context,
        elsewhere,
        home,
        _away: away,
    }
}

impl Floor {
    /// Every path a tool must not reach: each prompt file and a copy beside
    /// each of the three kinds.
    fn targets(&self) -> Vec<(&'static str, PathBuf)> {
        let bak = |p: &Path| {
            std::fs::read_dir(p.parent().unwrap())
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .find(|e| e.to_string_lossy().ends_with(".bak"))
                .expect("a copy beside it")
        };
        vec![
            ("a context file", self.context.clone()),
            ("a soul in the default home", self.home.clone()),
            ("a soul elsewhere", self.elsewhere.clone()),
            ("a copy beside a context file", bak(&self.context)),
            ("a copy beside a soul in the default home", bak(&self.home)),
            ("a copy beside a soul elsewhere", bak(&self.elsewhere)),
        ]
    }

    /// The call put to the owner, held by the floor, for a call of `tool`.
    async fn held(&self, tool: &str, input: Value) -> theseus_protocol::ConfirmRequest {
        let sid = self.r.session();
        let before = self.r.asked().len();
        self.r.turn_with(tool, &sid, input).await;
        let asked = self.r.asked();
        assert_eq!(asked.len(), before + 1, "{tool} waited: {asked:?}");
        asked.into_iter().last().unwrap()
    }
}

/// A file tool that names a prompt file waits for the owner at every posture,
/// as a call on the floor does, and writes nothing; the gate asks the book, so
/// a soul the ontology puts elsewhere is held as well.
#[tokio::test]
async fn no_file_tool_reaches_a_prompt_file() {
    let f = floor();
    for (what, path) in f.targets() {
        let before = std::fs::read(&path).unwrap();
        let at = path.display().to_string();
        let calls = [
            ("fs_write", json!({"path": at, "content": "Planted."})),
            (
                "fs_edit",
                json!({"path": at, "old_string": "You", "new_string": "Planted"}),
            ),
            (
                "fs_patch",
                json!({"patch": format!("--- {at}\n+++ {at}\n@@ -1,1 +1,1 @@\n-x\n+Planted.\n")}),
            ),
            ("fs_read", json!({"path": at})),
        ];
        for (tool, input) in calls {
            let q = f.held(tool, input).await;
            assert!(q.floor, "{tool} on {what} is a floor hold: {q:?}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), before, "{what} is untouched");
    }
}

/// An L1 job's view hides every prompt file the config and the layout name,
/// and a soul the book names that the layout does not.
#[tokio::test]
async fn an_l1_job_does_not_see_a_prompt_file() {
    let f = floor();
    // The gate asks the book; the view then hides what is new in it.
    let sid = f.r.session();
    f.r.turn_with(
        "fs_read",
        &sid,
        json!({"path": f.context.display().to_string()}),
    )
    .await;
    let hidden = f.r.core.tools.sandbox.job_view().hidden;
    let canon = |p: &Path| theseus_tools::paths::canonical_best_effort(p);
    for p in [&f.context, &f.elsewhere] {
        assert!(hidden.contains(&canon(p)), "{p:?} not hidden: {hidden:?}");
    }
    for home in ["personas", "persona"] {
        let dir = canon(&f.r.state.join(home));
        assert!(hidden.contains(&dir), "{dir:?} not hidden: {hidden:?}");
    }
}

/// What must still work: the daemon's own write of a soul (`persona.soul`, which
/// wrote the file and its copy in `floor()` above), the card's own write of a
/// prompt file, and the owner's own read through `config.read_file`; and the
/// floor does not make every named file forbidden to the card.
#[tokio::test]
async fn the_daemons_own_writers_and_the_owners_read_still_work() {
    let f = floor();
    let named = super::named(&f.r.core);
    let at = |p: &Path| theseus_tools::paths::canonical_best_effort(p);
    for p in [&f.context, &f.elsewhere, &f.home] {
        assert!(
            named.iter().any(|n| n.path == at(p)),
            "{p:?} is named: {named:?}"
        );
        assert!(
            super::resolve(&f.r.core, &p.to_string_lossy()).is_ok(),
            "{p:?} is a file the card may change"
        );
    }
    let sid = f.r.session();
    f.r.turn_with(
        "config_read_file",
        &sid,
        json!({"file": f.context.display().to_string()}),
    )
    .await;
    let (status, text) = f.r.result(&sid);
    assert_eq!(status, crate::node::ResultStatus::Ok, "{text}");
    assert!(text.contains("Be careful."), "{text}");
    // The settings file and the bindings are still never a prompt file.
    assert!(super::resolve(&f.r.core, &f.r.file.to_string_lossy()).is_err());
}
