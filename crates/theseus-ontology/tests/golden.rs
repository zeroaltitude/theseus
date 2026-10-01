//! Golden renders (M4 design §3, 21a's tests): the bytes a compile puts in
//! the system block for two sessions of the common fixture, the manifest's
//! entries for one, and the seed rows as the store holds them. The files
//! were written by hand, and the digests in them by `sha256sum`, so they
//! check the code rather than echo it. On a difference, the test writes what
//! this build renders under the target's tmp dir, to diff against the file.

mod common;

use common::*;
use serde_json::{json, Value};
use theseus_ontology::seeds;

/// A golden file's text, without the newline an editor ends a file with.
fn file(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

fn same(name: &str, got: &str, want: &str) {
    if got != file(want) {
        let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        std::fs::write(&path, got).unwrap();
        panic!(
            "{name}: this build renders something else, written to {}",
            path.display()
        );
    }
}

#[test]
fn a_guild_channels_session_renders_its_golden_guidance() {
    let c = kestrel().compose(&guild_channel_session());
    assert_eq!(c.skipped, vec![]);
    same(
        "guild-channel.txt",
        &c.render(),
        include_str!("golden/guild-channel.txt"),
    );
}

#[test]
fn a_dm_session_renders_its_golden_guidance() {
    let c = kestrel().compose(&dm_session());
    assert_eq!(c.skipped, vec![]);
    same("dm.txt", &c.render(), include_str!("golden/dm.txt"));
}

#[test]
fn the_manifest_records_each_membership_and_each_blocks_digest() {
    let c = kestrel().compose(&guild_channel_session());
    let got = json!({ "memberships": c.memberships, "guidance": c.guidance });
    let want: Value =
        serde_json::from_str(include_str!("golden/guild-channel-manifest.json")).unwrap();
    if got != want {
        same(
            "guild-channel-manifest.json",
            &serde_json::to_string_pretty(&got).unwrap(),
            "",
        );
    }
}

#[test]
fn the_seed_rows_are_stored_as_the_golden_file_has_them() {
    let got = serde_json::to_value(seeds()).unwrap();
    let want: Value = serde_json::from_str(include_str!("golden/seed-kinds.json")).unwrap();
    if got != want {
        same(
            "seed-kinds.json",
            &serde_json::to_string_pretty(&got).unwrap(),
            "",
        );
    }
}

#[test]
fn the_system_block_carries_the_sections_a_blank_line_apart() {
    let c = kestrel().compose(&dm_session());
    assert_eq!(c.render(), c.sections.join("\n\n"));
    assert_eq!(c.sections.len(), 4);
    assert!(c.sections.iter().all(|s| s.starts_with("# Guidance")));
}
