//! Step 44a's live check (design §3): join a voice channel through songbird,
//! play the stand-in clip, and log the frames received per SSRC.
//!
//! **Live**, on a private test voice channel where the bot may Connect and
//! Speak, and which Eddie's daemon doesn't bind (T1b's disjoint rule). The
//! channel's ids go in a TOML file; the bot's token comes from the
//! environment, never from the file:
//!
//! ```text
//! THESEUS_VOICE_TOKEN=<the bot's token> \
//!   cargo run --release -p theseus-voice --example join -- <join.toml>
//! ```
//!
//! ```toml
//! guild = 123        # the guild's id
//! channel = 456      # the voice channel's id
//! seconds = 60       # stay this long, then leave
//! echo = []          # user ids: run the engine too, listening to these users
//!                    # and answering each turn with the stand-in voice
//! greet = "Hello."   # optional (45a): say this through Deepgram instead of
//!                    # the stand-in clip, with THESEUS_DEEPGRAM_KEY set
//! ```
//!
//! `THESEUS_VOICE_LOG` (a tracing filter, such as `songbird=debug`) logs
//! songbird's own lines to stderr: the voice gateway, and DAVE's handshake.
//!
//! **Dry**, with no token and no network (`-- --dry`): builds songbird's
//! manager (no task spawned), then a driver (songbird's tasks start), decodes
//! the stand-in clip through songbird's codec registry, and hands it to the
//! unconnected driver as a track.
//!
//! Built without the `voice` feature, it connects the gateway alone: the
//! baseline for the size of what voice adds.

use std::time::Duration;

use serde::Deserialize;
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};

type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Join {
    guild: u64,
    channel: u64,
    #[serde(default = "a_minute")]
    seconds: u64,
    #[serde(default)]
    echo: Vec<u64>,
    #[serde(default)]
    greet: Option<String>,
}

fn a_minute() -> u64 {
    60
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    if let Ok(filter) = std::env::var("THESEUS_VOICE_LOG") {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
            .with_writer(std::io::stderr)
            .init();
    }
    let arg = std::env::args().nth(1);
    let result = match arg.as_deref() {
        Some("--dry") => dry().await,
        Some(path) => live(path).await,
        None => Err("usage: join --dry | join <join.toml> (with THESEUS_VOICE_TOKEN set)".into()),
    };
    if let Err(e) = result {
        eprintln!("join: {e}");
        std::process::exit(1);
    }
}

async fn live(path: &str) -> Result<(), Error> {
    let join: Join = toml::from_str(&std::fs::read_to_string(path)?)?;
    let token = std::env::var("THESEUS_VOICE_TOKEN")
        .map_err(|_| "THESEUS_VOICE_TOKEN is not set: the bot's token")?;
    let http = twilight_http::Client::new(token.clone());
    let me = http.current_user().await?.model().await?.id;
    let intents = Intents::GUILDS;
    #[cfg(feature = "voice")]
    let intents = intents | Intents::GUILD_VOICE_STATES;
    let mut shard = Shard::new(ShardId::ONE, token, intents);
    #[cfg(feature = "voice")]
    let voice = voice::Voice::new(&shard, me);
    #[cfg(feature = "voice")]
    let feeder = voice.clone();
    let (ready_tx, ready) = tokio::sync::oneshot::channel();
    let mut ready_tx = Some(ready_tx);
    tokio::spawn(async move {
        while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
            let event = match item {
                Ok(event) => event,
                Err(e) => {
                    eprintln!("gateway: {e}");
                    continue;
                }
            };
            if let Event::Ready(r) = &event {
                eprintln!("gateway: ready as {}", r.user.name);
                if let Some(tx) = ready_tx.take() {
                    let _ = tx.send(());
                }
            }
            #[cfg(feature = "voice")]
            feeder.feed(event);
        }
    });
    ready.await?;
    #[cfg(feature = "voice")]
    voice.join(join).await?;
    #[cfg(not(feature = "voice"))]
    {
        eprintln!(
            "built without the voice feature: the gateway alone, as {me}, for {} s \
             (not joining channel {} in guild {}, nor listening to {:?})",
            join.seconds, join.channel, join.guild, join.echo
        );
        tokio::time::sleep(Duration::from_secs(join.seconds)).await;
    }
    Ok(())
}

