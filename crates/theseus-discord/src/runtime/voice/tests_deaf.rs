//! A call that joins but never hears (theseus-d93y), through a fake
//! connection: one deaf join, a rejoin that hears, one that doesn't, and one
//! that fails; the rows, health's line, the two notices, and never a second
//! rejoin.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

use theseus_voice::{Speaker, SsrcCount};

use super::deaf::{watch, BoxFuture, Line, DEAF, REJOINED, STILL_DEAF};
use super::tests::{posts, LOUNGE, OWNER};
use super::tests_heard::{lounge_scripted, Lounge};
use super::VoicePlace;

/// The check's period here: short, so a test waits for several.
const AFTER: Duration = Duration::from_millis(30);

/// A connection whose counts are `before` until its rejoin, then `after`;
/// its rejoin answers `rejoined`, and counts itself.
struct Fake {
    before: SsrcCount,
    after: SsrcCount,
    rejoined: Result<(), String>,
    rejoins: AtomicU32,
}

impl Line for Fake {
    fn counts(&self) -> BTreeMap<u32, SsrcCount> {
        let c = match self.rejoins.load(Ordering::SeqCst) {
            0 => self.before,
            _ => self.after,
        };
        BTreeMap::from([(9, c)])
    }

    fn rejoin<'a>(
        &'a self,
        _wanted: &'a (dyn Fn() -> bool + Send + Sync),
    ) -> BoxFuture<'a, Result<(), String>> {
        self.rejoins.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::ready(self.rejoined.clone()))
    }
}

/// The owner's SSRC: `reports` sender reports, `decoded` decrypted ticks.
fn owner(reports: u64, decoded: u64) -> SsrcCount {
    SsrcCount {
        user: Some(Speaker(OWNER)),
        frames: decoded,
        decoded,
        reports,
    }
}

/// The lounge, joined, with the owner in it (or not), and `fake` as its
/// connection, watched until the watch ends (or 5 s).
async fn watched(dir: &std::path::Path, fake: Arc<Fake>, present: bool) -> (Lounge, bool) {
    let l = lounge_scripted(dir, vec![]);
    if present {
        l.place
            .shared
            .voice
            .states
            .lock()
            .unwrap()
            .insert(OWNER, LOUNGE);
    }
    let at = VoicePlace {
        key: l.place.key.clone(),
        label: "#lounge".into(),
        users: vec![OWNER],
        guild: 100_000_000_000_000_001,
    };
    let line: Arc<dyn Line> = fake;
    let w = tokio::spawn(watch(Arc::clone(&l.place.shared), 1, at, line, AFTER));
    let ended = tokio::time::timeout(Duration::from_secs(5), w)
        .await
        .is_ok();
    (l, ended)
}

/// The rows of `kind`, their data.
async fn rows(l: &Lounge, kind: &str) -> Vec<serde_json::Value> {
    l.rows(kind).await.into_iter().map(|(_, d)| d).collect()
}

/// A deaf join is a `voice.deaf` row, a notice and one rejoin; the rejoin
/// that hears is a `voice.rejoined` row and the second notice, and the
/// watch ends. Health says it rejoined, not that it is deaf.
#[tokio::test]
async fn a_deaf_join_rejoins_once_and_says_when_it_hears() {
    let d = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        before: owner(40, 0),
        after: owner(40, 25),
        rejoined: Ok(()),
        rejoins: AtomicU32::new(0),
    });
    let (l, ended) = watched(d.path(), Arc::clone(&fake), true).await;
    assert!(ended, "the watch ends once the call hears");
    assert_eq!(fake.rejoins.load(Ordering::SeqCst), 1);
    let deaf = rows(&l, "voice.deaf").await;
    assert_eq!(deaf.len(), 1, "{deaf:?}");
    assert_eq!(
        (&deaf[0]["why"], &deaf[0]["attempt"]),
        (&"undecrypted".into(), &1.into())
    );
    assert!(deaf[0]["elapsed_ms"].as_u64().unwrap() >= 30, "{deaf:?}");
    let rejoined = rows(&l, "voice.rejoined").await;
    assert_eq!(rejoined.len(), 1, "{rejoined:?}");
    assert_eq!(rejoined[0]["ok"], true);
    assert!(l
        .rows("voice.deaf")
        .await
        .iter()
        .all(|(s, _)| *s == Some(l.sid.clone())));
    assert_eq!(posts(&core_of(&l), &l.place.target), [DEAF, REJOINED]);
    let line = l.place.shared.voice.status().line();
    assert!(line.contains(" · rejoined once · "), "{line}");
    assert!(!line.contains("deaf"), "{line}");
}

