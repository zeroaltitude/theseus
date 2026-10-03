//! Deepgram, live (45a, rows 77 and 78), in two parts:
//!
//! 1. **The round trip**: a sentence synthesized, its audio transcribed, the
//!    words compared, each call timed.
//! 2. **The pipeline**: that audio said into a `WavIo` by a listed speaker,
//!    with the engine on the real provider: the utterance and its turn, the
//!    reply spoken a sentence at a time, and the reply's audio transcribed
//!    back, to show that what was played is speech.
//!
//! ```text
//! THESEUS_DEEPGRAM_KEY=… cargo run -p theseus-voice --example deepgram -- OUT_DIR ["sentence"]
//! ```
//!
//! The key comes from the environment, put there by a launcher that reads it
//! from the vault, and nothing here prints it. Each call costs a fraction of
//! a cent. It writes `sentence.wav` and `reply.wav` into `OUT_DIR`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use theseus_voice::{
    write_wav, Audio, Command, Config, DeepgramSettings, DeepgramSpeech, Engine, Event, Speaker,
    Speech, SpeechFuture, Spoken, Synthesis, Transcript, WavIo,
};

type Error = Box<dyn std::error::Error + Send + Sync>;

const SENTENCE: &str =
    "Theseus can hear you now. This is the round trip, from speech to text and back.";

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next().map(PathBuf::from) else {
        eprintln!("usage: deepgram OUT_DIR [\"sentence\"]  (the key in THESEUS_DEEPGRAM_KEY)");
        std::process::exit(1);
    };
    let sentence = args.next().unwrap_or_else(|| SENTENCE.to_string());
    if let Err(e) = run(&out, &sentence).await {
        eprintln!("deepgram: {e}");
        std::process::exit(1);
    }
}

async fn run(out: &Path, sentence: &str) -> Result<(), Error> {
    std::fs::create_dir_all(out)?;
    let key = std::env::var("THESEUS_DEEPGRAM_KEY")
        .map_err(|_| "THESEUS_DEEPGRAM_KEY is not set: the Deepgram key")?;
    let speech = DeepgramSpeech::new(&key, DeepgramSettings::default())?;
    drop(key);
    let heard = round_trip(&speech, out, sentence).await?;
    pipeline(speech, out, heard).await
}

/// Part 1: synthesize, transcribe, compare.
async fn round_trip(speech: &DeepgramSpeech, out: &Path, sentence: &str) -> Result<Audio, Error> {
    println!(
        "sentence: {sentence:?} ({} chars)",
        sentence.chars().count()
    );
    let t = Instant::now();
    let said = speech.synthesize(sentence).await?;
    println!(
        "synthesize: {} ms; {:.2} s of audio; {} chars billed ({}, {})",
        t.elapsed().as_millis(),
        said.audio.duration().as_secs_f64(),
        said.usage.chars,
        said.usage.provider,
        said.usage.model
    );
    write_wav(&out.join("sentence.wav"), &said.audio)?;
    let t = Instant::now();
    let heard = speech.transcribe(Speaker(1), &said.audio).await?;
    println!(
        "transcribe: {} ms; {:.2} s heard ({}); text: {:?}",
        t.elapsed().as_millis(),
        heard.usage.audio.as_secs_f64(),
        heard.usage.model,
        heard.text
    );
    let (same, of) = compare(sentence, &heard.text);
    println!("words: {same} of {of} the same, in order");
    Ok(said.audio)
}

/// Words, lower case, without punctuation.
fn words(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// How many of `said`'s words `heard` has, in order (their longest common
/// subsequence), of how many.
fn compare(said: &str, heard: &str) -> (usize, usize) {
    let (a, b) = (words(said), words(heard));
    let mut row = vec![0usize; b.len() + 1];
    for x in &a {
        let mut prev = 0;
        for (j, y) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = if x == y {
                prev + 1
            } else {
                row[j + 1].max(row[j])
            };
            prev = here;
        }
    }
    (row[b.len()], a.len())
}

