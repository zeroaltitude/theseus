//! Only synced frames ship (theseus-mgw.12), against the durability tender's
//! fake with state (`tests_durable`): a log written as the store's writer
//! writes it (each frame, then a sync for the batch), its bound the writer's
//! own `Wal::synced`, as the daemon's `Store::synced_to` hands it over. A
//! frame written and not yet synced waits after the cursor; a power loss's
//! cut, and the log written again after it, follows on from the cursor.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use theseus_store::{kinds, wal, NewRecord, Wal, WalConfig};

use super::durable::cursor::Paths;
use super::durable::{Hooks, Shipper};
use super::tests_durable::{layer, sends, shipped_segment, tuning, Fake, PREFIX};
use super::tests_restore::{records, restore_with, titles};
use super::Aws;
use crate::session::SessionRecord;
use crate::store::Store;

/// The log, closed and opened again as a machine's crash would leave it.
type Log = Arc<RwLock<Option<Wal>>>;

struct Lab {
    dir: tempfile::TempDir,
    store: PathBuf,
    log: Log,
    aws: Arc<Aws>,
}

impl Lab {
    fn new(fake: &Fake) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let lab = Lab {
            store: dir.path().join("live/store"),
            dir,
            log: Arc::default(),
            aws: layer(fake),
        };
        lab.open();
        lab
    }

    fn open(&self) {
        let w = Wal::open(
            &self.store.join("wal"),
            WalConfig {
                segment_bytes: 4096,
                fsync: false,
                ..WalConfig::default()
            },
        )
        .unwrap();
        *self.log.write().unwrap() = Some(w);
    }

    fn close(&self) {
        *self.log.write().unwrap() = None;
    }

    /// A session's record in a frame of its own, not synced.
    fn write(&self, name: &str) {
        let rec = SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            Some(name.to_string()),
        );
        let log = self.log.read().unwrap();
        log.as_ref()
            .unwrap()
            .write(&[NewRecord::json(kinds::SESSION, Some(&rec.session_id), &rec).unwrap()])
            .unwrap();
    }

    fn sync(&self) {
        self.log.read().unwrap().as_ref().unwrap().sync().unwrap();
    }

    fn segments(&self) -> Vec<u32> {
        wal::list_segments(&self.store.join("wal")).unwrap()
    }

    fn segment(&self, n: u32) -> Vec<u8> {
        std::fs::read(wal::segment_path(&self.store.join("wal"), n)).unwrap()
    }

    /// A shipper as the daemon opens one: its bound the writer's sync.
    fn shipper(&self) -> Shipper {
        let log = self.log.clone();
        let hooks = Hooks {
            synced_to: Some(Arc::new(move || {
                log.read().unwrap().as_ref().map_or(0, Wal::synced)
            })),
            ..Hooks::default()
        };
        let account = self.aws.accounts().next().unwrap().clone();
        Shipper::open(account, Paths::for_store(&self.store), tuning(), hooks).unwrap()
    }

    /// Every tail S3 holds of segment `n`: (from, to).
    fn tails(fake: &Fake, n: u32) -> Vec<(u64, u64)> {
        fake.keys(&format!("{PREFIX}wal/{n:09}.seg.tail/"))
            .iter()
            .map(|k| {
                let (a, b) = k.rsplit('/').next().unwrap().split_once('-').unwrap();
                (a.parse().unwrap(), b.parse().unwrap())
            })
            .collect()
    }
}