#[cfg(not(feature = "voice"))]
fn dry() -> std::future::Ready<Result<(), Error>> {
    std::future::ready(Err(
        "built without the voice feature: nothing to build".into()
    ))
}

#[cfg(feature = "voice")]
async fn dry() -> Result<(), Error> {
    use std::collections::HashMap;
    use std::num::NonZeroU64;
    use std::sync::Arc;

    use theseus_voice::songbird::input::codecs::{get_codec_registry, get_probe};
    use theseus_voice::songbird::shards::TwilightMap;
    use theseus_voice::songbird::Driver;
    use theseus_voice::{clip_input, manager, songbird_config, Audio};

    let metrics = tokio::runtime::Handle::current().metrics();
    let tasks = metrics.num_alive_tasks();
    let user = NonZeroU64::new(1).ok_or("a user id")?;
    let songbird = manager(Arc::new(TwilightMap::new(HashMap::new())), user);
    println!(
        "songbird's manager: built, {} new tasks",
        metrics.num_alive_tasks() - tasks
    );
    let mut driver = Driver::new(songbird_config());
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!(
        "songbird's driver: built, {} new tasks 100 ms on (a join builds one)",
        metrics.num_alive_tasks() - tasks
    );
    let chime = Audio::chime();
    let mut input = clip_input(&chime)
        .make_playable_async(get_codec_registry(), get_probe())
        .await?;
    let parsed = input.parsed_mut().ok_or("the clip wasn't parsed")?;
    let mut decoded = 0;
    while let Ok(packet) = parsed.format.next_packet() {
        decoded += parsed.decoder.decode(&packet)?.frames();
    }
    println!(
        "the stand-in clip: {decoded} of its {} samples decoded through songbird's codec registry",
        chime.len()
    );
    // An unconnected driver may never answer, so the question is bounded.
    let track = driver.play_input(clip_input(&chime));
    match tokio::time::timeout(Duration::from_secs(2), track.get_info()).await {
        Ok(Ok(state)) => println!(
            "a track in the unconnected driver: {:?}, {:?}",
            state.playing, state.ready
        ),
        Ok(Err(e)) => println!("a track in the unconnected driver: {e}"),
        Err(_) => println!("a track in the unconnected driver: added; no state reported in 2 s"),
    }
    driver.stop();
    drop(driver);
    drop(songbird);
    println!("dry run: ok");
    Ok(())
}

#[cfg(feature = "voice")]
mod voice {
    use std::collections::{BTreeMap, HashMap};
    use std::num::NonZeroU64;
    use std::sync::Arc;
    use std::time::Duration;

    use theseus_voice::songbird::shards::TwilightMap;
    use theseus_voice::songbird::Songbird;
    use theseus_voice::{
        manager, Audio, ClipId, Command, Config, DeepgramSettings, DeepgramSpeech, Engine, Event,
        Heard, SongbirdIo, Speaker, Speech, StandInSpeech, VoiceIo,
    };
    use twilight_gateway::Shard;
    use twilight_model::id::{marker::UserMarker, Id};

    use super::{Error, Join};

    /// songbird's manager: fed by the gateway task, joining from the main one.
    #[derive(Clone)]
    pub struct Voice(Arc<Songbird>);

    impl Voice {
        pub fn new(shard: &Shard, me: Id<UserMarker>) -> Self {
            let senders = HashMap::from([(shard.id().number(), shard.sender())]);
            Self(Arc::new(manager(Arc::new(TwilightMap::new(senders)), me)))
        }

        /// songbird must see voice state and server updates on a task apart
        /// from `join`, which waits for them.
        pub fn feed(&self, event: twilight_gateway::Event) {
            let songbird = Arc::clone(&self.0);
            tokio::spawn(async move { songbird.process(&event).await });
        }

