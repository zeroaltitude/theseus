//! A fresh install's defaults (theseus-is8t): recall on, live on `baseline`,
//! and the judge on exactly when its key resolves.

use super::*;

/// A config of `[secrets]` with the model's key, `extra` lines of its own,
/// then `rest`.
fn text(extra: &str, rest: &str) -> String {
    format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n{extra}\n{rest}\n")
}

fn parse(extra: &str, rest: &str) -> Result<Config> {
    Ok(Config::parse(&text(extra, rest))?.0)
}

/// An `env:` reference whose variable is set in every test process, and one
/// that is set in none.
const SET: &str = "jev_api_key = \"env:PATH\"";
const UNSET: &str = "jev_api_key = \"env:THESEUS_TEST_NO_SUCH_VARIABLE_IS8T\"";

/// A config that says nothing of `[memory]` recalls live on `baseline`, and
/// one that says `mode` keeps its own.
#[test]
fn a_config_with_nothing_in_memory_recalls_live_on_baseline() {
    let cfg = parse("", "").unwrap();
    assert_eq!(cfg.memory.mode, MemoryMode::Live);
    assert_eq!(cfg.memory.arm, memory::MemoryArm::Baseline);
    assert!(cfg.memory.on() && cfg.memory.is_default());
    let a = cfg.memory.assign("ses_any").unwrap();
    assert_eq!((a.arm, a.live), (memory::MemoryArm::Baseline, true));
    for (word, mode) in [
        ("off", MemoryMode::Off),
        ("shadow", MemoryMode::Shadow),
        ("canary", MemoryMode::Canary),
        ("live", MemoryMode::Live),
    ] {
        let cfg = parse("", &format!("[memory]\nmode = \"{word}\"")).unwrap();
        assert_eq!(cfg.memory.mode, mode, "{word}");
    }
    assert!(parse("", "[memory]\nmode = \"off\"")
        .unwrap()
        .memory
        .assign("ses_any")
        .is_none());
}

/// With no `enabled` line the judge follows its key: on with one that
/// resolves, off with none, an unset variable, or a placeholder, and the
/// words say which.
#[test]
fn the_judge_is_on_exactly_when_its_key_resolves() {
    let on = parse(SET, "").unwrap().judge;
    assert!(on.enabled && on.enabled_setting.is_none(), "{on:?}");
    assert!(on.auto_why.starts_with("on:") && on.off_why().is_empty());
    for (extra, why) in [
        ("", "no jev_api_key entry under [secrets]"),
        (UNSET, "does not resolve"),
        (
            "jev_api_key = \"op://<your vault>/<Jev API key item>/notesPlain\"",
            "still a placeholder",
        ),
    ] {
        let j = parse(extra, "").unwrap().judge;
        assert!(!j.enabled, "{extra}");
        assert!(j.off_why().contains(why), "{extra}: {}", j.off_why());
    }
    // A vault reference is not asked at the start: it turns the judge on,
    // and health says the key's state.
    let vault = parse("jev_api_key = \"op://v/jev/key\"", "").unwrap().judge;
    assert!(vault.enabled && vault.auto_why.contains("vault"));
    // A file that reads, and one that does not.
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("jev");
    std::fs::write(&key, "stand-in\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let entry = format!("jev_api_key = \"file:{}\"", key.display());
    assert!(parse(&entry, "").unwrap().judge.enabled);
    std::fs::remove_file(&key).unwrap();
    assert!(!parse(&entry, "").unwrap().judge.enabled);
}

/// The config's own word wins: `false` is off whatever the key, `true` with
/// no entry still refuses, and `true` with an unset variable is on, as it
/// was.
#[test]
fn enabled_false_wins_and_true_with_no_key_still_refuses() {
    let off = parse(SET, "[judge]\nenabled = false").unwrap().judge;
    assert!(!off.enabled && off.off_why() == "[judge] enabled = false");
    let e = format!("{:#}", parse("", "[judge]\nenabled = true").unwrap_err());
    assert!(
        e.contains("judge.key_secret") && e.contains("[secrets]"),
        "{e}"
    );
    let on = parse(UNSET, "[judge]\nenabled = true").unwrap().judge;
    assert!(on.enabled && on.auto_why.is_empty());
    // A key under another name: the entry `key_secret` names decides.
    let named = parse("other = \"env:PATH\"", "[judge]\nkey_secret = \"other\"")
        .unwrap()
        .judge;
    assert!(named.enabled);
}

/// The packs keep their own modes: with the judge on by auto, routing and
/// rerank act, and a pack with no line of its own is in shadow until the
/// ladder says otherwise; a ceiling lowers all of them.
#[test]
fn an_auto_judge_keeps_each_packs_own_mode() {
    let j = parse(SET, "").unwrap().judge;
    let live = PackMode::Live;
    for pack in ["route.v3", "rerank.v1"] {
        assert_eq!(j.mode_of(pack, live), live, "{pack}");
    }
    assert_eq!(j.mode_of("loop.v1", PackMode::Shadow), PackMode::Shadow);
    let off = parse(UNSET, "").unwrap().judge;
    assert_eq!(off.mode_of("route.v3", live), PackMode::Off);
}

/// A config that says nothing stays sparse: the note a fresh install writes
/// holds no `enabled` line and no `[memory]`, and a deliberate `false`
/// stays in the note.
#[test]
fn the_sparse_config_stays_sparse() {
    let note = sparse_note(&text(SET, "")).unwrap();
    assert!(
        !note.contains("[judge]") && !note.contains("[memory]"),
        "{note}"
    );
    let note = sparse_note(&text(SET, "[judge]\nenabled = false")).unwrap();
    assert!(note.contains("enabled = false"), "{note}");
    // `true` with a key is the auto answer in effect, but the config said it:
    // the note keeps it (a later key loss then refuses instead of going off).
    let note = sparse_note(&text(SET, "[judge]\nenabled = true")).unwrap();
    assert!(note.contains("enabled = true"), "{note}");
    // A memory mode of the operator's own is kept, and the default's goes.
    let note = sparse_note(&text("", "[memory]\nmode = \"shadow\"")).unwrap();
    assert!(note.contains("mode = \"shadow\""), "{note}");
    let note = sparse_note(&text("", "[memory]\nmode = \"live\"")).unwrap();
    assert!(!note.contains("[memory]"), "{note}");
    // The template cut is sparse in these two tables as well.
    let cut = sparse_note(Config::EXAMPLE_TOML).unwrap();
    assert!(
        !cut.contains("[memory]") && !cut.contains("[judge]"),
        "{cut}"
    );
}
