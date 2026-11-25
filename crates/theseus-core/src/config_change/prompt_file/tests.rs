//! A prompt file's change, through the whole core, on a scratch settings file,
//! a scratch state directory and files of their own (issue 23):
//!
//! - only a file the settings name is accepted: a file beside one, a link to
//!   one, a path with `..`, a relative path, a directory, the settings file,
//!   the bindings and the authenticator's file each are refused;
//! - the card is the daemon's own difference of the two texts: the changed
//!   lines whole with their context, the counts and the fingerprints, whatever
//!   the turn says; too long to show and it is a summary that says so, and the
//!   whole is readable by `config.whole`; unseen characters are escaped and counted;
//! - no yes, and no proof, writes nothing; a file edited since the card is
//!   not written; a yes writes atomically with the mode kept, keeps the copy,
//!   and the history lists it; undo puts the text back exactly;
//! - a task cannot read or propose.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde_json::{json, Value};
use theseus_protocol::{method, ConfigWholeParams};

use crate::config_change::shown;
use crate::config_change::tests::{rig_with, Rig};
use crate::node::ResultStatus;
use crate::tests_m3::{rpc_as, surface};

const PROPOSE_CALL: &str = "config_propose_file";
const READ_CALL: &str = "config_read_file";

pub(super) fn lines(n: usize) -> String {
    (1..=n)
        .map(|i| format!("Rule {i}: be careful.\n"))
        .collect()
}

pub(super) struct Files {
    pub(super) r: Rig,
    /// The file the settings name.
    pub(super) named: PathBuf,
}

/// A rig whose settings name one context file, 100 lines, mode 0640, and sit
/// beside the files a test tries to name instead.
pub(super) fn files() -> Files {
    let mut named = PathBuf::new();
    let r = rig_with(|cfg, state| {
        named = state.join("CONTEXT.md");
        std::fs::write(&named, lines(100)).unwrap();
        std::fs::set_permissions(&named, std::fs::Permissions::from_mode(0o640)).unwrap();
        std::fs::write(state.join("OTHER.md"), "not named\n").unwrap();
        std::fs::create_dir(state.join("dir")).unwrap();
        std::os::unix::fs::symlink(&named, state.join("LINK.md")).unwrap();
        // Files the settings name that a prompt file may never be.
        // (The rig has written the settings file already.)
        std::fs::write(state.join("bindings.toml"), "x = 1\n").unwrap();
        cfg.context.files = ["CONTEXT.md", "theseus.toml", "bindings.toml"]
            .iter()
            .map(|f| state.join(f).to_string_lossy().into_owned().into())
            .collect();
    });
    Files { r, named }
}

pub(super) fn replace(file: &std::path::Path, old: &str, new: &str) -> Value {
    json!({
        "file": file.to_string_lossy(),
        "replace": [{"old": old, "new": new}],
        "reason": "the owner asked for it",
    })
}

impl Files {
    pub(super) async fn propose(&self, call: Value) -> (ResultStatus, String) {
        let sid = self.r.session();
        self.r.turn_with(PROPOSE_CALL, &sid, call).await;
        self.r.result(&sid)
    }

    /// A proposal that made a card: its question.
    pub(super) async fn ask(&self, call: Value) -> theseus_protocol::ConfirmRequest {
        let (status, text) = self.propose(call).await;
        assert_eq!(status, ResultStatus::Ok, "{text}");
        self.r.asked().pop().expect("a card is waiting")
    }

    pub(super) fn text(&self) -> String {
        std::fs::read_to_string(&self.named).unwrap()
    }
}

fn cli() -> crate::approval::Client {
    use crate::approval::Surface;
    use crate::peer_proc::Behind;
    surface("sock#1", Surface::Cli).behind(Behind::Owner)
}

pub(super) async fn answer(
    r: &Rig,
    corr: &str,
    approve: bool,
    code: Option<&str>,
) -> Result<Value, theseus_protocol::RpcError> {
    rpc_as(
        &r.core,
        cli(),
        method::ACTION_CONFIRM,
        json!({"correlation_id": corr, "approve": approve, "code": code}),
    )
    .await
}