/// A frame written without its sync waits: the pass ships what the sync
/// covered, says `waiting`, keeps the cursor before the frame, and still
/// counts it in the exposure; once synced, the next pass ships it. A
/// segment the bound stops inside is not shipped whole, though a later one
/// exists, until the frames that sealed it are synced too.
#[tokio::test]
async fn a_write_without_its_sync_waits_and_ships_after_the_sync() {
    let fake = Fake::start();
    let lab = Lab::new(&fake);
    for i in 0..3 {
        lab.write(&format!("an invented session {i}"));
    }
    lab.sync();
    let synced = lab.segment(1).len() as u64;
    lab.write("written, not yet synced");
    lab.write("nor this one");
    let mut s = lab.shipper();
    s.pass().await.unwrap();
    assert!(s.held());
    assert_eq!(s.status().shipped_to_position, 3);
    assert_eq!(s.status().state, "waiting");
    assert!(
        s.status()
            .error
            .as_deref()
            .unwrap_or("")
            .contains("position 4"),
        "{:?}",
        s.status()
    );
    let held_at = records(&lab.store.join("wal"))[3].at_unix_ms;
    assert_eq!(
        s.status().oldest_unshipped_unix_ms,
        Some(held_at),
        "the held frame still counts"
    );
    assert_eq!(
        shipped_segment(&fake, 1).as_deref(),
        Some(&lab.segment(1)[..synced as usize])
    );
    assert!(Lab::tails(&fake, 1).iter().all(|(_, to)| *to <= synced));
    assert_eq!(s.status().shipped_to_position, 3);

    lab.sync();
    s.pass().await.unwrap();
    assert!(!s.held());
    assert_eq!(s.status().state, "caught_up");
    assert_eq!(s.status().oldest_unshipped_unix_ms, None);
    assert_eq!(s.status().shipped_to_position, 5);
    assert_eq!(shipped_segment(&fake, 1), Some(lab.segment(1)));

    // Unsynced frames that roll the segment: segment 1 is not named sealed
    // while the bound stops inside it.
    let mut i = 0;
    while lab.segments().len() < 2 {
        lab.write(&format!("a session that fills segment 1, {i}"));
        i += 1;
    }
    s.pass().await.unwrap();
    assert!(s.held());
    assert_eq!(s.status().shipped_to_position, 5);
    assert!(fake.object(&format!("{PREFIX}wal/000000001.seg")).is_none());
    lab.sync();
    s.pass().await.unwrap();
    assert!(!s.held());
    for n in lab.segments() {
        assert_eq!(
            shipped_segment(&fake, n),
            Some(lab.segment(n)),
            "segment {n}"
        );
    }
    assert!(fake.object(&format!("{PREFIX}wal/000000001.seg")).is_some());
    let once = sends(&fake);
    assert!(once.values().all(|n| *n == 1), "{once:?}");
}

/// A power loss: frames written and never synced are lost, the segment cut
/// where the sync ended, and the log written again differently. The next
/// start follows on from its cursor (no `Rewound`, nothing shipped again),
/// S3 holds no tail past the log, and a restore equals the log.
#[tokio::test]
async fn a_power_losss_cut_follows_on_from_the_cursor() {
    let fake = Fake::start();
    let lab = Lab::new(&fake);
    for i in 0..3 {
        lab.write(&format!("an invented session {i}"));
    }
    lab.sync();
    let synced = lab.segment(1).len() as u64;
    let mut s = lab.shipper();
    s.pass().await.unwrap();
    lab.write("lost in the power loss");
    lab.write("lost with it");
    s.pass().await.unwrap();
    assert!(s.held(), "the lost frames wait");
    assert!(Lab::tails(&fake, 1).iter().all(|(_, to)| *to <= synced));
    drop(s);

    // The power loss: the page cache's frames are gone.
    lab.close();
    std::fs::OpenOptions::new()
        .write(true)
        .open(wal::segment_path(&lab.store.join("wal"), 1))
        .unwrap()
        .set_len(synced)
        .unwrap();
    lab.open();
    lab.write("written again after the loss");
    lab.write("and another");
    lab.sync();
    let sent = sends(&fake);
    let mut s = lab.shipper();
    s.pass().await.unwrap();
    assert_eq!(s.status().state, "caught_up");
    assert_eq!(s.status().shipped_to_position, 5);
    let after = sends(&fake);
    assert!(
        sent.iter().all(|(k, n)| after.get(k) == Some(n)),
        "an object sent again: {sent:?} then {after:?}"
    );
    let len = lab.segment(1).len() as u64;
    assert!(
        Lab::tails(&fake, 1).iter().all(|(_, to)| *to <= len),
        "a tail past the log: {:?}",
        Lab::tails(&fake, 1)
    );
    assert_eq!(shipped_segment(&fake, 1), Some(lab.segment(1)));

    let into = lab.dir.path().join("fresh");
    let r = restore_with(&lab.aws, &into).await.unwrap();
    assert!(r.fetch.gap.is_none(), "{:?}", r.fetch.gap);
    assert!(r.fetch.segments.iter().all(|f| f.unjoined.is_none()));
    let live = records(&lab.store.join("wal"));
    let back = records(&into.join("store/wal"));
    assert_eq!(&back[..live.len()], &live[..]);
    let names: Vec<String> = titles(&Store::open(&into.join("store")).unwrap())
        .into_iter()
        .filter_map(|(_, t)| t)
        .collect();
    assert!(
        names.iter().any(|t| t == "written again after the loss"),
        "{names:?}"
    );
    assert!(!names.iter().any(|t| t.starts_with("lost")), "{names:?}");
}
