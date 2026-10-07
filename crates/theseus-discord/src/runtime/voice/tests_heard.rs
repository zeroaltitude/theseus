//! What the next voice turn is told (theseus-qb8o): the engine's facts
//! through `pump`, into the call's notes, and onto the next voice turn's
//! input through the place, as a call runs them.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_core::provider::Scripted;
use theseus_voice::{CutWhy, Event, HeardAs, Over, Spoken, TurnId, Utterance};
use tokio::sync::mpsc;
use twilight_model::id::Id;

use super::super::tests::core_scripted;
use super::super::{Place, PlaceMsg};
use super::tests::{heard, joined, place, LOUNGE, OWNER};
use super::{pump, VoicePlace, FAILED_TURN, FRAMING};

/// A call in the lounge on a session of a core whose model answers
/// `replies` in turn: the place, its mailbox, the engine's events into
/// `pump` (running), and the engine's commands.
struct Lounge {
    sid: String,
    place: Place,
    rx: mpsc::UnboundedReceiver<PlaceMsg>,
    events: mpsc::UnboundedSender<Event>,
    commands: mpsc::UnboundedReceiver<theseus_voice::Command>,
}

fn lounge(dir: &std::path::Path, replies: &[&str]) -> Lounge {
    lounge_scripted(dir, replies.iter().map(|r| Scripted::text(r)).collect())
}

fn lounge_scripted(dir: &std::path::Path, script: Vec<Scripted>) -> Lounge {
    let core = core_scripted(dir, script);
    let rec = theseus_core::session::SessionRecord::new(
        theseus_protocol::SessionKind::Conversation,
        None,
    );
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    let key = format!("channel:{LOUNGE}");
    core.outbox.bind_place(&key, &sid).unwrap();
    let (mut p, rx) = place(&core, &sid);
    p.key = key.clone();
    p.target = format!("discord:{key}");
    p.channel = Some(Id::new(LOUNGE));
    p.shared
        .voice
        .names
        .lock()
        .unwrap()
        .insert(OWNER, "zeroaltitude".into());
    p.shared
        .routes
        .lock()
        .unwrap()
        .by_channel
        .insert(LOUNGE, p.tx.clone());
    let (commands, engine) = mpsc::unbounded_channel();
    joined(&p, commands);
    let (events, events_rx) = mpsc::unbounded_channel();
    let at = VoicePlace {
        key,
        label: "#lounge".into(),
        users: vec![OWNER],
        guild: 100_000_000_000_000_001,
    };
    tokio::spawn(pump(Arc::clone(&p.shared), 1, at, events_rx));
    Lounge {
        sid,
        place: p,
        rx,
        events,
        commands: engine,
    }
}

impl Lounge {
    /// `events`, then a voice turn of `utterances` as the engine sends it:
    /// through `pump` to the place, which submits it. The input the session
    /// got.
    async fn turn(&mut self, id: u64, events: Vec<Event>, utterances: Vec<Utterance>) -> String {
        for e in events {
            self.events.send(e).unwrap();
        }
        self.events
            .send(Event::Turn {
                id: TurnId(id),
                utterances,
            })
            .unwrap();
        let rx = &mut self.rx;
        let wait = Duration::from_secs(10);
        let t = tokio::time::timeout(wait, async {
            loop {
                if let Some(PlaceMsg::Voice(t)) = rx.recv().await {
                    break t;
                }
            }
        })
        .await
        .expect("the turn came through pump");
        self.place.voice_turn(t);
        let rx = &mut self.rx;
        let r = tokio::time::timeout(wait, async {
            loop {
                if let Some(PlaceMsg::SubmitDone(r)) = rx.recv().await {
                    break r;
                }
            }
        })
        .await
        .unwrap();
        r.unwrap();
        self.place.inflight = false;
        self.inputs().await.pop().expect("an input")
    }

    /// The session's voice inputs, in order.
    async fn inputs(&self) -> Vec<String> {
        let history: theseus_protocol::SessionHistoryResult = self
            .place
            .shared
            .rpc
            .call(
                theseus_protocol::method::SESSION_HISTORY,
                json!({"session_id": self.sid}),
            )
            .await
            .unwrap();
        history
            .nodes
            .into_iter()
            .filter(|n| n.author.as_deref() == Some("discord:zeroaltitude"))
            .map(|n| n.text)
            .collect()
    }
}

