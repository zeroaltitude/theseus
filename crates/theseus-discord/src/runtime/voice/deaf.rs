//! A call that joins but never hears (theseus-d93y): Discord's voice
//! channels encrypt end to end (DAVE, over an MLS group), and when the
//! group's welcome never comes the call decrypts nothing while the join
//! looks fine. Noticed by one pure rule over a snapshot, on a timer after
//! the join and never on the audio path; rejoined once; said in health and
//! in the place's text, never aloud.
//!
//! A call that heard can stop hearing (theseus-sro04): the watch goes on
//! after the call hears, and between two checks a listed speaker whose
//! sender reports say they sent [`LOST_SENT`] packets, none of which
//! arrived, is lost audio, rejoined as a deaf join is (once a call). A
//! speaker who is silent sends no packets, so their reports say none: a
//! quiet call is never deaf, however long its sender reports go on.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_protocol::LedgerKind;
use theseus_voice::{Link, SsrcCount};

use super::{session, update, VoicePlace};
use crate::runtime::Shared;

/// Said in the place's text when the call is found deaf. Each is headed as
/// `theseus_core::owner_note::head` writes it (theseus-x8b9k).
pub(crate) const DEAF: &str = "⚠️ Alert · Voice can't hear the call\n\
    Discord's voice encryption didn't finish setting up\nRejoining…";
/// Said when a call that heard stops hearing a listed speaker who talks.
pub(crate) const LOST: &str =
    "⚠️ Alert · Voice stopped hearing you\nyour audio isn't reaching me\nRejoining…";
/// Said when audio arrives that no speaking event says whose it is.
pub(crate) const UNMAPPED: &str = "⚠️ Alert · Voice can't hear the call\n\
    Discord didn't say whose audio is whose\nRejoining…";
/// Said when the rejoined call hears a listed speaker.
pub(crate) const REJOINED: &str =
    "ℹ️ FYI · Voice rejoined: I can hear you now\nthe rejoin after the call went deaf";
/// Said when the rejoined call is deaf too, or the rejoin failed.
pub(crate) const STILL_DEAF: &str =
    "✋ Action · Voice still can't hear: try /leave and /join\nthe rejoin did not fix it";

/// Between the leave and the join of a rejoin.
const REJOIN_GAP: Duration = Duration::from_secs(2);
/// How long a rejoin's connect may take, as a join's.
const REJOIN_WITHIN: Duration = Duration::from_secs(20);

pub(crate) type BoxFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Why a call is deaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Why {
    /// DAVE's session is not ready: no welcome into the group.
    NotReady,
    /// Packets arrive for a listed speaker, and none decrypts.
    Undecrypted,
    /// Audio decodes from an SSRC no speaking event mapped to a user, and
    /// none of the listed speakers present is mapped: their speaking event
    /// was missed, so their audio is dropped (theseus-sro04). A rejoin's
    /// fresh connection sends it again.
    Unmapped,
    /// A call that heard: between two checks a listed speaker sent
    /// [`LOST_SENT`] packets, by their sender reports, and none arrived.
    Lost,
}

/// Packets a listed speaker's sender reports say they sent between two
/// checks, none arriving, that make a call that heard deaf: 5 s of speech.
pub(crate) const LOST_SENT: u64 = 250;

impl Why {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Why::NotReady => "dave_not_ready",
            Why::Undecrypted => "undecrypted",
            Why::Unmapped => "unmapped",
            Why::Lost => "lost",
        }
    }

    /// What the place is told as the call rejoins.
    fn notice(self) -> &'static str {
        match self {
            Why::NotReady | Why::Undecrypted => DEAF,
            Why::Unmapped => UNMAPPED,
            Why::Lost => LOST,
        }
    }
}

/// What a call has heard, at one moment.
#[derive(Clone, Debug, Default)]
pub(crate) struct Hearing {
    /// DAVE's session is ready: `None` when nothing says (songbird 0.6
    /// keeps the session private, so only a decrypted packet says yes).
    pub ready: Option<bool>,
    /// Since the join, or the rejoin.
    pub elapsed: Duration,
    /// The listed speakers in the channel now, from the gateway's voice
    /// states.
    pub present: Vec<u64>,
    /// Per SSRC: the speaking map's user, the packets that arrived, and
    /// those that decrypted.
    pub ssrcs: BTreeMap<u32, SsrcCount>,
}

impl Hearing {
    /// From songbird's counts: a packet is a tick with one or an RTCP
    /// sender report; any decoded audio means DAVE is ready.
    pub(crate) fn of(
        counts: BTreeMap<u32, SsrcCount>,
        present: Vec<u64>,
        elapsed: Duration,
    ) -> Self {
        let ready = counts.values().any(|c| c.decoded > 0).then_some(true);
        Self {
            ready,
            elapsed,
            present,
            ssrcs: counts,
        }
    }

