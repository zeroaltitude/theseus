//! A store restored from S3 (step 16), against the durability tender's fake
//! with state (`tests_durable`): a store shipped by the real tender, then
//! restored through the restore's own session, its reads, and the local
//! restore. What each test proves is in its name; the fake counts every
//! request, so "nothing" and "once" are counted.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use sha2::{Digest, Sha256};
use theseus_follow::{Cursor, WalFollower};
use theseus_store::{kinds, wal, NewRecord, Record, Wal, WalConfig};

use super::durable::cursor::Paths;
use super::durable::fetch::Source;
use super::durable::restore::{self, Knobs, S3Report};
use super::durable::{read, Hooks, Shipper};
use super::tests::ACCOUNT;
use super::tests_durable::{layer, sends, sha_b64, tuning, Fake, PREFIX};
use super::Aws;
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;
use crate::store::Store;

const URL: &str = "s3://theseus-111122223333-us-west-2/durability/theseus-lab/";

/// A store that ships: its WAL in small segments, written as a daemon
/// writes sessions and ledger rows, and closed and opened again as a crash
/// would leave it.
struct Lab {
    dir: tempfile::TempDir,
    store: PathBuf,
    wal: Option<Wal>,
    aws: Arc<Aws>,
}

impl Lab {
    fn new(fake: &Fake) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("live/store");
        let mut lab = Lab {
            dir,
            store,
            wal: None,
            aws: layer(fake),
        };
        lab.open();
        lab
    }

    fn open(&mut self) {
        self.wal = Some(
            Wal::open(
                &self.store.join("wal"),
                WalConfig {
                    segment_bytes: 4096,
                    fsync: false,
                    ..WalConfig::default()
                },
            )
            .unwrap(),
        );
    }

    fn wal(&self) -> &Wal {
        self.wal.as_ref().unwrap()
    }

    /// A session's record, with a title of `pad` bytes: its frame's size.
    fn session(&self, name: &str, pad: usize) -> u64 {
        let rec = SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            Some(format!("{name}{}", "x".repeat(pad))),
        );
        self.wal()
            .write(&[NewRecord::json(kinds::SESSION, Some(&rec.session_id), &rec).unwrap()])
            .unwrap();
        self.wal().total_bytes()
    }

    fn sessions(&self, count: usize) {
        for i in 0..count {
            self.session(&format!("an invented session {i}"), 8);
            let row = LedgerRow::named("test.row", None, None, serde_json::json!({"i": i}));
            self.wal()
                .write(&[NewRecord::json(kinds::LEDGER, None, &row).unwrap()])
                .unwrap();
        }
    }

    fn blob(&self, bytes: &[u8]) -> String {
        let d = hex::encode(Sha256::digest(bytes));
        std::fs::create_dir_all(self.store.join("blobs")).unwrap();
        std::fs::write(self.store.join("blobs").join(&d), bytes).unwrap();
        d
    }

    fn segments(&self) -> Vec<u32> {
        wal::list_segments(&self.store.join("wal")).unwrap()
    }

    async fn ship(&self) {
        shipper(&self.aws, &self.store).pass().await.unwrap();
    }

    /// A state dir to restore into.
    fn target(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    async fn restore(&self, into: &Path) -> anyhow::Result<S3Report> {
        restore_with(&self.aws, into).await
    }
}

async fn restore_with(aws: &Aws, into: &Path) -> anyhow::Result<S3Report> {
    let knobs = Knobs {
        page: Some(3),
        chunk: Some(100),
    };
    restore::from_s3(aws, URL, into, &into.join("no.sock"), false, &knobs).await
}

fn shipper(aws: &Arc<Aws>, store: &Path) -> Shipper {
    let account = aws.accounts().next().unwrap().clone();
    Shipper::open(account, Paths::for_store(store), tuning(), Hooks::default()).unwrap()
}

/// Every record of a WAL, in order.
fn records(wal_dir: &Path) -> Vec<Record> {
    let mut f = WalFollower::open(wal_dir, Cursor::start()).unwrap();
    let mut out = Vec::new();
    loop {
        let b = f.read(1 << 20).unwrap();
        if b.is_empty() {
            return out;
        }
        out.extend(b.records);
    }
}