/// A `Cut` of reply `turn`.
fn cut(turn: u64, why: CutWhy, sentences: usize, heard: usize, into_ms: u64) -> Event {
    Event::Cut {
        what: Spoken::Reply(TurnId(turn)),
        why,
        sentences,
        heard,
        into: Duration::from_millis(into_ms),
        last_heard: (heard > 0).then(|| "The second row is Tuesday's.".to_string()),
        cut: "The third row is Wednesday's, at four dollars.".into(),
    }
}

/// The bracketed line of `input`, when it has one.
fn line_of(input: &str) -> Option<&str> {
    input.lines().find(|l| l.starts_with("[Voice: "))
}

/// A reply cut by words is named on the next voice turn's input, with what
/// was heard of it and what was cut, and on that turn only: the notes are
/// drained.
#[tokio::test]
async fn a_cut_reply_is_named_on_the_next_voice_turn_and_not_after() {
    let d = tempfile::tempdir().unwrap();
    let mut l = lounge(d.path(), &["Here is the table.", "Sure.", "Okay."]);
    let first = l
        .turn(0, vec![], vec![heard("What did we spend last week?")])
        .await;
    assert!(line_of(&first).is_none(), "{first}");
    let second = l
        .turn(
            1,
            vec![cut(0, CutWhy::Words, 9, 2, 700)],
            vec![heard("Sorry to interrupt you, what about today?")],
        )
        .await;
    assert_eq!(
        line_of(&second),
        Some(
            "[Voice: they cut in on your reply to \"What did we spend last week?\": they heard \
             \"The second row is Tuesday's.\" (2 of 9 sentences); you were saying \"The third row \
             is Wednesday's, at four dollars.\" when they spoke, and the rest was not said]"
        ),
        "{second}"
    );
    assert!(second.ends_with("🎙️ Sorry to interrupt you, what about today?"));
    let third = l.turn(2, vec![], vec![heard("Thanks.")]).await;
    assert!(line_of(&third).is_none(), "drained: {third}");
}

/// A reply that never began (queued behind one cut) is named as never said
/// aloud; one superseded as not said; a cut at the call's end needs none.
#[tokio::test]
async fn a_reply_never_said_and_a_superseded_reply_are_named() {
    let d = tempfile::tempdir().unwrap();
    let mut l = lounge(d.path(), &["One.", "Two.", "Three."]);
    l.turn(0, vec![], vec![heard("Show me the table.")]).await;
    let input = l
        .turn(
            1,
            vec![cut(0, CutWhy::Words, 7, 0, 0)],
            vec![heard("Pizza.")],
        )
        .await;
    assert_eq!(
        line_of(&input),
        Some("[Voice: your reply to \"Show me the table.\" (7 sentences) was never said aloud]"),
        "{input}"
    );
    let input = l
        .turn(
            2,
            vec![
                cut(1, CutWhy::Superseded, 3, 0, 0),
                cut(1, CutWhy::CallEnded, 3, 0, 0),
            ],
            vec![heard("and also the totals")],
        )
        .await;
    assert_eq!(
        line_of(&input),
        Some("[Voice: your reply to \"Pizza.\" was not said: they kept talking before it began]"),
        "{input}"
    );
}

/// A backchannel or a "go on" said while Theseus spoke is no turn, so the
/// next turn is told; an utterance said while a reply was being prepared is
/// tagged with that reply.
#[tokio::test]
async fn a_backchannel_and_words_before_a_reply_are_named() {
    let d = tempfile::tempdir().unwrap();
    let mut l = lounge(d.path(), &["One.", "Two."]);
    l.turn(0, vec![], vec![heard("What changed today?")]).await;
    let mut yeah = heard("Yeah.");
    yeah.heard_as = HeardAs::Backchannel;
    let mut wordless = heard("");
    wordless.heard_as = HeardAs::Wordless;
    let mut early = heard("No.");
    early.over = Some(Over::Preparing { turn: TurnId(0) });
    let input = l
        .turn(
            1,
            vec![Event::Utterance(yeah), Event::Utterance(wordless)],
            vec![early],
        )
        .await;
    assert_eq!(
        line_of(&input),
        Some(
            "[Voice: while you spoke they said \"Yeah.\"; they said this before your reply to \
             \"What changed today?\" had been spoken]"
        ),
        "{input}"
    );
}

