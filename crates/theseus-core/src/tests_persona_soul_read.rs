//! `persona.soul_read` and `persona.soul`'s stale-write guard (theseus-lky63):
//! the cockpit's editor opens the whole file, writes it whole, and loses
//! nothing on the way.

use super::*;
use theseus_protocol::{PersonaAddParams, PersonaSoulParams, PersonaSoulReadParams};

const MULTI: &str = "# Identity\nA harbour pilot named Pilot.\nShe keeps the chart tidy.\n\n## Voice\nPlain words, dry humour.\nShort sentences.\n\n## Values\nHonest soundings.\nA clean wake.\nNo surprises at the dock.\n";

fn owner() -> Answerer {
    on_discord(OWNER, None)
}

fn add(core: &Core, key: &str, soul: Option<&str>) -> String {
    let made = core
        .persona_add(
            &PersonaAddParams {
                key: key.into(),
                soul: soul.map(str::to_string),
                ..Default::default()
            },
            &owner(),
        )
        .unwrap();
    made.soul.unwrap_or_default()
}

fn read(
    core: &Core,
    key: &str,
    bak_ms: Option<u64>,
) -> anyhow::Result<theseus_protocol::PersonaSoulReadResult> {
    core.persona_soul_read(
        &PersonaSoulReadParams {
            key: key.into(),
            bak_ms,
            ..Default::default()
        },
        &owner(),
    )
}

fn write(core: &Core, key: &str, text: &str, expect: Option<String>) -> anyhow::Result<()> {
    core.persona_soul(
        &PersonaSoulParams {
            key: key.into(),
            text: text.into(),
            expect_digest: expect,
            ..Default::default()
        },
        &owner(),
    )
    .map(|_| ())
}

fn baks(path: &str) -> usize {
    std::fs::read_dir(std::path::Path::new(path).parent().unwrap())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".bak")
        })
        .count()
}

/// The editor's round trip: open, write back unchanged, and the file holds
/// every section byte for byte.
#[tokio::test]
async fn a_soul_read_and_written_back_loses_nothing() {
    let r = rig();
    let path = add(&r.core, "scout", Some(MULTI));
    let on_disk = std::fs::read_to_string(&path).unwrap();
    let got = read(&r.core, "scout", None).unwrap();
    let text = got.text.clone().expect("the text is returned");
    assert_eq!(text, on_disk);
    assert!(text.contains("## Voice") && text.contains("## Values"));
    assert_eq!(got.path.as_deref(), Some(path.as_str()));
    assert_eq!(got.bytes, on_disk.len() as u64);
    assert_eq!(got.tokens, crate::personas::soul::tokens(&on_disk));
    assert_eq!(got.digest, Some(crate::persona::digest(&on_disk)));
    assert!(got.backups.is_empty());
    write(&r.core, "scout", &text, got.digest).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), on_disk);
}

/// Two writes leave the first as a backup that restores.
#[tokio::test]
async fn a_backup_is_listed_read_and_restored() {
    let r = rig();
    let path = add(&r.core, "scout", Some("First soul."));
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    write(&r.core, "scout", "Second soul, a little longer.", None).unwrap();
    let got = read(&r.core, "scout", None).unwrap();
    assert_eq!(got.backups.len(), 1);
    let bak = got.backups[0].clone();
    assert_eq!(bak.bytes, "First soul.\n".len() as u64);
    let old = read(&r.core, "scout", Some(bak.ms)).unwrap();
    assert_eq!(old.text.as_deref(), Some("First soul.\n"));
    assert_eq!(
        old.live_digest, got.digest,
        "the live digest rides a backup's read"
    );
    // Restoring is a write: the second soul becomes the newest backup.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    write(
        &r.core,
        "scout",
        old.text.as_deref().unwrap(),
        old.live_digest,
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "First soul.\n");
    let after = read(&r.core, "scout", None).unwrap();
    assert_eq!(after.backups.len(), 2);
    assert!(after.backups[0].ms > after.backups[1].ms, "newest first");
    let newest = read(&r.core, "scout", Some(after.backups[0].ms)).unwrap();
    assert_eq!(
        newest.text.as_deref(),
        Some("Second soul, a little longer.\n")
    );
}

/// A stale write is refused and changes nothing; a current one is written.
#[tokio::test]
async fn a_stale_write_is_refused_and_writes_nothing() {
    let r = rig();
    let path = add(&r.core, "scout", Some("Opened soul."));
    let opened = read(&r.core, "scout", None).unwrap().digest;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    write(&r.core, "scout", "Written elsewhere.", None).unwrap();
    let (rows, files) = (rows_of(&r.core, "persona.soul", None).len(), baks(&path));
    let before = std::fs::read(&path).unwrap();
    let e = write(&r.core, "scout", "My edit.", opened).unwrap_err();
    assert!(
        format!("{e}").contains("changed since you opened it: reload it and edit again"),
        "{e}"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the file is untouched"
    );
    assert_eq!(baks(&path), files, "no new backup");
    assert_eq!(rows_of(&r.core, "persona.soul", None).len(), rows, "no row");
    let now = read(&r.core, "scout", None).unwrap().digest;
    write(&r.core, "scout", "My edit.", now).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "My edit.\n");
}

/// A backup that is not listed, a path-like key, an awaiting persona, and a
/// caller the write would refuse.
#[tokio::test]
async fn the_read_is_bounded_like_the_write() {
    let r = rig();
    add(&r.core, "scout", Some("First soul."));
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    write(&r.core, "scout", "Second soul.", None).unwrap();
    let listed = read(&r.core, "scout", None).unwrap().backups[0].ms;
    let e = read(&r.core, "scout", Some(listed + 1)).unwrap_err();
    assert!(format!("{e}").contains("no backup"), "{e}");
    for key in ["../scout", "a/b", ""] {
        assert!(read(&r.core, key, None).is_err(), "{key:?}");
    }
    add(&r.core, "wren", None);
    let w = read(&r.core, "wren", None).unwrap();
    assert!((w.text, w.path, w.digest, w.backups.len(), w.bytes) == (None, None, None, 0, 0));
    assert!(read(&r.core, "wren", Some(1)).is_err());
    let unnamed = Answerer {
        label: "sock#9".into(),
        surface: Surface::Unnamed,
        discord: None,
        agent: None,
    };
    for who in [
        unnamed,
        on_discord(ALICE, None),
        on_discord(OWNER, Some(LAB)),
    ] {
        let e = r
            .core
            .persona_soul_read(
                &PersonaSoulReadParams {
                    key: "scout".into(),
                    ..Default::default()
                },
                &who,
            )
            .unwrap_err();
        assert!(
            e.downcast_ref::<crate::approval::Refusal>().is_some(),
            "{e}"
        );
    }
}