    /// The SSRCs of the listed speakers present.
    fn present(&self) -> impl Iterator<Item = &SsrcCount> {
        self.ssrcs
            .values()
            .filter(|c| c.user.is_some_and(|u| self.present.contains(&u.0)))
    }

    /// A listed speaker's audio decrypted: the call hears.
    pub(crate) fn hears(&self) -> bool {
        self.present().any(|c| c.decoded > 0)
    }
}

/// The rule: deaf when, `after` past the join, a listed speaker is in the
/// channel and DAVE's session is not ready, or audio decodes from an SSRC
/// mapped to no one while no listed speaker present is mapped, or packets
/// arrive for a listed speaker and none of theirs decrypts. A silent
/// channel is not deaf, nor a bot alone.
pub(crate) fn deaf(h: &Hearing, after: Duration) -> Option<Why> {
    if h.elapsed < after || h.present.is_empty() {
        return None;
    }
    if h.ready == Some(false) {
        return Some(Why::NotReady);
    }
    let unmapped = h.ssrcs.values().any(|c| c.user.is_none() && c.decoded > 0);
    if unmapped && h.present().next().is_none() {
        return Some(Why::Unmapped);
    }
    h.present()
        .any(|c| c.frames + c.reports > 0 && c.decoded == 0)
        .then_some(Why::Undecrypted)
}

/// The rule for a call that heard, between the counts of the last check,
/// `before`, and now: a listed speaker present whose sender reports say they
/// sent [`LOST_SENT`] packets or more, of which none arrived. A silent
/// speaker sends none, nor does a speaker that left.
pub(crate) fn lost(before: &BTreeMap<u32, SsrcCount>, h: &Hearing) -> Option<Why> {
    h.ssrcs
        .iter()
        .filter(|(_, c)| c.user.is_some_and(|u| h.present.contains(&u.0)))
        .any(|(ssrc, c)| {
            let b = before.get(ssrc).copied().unwrap_or_default();
            c.sent.saturating_sub(b.sent) >= LOST_SENT && c.frames == b.frames
        })
        .then_some(Why::Lost)
}

/// The call's connection, as the check reads and rejoins it: songbird's, or
/// a test's.
pub(crate) trait Line: Send + Sync {
    /// Per SSRC, since the connection began.
    fn counts(&self) -> BTreeMap<u32, SsrcCount>;
    /// Leave the channel and join it again, unless `wanted`, asked just
    /// before the join, says the call ended.
    fn rejoin<'a>(
        &'a self,
        wanted: &'a (dyn Fn() -> bool + Send + Sync),
    ) -> BoxFuture<'a, Result<(), String>>;
}

impl Line for Link {
    fn counts(&self) -> BTreeMap<u32, SsrcCount> {
        Link::counts(self)
    }

    fn rejoin<'a>(
        &'a self,
        wanted: &'a (dyn Fn() -> bool + Send + Sync),
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(Link::rejoin(self, REJOIN_GAP, REJOIN_WITHIN, wanted))
    }
}

/// Watch call `serial` from its join: every `after`, until it ends. Deaf
/// before it hears ([`deaf`]), or once it heard ([`lost`]), it says so and
/// rejoins once; deaf again, or a rejoin that fails, it says so and stops.
/// Never a second rejoin.
pub(crate) async fn watch(
    shared: Arc<Shared>,
    serial: u64,
    place: VoicePlace,
    line: Arc<dyn Line>,
    after: Duration,
) {
    let mut since = Instant::now();
    let mut attempt = 1;
    // Once the call hears: the counts at the last check.
    let mut heard: Option<BTreeMap<u32, SsrcCount>> = None;
    loop {
        tokio::time::sleep(after).await;
        let Some(channel) = current(&shared, serial) else {
            return;
        };
        let present = shared.voice.present(&place, channel);
        let counts = line.counts();
        let h = Hearing::of(counts.clone(), present, since.elapsed());
        let why = match heard.replace(counts) {
            Some(before) => lost(&before, &h),
            None if h.hears() => {
                if attempt > 1 {
                    update(&shared, |s| s.deaf_since_ms = None);
                    notice(&shared, &place, REJOINED);
                }
                continue;
            }
            None => {
                heard = None;
                deaf(&h, after)
            }
        };
        let Some(why) = why else {
            continue;
        };
        let elapsed_ms = h.elapsed.as_millis() as u64;
        let sid = session(&shared, &place);
        shared.core.binding_ledger(
            LedgerKind::VoiceDeaf,
            sid.as_deref(),
            json!({"why": why.as_str(), "elapsed_ms": elapsed_ms, "attempt": attempt, "place": place.label}),
        );
        shared.core.telemetry().record_voice_deaf(why.as_str());
        if attempt > 1 {
            give_up(&shared, &place);
            return;
        }
        update(&shared, |s| {
            s.deaf_since_ms = Some(theseus_protocol::now_unix_ms());
        });
        notice(&shared, &place, why.notice());
        let t0 = Instant::now();
        let wanted = || current(&shared, serial).is_some();
        let rejoined = line.rejoin(&wanted).await;
        // A call that ended while it rejoined (a `/leave`, a drop) is told
        // nothing more, and health's block may be the next call's by now.
        if current(&shared, serial).is_none() {
            return;
        }
        update(&shared, |s| s.rejoins += 1);
        let ms = t0.elapsed().as_millis() as u64;
        let row = match &rejoined {
            Ok(()) => json!({"ok": true, "elapsed_ms": ms, "place": place.label}),
            Err(e) => json!({"ok": false, "error": e, "elapsed_ms": ms, "place": place.label}),
        };
        shared
            .core
            .binding_ledger(LedgerKind::VoiceRejoined, sid.as_deref(), row);
        if rejoined.is_err() {
            give_up(&shared, &place);
            return;
        }
        attempt += 1;
        since = Instant::now();
        heard = None;
    }
}