/// A file is accepted only when the settings name it, after the settings' own
/// expansion, and it is a regular file with no link at its end; never the
/// settings file, the bindings or the authenticator's file, though the
/// settings name them.
#[tokio::test]
async fn only_a_named_regular_file_is_accepted() {
    let f = files();
    let dir = f.named.parent().unwrap().to_path_buf();
    let core = &f.r.core;
    assert!(super::resolve(core, &f.named.to_string_lossy()).is_ok());
    assert!(super::resolve(core, &format!("  {}  ", f.named.display())).is_ok());
    let refused = |given: &str| super::resolve(core, given).expect_err(given);
    let not_named = "not one of the files the settings name";
    assert!(refused(&dir.join("OTHER.md").to_string_lossy()).contains(not_named));
    assert!(refused(&dir.join("dir").to_string_lossy()).contains(not_named));
    assert!(refused(&dir.join("NEW.md").to_string_lossy()).contains(not_named));
    assert!(refused(&dir.join("LINK.md").to_string_lossy()).contains(not_named));
    // A `.` component names the same path.
    assert!(super::resolve(core, &format!("{}/./CONTEXT.md", dir.display())).is_ok());
    for way_in in [
        format!("{}/dir/../CONTEXT.md", dir.display()),
        "CONTEXT.md".to_string(),
        String::new(),
        "/".to_string(),
    ] {
        assert!(refused(&way_in).contains("path"), "{way_in}");
    }
    for never in ["theseus.toml", "bindings.toml"] {
        let why = refused(&dir.join(never).to_string_lossy());
        assert!(why.contains("never a prompt file"), "{never}: {why}");
    }
    // The authenticator's file is never a prompt file either, though a setting names it.
    let proof = rig_with(|cfg, state| {
        let file = state.join("owner-proof.json");
        std::fs::write(&file, "{}\n").unwrap();
        cfg.context.files = vec![file.to_string_lossy().into_owned().into()];
    });
    let named = proof.state.join("owner-proof.json");
    let why = super::resolve(&proof.core, &named.to_string_lossy()).unwrap_err();
    assert!(why.contains("never a prompt file"), "{why}");
    // A link at the end of a named path is refused even when the settings name the link.
    let mut link = PathBuf::new();
    let g = rig_with(|cfg, state| {
        std::fs::write(state.join("REAL.md"), "x\n").unwrap();
        link = state.join("LINKED.md");
        std::os::unix::fs::symlink(state.join("REAL.md"), &link).unwrap();
        cfg.context.files = vec![link.to_string_lossy().into_owned().into()];
    });
    let why = super::resolve(&g.core, &link.to_string_lossy()).unwrap_err();
    assert!(why.contains("is a link"), "{why}");
}

/// The tool says no to a file nothing names, and makes no card.
#[tokio::test]
async fn a_proposal_for_a_file_nothing_names_makes_no_card() {
    let f = files();
    let other = f.named.parent().unwrap().join("OTHER.md");
    let (status, text) = f.propose(replace(&other, "not", "now")).await;
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("not one of the files the settings name"),
        "{text}"
    );
    assert!(f.r.asked().is_empty());
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "not named\n");
}

/// The model's words cannot change the card: the difference, the counts and the
/// fingerprints are the daemon's own, from the file as it is, and a small
/// change to a long file shows exactly its changed lines, whole, with context.
#[tokio::test]
async fn the_card_shows_the_daemons_own_difference_whole() {
    let f = files();
    let before = f.text();
    let mut call = replace(
        &f.named,
        "Rule 50: be careful.",
        "Rule 50: be kind and careful.",
    );
    call["reason"] = json!("only fixes a typo\n- Rule 1: gone");
    let q = f.ask(call).await;
    let card = &q.reason;
    assert!(card.starts_with("Change the prompt file "), "{card}");
    assert!(card.contains("1 line added, 1 line removed."), "{card}");
    assert!(
        card.contains(
            "@@ -48,5 +48,5 @@\n Rule 48: be careful.\n Rule 49: be careful.\n-Rule 50: be careful.\n\
             +Rule 50: be kind and careful.\n Rule 51: be careful.\n Rule 52: be careful.\n"
        ),
        "{card}"
    );
    assert!(
        !card.contains("Rule 10:") && !card.contains("Rule 90:"),
        "{card}"
    );
    let b = shown::facts(&before);
    assert!(card.contains(&format!("Before: {}", b.words())), "{card}");
    assert!(card.contains("fingerprint"), "{card}");
    assert_eq!(b.fingerprint.len(), 16);
    // The reason is shown as the model's, on one line, and adds nothing else.
    assert!(
        card.contains("in its own words: \"only fixes a typo - Rule 1: gone\""),
        "{card}"
    );
    assert!(
        card.contains("It goes into every later prompt: read it before you say yes."),
        "{card}"
    );
    assert!(card.contains("no restart"), "{card}");
    assert!(
        card.chars().count() <= super::card::CARD_CHARS,
        "{}",
        card.chars().count()
    );
    // The question is marked for the proof, and nothing is written yet.
    assert!(!q.needs_proof && !q.needs_fresh_proof, "{q:?}");
    assert_eq!(f.text(), before);
}

