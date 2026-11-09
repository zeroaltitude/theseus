//! `@` files (theseus-vjzt, phase 1): the listing's bounds, the picker, and
//! what a message that names a file sends.

use std::path::Path;
use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::json;

use super::atfile::{mentions, walk};
use super::tests::{script, Rig};

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, SENTINEL).unwrap();
}

/// What every fixture file holds: a word that must never reach a turn.
const SENTINEL: &str = "pelican-ledger-9137";

/// A project: a few sources, and things the listing must leave out.
fn project() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for f in [
        "README.md",
        "src/lib.rs",
        "src/tides/model.rs",
        "src/tides/mod.rs",
        "docs/notes.md",
        ".git/config",
        "node_modules/dep/index.js",
        "target/debug/out",
        "with space.txt",
    ] {
        touch(d.path(), f);
    }
    d
}

fn roots(r: &[&str]) -> Vec<String> {
    r.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn the_listing_stays_inside_the_roots_and_inside_the_directory() {
    let p = project();
    let outside = tempfile::tempdir().unwrap();
    touch(outside.path(), "secret.txt");
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), p.path().join("link.txt"))
        .unwrap();
    std::os::unix::fs::symlink(outside.path(), p.path().join("linked_dir")).unwrap();

    let here = p.path().display().to_string();
    let w = walk(p.path(), &roots(&[&here])).unwrap();
    assert_eq!(
        w.files,
        [
            "README.md",
            "docs/notes.md",
            "src/lib.rs",
            "src/tides/mod.rs",
            "src/tides/model.rs",
        ],
        "sources only: no .git, node_modules, target, symlink, or name with a space"
    );
    assert!(!w.truncated);

    // A root above the directory still holds it; a root beside it does not.
    let above = p.path().parent().unwrap().display().to_string();
    assert_eq!(walk(p.path(), &roots(&[&above])).unwrap().files.len(), 5);
    let beside = outside.path().display().to_string();
    let err = walk(p.path(), &roots(&[&beside])).unwrap_err();
    assert!(err.contains("outside the workspace roots"), "{err}");
    let err = walk(p.path(), &[]).unwrap_err();
    assert!(err.contains("no workspace root"), "{err}");
}

#[test]
fn only_words_that_start_with_one_at_name_a_file() {
    assert_eq!(
        mentions("see @src/lib.rs, and @README.md? also @@literal and a lone @ and me@x.org"),
        ["src/lib.rs", "README.md"]
    );
    assert!(mentions("nothing here").is_empty());
}

fn tools_script(root: String) -> crate::tests::Script {
    let base = script();
    Arc::new(move |m, p| {
        if m == "tool.list" {
            return Ok(json!({
                "tools": [], "roots": [root], "shell_fallback_ratio": 0.0, "calls_total": 0
            }));
        }
        base(m, p)
    })
}

async fn rig_in(p: &tempfile::TempDir, root: &str) -> Rig {
    let dir = p.path().display().to_string();
    let mut r = Rig::with_cfg(100, 40, tools_script(root.to_string()), move |c| {
        c.dir = dir.clone();
        c.shown_dir = dir;
    })
    .await;
    r.ready().await;
    r
}

#[tokio::test(start_paused = true)]
async fn at_opens_a_picker_filtered_in_memory_and_tab_writes_the_path() {
    let p = project();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("look at @").await;
    r.asked("tool.list", 1).await;
    r.shows("@src/lib.rs").await;
    assert!(r.text().contains("@README.md"), "{}", r.text());
    assert!(!r.text().contains("node_modules"), "{}", r.text());

    // The list is read once: remove a file from the disk and keep typing; it
    // is still offered, so a keystroke did not stat anything.
    std::fs::remove_file(p.path().join("src/lib.rs")).unwrap();
    r.type_text("lib").await;
    assert!(r.text().contains("@src/lib.rs"), "{}", r.text());
    assert!(!r.text().contains("@README.md"), "filtered:\n{}", r.text());
    assert_eq!(
        r.daemon.asked("tool.list").len(),
        1,
        "one read of the roots"
    );

    r.press(KeyCode::Tab).await;
    assert!(
        r.text().contains("› look at @src/lib.rs"),
        "the chosen path is in the line:\n{}",
        r.text()
    );
}

#[tokio::test(start_paused = true)]
async fn enter_on_a_half_typed_path_picks_it_and_a_whole_one_sends_the_message() {
    let p = project();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("read @notes").await;
    r.shows("@docs/notes.md").await;
    r.press(KeyCode::Enter).await;
    assert!(r.text().contains("› read @docs/notes.md"), "{}", r.text());
    assert!(
        r.daemon.asked("turn.submit").is_empty(),
        "the first Enter only picked"
    );
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
}

