//! `theseus persona voice …` (theseus-slvl.7): the speaking voice. `voice` shows the
//! voice a call speaks in now and where it comes from; `voice audition` makes
//! the persona's greeting (or your words) in several Aura voices, writes each
//! to a WAV file, and prints the paths and what the run cost, so you pick by
//! ear. Set the pick with `theseus persona edit --voice ID`. The audition
//! spends a few cents of the daemon's Deepgram key, which this command never
//! sees (the daemon makes the clips); it is the owner's, refused inside a job
//! (`client::OPERATORS`).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::voice_pick::{
    VoiceAuditionParams, VoiceAuditionResult, VoiceShowParams, VoiceShowResult,
};
use theseus_protocol::{method, usd};

use crate::cmd::output;

#[derive(Subcommand, Debug, Default)]
pub enum VoiceCmd {
    /// The voice a call speaks in now, where it comes from, and the greeting (default).
    #[default]
    Show,
    /// Say the greeting (or --text) in several voices, to WAV files you can play, and print
    /// their paths and the cost. Yours alone (refused inside a job); needs a Deepgram key.
    Audition {
        /// Aura voice ids, comma-separated; omitted: a default spread of six.
        #[arg(long, value_delimiter = ',')]
        voices: Vec<String>,
        /// What each says; omitted: the persona's greeting.
        #[arg(long)]
        text: Option<String>,
        /// The directory for the clips (made if missing); omitted: ./voice-audition.
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: VoiceCmd) -> Result<()> {
    match cmd {
        VoiceCmd::Show => {
            let v = conn
                .request(
                    method::VOICE_SHOW,
                    serde_json::to_value(VoiceShowParams::default())?,
                )
                .await?;
            output(json, v, |r: VoiceShowResult| {
                for l in show_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        VoiceCmd::Audition { voices, text, out } => {
            theseus_client::client::refuse_in_a_job(
                method::VOICE_AUDITION,
                std::env::var("THESEUS_SESSION")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .as_deref(),
            )?;
            let v = conn
                .request(
                    method::VOICE_AUDITION,
                    serde_json::to_value(VoiceAuditionParams {
                        voices,
                        text,
                        session_id: None,
                    })?,
                )
                .await?;
            let dir = out.unwrap_or_else(|| PathBuf::from("voice-audition"));
            let r: VoiceAuditionResult = serde_json::from_value(v)?;
            let paths = write_clips(&r, &dir)?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({"text": r.text, "files": paths, "failed": r.failed,
                        "estimate_micros": r.estimate_micros})
                );
                return Ok(());
            }
            for l in audition_lines(&r, &paths) {
                println!("{l}");
            }
            Ok(())
        }
    }
}

/// `voice`'s lines.
pub fn show_lines(r: &VoiceShowResult) -> Vec<String> {
    let who = match &r.persona {
        Some(k) => format!("{} ({k})", r.name),
        None => format!("{} (the built-in persona)", r.name),
    };
    let mut out = vec![
        format!("voice: {} (from {})", r.voice, r.from),
        format!("persona in play: {who}"),
        format!("[voice] tts_voice: {} (the fallback)", r.global),
        match &r.greeting {
            Some(g) => format!("greeting: {g:?}"),
            None => "greeting: none".into(),
        },
        format!(
            "voice is {}; key {} {}",
            if r.enabled {
                "on"
            } else {
                "off ([voice] enabled)"
            },
            r.key_secret,
            if r.key_ready {
                "resolved"
            } else {
                "not resolved"
            },
        ),
    ];
    out.push(
        "pick by ear: `theseus persona voice audition`, then `theseus persona edit --voice ID`"
            .into(),
    );
    out
}

/// Write each clip as `<dir>/<n>-<voice>.wav` (n from 1, in the order asked),
/// and return the paths.
pub fn write_clips(r: &VoiceAuditionResult, dir: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    let mut paths = Vec::new();
    for (i, clip) in r.clips.iter().enumerate() {
        let wav = base64::engine::general_purpose::STANDARD
            .decode(&clip.wav)
            .with_context(|| format!("{}'s clip is not base64", clip.voice))?;
        if !wav.starts_with(b"RIFF") {
            bail!("{}'s clip is not a WAV file", clip.voice);
        }
        let path = dir.join(format!("{}-{}.wav", i + 1, clip.voice));
        std::fs::write(&path, wav).with_context(|| format!("writing {}", path.display()))?;
        paths.push(path);
    }
    Ok(paths)
}

/// `voice audition`'s lines.
pub fn audition_lines(r: &VoiceAuditionResult, paths: &[PathBuf]) -> Vec<String> {
    let mut out = vec![format!("said: {:?}", r.text)];
    for (clip, path) in r.clips.iter().zip(paths) {
        out.push(format!("{}  {}", clip.voice, path.display()));
    }
    for f in &r.failed {
        out.push(format!("{}  failed: {}", f.voice, f.error));
    }
    out.push(format!(
        "estimated cost: {}",
        usd(r.estimate_micros as f64 / 1_000_000.0)
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::voice_pick::{AuditionClip, AuditionFailure};

    fn clip(voice: &str) -> AuditionClip {
        let mut wav = b"RIFF\x24\0\0\0WAVE".to_vec();
        wav.extend_from_slice(&[0u8; 8]);
        AuditionClip {
            voice: voice.into(),
            wav: base64::engine::general_purpose::STANDARD.encode(wav),
            cost_micros: Some(270),
        }
    }

    fn result() -> VoiceAuditionResult {
        VoiceAuditionResult {
            text: "Sage here. Go ahead.".into(),
            clips: vec![clip("aura-2-orion-en"), clip("aura-2-luna-en")],
            failed: vec![AuditionFailure {
                voice: "aura-2-zeus-en".into(),
                error: "Deepgram answered 404: no such model".into(),
            }],
            estimate_micros: 540,
        }
    }

    /// One WAV a voice, numbered in the order asked, in a directory made if missing; the lines
    /// give each path, a failed voice and why, and the estimate.
    #[test]
    fn the_clips_are_files_named_by_order_and_voice() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("clips");
        let r = result();
        let paths = write_clips(&r, &dir).unwrap();
        let names: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["1-aura-2-orion-en.wav", "2-aura-2-luna-en.wav"]);
        assert!(std::fs::read(&paths[0]).unwrap().starts_with(b"RIFF"));
        let lines = audition_lines(&r, &paths);
        assert_eq!(lines[0], "said: \"Sage here. Go ahead.\"");
        assert!(
            lines[1].starts_with("aura-2-orion-en  ")
                && lines[1].ends_with("1-aura-2-orion-en.wav")
        );
        assert_eq!(
            lines[3],
            "aura-2-zeus-en  failed: Deepgram answered 404: no such model"
        );
        assert_eq!(lines[4], "estimated cost: $0.0005");
    }

    /// A clip that is not a WAV is refused, and nothing is written for it.
    #[test]
    fn a_clip_that_is_not_a_wav_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let mut r = result();
        r.clips[0].wav = base64::engine::general_purpose::STANDARD.encode(b"not audio");
        let e = write_clips(&r, d.path()).unwrap_err().to_string();
        assert!(e.contains("not a WAV"), "{e}");
    }

    /// `voice` says the voice, its source, the greeting and whether the key resolved; it names
    /// the key's entry and never a value.
    #[test]
    fn show_says_the_voice_and_where_it_comes_from() {
        let r = VoiceShowResult {
            enabled: true,
            persona: Some("sage".into()),
            name: "Sage".into(),
            voice: "aura-2-orion-en".into(),
            from: "persona".into(),
            global: "aura-2-andromeda-en".into(),
            greeting: Some("Sage here. Go ahead.".into()),
            key_secret: "deepgram_api_key".into(),
            key_ready: true,
        };
        let lines = show_lines(&r);
        assert_eq!(lines[0], "voice: aura-2-orion-en (from persona)");
        assert_eq!(lines[1], "persona in play: Sage (sage)");
        assert!(lines
            .iter()
            .any(|l| l == "greeting: \"Sage here. Go ahead.\""));
        assert!(lines
            .iter()
            .any(|l| l == "voice is on; key deepgram_api_key resolved"));
    }

    /// The audition spends the owner's money: a job's shell is refused before anything is sent.
    #[test]
    fn an_audition_is_refused_inside_a_job() {
        let e = theseus_client::client::refuse_in_a_job(
            method::VOICE_AUDITION,
            Some("ses_0000aa1b2c3"),
        )
        .unwrap_err()
        .to_string();
        assert!(
            e.starts_with("theseus persona voice audition refused:"),
            "{e}"
        );
    }
}