/// The line is bounded: each quote clipped on a word with an ellipsis, at
/// most the 3 newest notes, and under 400 characters.
#[tokio::test]
async fn the_line_is_bounded() {
    let d = tempfile::tempdir().unwrap();
    let mut l = lounge(d.path(), &["One.", "Two."]);
    let long = "word ".repeat(60);
    l.turn(0, vec![], vec![heard(&long)]).await;
    let mut events: Vec<Event> = (1..=5)
        .map(|i| {
            let mut u = heard(&format!("yeah number {i} {long}"));
            u.heard_as = HeardAs::Backchannel;
            Event::Utterance(u)
        })
        .collect();
    events.push(Event::Cut {
        what: Spoken::Reply(TurnId(0)),
        why: CutWhy::Words,
        sentences: 9,
        heard: 3,
        into: Duration::from_millis(400),
        last_heard: Some(long.clone()),
        cut: long.clone(),
    });
    let input = l.turn(1, events, vec![heard("Stop.")]).await;
    let line = line_of(&input).expect("a line");
    assert!(
        line.chars().count() < 400,
        "{}: {line}",
        line.chars().count()
    );
    assert!(line.ends_with(']'), "{line}");
    // The newest note, the cut, always; the first words of the reply about
    // 40 characters, and each quote clipped on a word with an ellipsis.
    assert!(
        line.contains("they cut in on your reply to \"word word word word word word word word…\""),
        "{line}"
    );
    assert!(
        !line.contains("number 1") && !line.contains("number 2"),
        "{line}"
    );
    assert!(line.matches('…').count() >= 3, "{line}");
    let notes = line.matches("while you spoke").count() + line.matches("they cut in").count();
    assert!(notes <= 3, "{line}");
}