fn titles(store: &Store) -> Vec<(String, Option<String>)> {
    let mut s: Vec<(String, Option<String>)> = store
        .list_sessions::<SessionRecord>()
        .unwrap()
        .into_iter()
        .map(|r| (r.session_id, r.label))
        .collect();
    s.sort();
    s
}

fn ops_since(fake: &Fake, from: usize) -> Vec<String> {
    fake.state.lock().unwrap().seen[from..]
        .iter()
        .map(super::tests_durable::Req::op)
        .collect()
}

/// The whole round: a store shipped by the real tender (sealed segments
/// whole, the open one in tails, a blob), restored from the fake into a
/// fresh state dir, equals the original: every record and blob, the last
/// position, and its sessions, then the `store.restored` row. Each sealed
/// segment came from its object, read in ranges, and the open one from its
/// tails; the rows were read over every page.
#[tokio::test]
async fn a_shipped_store_restores_from_s3_equal_to_the_original() {
    let fake = Fake::start();
    let lab = Lab::new(&fake);
    lab.sessions(12);
    lab.ship().await;
    lab.sessions(4);
    lab.ship().await;
    lab.sessions(1);
    let digest = lab.blob(b"an invented image's bytes");
    lab.ship().await;
    let segs = lab.segments();
    assert!(segs.len() >= 3, "{segs:?}");
    let original = records(&lab.store.join("wal"));
    let into = lab.target("fresh");
    let r = lab.restore(&into).await.unwrap();

    let sources: Vec<(u32, Source)> = r
        .fetch
        .segments
        .iter()
        .map(|s| (s.segment, s.source.clone()))
        .collect();
    assert_eq!(sources.len(), segs.len());
    for (n, src) in &sources[..segs.len() - 1] {
        assert_eq!(*src, Source::Object, "sealed segment {n} from its object");
    }
    assert!(
        matches!(sources.last().unwrap().1, Source::Tails { .. }),
        "{sources:?}"
    );
    assert!(r.fetch.gap.is_none() && r.fetch.blobs_missing.is_empty());
    assert_eq!(r.fetch.blobs, 1);
    assert!(
        fake.ops("GetObject")
            .iter()
            .any(|q| q.header("range").is_some()),
        "objects past the chunk read in ranges"
    );
    assert!(
        fake.ops("Query").len() > 2,
        "every page of the rows followed"
    );

    let restored = records(&into.join("store/wal"));
    assert_eq!(&restored[..original.len()], &original[..]);
    assert_eq!(restored.len(), original.len() + 1);
    let last = original.last().unwrap().position;
    assert_eq!(r.restore.last_position, last);
    let back = Store::open(&into.join("store")).unwrap();
    let live = Store::open(&lab.store).unwrap();
    assert_eq!(titles(&back), titles(&live));
    assert_eq!(titles(&back).len(), 17);
    assert_eq!(
        back.ledger_tail::<LedgerRow>(1).unwrap()[0].1.kind,
        "store.restored"
    );
    assert_eq!(
        std::fs::read(into.join("store/blobs").join(&digest)).unwrap(),
        b"an invented image's bytes"
    );
    // The staging copy is gone; the lines name each segment's source.
    let names: Vec<String> = std::fs::read_dir(&into)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|n| !n.starts_with("store.from-s3")),
        "{names:?}"
    );
    let lines = restore::lines(&r);
    assert!(lines.contains("segment 1 from its object"), "{lines}");
    assert!(lines.contains("tail(s)"), "{lines}");
    assert!(r.same_deployment);
    assert!(restore::after_lines(&r).contains("warning: the config's deployment is theseus-lab"));
}