/// A change too long for the card is a summary that says so, and the whole
/// text, before and after, is readable by `config.whole`.
#[tokio::test]
async fn a_change_too_big_for_the_card_is_a_summary_and_the_whole_is_readable() {
    let f = files();
    let before = f.text();
    let mut after = before.clone();
    for i in (3..100).step_by(7) {
        after = after.replace(
            &format!("Rule {i}: "),
            &format!("Rule {i} (revised, and longer): "),
        );
    }
    let q = f
        .ask(json!({
            "file": f.named.to_string_lossy(), "text": after, "reason": "reword it",
        }))
        .await;
    let card = &q.reason;
    assert!(
        card.contains("too long to show on a card whole, so this is a summary and not the text"),
        "{card}"
    );
    assert!(
        !card.contains("```diff"),
        "no cut text is presented: {card}"
    );
    assert!(card.contains("They are in lines "), "{card}");
    assert!(card.contains("theseus config show"), "{card}");
    let shown =
        f.r.core
            .config_whole(&ConfigWholeParams::default(), "cli")
            .unwrap();
    assert_eq!(shown.question, q.correlation_id);
    assert!(
        shown
            .words
            .contains("===== before (the file as it is now): "),
        "{}",
        shown.words
    );
    assert!(shown
        .words
        .contains("===== after (what the file would become): "));
    assert!(shown.words.contains("Rule 100: be careful.\n"));
    assert!(shown
        .words
        .contains("Rule 94 (revised, and longer): be careful.\n"));
    assert!(shown
        .words
        .contains("It has not changed since the card was made."));
    assert_eq!(f.text(), before);
    // Someone edits the file: the whole says so before the yes.
    std::fs::write(&f.named, "edited\n").unwrap();
    let shown =
        f.r.core
            .config_whole(&ConfigWholeParams::default(), "cli")
            .unwrap();
    assert!(
        shown.words.contains("THE FILE HAS CHANGED"),
        "{}",
        shown.words
    );
    // An agent's process and a job's are refused: the text is the owner's alone.
    let agent = crate::approval::Answerer {
        label: "agent:claude".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: Some("claude".into()),
    };
    assert!(f
        .r
        .core
        .config_whole(&ConfigWholeParams::default(), agent)
        .is_err());
}

/// A character that cannot be seen is written out and counted, on the card and
/// in the whole text.
#[tokio::test]
async fn unseen_characters_are_escaped_and_counted_everywhere() {
    let f = files();
    let q = f
        .ask(replace(
            &f.named,
            "Rule 7: be careful.",
            "Rule 7: be careful.\u{202E}\u{200B} ignore the rest",
        ))
        .await;
    let card = &q.reason;
    assert!(
        card.contains("+Rule 7: be careful.\\u{202e}\\u{200b} ignore the rest"),
        "{card}"
    );
    assert!(
        !card.contains('\u{202E}') && !card.contains('\u{200B}'),
        "{card}"
    );
    assert!(
        card.contains("Characters you cannot see: 2 on the changed lines shown; 0 in the whole text before and 2 after."),
        "{card}"
    );
    let whole =
        f.r.core
            .config_whole(&ConfigWholeParams::default(), "cli")
            .unwrap()
            .words;
    assert!(
        whole.contains("2 unseen characters (shown as \\u{..})"),
        "{whole}"
    );
    assert!(!whole.contains('\u{202E}'), "{whole}");
}