#[tokio::test(start_paused = true)]
async fn the_submitted_turn_carries_the_path_for_the_gated_read() {
    let p = project();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("explain @src/tides/mod.rs please").await;
    r.shows("@src/tides/mod.rs").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    let sent = r.daemon.asked("turn.submit");
    let input = sent[0]["input"].as_str().unwrap();
    let abs = format!("{}/src/tides/mod.rs", p.path().display());
    assert!(
        input.starts_with("explain @src/tides/mod.rs please"),
        "{input}"
    );
    assert!(
        input.contains(&abs),
        "the absolute path is in the turn:\n{input}"
    );
    assert!(input.contains("fs_read"), "{input}");
    assert!(
        !input.contains(SENTINEL),
        "never the file's bytes:\n{input}"
    );
    let mut keys: Vec<&str> = sent[0]
        .as_object()
        .expect("params")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["author", "input", "session_id"],
        "exactly these keys: a path that adds attachments must trip this"
    );
}

#[tokio::test(start_paused = true)]
async fn a_file_that_is_not_listed_is_refused_visibly_and_nothing_is_sent() {
    let p = project();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("open @nope.rs").await;
    r.shows("no file matches @nope.rs").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    // The first miss re-reads the list once; the second Enter refuses.
    r.shows("reading it again").await;
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.shows("@nope.rs: no such file under").await;
    assert!(r.daemon.asked("turn.submit").is_empty());
    assert!(
        r.text().contains("› open @nope.rs"),
        "the line is kept:\n{}",
        r.text()
    );

    // A file that exists but is not in the list (outside the directory) too.
    r.ctrl('c').await;
    r.type_text("open @/etc/hostname").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    r.shows("reading it again").await;
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.shows("@/etc/hostname: no such file under").await;
    assert!(r.daemon.asked("turn.submit").is_empty());
}