/// The call is deaf after its one rejoin: it stays joined, health says so,
/// and so does the place, once.
fn give_up(shared: &Shared, place: &VoicePlace) {
    update(shared, |s| s.deaf_failed = true);
    notice(shared, place, STILL_DEAF);
}

impl super::Voice {
    /// The place's users in `channel` now, from the gateway's voice states.
    fn present(&self, place: &VoicePlace, channel: u64) -> Vec<u64> {
        let states = self.states.lock().unwrap();
        place
            .users
            .iter()
            .copied()
            .filter(|u| states.get(u) == Some(&channel))
            .collect()
    }
}

/// The channel of call `serial`, while it is the one joined.
fn current(shared: &Shared, serial: u64) -> Option<u64> {
    shared
        .voice
        .call
        .lock()
        .unwrap()
        .as_ref()
        .filter(|c| c.serial == serial)
        .map(|c| c.channel)
}

/// A notice in the voice place's text, through the outbox.
fn notice(shared: &Shared, place: &VoicePlace, text: &str) {
    let Some(sid) = session(shared, place) else {
        return;
    };
    let body = json!({"kind": "notice", "text": text});
    let target = format!("discord:{}", place.key);
    if let Err(e) = shared.core.outbox.post(&sid, "", &target, body) {
        shared
            .board
            .error("post notice", Some(&sid), format!("{e:#}"));
    }
    shared.wake_lanes();
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_voice::Speaker;

    const OWNER: u64 = 42;
    const GUEST: u64 = 77;

    fn ssrc(user: Option<u64>, frames: u64, decoded: u64, reports: u64) -> SsrcCount {
        SsrcCount {
            user: user.map(Speaker),
            frames,
            decoded,
            reports,
            sent: 0,
        }
    }

    fn hearing(ready: Option<bool>, present: &[u64], ssrcs: &[(u32, SsrcCount)]) -> Hearing {
        Hearing {
            ready,
            elapsed: Duration::from_secs(12),
            present: present.to_vec(),
            ssrcs: ssrcs.iter().copied().collect(),
        }
    }

    const AFTER: Duration = Duration::from_secs(10);

    /// DAVE not ready past the bound, with a listed speaker in the channel:
    /// deaf, even before they speak; within the bound, not yet.
    #[test]
    fn not_ready_past_the_bound_with_a_listed_speaker_is_deaf() {
        let mut h = hearing(Some(false), &[OWNER], &[]);
        assert_eq!(deaf(&h, AFTER), Some(Why::NotReady));
        h.elapsed = Duration::from_secs(9);
        assert_eq!(deaf(&h, AFTER), None);
    }

    /// A bot alone is never deaf, ready or not, whatever an unlisted
    /// person's packets do.
    #[test]
    fn alone_is_not_deaf() {
        assert_eq!(deaf(&hearing(Some(false), &[], &[]), AFTER), None);
        let guest = [(5, ssrc(Some(GUEST), 0, 0, 40))];
        assert_eq!(deaf(&hearing(None, &[], &guest), AFTER), None);
    }

    /// A listed speaker present and silent, with DAVE ready or unknown: not
    /// deaf. A silent speaker's SSRC with nothing sent is no evidence.
    #[test]
    fn silent_but_ready_is_not_deaf() {
        assert_eq!(deaf(&hearing(Some(true), &[OWNER], &[]), AFTER), None);
        let quiet = [(9, ssrc(Some(OWNER), 0, 0, 0))];
        assert_eq!(deaf(&hearing(None, &[OWNER], &quiet), AFTER), None);
    }

    /// Ready, but a listed speaker's packets arrive and none decrypts: deaf.
    /// Their sender reports count as packets; one decrypted frame of theirs
    /// is hearing; an unlisted speaker's packets don't count.
    #[test]
    fn ready_but_nothing_decrypts_while_packets_arrive_is_deaf() {
        let reports = [(9, ssrc(Some(OWNER), 0, 0, 30))];
        assert_eq!(
            deaf(&hearing(Some(true), &[OWNER], &reports), AFTER),
            Some(Why::Undecrypted)
        );
        assert_eq!(
            deaf(&hearing(None, &[OWNER], &reports), AFTER),
            Some(Why::Undecrypted)
        );
        let heard = [(9, ssrc(Some(OWNER), 50, 1, 30))];
        let h = hearing(Some(true), &[OWNER], &heard);
        assert_eq!(deaf(&h, AFTER), None);
        assert!(h.hears());
        let guest = [(5, ssrc(Some(GUEST), 0, 0, 30))];
        assert_eq!(deaf(&hearing(Some(true), &[OWNER], &guest), AFTER), None);
    }

    /// Audio decoding from an SSRC no speaking event mapped, with no listed
    /// speaker present mapped (the v1 voice check, theseus-sro04): deaf, and
    /// told so. Once the listed speaker is mapped, another unmapped SSRC is
    /// no evidence; alone, it is nothing.
    #[test]
    fn audio_from_an_unmapped_ssrc_with_no_listed_speaker_mapped_is_deaf() {
        let lost = [(3986, ssrc(None, 400, 380, 20))];
        let h = hearing(Some(true), &[OWNER], &lost);
        assert_eq!(deaf(&h, AFTER), Some(Why::Unmapped));
        assert_eq!(Why::Unmapped.notice(), UNMAPPED);
        assert_eq!(Why::Unmapped.as_str(), "unmapped");
        let mapped = [
            (9, ssrc(Some(OWNER), 0, 0, 0)),
            (3986, ssrc(None, 400, 380, 20)),
        ];
        assert_eq!(deaf(&hearing(Some(true), &[OWNER], &mapped), AFTER), None);
        assert_eq!(deaf(&hearing(Some(true), &[], &lost), AFTER), None);
        let undecoded = [(3986, ssrc(None, 400, 0, 20))];
        assert_eq!(deaf(&hearing(None, &[OWNER], &undecoded), AFTER), None);
    }

    /// A call that heard (theseus-sro04): a listed speaker's reports say
    /// they sent 5 s of packets since the last check and none arrived: lost.
    /// Silent (their reports say nothing sent), or their packets arriving,
    /// or a guest's loss, or an absent owner's: not.
    #[test]
    fn a_call_that_heard_is_lost_when_sent_packets_stop_arriving() {
        let at = |frames, sent| SsrcCount {
            sent,
            ..ssrc(Some(OWNER), frames, frames, 40)
        };
        let before = BTreeMap::from([(9, at(300, 1000))]);
        let now = |c: SsrcCount, present: &[u64]| hearing(Some(true), present, &[(9, c)]);
        let gone = at(300, 1000 + LOST_SENT);
        assert_eq!(lost(&before, &now(gone, &[OWNER])), Some(Why::Lost));
        assert_eq!(Why::Lost.notice(), LOST);
        for n in [DEAF, LOST, UNMAPPED, REJOINED, STILL_DEAF] {
            assert!(theseus_core::owner_note::is_headed(n), "{n}");
        }
        assert_eq!(Why::Lost.as_str(), "lost");
        assert_eq!(lost(&before, &now(at(300, 1000), &[OWNER])), None, "silent");
        assert_eq!(
            lost(&before, &now(at(301, 1250), &[OWNER])),
            None,
            "arriving"
        );
        assert_eq!(
            lost(&before, &now(at(300, 1249), &[OWNER])),
            None,
            "under 5 s"
        );
        assert_eq!(lost(&before, &now(gone, &[])), None, "not in the channel");
        let guest = SsrcCount {
            user: Some(Speaker(GUEST)),
            ..gone
        };
        assert_eq!(lost(&before, &now(guest, &[OWNER])), None, "a guest's");
    }

    /// From songbird's counts: any decoded audio says DAVE is ready, and
    /// nothing says it isn't.
    #[test]
    fn songbirds_counts_say_ready_only_by_a_decrypted_packet() {
        let none = Hearing::of(BTreeMap::new(), vec![OWNER], AFTER);
        assert_eq!(none.ready, None);
        let guest = BTreeMap::from([(5, ssrc(Some(GUEST), 3, 3, 0))]);
        let h = Hearing::of(guest, vec![OWNER], AFTER);
        assert_eq!(h.ready, Some(true));
        assert!(!h.hears(), "a guest's audio is not a listed speaker's");
    }
}
