//! A short or unclear transcript asked of a judge (theseus-s3dwj): through
//! the seam, from WAV fixtures, in virtual time, each at its exact times.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use theseus_voice::{
    write_wav, Audio, Began, Command, Config, Engine, Event, HeardAs, Judge, Said, Speaker,
    StandInJudge, StandInSpeech, TurnId, Utterance, WavIo,
};
use tokio::time::{sleep, Instant};

const OWNER: Speaker = Speaker(101);

/// The reply's two sentences, the second a question the owner may answer.
const REPLY: &str = "Here is the plan. Should I deploy it now?";

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn len(text: &str) -> Duration {
    StandInSpeech::tone_for(text).duration()
}

/// What the engine said in the journal, as tracing's fmt layer wrote it.
#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<u8>>>);

impl Write for Journal {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn line(dir: &Path, io: &mut WavIo, n: usize, at: u64, length: u64) {
    let path = dir.join(format!("line-{n}.wav"));
    write_wav(&path, &Audio::tone(220.0, ms(length), 0.3)).expect("write the fixture");
    io.say_wav(OWNER, ms(at), &path).expect("the fixture");
}

/// The owner asks ("what's the plan?", 0 to 0.5 s, a turn at 1.2 s) and
/// Theseus answers with [`REPLY`]; the second sentence ends `after` before
/// the owner says `second` for 400 ms. Returns every event with its time,
/// the journal, and the judge's cases.
async fn call(
    judge: Option<Arc<StandInJudge>>,
    second: &str,
    confidence: Option<f32>,
    after_end: i64,
) -> (Vec<(Duration, Event)>, String, Vec<theseus_voice::Case>) {
    let end = ms(1200) + len("Here is the plan.") + len("Should I deploy it now?");
    let at = (end.as_millis() as i64 + after_end) as u64;
    let dir = tempfile::tempdir().unwrap();
    let mut io = WavIo::new(ms(at + 3000));
    line(dir.path(), &mut io, 1, 0, 500);
    line(dir.path(), &mut io, 2, at, 400);
    let journal = Journal::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer({
            let journal = journal.clone();
            move || journal.clone()
        })
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let mut config = Config::new([OWNER]);
    config.acknowledge_after = Duration::from_secs(60);
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "what's the plan?")
            .transcript_with(OWNER, second, confidence),
    );
    let origin = Instant::now();
    let (engine, handle) = Engine::new(config, Box::new(io), speech);
    let engine = match &judge {
        Some(j) => engine.with_judge(Arc::clone(j) as Arc<dyn Judge>),
        None => engine,
    };
    let engine = tokio::spawn(engine.run());
    let mut events = handle.events;
    let mut seen = Vec::new();
    while let Some(event) = events.recv().await {
        if let Event::Turn { id, .. } = &event {
            let (turn, commands) = (*id, handle.commands.clone());
            tokio::spawn(async move {
                sleep(Duration::ZERO).await;
                let text = if turn == TurnId(0) { REPLY } else { "" };
                let _ = commands.send(Command::Reply {
                    turn,
                    text: text.into(),
                });
            });
        }
        seen.push((origin.elapsed(), event));
    }
    engine.await.unwrap();
    let journal = String::from_utf8(journal.0.lock().unwrap().clone()).unwrap();
    let cases = judge.map(|j| j.cases()).unwrap_or_default();
    (seen, journal, cases)
}

fn turns(seen: &[(Duration, Event)]) -> Vec<(Duration, String)> {
    seen.iter()
        .filter_map(|(at, e)| match e {
            Event::Turn { utterances, .. } => Some((*at, utterances[0].text.clone())),
            _ => None,
        })
        .collect()
}

fn utterance(seen: &[(Duration, Event)], n: usize) -> (Duration, &Utterance) {
    seen.iter()
        .filter_map(|(at, e)| match e {
            Event::Utterance(u) => Some((*at, u)),
            _ => None,
        })
        .nth(n)
        .unwrap()
}

/// When the owner's "Yeah." closes, 0.5 s after the reply's last sentence: 400
/// ms of speech and the 700 ms of silence that ends an utterance.
fn closed() -> Duration {
    let end = ms(1200) + len("Here is the plan.") + len("Should I deploy it now?");
    end + ms(500 + 400 + 700)
}

/// KEY. The rules make a turn of a "Yeah." in the echo tail; the judge, at
/// 0.9, says it was Theseus's own echo: no turn, and the journal names the
/// judgment.
#[tokio::test(start_paused = true)]
async fn an_echo_the_judge_is_sure_of_is_no_turn_and_the_journal_names_the_judgment() {
    let judge = Arc::new(StandInJudge::new(Said::Echo, 0.9));
    let (seen, journal, cases) = call(Some(judge), "Yeah.", None, 500).await;
    assert_eq!(turns(&seen), [(ms(1200), "what's the plan?".to_string())]);
    let (at, yeah) = utterance(&seen, 1);
    assert_eq!((at, yeah.heard_as), (closed(), HeardAs::Echo));
    let judged = yeah.judged.as_ref().expect("the judge was asked");
    assert_eq!(
        (judged.verdict.said, judged.verdict.acts),
        (Said::Echo, true)
    );
    assert!(journal.contains("jdg_standin0"), "{journal}");
    assert!(journal.contains("its own echo, no turn"), "{journal}");
    // What it was asked: the words, the sentences it may echo, the timing.
    assert_eq!(cases.len(), 1);
    let c = &cases[0];
    assert_eq!((c.text.as_str(), c.began), ("Yeah.", Began::Tail));
    // The strings the rules match: what ended within the echo tail.
    assert_eq!(c.said, ["Should I deploy it now?"]);
    assert!(c.timing.after_end.is_some_and(|d| d <= ms(600)), "{c:?}");
    assert_eq!((c.timing.after_start, c.echo_prone), (None, false));
}