/// A rejoin that hears nothing either: a second `voice.deaf` row, the
/// notice to leave and join, health's `deaf (rejoin failed)`, and the
/// watch ends with one rejoin: never a third attempt.
#[tokio::test]
async fn a_rejoin_that_is_deaf_too_stays_joined_and_stops_trying() {
    let d = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        before: owner(40, 0),
        after: owner(12, 0),
        rejoined: Ok(()),
        rejoins: AtomicU32::new(0),
    });
    let (l, ended) = watched(d.path(), Arc::clone(&fake), true).await;
    assert!(ended, "the watch stops");
    // Many periods later, still one rejoin.
    tokio::time::sleep(AFTER * 10).await;
    assert_eq!(fake.rejoins.load(Ordering::SeqCst), 1);
    let attempts: Vec<_> = rows(&l, "voice.deaf")
        .await
        .iter()
        .map(|r| r["attempt"].as_u64().unwrap())
        .collect();
    assert_eq!(attempts, [1, 2]);
    assert_eq!(rows(&l, "voice.rejoined").await.len(), 1);
    assert_eq!(posts(&core_of(&l), &l.place.target), [DEAF, STILL_DEAF]);
    assert!(l.place.shared.voice.joined().is_some(), "still joined");
    let line = l.place.shared.voice.status().line();
    assert!(line.contains(" · deaf (rejoin failed) · "), "{line}");
}

/// A rejoin that fails to connect is said as a deaf rejoin is, with its
/// error on the row; no second rejoin.
#[tokio::test]
async fn a_rejoin_that_fails_says_so_and_stops() {
    let d = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        before: owner(40, 0),
        after: owner(0, 0),
        rejoined: Err("no voice connection in 20 s".into()),
        rejoins: AtomicU32::new(0),
    });
    let (l, ended) = watched(d.path(), Arc::clone(&fake), true).await;
    assert!(ended);
    assert_eq!(fake.rejoins.load(Ordering::SeqCst), 1);
    let rejoined = rows(&l, "voice.rejoined").await;
    assert_eq!(
        (&rejoined[0]["ok"], &rejoined[0]["error"]),
        (&false.into(), &"no voice connection in 20 s".into())
    );
    assert_eq!(posts(&core_of(&l), &l.place.target), [DEAF, STILL_DEAF]);
}

/// A bot alone in the channel is never deaf, nor one whose listed speaker
/// is silent: no row, no notice, no rejoin; the watch ends with the call.
#[tokio::test]
async fn alone_or_silent_the_call_is_not_deaf() {
    for (present, ssrc) in [(false, owner(40, 0)), (true, owner(0, 0))] {
        let d = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake {
            before: ssrc,
            after: ssrc,
            rejoined: Ok(()),
            rejoins: AtomicU32::new(0),
        });
        let l = lounge_scripted(d.path(), vec![]);
        if present {
            l.place
                .shared
                .voice
                .states
                .lock()
                .unwrap()
                .insert(OWNER, LOUNGE);
        }
        let at = VoicePlace {
            key: l.place.key.clone(),
            label: "#lounge".into(),
            users: vec![OWNER],
            guild: 100_000_000_000_000_001,
        };
        let line: Arc<dyn Line> = fake.clone();
        let w = tokio::spawn(watch(Arc::clone(&l.place.shared), 1, at, line, AFTER));
        tokio::time::sleep(AFTER * 10).await;
        assert!(!w.is_finished(), "still watching while the call is up");
        // The call ends: the watch with it.
        *l.place.shared.voice.call.lock().unwrap() = None;
        tokio::time::timeout(Duration::from_secs(5), w)
            .await
            .expect("the watch ends with the call")
            .unwrap();
        assert_eq!(fake.rejoins.load(Ordering::SeqCst), 0, "present: {present}");
        assert!(rows(&l, "voice.deaf").await.is_empty());
        assert!(posts(&core_of(&l), &l.place.target).is_empty());
    }
}

fn core_of(l: &Lounge) -> Arc<theseus_core::Core> {
    Arc::clone(&l.place.shared.core)
}

/// The usual call: a listed speaker heard by the first check ends the watch
/// with nothing said, written or rejoined.
#[tokio::test]
async fn a_call_that_hears_ends_its_watch_in_silence() {
    let d = tempfile::tempdir().unwrap();
    let fake = Arc::new(Fake {
        before: owner(40, 25),
        after: owner(40, 25),
        rejoined: Ok(()),
        rejoins: AtomicU32::new(0),
    });
    let (l, ended) = watched(d.path(), Arc::clone(&fake), true).await;
    assert!(ended, "the watch ends once the call hears");
    assert_eq!(fake.rejoins.load(Ordering::SeqCst), 0);
    assert!(rows(&l, "voice.deaf").await.is_empty());
    assert!(posts(&core_of(&l), &l.place.target).is_empty());
    let line = l.place.shared.voice.status().line();
    assert!(
        !line.contains("deaf") && !line.contains("rejoined"),
        "{line}"
    );
}

