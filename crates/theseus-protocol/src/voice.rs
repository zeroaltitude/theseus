//! Voice in health (rows 77 and 78): the Discord binding's call, whom it
//! hears, and what its speech has cost since the start.

use serde::{Deserialize, Serialize};

/// The binding's voice, for health and `theseus health`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VoiceStatus {
    /// off | ready | joined
    pub state: String,
    /// The voice place joined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub channel: Option<String>,
    /// Whom it hears there, by name: the place's users.
    #[serde(default)]
    pub hears: Vec<String>,
    /// Since when, while joined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_ms: Option<u64>,
    /// Since the start: calls joined, utterances transcribed, sentences
    /// synthesized, barge-ins, stops that resumed (theseus-qb8o), failed
    /// calls.
    #[serde(default)]
    pub joins: u64,
    #[serde(default)]
    pub utterances: u64,
    #[serde(default)]
    pub sentences: u64,
    #[serde(default)]
    pub barge_ins: u64,
    #[serde(default)]
    pub resumes: u64,
    #[serde(default)]
    pub failures: u64,
    /// The speech heard, and the characters spoken.
    #[serde(default)]
    pub heard_ms: u64,
    #[serde(default)]
    pub spoken_chars: u64,
    /// What speech cost since the start, booked to its sessions.
    #[serde(default)]
    pub spend_micros: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
}

impl VoiceStatus {
    /// `theseus health`'s line: `voice: joined #lounge (eddie) · 4 utterances
    /// (12.3 s) · 9 sentences (512 chars) · 1 barge-in · 2 resumed · $0.0164`.
    pub fn line(&self) -> String {
        let at = match (&self.channel, self.state.as_str()) {
            (Some(c), _) => format!("{} {c} ({})", self.state, self.hears.join(", ")),
            (None, s) => s.to_string(),
        };
        let mut out = format!(
            "voice: {at} · {} utterance(s) ({:.1} s) · {} sentence(s) ({} chars) · {} barge-in(s) · \
             {} resumed · ${:.4}",
            self.utterances,
            self.heard_ms as f64 / 1000.0,
            self.sentences,
            self.spoken_chars,
            self.barge_ins,
            self.resumes,
            self.spend_micros as f64 / 1_000_000.0
        );
        if self.failures > 0 {
            out.push_str(&format!(" · {} failed", self.failures));
            if let Some(e) = &self.last_error {
                out.push_str(&format!(" (last: {e})"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_health_line_says_the_call_and_its_spend() {
        let v = VoiceStatus {
            state: "joined".into(),
            channel: Some("#lounge".into()),
            hears: vec!["robin".into()],
            utterances: 4,
            heard_ms: 12_340,
            sentences: 9,
            spoken_chars: 512,
            barge_ins: 1,
            resumes: 2,
            spend_micros: 16_400,
            ..VoiceStatus::default()
        };
        assert_eq!(
            v.line(),
            "voice: joined #lounge (robin) · 4 utterance(s) (12.3 s) · 9 sentence(s) (512 chars) · \
             1 barge-in(s) · 2 resumed · $0.0164"
        );
        let off = VoiceStatus {
            state: "ready".into(),
            failures: 2,
            last_error: Some("Deepgram synthesis: 503".into()),
            ..VoiceStatus::default()
        };
        assert!(
            off.line().starts_with("voice: ready · 0 utterance(s)"),
            "{}",
            off.line()
        );
        assert!(off
            .line()
            .ends_with("· 2 failed (last: Deepgram synthesis: 503)"));
    }
}
