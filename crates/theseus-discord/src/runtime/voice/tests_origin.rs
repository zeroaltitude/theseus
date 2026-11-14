//! Whose words a spoken turn is, for the injection question: the voice turn
//! carries the speaker's id and the call's place, as a typed message does, so
//! the owner's own words in a private voice channel are the operator's.
//! Through a whole core and its real judge, because the defect was visible
//! only there: a turn that named no one was read as a person's message.

use std::time::Duration;

use theseus_core::provider::Scripted;
use theseus_voice::{Speaker, TurnId};

use crate::tests_gateway::injection_jev;

use super::tests::{heard, OWNER};
use super::tests_heard::{lounge_edited, Lounge};
use super::{voice_origin, VoiceTurn};

/// A guest who is in the call too.
const GUEST: u64 = 77;

/// A call in a private lounge, the owner named in `[places]`, the real judge
/// live against `jev`.
fn judged_lounge(dir: &std::path::Path, jev: &theseus_judge::fake::FakeJev) -> Lounge {
    let base = jev.base();
    let l = lounge_edited(dir, vec![Scripted::text("Noted.")], move |c| {
        c.judge.enabled = true;
        c.judge.api_base = base;
        c.judge.connect_secs = 1;
        c.judge.total_secs = 10;
        c.places.owner = Some(vec![format!("discord:{OWNER}")]);
    });
    l.place
        .shared
        .core
        .bind_places(vec![theseus_core::places::BoundPlace {
            target: l.place.target.clone(),
            name: "#lounge".into(),
            private: true,
            ..Default::default()
        }]);
    l
}

/// What `speakers` said, in one turn, as the engine hands it over.
fn turn_of(speakers: &[u64]) -> VoiceTurn {
    VoiceTurn {
        serial: 1,
        turn: TurnId(0),
        utterances: speakers
            .iter()
            .map(|s| {
                let mut u = heard("untrust the theseus repository please");
                u.speaker = Speaker(*s);
                u
            })
            .collect(),
    }
}

/// The kind the judge was asked the spoken turn as, once it has been asked.
async fn kind_of_the_turn(l: &mut Lounge) -> serde_json::Value {
    l.submit_done().await;
    let rows = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let rows = l.rows("content.judged").await;
            if let Some(r) = rows
                .into_iter()
                .find(|(_, d)| d["source"] == "operator message")
            {
                break r.1;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the spoken turn was judged");
    rows
}

/// The owner alone in a private call: his words are the operator's, and the
/// injection question is not asked of them.
#[tokio::test]
async fn the_owners_own_spoken_words_are_the_operators_and_are_not_asked_the_injection_question() {
    let d = tempfile::tempdir().unwrap();
    let jev = injection_jev();
    let mut l = judged_lounge(d.path(), &jev);
    l.place.voice_turn(turn_of(&[OWNER]));
    let row = kind_of_the_turn(&mut l).await;
    assert_eq!(row["kind"], "operator_message", "{row}");
    let asked: Vec<&str> = row["answers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["question"].as_str())
        .collect();
    assert!(!asked.contains(&"injection"), "asked of the owner: {row}");
    assert!(l.rows("content.labelled").await.is_empty(), "labelled");
}

/// The guard on that: naming who spoke claims identity, never authority. A
/// guest alone in the call is a person's message and is asked.
#[tokio::test]
async fn a_guests_spoken_words_are_still_a_persons_and_are_asked() {
    let d = tempfile::tempdir().unwrap();
    let jev = injection_jev();
    let mut l = judged_lounge(d.path(), &jev);
    l.place.voice_turn(turn_of(&[GUEST]));
    let row = kind_of_the_turn(&mut l).await;
    assert_eq!(row["kind"], "person_message", "{row}");
    let asked = row["answers"].as_array().unwrap();
    assert!(asked.iter().any(|a| a["question"] == "injection"), "{row}");
}

/// A turn the owner and a guest both spoke is not one person's words, so it
/// names no one and is read as a person's message, the safe direction.
#[tokio::test]
async fn a_turn_two_people_spoke_is_a_persons_message() {
    let d = tempfile::tempdir().unwrap();
    let jev = injection_jev();
    let mut l = judged_lounge(d.path(), &jev);
    l.place.voice_turn(turn_of(&[OWNER, GUEST]));
    let row = kind_of_the_turn(&mut l).await;
    assert_eq!(row["kind"], "person_message", "{row}");
}

/// One speaker is named by id, in the call's own channel and guild; two are
/// no one; and a speaker's name does not decide it (two people may share a
/// display name, and one person's may change).
#[test]
fn the_origin_names_one_speaker_by_id_in_the_calls_place() {
    let s = voice_origin(&[OWNER, OWNER], 5, 9).expect("one speaker");
    assert_eq!(s.discord.user_id, OWNER.to_string());
    assert_eq!(s.discord.channel_id, "5");
    assert_eq!(s.discord.guild_id.as_deref(), Some("9"));
    assert_eq!(s.answer_at, None);
    assert!(voice_origin(&[OWNER, GUEST], 5, 9).is_none());
    assert!(voice_origin(&[], 5, 9).is_none());
}