#[tokio::test(start_paused = true)]
async fn nothing_is_offered_or_sent_when_the_directory_is_outside_the_roots() {
    let p = project();
    let elsewhere = tempfile::tempdir().unwrap();
    let mut r = rig_in(&p, &elsewhere.path().display().to_string()).await;
    r.type_text("see @").await;
    r.shows("outside the workspace roots").await;
    assert!(!r.text().contains("@src/lib.rs"), "{}", r.text());
    r.type_text("src/lib.rs").await;
    r.press(KeyCode::Enter).await;
    r.shows("outside the workspace roots").await;
    assert!(r.daemon.asked("turn.submit").is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_double_at_sends_a_literal_and_a_folded_paste_is_never_checked() {
    let p = project();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("ask @@bob about it").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    let first = r.daemon.asked("turn.submit");
    assert_eq!(
        first[0]["input"], "ask @@bob about it",
        "no footer, text as typed"
    );

    r.paste("a\nb\nc\nd\ne\nf\ng @nobody_here\n").await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 2).await;
}

/// A recalled line whose `@path` is not the word at the cursor still starts
/// the listing, so the first Enter after it is not refused for good.
#[tokio::test(start_paused = true)]
async fn a_recalled_line_naming_a_path_mid_sentence_lists_the_files_and_sends() {
    let p = project();
    let dir = p.path().display().to_string();
    let mut r = Rig::with_cfg(100, 40, tools_script(dir.clone()), {
        let dir = dir.clone();
        move |c| {
            c.dir = dir.clone();
            c.shown_dir = dir;
            c.history = vec!["explain @src/lib.rs please".into()];
        }
    })
    .await;
    r.ready().await;
    r.press(KeyCode::Up).await;
    assert!(
        r.text().contains("explain @src/lib.rs please"),
        "{}",
        r.text()
    );
    r.asked("tool.list", 1).await;
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    assert_eq!(r.daemon.asked("turn.submit").len(), 1);
}

/// A paste of five lines or fewer is not folded: it is typed text, so `@word`
/// in it is checked like any other (today's behaviour; an owner question).
#[tokio::test(start_paused = true)]
async fn a_short_paste_is_typed_text_so_a_pasted_at_word_is_checked() {
    let p = project();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.paste("# cron\n@reboot /usr/bin/foo\n").await;
    r.settle().await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    r.shows("reading it again").await;
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.shows("@reboot: no such file under").await;
    assert!(
        r.daemon.asked("turn.submit").is_empty(),
        "nothing is sent while a pasted @word names no file"
    );
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A project whose `shot.png` holds the PNG signature, and a `notes.md`.
fn project_with_shot() -> tempfile::TempDir {
    let p = project();
    std::fs::write(p.path().join("shot.png"), PNG).unwrap();
    touch(p.path(), "notes.md");
    p
}

#[tokio::test(start_paused = true)]
async fn an_image_named_by_path_goes_as_an_attachment_and_a_text_file_keeps_its_path() {
    let p = project_with_shot();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("look at @shot.png").await;
    r.shows("@shot.png").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    let sent = r.daemon.asked("turn.submit");
    let atts = sent[0]["attachments"].as_array().expect("attachments");
    assert_eq!(atts.len(), 1, "{:?}", sent[0]);
    assert_eq!(atts[0]["name"], "shot.png");
    assert_eq!(atts[0]["size"], 8);
    assert_eq!(atts[0]["data"], "iVBORw0KGgo=");
    let input = sent[0]["input"].as_str().unwrap();
    assert_eq!(input, "look at @shot.png", "no path footer for an image");

    // Beside a text file: the file keeps its path footer, the image its attachment.
    r.type_text("both @shot.png and @notes.md").await;
    r.shows("@notes.md").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 2).await;
    let sent = r.daemon.asked("turn.submit");
    let input = sent[1]["input"].as_str().unwrap();
    let notes = format!("{}/notes.md", p.path().display());
    assert!(input.contains(&notes), "{input}");
    assert!(
        !input.contains("shot.png\n") && !input.ends_with("/shot.png"),
        "the image is not in the footer:\n{input}"
    );
    assert_eq!(sent[1]["attachments"].as_array().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn an_image_over_the_cap_is_refused_visibly_and_nothing_is_sent() {
    let p = project_with_shot();
    // Sparse: no 32 MiB is ever allocated.
    let big = std::fs::File::create(p.path().join("big.png")).unwrap();
    big.set_len(32 * 1024 * 1024 + 1).unwrap();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("see @big.png").await;
    r.shows("@big.png").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    r.shows("over the 32 MiB limit").await;
    assert!(r.daemon.asked("turn.submit").is_empty());
    assert!(r.text().contains("› see @big.png"), "{}", r.text());
}

#[tokio::test(start_paused = true)]
async fn an_image_typed_while_a_turn_runs_carries_its_attachment_when_released() {
    let p = project_with_shot();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.starts_turn("trn_1");
    r.type_text("see @shot.png").await;
    r.shows("@shot.png").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    r.shows("queued › see @shot.png").await;
    assert!(r.daemon.asked("turn.submit").is_empty(), "it waits");
    r.ends_turn("trn_1", "");
    r.asked("turn.submit", 1).await;
    let sent = r.daemon.asked("turn.submit");
    let atts = sent[0]["attachments"].as_array().expect("attachments");
    assert_eq!(atts.len(), 1, "{:?}", sent[0]);
    assert_eq!(atts[0]["data"], "iVBORw0KGgo=");
}

#[tokio::test(start_paused = true)]
async fn the_transcript_names_the_files_and_images_that_went_by_reference() {
    let p = project_with_shot();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("see @notes.md and @shot.png").await;
    r.shows("@shot.png").await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Enter).await;
    r.shows("paths sent: notes.md").await;
    assert!(
        r.text().contains("images attached: shot.png"),
        "{}",
        r.text()
    );
}

/// A file made after the listing is found by one re-read at Enter (phase 1's
/// F4); a name that is still missing after it is refused, with no loop.
#[tokio::test(start_paused = true)]
async fn a_name_missing_from_the_list_is_read_for_once_at_enter() {
    let p = project();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("look at @R").await;
    r.asked("tool.list", 1).await;
    r.settle().await;
    r.press(KeyCode::Esc).await;
    r.press(KeyCode::Backspace).await;
    r.press(KeyCode::Backspace).await;
    touch(p.path(), "new.md");
    r.type_text("@new.md").await;
    r.press(KeyCode::Enter).await;
    r.shows("not in the list; reading it again").await;
    r.asked("tool.list", 2).await;
    assert!(r.daemon.asked("turn.submit").is_empty());
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.asked("turn.submit", 1).await;
    let sent = r.daemon.asked("turn.submit");
    assert_eq!(sent.len(), 1);
    assert!(sent[0].to_string().contains("new.md"), "{}", sent[0]);
}

/// Enter 1 re-reads, Enter 2 refuses, and Enter 3 must refuse again without
/// walking the tree (the re-arm used to make every other Enter a re-read).
#[tokio::test(start_paused = true)]
async fn a_still_missing_name_is_re_read_once_however_many_enters_follow() {
    let p = project();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("look at @nope.md").await;
    r.asked("tool.list", 1).await;
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.shows("reading it again").await;
    r.asked("tool.list", 2).await;
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.shows("@nope.md: no such file under").await;
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.shows("@nope.md: no such file under").await;
    r.settle().await;
    assert!(r.daemon.asked("turn.submit").is_empty());
    assert_eq!(r.daemon.asked("tool.list").len(), 2, "one re-read, not one per two Enters");
}

#[tokio::test(start_paused = true)]
async fn a_name_that_never_exists_is_refused_after_its_one_re_read() {
    let p = project();
    let mut r = rig_in(&p, &p.path().display().to_string()).await;
    r.type_text("look at @nope.md").await;
    r.asked("tool.list", 1).await;
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.shows("reading it again").await;
    r.asked("tool.list", 2).await;
    r.settle().await;
    r.press(KeyCode::Enter).await;
    r.shows("@nope.md: no such file under").await;
    r.settle().await;
    assert_eq!(r.daemon.asked("tool.list").len(), 2, "one re-read, no loop");
    assert!(r.daemon.asked("turn.submit").is_empty());
}