impl Lounge {
    /// The ledger's rows of `kind`, as (session, data), oldest first.
    async fn rows(&self, kind: &str) -> Vec<(Option<String>, serde_json::Value)> {
        let tail: theseus_protocol::LedgerTailResult = self
            .place
            .shared
            .rpc
            .call(
                theseus_protocol::method::LEDGER_TAIL,
                theseus_protocol::LedgerTailParams {
                    n: Some(200),
                    kind: Some(kind.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        tail.rows
            .into_iter()
            .map(|r| (r.session_id, r.data))
            .collect()
    }
}

/// A cut and a resumed stop are rows on the place's session, with their
/// fields; a transcription's row says what it was heard as and over; and
/// health counts the resumes.
#[tokio::test]
async fn the_cut_and_resumed_rows_carry_their_session_and_fields() {
    let d = tempfile::tempdir().unwrap();
    let mut l = lounge(d.path(), &["One.", "Two."]);
    l.turn(0, vec![], vec![heard("What changed?")]).await;
    let mut yeah = heard("Yeah.");
    yeah.heard_as = HeardAs::Backchannel;
    yeah.over = Some(Over::Saying {
        what: Spoken::Reply(TurnId(0)),
        sentence: 2,
        text: "The third row is Wednesday's.".into(),
    });
    let resumed = Event::Resumed {
        what: Spoken::Reply(TurnId(0)),
        why: HeardAs::Backchannel,
        held: Duration::from_millis(640),
    };
    let mut early = heard("Wait.");
    early.over = Some(Over::Preparing { turn: TurnId(1) });
    let events = vec![
        Event::Utterance(yeah),
        resumed,
        cut(0, CutWhy::Words, 9, 2, 700),
        Event::Utterance(early.clone()),
    ];
    l.turn(1, events, vec![early]).await;
    let sid = Some(l.sid.clone());
    let cuts = l.rows("voice.cut").await;
    assert_eq!(cuts.len(), 1, "{cuts:?}");
    assert_eq!(cuts[0].0, sid);
    assert_eq!(
        cuts[0].1,
        json!({"what": "reply 0", "why": "words", "sentences": 9, "heard": 2, "into_ms": 700})
    );
    let resumes = l.rows("voice.resumed").await;
    assert_eq!(resumes.len(), 1, "{resumes:?}");
    assert_eq!(resumes[0].0, sid);
    assert_eq!(
        resumes[0].1,
        json!({"what": "reply 0", "why": "backchannel", "held_ms": 640})
    );
    // The transcriptions' rows are booked off the runtime's workers.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let heard_rows = loop {
        let rows = l.rows("speech.transcribed").await;
        if rows.len() >= 2 || std::time::Instant::now() >= deadline {
            break rows;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(heard_rows.len(), 2, "{heard_rows:?}");
    assert!(heard_rows.iter().all(|(s, _)| *s == sid));
    let mut seen: Vec<(String, serde_json::Value)> = heard_rows
        .iter()
        .map(|(_, d)| {
            (
                d["heard_as"].as_str().unwrap().to_string(),
                d["over"].clone(),
            )
        })
        .collect();
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        seen,
        [
            (
                "backchannel".to_string(),
                json!({"saying": "reply 0", "sentence": 2})
            ),
            ("words".to_string(), json!({"preparing": "reply 1"})),
        ]
    );
    let status = l.place.shared.voice.status();
    assert_eq!((status.resumes, status.barge_ins), (1, 0));
    assert!(
        status.line().contains("· 0 barge-in(s) · 1 resumed ·"),
        "{}",
        status.line()
    );
}

/// The framing line leads every voice turn's input, a line of its own,
/// ahead of the heard line when there is one.
#[tokio::test]
async fn the_framing_line_leads_every_voice_turns_input() {
    let d = tempfile::tempdir().unwrap();
    let mut l = lounge(d.path(), &["One.", "Two."]);
    let first = l.turn(0, vec![], vec![heard("What changed?")]).await;
    assert_eq!(first, format!("{FRAMING}\n🎙️ What changed?"));
    let second = l
        .turn(
            1,
            vec![cut(0, CutWhy::Superseded, 2, 0, 0)],
            vec![heard("And then?")],
        )
        .await;
    let lines: Vec<&str> = second.lines().collect();
    assert_eq!(lines[0], FRAMING);
    assert!(lines[1].starts_with("[Voice: your reply to"), "{second}");
    assert_eq!(lines[2], "🎙️ And then?");
    assert!(FRAMING.chars().count() < 300);
}

/// A voice turn that fails is said aloud: the engine gets the one sentence
/// that says so, and the place says why in text.
#[tokio::test]
async fn a_failed_voice_turn_sends_the_constant_sentence() {
    let d = tempfile::tempdir().unwrap();
    // The model refuses the key: the turn fails.
    let refused = theseus_core::provider::ProviderError::Auth {
        status: 401,
        message: "invalid x-api-key".into(),
    };
    let mut l = lounge_scripted(d.path(), vec![Scripted::Fail(refused)]);
    l.events
        .send(Event::Turn {
            id: TurnId(0),
            utterances: vec![heard("What changed today?")],
        })
        .unwrap();
    let rx = &mut l.rx;
    let wait = Duration::from_secs(10);
    let t = tokio::time::timeout(wait, async {
        loop {
            if let Some(PlaceMsg::Voice(t)) = rx.recv().await {
                break t;
            }
        }
    })
    .await
    .unwrap();
    l.place.voice_turn(t);
    let rx = &mut l.rx;
    let r = tokio::time::timeout(wait, async {
        loop {
            if let Some(PlaceMsg::SubmitDone(r)) = rx.recv().await {
                break r;
            }
        }
    })
    .await
    .unwrap();
    assert!(r.is_err(), "the turn failed");
    assert_eq!(
        l.commands.try_recv().unwrap(),
        theseus_voice::Command::Reply {
            turn: TurnId(0),
            text: FAILED_TURN.into()
        }
    );
}