/// A deaf connection whose rejoin waits for `go`, then asks `wanted` as
/// songbird's does before its join: a call that ended is not joined, and
/// the rejoin errs.
struct Slow {
    go: Notify,
    asked: Mutex<Option<bool>>,
}

impl Line for Slow {
    fn counts(&self) -> BTreeMap<u32, SsrcCount> {
        BTreeMap::from([(9, owner(40, 0))])
    }

    fn rejoin<'a>(
        &'a self,
        wanted: &'a (dyn Fn() -> bool + Send + Sync),
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.go.notified().await;
            let still = wanted();
            *self.asked.lock().unwrap() = Some(still);
            match still {
                true => Ok(()),
                false => Err("the call ended during the rejoin".into()),
            }
        })
    }
}

/// The lounge's call 1, deaf, with the owner in it: its watch spawned on a
/// `Slow` connection, returned once the DEAF notice is posted, so its rejoin
/// waits for `go`.
async fn rejoining(dir: &std::path::Path) -> (Lounge, Arc<Slow>, tokio::task::JoinHandle<()>) {
    let l = lounge_scripted(dir, vec![]);
    let shared = &l.place.shared;
    shared.voice.states.lock().unwrap().insert(OWNER, LOUNGE);
    let at = VoicePlace {
        key: l.place.key.clone(),
        label: "#lounge".into(),
        users: vec![OWNER],
        guild: 100_000_000_000_000_001,
    };
    let slow = Arc::new(Slow {
        go: Notify::new(),
        asked: Mutex::new(None),
    });
    let line: Arc<dyn Line> = slow.clone();
    let w = tokio::spawn(watch(Arc::clone(shared), 1, at, line, AFTER));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while posts(&shared.core, &l.place.target) != [DEAF] {
        assert!(std::time::Instant::now() < deadline, "the DEAF notice");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    (l, slow, w)
}

/// `/leave` while the call rejoins: the rejoin is told the call ended, so it
/// doesn't join again, and the left call is told nothing more: no "Still
/// can't hear", no row, and health as the leave left it.
#[tokio::test]
async fn a_call_left_while_it_rejoins_is_not_joined_again_or_told_more() {
    let d = tempfile::tempdir().unwrap();
    let (l, slow, w) = rejoining(d.path()).await;
    let shared = &l.place.shared;
    // `/leave`, as Place::leave does it: the call's slot taken, then ended.
    let call = shared.voice.call.lock().unwrap().take().expect("joined");
    super::ended(shared, &call, "left", "the test");
    slow.go.notify_one();
    tokio::time::timeout(Duration::from_secs(5), w)
        .await
        .expect("the watch ends")
        .unwrap();
    assert_eq!(*slow.asked.lock().unwrap(), Some(false));
    assert_eq!(posts(&core_of(&l), &l.place.target), [DEAF]);
    assert!(rows(&l, "voice.rejoined").await.is_empty());
    let s = shared.voice.status();
    assert_eq!(
        (s.state.as_str(), s.rejoins, s.deaf_failed),
        ("ready", 0, false)
    );
}

/// `/leave` then `/join` while the old call rejoins: the new call is never
/// marked deaf by it, nor told so.
#[tokio::test]
async fn the_next_call_is_not_marked_by_the_old_calls_rejoin() {
    let d = tempfile::tempdir().unwrap();
    let (l, slow, w) = rejoining(d.path()).await;
    let shared = &l.place.shared;
    let mut call = shared.voice.call.lock().unwrap().take().expect("joined");
    super::ended(shared, &call, "left", "the test");
    // The next call, serial 2, as start_call sets it up.
    call.serial = 2;
    *shared.voice.call.lock().unwrap() = Some(call);
    super::update(shared, |s| {
        s.state = "joined".into();
        s.channel = Some("#lounge".into());
    });
    slow.go.notify_one();
    tokio::time::timeout(Duration::from_secs(5), w)
        .await
        .expect("the watch ends")
        .unwrap();
    assert_eq!(*slow.asked.lock().unwrap(), Some(false));
    assert_eq!(posts(&core_of(&l), &l.place.target), [DEAF]);
    let s = shared.voice.status();
    assert_eq!((s.rejoins, s.deaf_failed), (0, false));
    assert!(!s.line().contains("deaf"), "{}", s.line());
}
