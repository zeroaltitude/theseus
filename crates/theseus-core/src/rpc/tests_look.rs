//! The store's look (issue 46): where the owner last looked at the work board
//! is kept in the store it points into (META `look`), so a restore of an older
//! backup, a switch of store and a copy each tell "since you last looked" from
//! their own history. A look never moves back, never past the store's head, and
//! a look at its own record writes nothing.

use std::path::Path;
use std::time::Duration;

use theseus_protocol::{
    error_code, method, DigestKind, LedgerKind, Looked, WorkDigestParams, WorkDigestResult,
    WorkLookParams,
};
use theseus_store::kinds;

use super::look::{LOOK_KEY, PAST_HEAD};
use crate::approval::Surface;
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;
use crate::tests_task_graph::{rig_on, session, turn, Rig};

fn rig_in(dir: tempfile::TempDir) -> Rig {
    rig_on(dir, |_| {})
}

/// A conversation that ran one turn: its execution id.
async fn conversation(r: &Rig) -> String {
    let s = session(&r.core);
    turn(&r.core, &s, "chart the outer harbour").await;
    let rec: SessionRecord = r.core.store.get_session(&s).unwrap().unwrap();
    rec.execution_id.expect("the turn ran an execution")
}

/// Something the digest tells: a wake of `exe` fired. Its position.
fn woke(r: &Rig, exe: &str) -> u64 {
    let row = LedgerRow::new(
        LedgerKind::WakeFired,
        None,
        None,
        serde_json::json!({"execution_id": exe, "purpose": "check"}),
    );
    r.core.store.append_ledger(&row).unwrap()
}

async fn look(r: &Rig, position: u64) -> Looked {
    r.core
        .work_look_for(WorkLookParams { position }, Surface::Cli)
        .await
        .unwrap()
        .look
}

/// The look as the store holds it, and the position of its record.
fn stored(r: &Rig) -> Option<(Looked, u64)> {
    let rec = theseus_store::Store::latest_by_key(&**r.core.store.inner(), kinds::META, LOOK_KEY)
        .unwrap()?;
    Some((rec.decode().unwrap(), rec.position))
}

fn since_look() -> WorkDigestParams {
    WorkDigestParams {
        since_look: Some(true),
        ..Default::default()
    }
}

fn woke_count(d: &WorkDigestResult) -> u32 {
    d.lines
        .iter()
        .filter(|l| l.kind == DigestKind::Woke)
        .map(|l| l.count)
        .sum()
}

/// The digest once it tells `n` wakes: the board takes a frame a moment after
/// it is written.
async fn digest_telling(r: &Rig, p: WorkDigestParams, n: u32) -> WorkDigestResult {
    let mut last = None;
    for _ in 0..100 {
        let d = r
            .core
            .work_digest_for(p.clone(), Surface::Cli)
            .await
            .unwrap();
        if woke_count(&d) == n {
            return d;
        }
        last = Some(d);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the digest never told {n} wakes: {last:?}");
}

/// A copy of a store's WAL and blobs into a new state directory, as another
/// start or another machine opens it (the index is rebuilt from the WAL).
fn copy_store(from: &Path, to: &Path) {
    for part in ["wal", "blobs"] {
        let src = from.join(part);
        if !src.is_dir() {
            continue;
        }
        let dst = to.join("store").join(part);
        std::fs::create_dir_all(&dst).unwrap();
        for e in std::fs::read_dir(&src).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_file() {
                std::fs::copy(e.path(), dst.join(e.file_name())).unwrap();
            }
        }
    }
}

fn copied(r: &Rig) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    copy_store(r.core.store.dir(), dir.path());
    rig_in(dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_look_lives_in_the_store() {
    let r = rig_in(tempfile::tempdir().unwrap());
    let exe = conversation(&r).await;
    // No look yet: `since_look` reads as the time it was given says.
    let first = r
        .core
        .work_digest_for(
            WorkDigestParams {
                since_look: Some(true),
                since_ms: Some(0),
                ..Default::default()
            },
            Surface::Cli,
        )
        .await
        .unwrap();
    assert_eq!(first.look, None);
    woke(&r, &exe);
    let p = r.core.store.last_position();
    let l = look(&r, p).await;
    assert_eq!(l.position, p);
    assert!(l.at_ms > 0);
    assert_eq!(stored(&r).map(|(l, _)| l), Some(l));
    // Another start of the same store: the look is there.
    let again = copied(&r);
    let d = again
        .core
        .work_digest_for(since_look(), Surface::Cli)
        .await
        .unwrap();
    assert_eq!(d.from, p);
    assert_eq!(d.look, Some(l));
    assert_eq!(woke_count(&d), 0, "the wake was before the look: {d:?}");
}

/// (a) A restore of an older backup: the restored store tells from the look it
/// had when the backup was taken, so what happens after the restore is told,
/// even at positions the newer history had already passed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restored_older_backup_starts_from_its_own_look() {
    let r = rig_in(tempfile::tempdir().unwrap());
    let exe = conversation(&r).await;
    woke(&r, &exe);
    let p1 = r.core.store.last_position();
    look(&r, p1).await;
    // The backup: the store as it was then.
    let backup = tempfile::tempdir().unwrap();
    copy_store(r.core.store.dir(), backup.path());
    // The newer history runs on, and the owner looks again, far ahead.
    for _ in 0..40 {
        woke(&r, &exe);
    }
    let p2 = r.core.store.last_position();
    assert_eq!(look(&r, p2).await.position, p2);
    // The restore, and new work in the restored history.
    let into = tempfile::tempdir().unwrap();
    crate::restore::restore(&backup.path().join("store"), into.path(), false).unwrap();
    let restored = rig_in(into);
    // A terminal connects to the restored daemon, and then new work lands.
    restored
        .core
        .work_digest_for(since_look(), Surface::Cli)
        .await
        .unwrap();
    let fresh = woke(&restored, &exe);
    assert!(
        fresh < p2,
        "the new work lands below the newer history's look"
    );
    let d = digest_telling(&restored, since_look(), 1).await;
    assert_eq!(d.from, p1);
    assert_eq!(d.look.map(|l| l.position), Some(p1));
    // A marker kept beside the store would still say p2: nothing is told.
    let beside = restored
        .core
        .work_digest_for(
            WorkDigestParams {
                since: Some(p2),
                ..Default::default()
            },
            Surface::Cli,
        )
        .await
        .unwrap();
    assert_eq!(woke_count(&beside), 0, "{beside:?}");
}