        pub async fn join(&self, join: Join) -> Result<(), Error> {
            let guild = NonZeroU64::new(join.guild).ok_or("guild must be nonzero")?;
            let channel = NonZeroU64::new(join.channel).ok_or("channel must be nonzero")?;
            // The greeting is made before the join, so the call plays it at once.
            let clip = match &join.greet {
                Some(text) => greeting(text).await?,
                None => Audio::chime(),
            };
            let t = std::time::Instant::now();
            let call = self.0.join(guild, channel).await?;
            eprintln!(
                "joined channel {channel} in guild {guild} in {} ms",
                t.elapsed().as_millis()
            );
            let mut io = SongbirdIo::attach(call).await;
            let stay = Duration::from_secs(join.seconds);
            if join.echo.is_empty() {
                listen(&mut io, stay, clip).await;
            } else {
                echo(io, &join.echo, stay).await;
            }
            self.0.leave(guild).await?;
            eprintln!("left");
            Ok(())
        }
    }

    /// `text` through Deepgram (45a), the key from the environment.
    async fn greeting(text: &str) -> Result<Audio, Error> {
        let key = std::env::var("THESEUS_DEEPGRAM_KEY")
            .map_err(|_| "THESEUS_DEEPGRAM_KEY is not set: the Deepgram key")?;
        let speech = DeepgramSpeech::new(&key, DeepgramSettings::default())?;
        drop(key);
        let t = std::time::Instant::now();
        let said = speech.synthesize(text).await?;
        eprintln!(
            "greeting: {:?} synthesized by Deepgram ({}) in {} ms: {:.2} s of audio, {} chars",
            text,
            said.usage.model,
            t.elapsed().as_millis(),
            said.audio.duration().as_secs_f64(),
            said.usage.chars
        );
        Ok(said.audio)
    }

    /// Play `clip`, and log frames per SSRC every 2 s.
    async fn listen(io: &mut SongbirdIo, stay: Duration, clip: Audio) {
        let length = clip.duration();
        let started = tokio::time::Instant::now();
        io.play(ClipId(0), clip).await;
        eprintln!("clip 0 playing: {:.2} s", length.as_secs_f64());
        let end = tokio::time::Instant::now() + stay;
        let mut log = tokio::time::interval(Duration::from_secs(2));
        let mut heard: BTreeMap<Speaker, u64> = BTreeMap::new();
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(end) => break,
                _ = log.tick() => {
                    for (ssrc, n) in io.ssrcs() {
                        eprintln!(
                            "ssrc {ssrc} (user {:?}): {} frames, {} decoded",
                            n.user.map(|u| u.0), n.frames, n.decoded
                        );
                    }
                    eprintln!("frames per user: {heard:?}");
                }
                next = io.next() => match next {
                    Some(Heard::Tick(frames)) => {
                        for f in frames {
                            *heard.entry(f.speaker).or_default() += 1;
                        }
                    }
                    Some(Heard::Ended(id)) => eprintln!(
                        "clip {} ended, {:.2} s after it began",
                        id.0,
                        started.elapsed().as_secs_f64()
                    ),
                    None => {
                        eprintln!("the connection dropped");
                        break;
                    }
                },
            }
        }
    }

    /// The engine on the call, listening to `users`: each turn is answered
    /// with what the stand-in heard, in the stand-in voice.
    async fn echo(io: SongbirdIo, users: &[u64], stay: Duration) {
        let config = Config::new(users.iter().map(|&u| Speaker(u)));
        let (engine, mut handle) =
            Engine::new(config, Box::new(io), Arc::new(StandInSpeech::new()));
        let run = tokio::spawn(engine.run());
        let end = tokio::time::Instant::now() + stay;
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(end) => break,
                event = handle.events.recv() => match event {
                    Some(Event::Turn { id, utterances }) => {
                        let heard: Vec<_> = utterances.iter().map(|u| u.text.as_str()).collect();
                        let text = format!("You said {}. That's all.", heard.join(", then "));
                        let _ = handle.commands.send(Command::Reply { turn: id, text });
                    }
                    Some(event) => eprintln!("{event:?}"),
                    None => break,
                },
            }
        }
        let _ = handle.commands.send(Command::Leave);
        let _ = run.await;
    }
}