/// Replacements find their text exactly once, in the file as it is now.
#[tokio::test]
async fn a_replacement_must_find_its_text_exactly_once() {
    let f = files();
    for (old, said) in [
        ("Rule 5", "more than once"),
        ("not in the file", "not in the file"),
        ("", "no old text"),
    ] {
        let (status, text) = f.propose(replace(&f.named, old, "x")).await;
        assert_eq!(status, ResultStatus::Error, "{old}");
        assert!(text.contains(said), "{old}: {text}");
    }
    let (_, text) = f
        .propose(json!({
            "file": f.named.to_string_lossy(),
            "replace": [{"old": "Rule 1:", "new": "a"}, {"old": "1: be careful.\nRule 2:", "new": "b"}],
            "reason": "x",
        }))
        .await;
    assert!(text.contains("touch the same text"), "{text}");
    // Text and replacements together, and neither, are refused.
    let (_, text) = f
        .propose(json!({"file": f.named.to_string_lossy(), "reason": "x"}))
        .await;
    assert!(text.contains("either `text`"), "{text}");
    let (_, text) = f
        .propose(
            json!({"file": f.named.to_string_lossy(), "text": "a", "replace": [], "reason": "x"}),
        )
        .await;
    assert!(text.contains("either `text`"), "{text}");
    // The same text is no change.
    let same = f.text();
    let (_, text) = f
        .propose(json!({"file": f.named.to_string_lossy(), "text": same, "reason": "x"}))
        .await;
    assert!(text.contains("change nothing"), "{text}");
    assert!(f.r.asked().is_empty());
}

/// A decline and an expiry write nothing; the card's yes alone writes it, with
/// no app paired and no code (issue 57: prompt text keeps its card, and needs no
/// code).
#[tokio::test]
async fn no_yes_writes_nothing_and_the_yes_alone_writes_it() {
    let f = files();
    let before = f.text();
    let call = || replace(&f.named, "Rule 20: be careful.", "Rule 20: be kind.");
    let q = f.ask(call()).await;
    answer(&f.r, &q.correlation_id, false, None).await.unwrap();
    assert_eq!(f.text(), before);
    assert_eq!(f.r.rows("config.declined")[0]["outcome"], "declined");
    let q = f.ask(call()).await;
    let ttl = f.r.core.kernel.config().confirm_ttl_ms;
    assert_eq!(f.r.core.expire_questions(q.requested_at_ms + ttl + 1), 1);
    assert_eq!(f.text(), before);
    assert!(f.r.rows("config.changed").is_empty());
    assert!(crate::config_change::write::previous_files(&f.r.state).is_empty());
    // No app paired, no code: the yes alone writes it.
    let q = f.ask(call()).await;
    assert!(!q.needs_proof, "{q:?}");
    answer(&f.r, &q.correlation_id, true, None).await.unwrap();
    assert!(f.text().contains("Rule 20: be kind."), "{}", f.text());
    assert_eq!(f.r.rows("config.changed").len(), 1);
}

/// A file edited between the card and the yes is refused and left alone.
#[tokio::test]
async fn a_file_edited_between_the_card_and_the_yes_is_refused() {
    let f = files();
    let secret = f.r.core.proofs.enrol_stand_in();
    let q = f
        .ask(replace(
            &f.named,
            "Rule 20: be careful.",
            "Rule 20: be kind.",
        ))
        .await;
    let edited = f.text().replace("Rule 3:", "Rule three:");
    std::fs::write(&f.named, &edited).unwrap();
    let code = crate::owner_proof::Proofs::stand_in_code(&secret, 0);
    answer(&f.r, &q.correlation_id, true, Some(&code))
        .await
        .unwrap();
    assert_eq!(f.text(), edited, "the owner's own edit is untouched");
    let row = &f.r.rows("config.declined")[0];
    assert_eq!(row["outcome"], "refused");
    assert!(
        row["why"]
            .as_str()
            .unwrap()
            .contains("changed after this card was made"),
        "{row}"
    );
    assert!(crate::config_change::write::previous_files(&f.r.state).is_empty());
}

