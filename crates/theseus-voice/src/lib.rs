//! Theseus's voice engine (M7 design §2.8, step 44a).
//!
//! - **The seam**, [`VoiceIo`]: frames in per speaker, audio out. songbird
//!   implements it for real ([`SongbirdIo`], with the `voice` feature), and
//!   [`WavIo`] implements it for tests from WAV files.
//! - **The pipeline**, [`Engine`]: an utterance per speaker, ended by 700 ms
//!   of silence; only listed speakers heard; utterances during a turn
//!   coalesced into the next; replies spoken a sentence at a time, with
//!   barge-in; the acknowledgment for a slow turn; reports at the pause.
//! - **The [`Speech`] contract**, transcribe and synthesize with their usage,
//!   and its stand-ins ([`StandInSpeech`]). The real providers are 45a.
//!
//! The binding's wire-in (gateway intents, `[[voice]]` places, `/join`) is
//! 44b. Nothing here runs before a join: the engine and songbird's manager
//! spawn nothing when they're built (FAST).

pub mod audio;
mod engine;
mod io;
mod sentences;
#[cfg(feature = "voice")]
mod songbird_io;
mod speech;
mod vad;

pub use audio::{read_wav, write_wav, Audio, WavError, FRAME, FRAME_SAMPLES, SAMPLE_RATE};
pub use engine::{
    Command, Config, Engine, EngineHandle, Event, Failure, Spoken, TurnId, Utterance,
};
pub use io::{ClipId, Frame, Heard, PlayLog, Played, Speaker, VoiceIo, WavIo};
pub use sentences::sentences;
pub use speech::{
    Speech, SpeechError, SpeechFuture, StandInSpeech, Synthesis, Transcript, Usage, ACKNOWLEDGMENT,
};

/// songbird, as this crate depends on it, so the binding (44b) builds on the
/// same version and features.
#[cfg(feature = "voice")]
pub use songbird;
#[cfg(feature = "voice")]
pub use songbird_io::{clip_input, manager, songbird_config, SongbirdIo, SsrcCount};