/// After a restore the config's deployment is the restored one: its next
/// start ships the `store.restored` row and nothing it shipped before (no
/// segment, tail, or blob sent again), and the store restores again whole
/// from what is there then, the restore's row included.
#[tokio::test]
async fn the_restored_stores_next_start_ships_nothing_again() {
    let fake = Fake::start();
    let lab = Lab::new(&fake);
    lab.sessions(20);
    lab.blob(b"another invented image");
    lab.ship().await;
    let into = lab.target("fresh");
    let r = lab.restore(&into).await.unwrap();
    assert_eq!(r.seeded_at, Some(r.restore.last_position));
    let before = sends(&fake);
    let puts = fake.ops("PutObject").len() + fake.ops("UploadPart").len();

    shipper(&lab.aws, &into.join("store")).pass().await.unwrap();
    let after = sends(&fake);
    let new: Vec<&String> = after.keys().filter(|k| !before.contains_key(*k)).collect();
    assert!(
        after
            .iter()
            .all(|(k, n)| before.get(k).is_none_or(|b| b == n)),
        "an object sent again: {before:?} then {after:?}"
    );
    assert_eq!(
        new.len(),
        1,
        "only the store.restored frame's tail: {new:?}"
    );
    assert!(new[0].contains(".seg.tail/"), "{new:?}");
    assert_eq!(
        fake.ops("PutObject").len() + fake.ops("UploadPart").len(),
        puts + 1
    );
    // And a second restore takes that tail too: the join goes on.
    let again = lab.target("again");
    let r2 = lab.restore(&again).await.unwrap();
    assert_eq!(r2.restore.last_position, r.restore.last_position + 1);
    let back = Store::open(&again.join("store")).unwrap();
    let rows = back.ledger_tail::<LedgerRow>(2).unwrap();
    assert_eq!(
        (rows[0].1.kind.as_str(), rows[1].1.kind.as_str()),
        ("store.restored", "store.restored")
    );
}

/// With a deployment of its own, the config ships the restored store whole
/// into its own prefix: no cursor is seeded, and a cursor that was beside
/// the replaced store is moved aside, never deleted.
#[tokio::test]
async fn another_deployment_seeds_no_cursor_and_keeps_the_old_one_aside() {
    let fake = Fake::start();
    let lab = Lab::new(&fake);
    lab.sessions(3);
    lab.ship().await;
    let mut cfg = lab.aws.accounts().next().unwrap().cfg.clone();
    cfg.deployment = Some("theseus-elsewhere".into());
    let other = Aws::from_config(
        &crate::config::AwsConfig {
            accounts: std::collections::BTreeMap::from([(ACCOUNT.to_string(), cfg)]),
        },
        super::tests::board(),
    )
    .unwrap();
    let into = lab.target("fresh");
    std::fs::create_dir_all(into.join("durability")).unwrap();
    std::fs::write(into.join("durability/cursor.json"), b"{}").unwrap();
    let r = restore_with(&other, &into).await.unwrap();
    assert!(!r.same_deployment);
    assert_eq!(r.seeded_at, None);
    let aside = PathBuf::from(r.cursor_aside.clone().unwrap());
    assert_eq!(std::fs::read(aside.join("cursor.json")).unwrap(), b"{}");
    assert!(!into.join("durability").exists());
    assert!(restore::after_lines(&r).contains("ships it whole into its own prefix"));
}