/// The provider, keeping what it synthesized, to transcribe it back.
struct Recording {
    inner: DeepgramSpeech,
    said: Mutex<Vec<Audio>>,
}

impl Speech for Recording {
    fn transcribe<'a>(
        &'a self,
        speaker: Speaker,
        audio: &'a Audio,
    ) -> SpeechFuture<'a, Transcript> {
        self.inner.transcribe(speaker, audio)
    }

    fn synthesize<'a>(&'a self, text: &'a str) -> SpeechFuture<'a, Synthesis> {
        Box::pin(async move {
            let s = self.inner.synthesize(text).await?;
            self.said
                .lock()
                .expect("the recording's lock")
                .push(s.audio.clone());
            Ok(s)
        })
    }
}

/// Part 2: the audio said into a `WavIo` by a listed speaker, and the engine
/// on the real provider.
async fn pipeline(speech: DeepgramSpeech, out: &Path, audio: Audio) -> Result<(), Error> {
    let speaker = Speaker(1);
    let mut io = WavIo::new(audio.duration() + Duration::from_secs(25));
    io.say(speaker, Duration::from_millis(500), audio);
    let played = io.played();
    let recording = Arc::new(Recording {
        inner: speech,
        said: Mutex::new(Vec::new()),
    });
    let (engine, mut handle) = Engine::new(
        Config::new([speaker]),
        Box::new(io),
        Arc::clone(&recording) as Arc<dyn Speech>,
    );
    let run = tokio::spawn(engine.run());
    let t0 = Instant::now();
    let at = || format!("{:>6.2} s", t0.elapsed().as_secs_f64());
    let deadline = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(deadline);
    let mut spoke = false;
    loop {
        let event = tokio::select! {
            () = &mut deadline => break,
            e = handle.events.recv() => match e { Some(e) => e, None => break },
        };
        match event {
            Event::Utterance(u) => println!(
                "{} utterance by {}: {:.2} s, transcribed in {} ms: {:?}",
                at(),
                u.speaker,
                u.length.as_secs_f64(),
                u.latency.as_millis(),
                u.text
            ),
            Event::Turn { id, utterances } => {
                let heard: Vec<&str> = utterances.iter().map(|u| u.text.as_str()).collect();
                let text = format!("I heard you say: {} That was the whole round trip.", heard.join(" "));
                println!("{} turn {}: {} utterance(s); replying {text:?}", at(), id.0, utterances.len());
                let _ = handle.commands.send(Command::Reply { turn: id, text });
            }
            Event::Synthesized { what, usage, latency } => println!(
                "{} synthesized ({what:?}): {} chars, {:.2} s of audio, in {} ms",
                at(),
                usage.chars,
                usage.audio.as_secs_f64(),
                latency.as_millis()
            ),
            Event::Speaking { what, sentences, first_audio } => println!(
                "{} speaking ({what:?}): {sentences} sentence(s), first audio {} ms after the reply",
                at(),
                first_audio.as_millis()
            ),
            Event::Spoke { what } => {
                println!("{} spoke ({what:?})", at());
                if matches!(what, Spoken::Reply(_)) {
                    spoke = true;
                    let _ = handle.commands.send(Command::Leave);
                }
            }
            other => println!("{} {other:?}", at()),
        }
    }
    let _ = run.await;
    for p in played.get() {
        println!(
            "played clip {}: {:.2} s, from {:.2} s{}",
            p.id.0,
            p.length.as_secs_f64(),
            p.started.as_secs_f64(),
            if p.stopped { ", stopped" } else { "" }
        );
    }
    let said = Audio::concat(&recording.said.lock().expect("the recording's lock"));
    write_wav(&out.join("reply.wav"), &said)?;
    if said.is_empty() {
        return Err("the reply was never synthesized".into());
    }
    let back = recording.inner.transcribe(Speaker(2), &said).await?;
    println!("the reply's audio, transcribed back: {:?}", back.text);
    match spoke {
        true => {
            println!("pipeline: ok");
            Ok(())
        }
        false => Err("the reply was not spoken within 60 s".into()),
    }
}