/// A yes with a good code writes the text the card was built from, whole and
/// atomically, with the mode kept; the copy before is kept; history lists it;
/// the next request reads the new text; and an undo puts the old text back
/// exactly, by the same card and the same proof.
#[tokio::test]
async fn a_yes_writes_it_keeps_the_copy_and_an_undo_restores_it_exactly() {
    let f = files();
    let secret = f.r.core.proofs.enrol_stand_in();
    let before = f.text();
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
    let after = before.replace("Rule 20: be careful.", "Rule 20: be kind.");
    assert_eq!(f.text(), after);
    assert_eq!(
        std::fs::metadata(&f.named).unwrap().permissions().mode() & 0o7777,
        0o640
    );
    let left: Vec<_> = std::fs::read_dir(f.named.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "no temporary file is left: {left:?}");
    let kept = crate::config_change::write::previous_files(&f.r.state);
    assert_eq!(kept.len(), 1);
    assert_eq!(std::fs::read_to_string(&kept[0].1).unwrap(), before);
    let row = &f.r.rows("config.changed")[0];
    assert_eq!(row["file"]["path"], json!(f.named.to_string_lossy()));
    assert_eq!(row["restart"], false);
    assert_eq!(row["digest_before"], crate::config_copy::sha256(&before));
    assert_eq!(row["digest_after"], crate::config_copy::sha256(&after));
    assert!(
        !crate::config_change::restart::marker_path(&f.r.state).exists(),
        "nothing restarts"
    );
    // The compiler's own reader sees the new text at its next request.
    let paths = f.r.core.cfg.context_paths(&f.r.core.personas(), None, true);
    let (sections, _) = f.r.core.runner.context_files.load(&paths);
    assert!(
        sections[0].section.contains("Rule 20: be kind."),
        "{}",
        sections[0].section
    );
    // The history lists it.
    let said =
        f.r.core
            .config_history(&theseus_protocol::ConfigHistoryParams::default(), "cli")
            .unwrap()
            .words;
    assert!(
        said.contains("#1 ") && said.contains("the prompt file "),
        "{said}"
    );
    assert!(said.contains("1 line added, 1 line removed"), "{said}");
    // The undo: the same card, the same proof, the exact text before.
    let sid = f.r.session();
    f.r.turn_with("config_undo", &sid, json!({"reason": "put it back"}))
        .await;
    let (status, text) = f.r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let u = f.r.asked().pop().unwrap();
    assert!(
        u.reason
            .starts_with("Undo change #1: put back the prompt file "),
        "{}",
        u.reason
    );
    assert!(
        u.reason
            .contains("-Rule 20: be kind.\n+Rule 20: be careful.\n"),
        "{}",
        u.reason
    );
    assert!(!u.reason.contains("changed again since"), "{}", u.reason);
    assert!(!u.needs_proof);
    let code = crate::owner_proof::Proofs::stand_in_code(&secret, 1);
    answer(&f.r, &u.correlation_id, true, Some(&code))
        .await
        .unwrap();
    assert_eq!(f.text(), before, "exactly the text before");
    assert_eq!(f.r.rows("config.undone").len(), 1);
}

/// An undo says so when the file changed again since the change it undoes.
#[tokio::test]
async fn an_undo_says_the_file_changed_again_since() {
    let f = files();
    let secret = f.r.core.proofs.enrol_stand_in();
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
    let later = f
        .text()
        .replace("Rule 90: be careful.", "Rule 90: be gentle.");
    std::fs::write(&f.named, &later).unwrap();
    // The owner accepts his later edit at his terminal: an undo, as a
    // change, is asked only on the version he accepted.
    super::tests_held::owner_accepts(&f.r.core, &f.named).await;
    let sid = f.r.session();
    f.r.turn_with("config_undo", &sid, json!({})).await;
    let (status, text) = f.r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let u = f.r.asked().pop().unwrap();
    assert!(u.reason.contains("changed again since"), "{}", u.reason);
    assert!(u.reason.contains("-Rule 90: be gentle."), "{}", u.reason);
    assert_eq!(f.text(), later);
}