/// (b) Two stores, each with its own look: a switch to one and back finds
/// each where the owner left it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_stores_each_keep_their_own_look() {
    let a = rig_in(tempfile::tempdir().unwrap());
    let b = rig_in(tempfile::tempdir().unwrap());
    let ea = conversation(&a).await;
    let eb = conversation(&b).await;
    woke(&a, &ea);
    for _ in 0..20 {
        woke(&b, &eb);
    }
    let pa = a.core.store.last_position();
    let pb = b.core.store.last_position();
    assert_ne!(pa, pb);
    look(&a, pa).await;
    look(&b, pb).await;
    let a2 = copied(&a);
    let b2 = copied(&b);
    let da = a2
        .core
        .work_digest_for(since_look(), Surface::Cli)
        .await
        .unwrap();
    let db = b2
        .core
        .work_digest_for(since_look(), Surface::Cli)
        .await
        .unwrap();
    assert_eq!((da.from, db.from), (pa, pb));
}

/// (c) A copied store carries its look, and the two copies part: each keeps
/// the look taken in it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copied_store_carries_its_look_and_copies_part() {
    let a = rig_in(tempfile::tempdir().unwrap());
    let exe = conversation(&a).await;
    woke(&a, &exe);
    let p = a.core.store.last_position();
    look(&a, p).await;
    let other = copied(&a);
    assert_eq!(stored(&other).map(|(l, _)| l.position), Some(p));
    for _ in 0..5 {
        woke(&a, &exe);
    }
    woke(&other, &exe);
    let pa = a.core.store.last_position();
    let po = other.core.store.last_position();
    look(&a, pa).await;
    look(&other, po).await;
    assert_eq!(stored(&a).map(|(l, _)| l.position), Some(pa));
    assert_eq!(stored(&other).map(|(l, _)| l.position), Some(po));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_look_never_moves_back_and_never_past_the_head() {
    let r = rig_in(tempfile::tempdir().unwrap());
    let exe = conversation(&r).await;
    for _ in 0..10 {
        woke(&r, &exe);
    }
    let p = r.core.store.last_position();
    look(&r, p).await;
    let back = look(&r, p - 5).await;
    assert_eq!(back.position, p, "a look never moves back");
    assert_eq!(stored(&r).map(|(l, _)| l.position), Some(p));
    let head = r.core.store.last_position();
    let past = r
        .core
        .work_look_for(
            WorkLookParams {
                position: head + 100,
            },
            Surface::Cli,
        )
        .await
        .unwrap_err();
    assert_eq!(past.code, error_code::INVALID_PARAMS);
    assert_eq!(past.message, PAST_HEAD);
    assert_eq!(
        stored(&r).map(|(l, _)| l.position),
        Some(p),
        "never clamped"
    );
}

/// A look at the look's own record writes nothing: a client that looks at
/// whatever the store's head is must not write a look for every look.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_look_with_nothing_new_writes_nothing() {
    let r = rig_in(tempfile::tempdir().unwrap());
    let exe = conversation(&r).await;
    woke(&r, &exe);
    let p = r.core.store.last_position();
    look(&r, p).await;
    let (first, own) = stored(&r).expect("the first look is written");
    assert_eq!(first.position, p);
    assert!(own > p, "the look's record comes after what it looked at");
    // The owner looks at the look's own record: nothing came since but it.
    let again = look(&r, own).await;
    assert_eq!(again, first);
    assert_eq!(stored(&r), Some((first, own)), "no second record");
    // Something new, and the look moves.
    woke(&r, &exe);
    let q = r.core.store.last_position();
    assert_eq!(look(&r, q).await.position, q);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_mcp_client_and_a_guarded_connection_cannot_move_the_look() {
    let r = rig_in(tempfile::tempdir().unwrap());
    let exe = conversation(&r).await;
    woke(&r, &exe);
    let p = r.core.store.last_position();
    for surface in [Surface::Mcp, Surface::Discord] {
        let e = r
            .core
            .work_look_for(WorkLookParams { position: p }, surface)
            .await
            .unwrap_err();
        assert_eq!(e.code, error_code::REFUSED, "{}", surface.name());
    }
    assert_eq!(stored(&r), None);
    assert!(!crate::mcp_server::allowed(method::WORK_LOOK));
}