/// A sealed segment's object is preferred to the tails shipped while it was
/// open: none of them is read. And the open segment's log rewound (a power
/// loss before its sync), then written again shorter: the tail of the lost
/// write stays in S3 past where the new one ends, and is not stitched,
/// though its frame would check and follow by position.
#[tokio::test]
async fn a_sealed_object_is_preferred_and_a_stale_tail_is_not_stitched() {
    let fake = Fake::start();
    let mut lab = Lab::new(&fake);
    // Segment 1 shipped in tails as it grew, then sealed and shipped whole.
    for i in 0..3 {
        lab.session(&format!("an early session {i}"), 8);
        lab.ship().await;
    }
    while lab.segments().len() < 2 {
        lab.session("a session that fills segment 1", 8);
    }
    lab.ship().await;
    assert!(fake.keys(&format!("{PREFIX}wal/000000001.seg.tail/")).len() >= 3);
    assert!(fake.object(&format!("{PREFIX}wal/000000001.seg")).is_some());
    let open = *lab.segments().last().unwrap();
    // The open segment: a, then b and c, each shipped as its own tail.
    lab.session("a", 8);
    lab.ship().await;
    let end_a = std::fs::metadata(wal::segment_path(&lab.store.join("wal"), open))
        .unwrap()
        .len();
    lab.session("b, a long one", 60);
    lab.ship().await;
    lab.session("c, from a log that will be rewound", 8);
    lab.ship().await;
    // The power loss: b and c were never synced.
    lab.wal = None;
    let seg = wal::segment_path(&lab.store.join("wal"), open);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&seg)
        .unwrap()
        .set_len(end_a)
        .unwrap();
    lab.open();
    lab.session("b again, short", 0);
    lab.ship().await;
    let live = std::fs::read(&seg).unwrap();

    let gets = fake.ops("GetObject").len();
    let into = lab.target("fresh");
    let r = lab.restore(&into).await.unwrap();
    let read: Vec<String> = fake.ops("GetObject")[gets..]
        .iter()
        .map(super::tests_durable::Req::key)
        .collect();
    assert!(
        read.iter().all(|k| !k.contains("000000001.seg.tail/")),
        "a tail of a sealed segment read: {read:?}"
    );
    assert_eq!(r.fetch.segments[0].source, Source::Object);
    let last = r.fetch.segments.last().unwrap();
    assert_eq!(last.segment, open);
    assert!(last.unjoined.is_some(), "the stale tail is said: {last:?}");
    let restored = std::fs::read(wal::segment_path(&into.join("store/wal"), open)).unwrap();
    assert_eq!(&restored[..live.len()], &live[..]);
    let back = Store::open(&into.join("store")).unwrap();
    let names: Vec<String> = titles(&back).into_iter().filter_map(|(_, t)| t).collect();
    assert!(
        names.iter().any(|t| t.starts_with("b again, short")),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|t| t.starts_with("c, from a log")),
        "the stale tail was stitched: {names:?}"
    );
}

/// An object whose bytes are not its row's is refused, naming it, and
/// nothing is restored: here a blob replaced in S3 with bytes S3's own
/// checksum agrees with, which only the row's digest tells. A segment's
/// bytes changed are refused by the row too.
#[tokio::test]
async fn a_corrupted_object_is_refused_by_its_checksum() {
    let fake = Fake::start();
    let lab = Lab::new(&fake);
    lab.sessions(12);
    let digest = lab.blob(b"an invented image");
    lab.ship().await;
    let key = format!("{PREFIX}blobs/{digest}");
    {
        let mut s = fake.state.lock().unwrap();
        let bytes = b"an invented imagE".to_vec(); // its length, not its bytes
        let sha = sha_b64(&bytes);
        s.objects.insert(key.clone(), (bytes, sha));
    }
    let into = lab.target("fresh");
    let e = format!("{:#}", lab.restore(&into).await.unwrap_err());
    assert!(e.contains(&key) && e.contains("SHA-256"), "{e}");
    assert!(e.contains("nothing was restored"), "{e}");
    assert!(!into.join("store").exists());
    assert!(
        std::fs::read_dir(&into).unwrap().next().is_none(),
        "no staging left"
    );

    // A sealed segment's bytes changed, S3's checksum with them.
    let seg = format!("{PREFIX}wal/000000001.seg");
    {
        let mut s = fake.state.lock().unwrap();
        let (mut bytes, _) = s.objects[&seg].clone();
        let at = bytes.len() / 2;
        bytes[at] ^= 1;
        let sha = sha_b64(&bytes);
        s.objects.insert(seg.clone(), (bytes, sha));
    }
    let e = format!("{:#}", lab.restore(&into).await.unwrap_err());
    assert!(e.contains(&seg) && e.contains("SHA-256"), "{e}");
}

