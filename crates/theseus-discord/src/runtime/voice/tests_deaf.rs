//! A call that joins but never hears (theseus-d93y), through a fake
//! connection: one deaf join, a rejoin that hears, one that doesn't, and one
//! that fails; the rows, health's line, the two notices, and never a second
//! rejoin.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

    fn rejoin(&self) -> BoxFuture<'_, Result<(), String>> {
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