/// `config.read_file` lists the files that can be changed and gives one whole.
#[tokio::test]
async fn the_read_gives_the_named_files_and_one_whole() {
    let f = files();
    let sid = f.r.session();
    f.r.turn_with(READ_CALL, &sid, json!({})).await;
    let (status, text) = f.r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("CONTEXT.md") && text.contains("system context file"),
        "{text}"
    );
    for never in ["theseus.toml", "bindings.toml", "OTHER.md"] {
        assert!(!text.contains(never), "{never}: {text}");
    }
    let sid = f.r.session();
    f.r.turn_with(READ_CALL, &sid, json!({"file": f.named.to_string_lossy()}))
        .await;
    let (status, text) = f.r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("Rule 100: be careful.\n\n-----")
            || text.contains("Rule 100: be careful.\n-----"),
        "{text}"
    );
    assert!(
        text.contains(&shown::facts(&f.text()).fingerprint),
        "{text}"
    );
}

/// A task's session can neither propose nor read.
#[tokio::test]
async fn a_task_can_neither_propose_nor_read() {
    let f = files();
    let task = || {
        let parent = f.r.session();
        let mut rec =
            crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        rec.task = Some(crate::session::TaskOf {
            parent_session: parent,
            parent_execution: "exe_x".into(),
            by: "tool_x".into(),
            target: None,
            arrangement: None,
            check: None,
            notify_done: false,
        });
        f.r.core.store.put_session(&rec.session_id, &rec).unwrap();
        rec.session_id
    };
    let one = task();
    f.r.turn_with(
        PROPOSE_CALL,
        &one,
        replace(&f.named, "Rule 20:", "Rule 21:"),
    )
    .await;
    let (status, text) = f.r.result(&one);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("not for a task"), "{text}");
    assert!(
        text.contains(super::PROPOSE),
        "it names its own tool: {text}"
    );
    let two = task();
    f.r.turn_with(READ_CALL, &two, json!({})).await;
    let (status, text) = f.r.result(&two);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("not for a task"), "{text}");
    assert!(f.r.asked().is_empty());
}

/// A persona's file may not grow past the persona cap; it may shrink.
#[test]
fn a_persona_file_may_not_grow_past_its_cap() {
    let f = files();
    let named = super::Named {
        expanded: f.named.clone(),
        path: f.named.clone(),
        levels: vec![crate::context_files::Level::Persona("warm".into())],
        public: false,
    };
    let cap = f.r.core.cfg.context.persona_cap();
    let big = "x".repeat(cap as usize * 8);
    let why = super::put::refusal(&f.r.core, &named, "small\n", &big).expect("over the cap");
    assert!(why.contains("over the cap"), "{why}");
    assert_eq!(
        super::put::refusal(&f.r.core, &named, &big, &big[..big.len() - 10]),
        None
    );
    let system = super::Named {
        levels: vec![crate::context_files::Level::System],
        ..named
    };
    assert_eq!(
        super::put::refusal(&f.r.core, &system, "small\n", &big),
        None
    );
    let huge = "y".repeat(super::MAX_FILE_BYTES + 1);
    assert!(super::put::refusal(&f.r.core, &system, "a", &huge)
        .unwrap()
        .contains("KiB"));
}

/// The copies a prompt file's change keeps, and the texts a waiting card keeps,
/// are in the directory the floor holds: a tool reads or writes none of them.
#[test]
fn the_kept_copies_of_a_prompt_file_are_on_the_floor() {
    let f = files();
    let floor = &f.r.core.tools.ctx.floor;
    let held = |p: &std::path::Path| {
        let p = theseus_tools::paths::canonical_best_effort(p);
        floor.iter().any(|x| theseus_tools::paths::within(&p, x))
    };
    let dir = crate::config_change::write::dir(&f.r.state);
    for name in [
        "previous-0001.txt",
        "file-base-abc.txt",
        "file-new-abc.txt",
        "settings-base-abc.txt",
    ] {
        assert!(
            held(&dir.join(name)),
            "{name} is not under the floor {floor:?}"
        );
    }
}