/// The same utterance, the judge sure it was addressed: a turn, at the
/// rules' time.
#[tokio::test(start_paused = true)]
async fn an_answer_the_judge_is_sure_of_is_a_turn_at_the_rules_time() {
    let judge = Arc::new(StandInJudge::new(Said::Addressed, 0.9));
    let (seen, _, _) = call(Some(judge), "Yeah.", None, 500).await;
    assert_eq!(
        turns(&seen),
        [
            (ms(1200), "what's the plan?".to_string()),
            (closed(), "Yeah.".to_string())
        ]
    );
    assert_eq!(utterance(&seen, 1).1.heard_as, HeardAs::Words);
}

/// Noise is no turn either, and is not an echo.
#[tokio::test(start_paused = true)]
async fn noise_the_judge_is_sure_of_is_no_turn() {
    let judge = Arc::new(StandInJudge::new(Said::Other, 0.85));
    let (seen, _, _) = call(Some(judge), "the", None, 500).await;
    assert_eq!(turns(&seen).len(), 1);
    assert_eq!(utterance(&seen, 1).1.heard_as, HeardAs::Other);
}

/// A judge that answers after 400 ms is cut at 250: the rules decide, 250 ms
/// after they would have.
#[tokio::test(start_paused = true)]
async fn a_judge_too_slow_costs_the_deadline_and_the_rules_decide() {
    let judge = Arc::new(StandInJudge::new(Said::Echo, 0.95).after(ms(400)));
    let (seen, journal, cases) = call(Some(judge), "Yeah.", None, 500).await;
    assert_eq!(cases.len(), 1, "it was asked");
    assert_eq!(
        turns(&seen)[1],
        (closed() + ms(250), "Yeah.".to_string()),
        "the rules' time and at most the deadline"
    );
    let (_, yeah) = utterance(&seen, 1);
    assert_eq!(
        (yeah.heard_as, yeah.judged.is_none()),
        (HeardAs::Words, true)
    );
    assert!(journal.contains("no judgment within 250 ms"), "{journal}");
}

/// No answer at all (a failure) is the rules' verdict at once.
#[tokio::test(start_paused = true)]
async fn a_judge_with_no_answer_leaves_the_rules_verdict_at_their_time() {
    let judge = Arc::new(StandInJudge::silent());
    let (seen, _, _) = call(Some(judge), "Yeah.", None, 500).await;
    assert_eq!(turns(&seen)[1].0, closed());
}

/// An answer in the confirm band (0.6) decides nothing: the rules' verdict.
#[tokio::test(start_paused = true)]
async fn an_answer_below_the_act_band_leaves_the_rules_verdict() {
    let judge = Arc::new(StandInJudge::new(Said::Echo, 0.6));
    let (seen, journal, _) = call(Some(judge), "Yeah.", None, 500).await;
    assert_eq!(turns(&seen)[1], (closed(), "Yeah.".to_string()));
    let (_, yeah) = utterance(&seen, 1);
    assert_eq!(yeah.heard_as, HeardAs::Words);
    assert_eq!(yeah.judged.as_ref().map(|j| j.verdict.acts), Some(false));
    assert!(journal.contains("below the act band"), "{journal}");
}

/// Not asked: a clear six-word transcript, a short one far from Theseus's
/// speech, and a long one the recogniser is sure of. No call at all.
#[tokio::test(start_paused = true)]
async fn what_is_clear_or_far_from_the_speech_is_not_asked() {
    let judge = || Arc::new(StandInJudge::new(Said::Echo, 0.95));
    let j = judge();
    let (seen, _, cases) = call(Some(j), "yes please deploy it right now", Some(0.97), 500).await;
    assert!(cases.is_empty(), "{cases:?}");
    assert_eq!(turns(&seen).len(), 2);
    let j = judge();
    let (seen, _, cases) = call(Some(j), "Yeah.", None, 2500).await;
    assert!(cases.is_empty(), "far from the speech: {cases:?}");
    assert_eq!(turns(&seen).len(), 2);
    assert!(utterance(&seen, 1).1.judged.is_none());
}

/// A long transcript the recogniser is unsure of is asked.
#[tokio::test(start_paused = true)]
async fn a_long_transcript_with_low_confidence_is_asked() {
    let j = Arc::new(StandInJudge::new(Said::Echo, 0.95));
    let (seen, _, cases) = call(Some(j), "yes please deploy it right now", Some(0.41), 500).await;
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].confidence, Some(0.41));
    assert_eq!(utterance(&seen, 1).1.heard_as, HeardAs::Echo);
}

/// With no judge (`[voice] judge_unclear = false`) the events and their
/// times are today's: a turn of the "Yeah." at the utterance's close, and no
/// utterance carries a judgment.
#[tokio::test(start_paused = true)]
async fn with_no_judge_the_events_and_times_are_the_rules_alone() {
    let (seen, journal, _) = call(None, "Yeah.", None, 500).await;
    assert_eq!(
        turns(&seen),
        [
            (ms(1200), "what's the plan?".to_string()),
            (closed(), "Yeah.".to_string())
        ]
    );
    assert!(seen.iter().all(|(_, e)| match e {
        Event::Utterance(u) => u.judged.is_none(),
        _ => true,
    }));
    assert!(!journal.contains("the judge"), "{journal}");
    // A judge that is never asked leaves the same events at the same times.
    let idle = Arc::new(StandInJudge::new(Said::Echo, 0.95));
    let (with, _, cases) = call(Some(idle), "Yeah.", None, 2500).await;
    let (without, _, _) = call(None, "Yeah.", None, 2500).await;
    assert!(cases.is_empty());
    assert_eq!(with, without);
}