/// A blob a row names that S3 does not hold is said, and the rest is
/// restored; a segment with no row is a gap, said and not filled: the
/// restore holds what precedes it. The fake answers the missing blob as S3
/// would the restore's session: 404, since its policy lists without needing
/// `s3:prefix` (under `StringLike` it is 403, and the restore fails whole).
#[tokio::test]
async fn a_missing_blob_and_a_gap_are_said() {
    let fake = Fake::start();
    let lab = Lab::new(&fake);
    lab.sessions(20);
    let digest = lab.blob(b"an invented image");
    lab.ship().await;
    assert!(lab.segments().len() >= 3, "{:?}", lab.segments());
    {
        let mut s = fake.state.lock().unwrap();
        s.objects.remove(&format!("{PREFIX}blobs/{digest}"));
        s.items
            .remove(&("theseus-lab#wal".to_string(), "000000002".to_string()));
    }
    let into = lab.target("fresh");
    let r = lab.restore(&into).await.unwrap();
    assert_eq!(r.fetch.blobs_missing, vec![digest]);
    let gap = r.fetch.gap.clone().expect("the gap is said");
    assert!(gap.contains("segment 2 has no row"), "{gap}");
    assert_eq!(r.fetch.segments.len(), 1);
    assert_eq!(r.restore.last_position, r.fetch.segments[0].last_position);
    let lines = restore::lines(&r);
    assert!(lines.contains("a gap, not filled"), "{lines}");
    assert!(lines.contains("not in S3"), "{lines}");
}

/// The restore's session reads and nothing else: its policy has no write,
/// its name is its own, and every request it made is a read. And nothing is
/// fetched, nor a session minted, while a daemon serves the store.
#[tokio::test]
async fn the_restore_session_only_reads_and_nothing_is_fetched_while_a_daemon_serves() {
    let p = read::policy(ACCOUNT, "us-west-2", "theseus-lab");
    let actions: Vec<String> = p["Statement"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| match &s["Action"] {
            Value::Array(a) => a
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            Value::String(a) => vec![a.clone()],
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(actions, ["s3:GetObject", "s3:ListBucket", "dynamodb:Query"]);
    assert_eq!(
        p["Statement"][0]["Resource"],
        format!("arn:aws:s3:::theseus-{ACCOUNT}-us-west-2/durability/theseus-lab/*")
    );
    // The list passes without `s3:prefix`, so a missing key is 404 to a
    // `GetObject` (theseus-mgw.10).
    assert_eq!(
        p["Statement"][1]["Condition"],
        serde_json::json!({"StringLikeIfExists": {"s3:prefix": ["durability/theseus-lab/*"]}})
    );
    assert_eq!(
        p["Statement"][2]["Condition"]["ForAllValues:StringLike"]["dynamodb:LeadingKeys"],
        serde_json::json!(["theseus-lab#*"])
    );

    let fake = Fake::start();
    let lab = Lab::new(&fake);
    lab.sessions(4);
    lab.ship().await;
    let into = lab.target("fresh");
    std::fs::create_dir_all(&into).unwrap();
    let sock = into.join("theseusd.sock");
    let _serving = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let seen = fake.state.lock().unwrap().seen.len();
    let knobs = Knobs::default();
    let e = restore::from_s3(&lab.aws, URL, &into, &sock, false, &knobs)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("serving") && e.contains("nothing was fetched"),
        "{e}"
    );
    assert_eq!(
        fake.state.lock().unwrap().seen.len(),
        seen,
        "a request while it serves"
    );

    // Stopped: the restore's requests are its session's mint and reads.
    let r = restore::from_s3(&lab.aws, URL, &into, &into.join("gone.sock"), false, &knobs)
        .await
        .unwrap();
    assert!(r.restore.sessions >= 4);
    let ops = ops_since(&fake, seen);
    assert!(
        ops.iter().all(
            |o| ["AssumeRole", "GetCallerIdentity", "GetObject", "Query"].contains(&o.as_str())
        ),
        "{ops:?}"
    );
    let minted: Vec<String> = fake
        .ops("AssumeRole")
        .iter()
        .map(|q| String::from_utf8_lossy(&q.body).into_owned())
        .filter(|b| b.contains("RoleSessionName=theseus-restore"))
        .collect();
    assert_eq!(minted.len(), 1, "one restore session");
    for write in ["PutObject", "BatchWriteItem", "Delete"] {
        assert!(!minted[0].contains(write), "{write} in {}", minted[0]);
    }
}
