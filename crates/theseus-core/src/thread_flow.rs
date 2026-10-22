//! What the router remembers, in memory (theseus-rha2): each place's last
//! exchange (for stickiness and the reply chain), the turns that ran in a
//! thread (for a reply to find its thread), the unaddressed messages it
//! routed (for `thread.move`), the handoffs from the router to the turn it
//! chose, and the questions waiting for an answer.
//!
//! None of it is the record: a route is the `thread.routed` row its turn
//! writes in its own first frame, an ask is `thread.asked` and
//! `thread.answered` rows, and a move is `thread.moved`. A restart forgets
//! the place memory (the router starts cold: stickiness and a reply to an
//! old message need the new run's turns), and forgets a question nobody had
//! answered (its message is lost with the `turn.submit` that waited).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use theseus_protocol::thread_route::RouteHow;
use theseus_protocol::threads::WorkingSet;

use crate::approval::Answerer;
use crate::judge::inbound::Early;
use crate::thread_route::{AskChoice, Choice, Last};

/// The turns remembered for a reply to find.
const TURNS: usize = 512;
/// The routed messages remembered for a move to find.
const ROUTED: usize = 128;
/// A handoff the turn has not taken in this long is stale.
const STALE: Duration = Duration::from_secs(60);

/// An unaddressed message the router placed: what `thread.move` needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Routed {
    pub turn: String,
    /// The place's own session, where the message arrived.
    pub home: String,
    /// The thread it ran in (none: at home) and the session that ran it.
    pub to: Option<String>,
    pub session: String,
    pub text: String,
    pub author: String,
    pub how: RouteHow,
    /// The `thread` judgment, when Jev was asked, and the slugs it offered.
    pub judgment: Option<(String, Vec<String>)>,
    pub at_ms: u64,
}

/// What the router hands the turn it chose: the turn's id (minted before the
/// choice, so Jev's judgments name it), the point begun before the choice,
/// and the row the turn writes in its first frame.
pub struct Handoff {
    pub turn_id: String,
    pub text: String,
    pub early: Option<Early>,
    pub row: Option<crate::fact::thread_route::RoutedRow>,
    pub at: Instant,
}

/// One question waiting.
pub struct Pending {
    pub home: String,
    pub text: String,
    pub options: Vec<AskChoice>,
    /// The live threads it was asked over, by slug: any of them answers it
    /// (theseus-98bz.4; none with `[threads] ask_every_thread` off).
    pub slugs: Vec<String>,
    pub tx: tokio::sync::oneshot::Sender<Answered>,
}

/// The answer to a question: the choice, and who gave it.
pub struct Answered {
    pub choice: Choice,
    pub who: Answerer,
}

#[derive(Default)]
struct Recent {
    places: HashMap<String, Last>,
    /// (turn, thread slug) of turns that ran in a thread, newest last.
    turns: VecDeque<(String, String)>,
    routed: VecDeque<Routed>,
    /// (turn, thread slug, what the turn's message grew into it) of turns
    /// whose message named something new, newest last: a move takes it back.
    grew: VecDeque<(String, String, WorkingSet)>,
    /// Turns whose growth a move took back (or found none to take): their
    /// growth, if it has not run yet, grows nothing.
    ungrown: VecDeque<String>,
}

/// The router's memory.
#[derive(Default)]
pub struct State {
    recent: Mutex<Recent>,
    handoffs: Mutex<HashMap<String, Handoff>>,
    asks: Mutex<HashMap<String, Pending>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl State {
    /// The place's last exchange.
    pub fn last(&self, home: &str) -> Option<Last> {
        lock(&self.recent).places.get(home).cloned()
    }

    /// A message arrived in the place and ran in `slug` (none: at home).
    pub fn arrived(&self, home: &str, slug: Option<&str>, now_ms: u64) {
        lock(&self.recent).places.insert(
            home.to_string(),
            Last {
                slug: slug.map(str::to_string),
                at_ms: now_ms,
                replied_ms: None,
            },
        );
    }

    /// The place's turn ended with a reply, at `now_ms`.
    pub fn replied(&self, home: &str, now_ms: u64) {
        if let Some(l) = lock(&self.recent).places.get_mut(home) {
            l.replied_ms = Some(now_ms);
        }
    }

    /// A turn ran in the thread `slug`.
    pub fn turn_ran(&self, turn: &str, slug: &str) {
        let mut r = lock(&self.recent);
        r.turns.push_back((turn.to_string(), slug.to_string()));
        while r.turns.len() > TURNS {
            r.turns.pop_front();
        }
    }

    /// The turn's message grew `added` into the thread `slug`, unless a move
    /// took the turn's growth back already: then nothing is noted and this
    /// says false, the check and the note under one lock. An empty growth
    /// notes nothing and is true.
    pub fn grew(&self, turn: &str, slug: &str, added: WorkingSet) -> bool {
        let mut r = lock(&self.recent);
        if r.ungrown.iter().any(|t| t == turn) {
            return false;
        }
        if added == WorkingSet::default() {
            return true;
        }
        r.grew
            .push_back((turn.to_string(), slug.to_string(), added));
        while r.grew.len() > TURNS {
            r.grew.pop_front();
        }
        true
    }

    /// What the turn's message grew, once: the thread and the entries. The
    /// turn is marked either way, so a growth that runs after the move (the
    /// turn still running) grows nothing in the thread it was moved from.
    pub fn take_grew(&self, turn: &str) -> Option<(String, WorkingSet)> {
        let mut r = lock(&self.recent);
        r.ungrown.push_back(turn.to_string());
        while r.ungrown.len() > TURNS {
            r.ungrown.pop_front();
        }
        let i = r.grew.iter().position(|(t, _, _)| t == turn)?;
        r.grew.remove(i).map(|(_, s, w)| (s, w))
    }

    /// Whether a move already took the turn's growth back.
    pub fn is_ungrown(&self, turn: &str) -> bool {
        lock(&self.recent).ungrown.iter().any(|t| t == turn)
    }

    /// The thread a turn ran in.
    pub fn thread_of_turn(&self, turn: &str) -> Option<String> {
        lock(&self.recent)
            .turns
            .iter()
            .rev()
            .find(|(t, _)| t == turn)
            .map(|(_, s)| s.clone())
    }

    /// An unaddressed message was placed.
    pub fn routed(&self, r: Routed) {
        let mut g = lock(&self.recent);
        g.routed.retain(|x| x.turn != r.turn);
        g.routed.push_back(r);
        while g.routed.len() > ROUTED {
            g.routed.pop_front();
        }
    }

    /// The routed message `turn` names, or the newest of the place `home` (or
    /// of the thread session `home` names).
    pub fn find_routed(&self, session: &str, turn: Option<&str>) -> Option<Routed> {
        let g = lock(&self.recent);
        g.routed
            .iter()
            .rev()
            .find(|r| match turn {
                Some(t) => r.turn == t && (r.home == session || r.session == session),
                None => r.home == session || r.session == session,
            })
            .cloned()
    }

    /// The message has moved: it is no longer placed where it was.
    pub fn moved(&self, turn: &str) {
        lock(&self.recent).routed.retain(|r| r.turn != turn);
    }

    // ---- handoffs

    pub fn hand_off(&self, session: &str, h: Handoff) {
        lock(&self.handoffs).insert(session.to_string(), h);
    }

    /// The id the router minted for the session's next turn, while its
    /// handoff is fresh.
    pub fn turn_id(&self, session: &str) -> Option<String> {
        lock(&self.handoffs)
            .get(session)
            .filter(|h| h.at.elapsed() < STALE)
            .map(|h| h.turn_id.clone())
    }

    /// The handoff, if it is the session's and for this message.
    pub fn take_handoff(&self, session: &str, text: &str) -> Option<Handoff> {
        let mut g = lock(&self.handoffs);
        let fresh = g
            .get(session)
            .is_some_and(|h| h.at.elapsed() < STALE && h.text == text);
        match fresh {
            true => g.remove(session),
            false => None,
        }
    }

    /// The turn is over: whatever handoff it left is gone.
    pub fn clear_handoff(&self, session: &str) {
        lock(&self.handoffs).remove(session);
    }

    // ---- questions

    pub fn ask(&self, id: &str, p: Pending) {
        lock(&self.asks).insert(id.to_string(), p);
    }

    /// A question's pending state, to answer it: `Err` names why not.
    pub fn answer(&self, id: &str, key: &str, who: Answerer) -> Result<(String, Choice), String> {
        let mut g = lock(&self.asks);
        let Some(p) = g.get(id) else {
            return Err(format!(
                "no question {id} is waiting: it was answered, or it ran out"
            ));
        };
        let key = key.trim().to_lowercase();
        let key = match key.as_str() {
            "n" => "new",
            "h" => "home",
            k => k,
        };
        let keyed = p
            .options
            .iter()
            .find(|o| o.option.key == key && o.choice != Choice::Other);
        let choice = match keyed {
            Some(c) => c.choice.clone(),
            None => match p.slugs.iter().find(|s| s.to_lowercase() == key) {
                Some(slug) => Choice::Thread(slug.clone()),
                None => {
                    let keys: Vec<&str> = p
                        .options
                        .iter()
                        .filter(|o| o.choice != Choice::Other)
                        .map(|o| o.option.key.as_str())
                        .collect();
                    let slug = match p.slugs.is_empty() {
                        true => "",
                        false => ", or a thread's slug",
                    };
                    return Err(format!(
                        "{key:?} is not an option: {}{slug}",
                        keys.join(", ")
                    ));
                }
            },
        };
        let p = g.remove(id).expect("held");
        let home = p.home.clone();
        let _ = p.tx.send(Answered {
            choice: choice.clone(),
            who,
        });
        Ok((home, choice))
    }

    /// The question ran out: it waits no more.
    pub fn expire(&self, id: &str) -> bool {
        lock(&self.asks).remove(id).is_some()
    }

    /// The questions waiting, as (id, home session, text).
    pub fn waiting(&self) -> Vec<(String, String, String)> {
        lock(&self.asks)
            .iter()
            .map(|(id, p)| (id.clone(), p.home.clone(), p.text.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routed(turn: &str, home: &str, session: &str) -> Routed {
        Routed {
            turn: turn.into(),
            home: home.into(),
            to: Some("a".into()),
            session: session.into(),
            text: "hello".into(),
            author: "a".into(),
            how: RouteHow::Quiet,
            judgment: None,
            at_ms: 1,
        }
    }

    /// The router finds a place's last exchange, a turn's thread, and the
    /// message a move names (by turn, or the place's newest).
    #[test]
    fn the_places_memory_is_found_by_place_turn_and_thread_session() {
        let s = State::default();
        assert!(s.last("ses_h").is_none());
        s.arrived("ses_h", Some("a"), 100);
        s.replied("ses_h", 150);
        let l = s.last("ses_h").unwrap();
        assert_eq!(
            (l.slug.as_deref(), l.at_ms, l.replied_ms),
            (Some("a"), 100, Some(150))
        );
        s.arrived("ses_h", None, 200);
        assert_eq!(
            s.last("ses_h").unwrap().replied_ms,
            None,
            "a newer message clears the reply"
        );
        s.turn_ran("turn_1", "a");
        assert_eq!(s.thread_of_turn("turn_1").as_deref(), Some("a"));
        assert_eq!(s.thread_of_turn("turn_2"), None);
        s.routed(routed("turn_1", "ses_h", "ses_a"));
        s.routed(routed("turn_3", "ses_h", "ses_b"));
        assert_eq!(s.find_routed("ses_h", None).unwrap().turn, "turn_3");
        assert_eq!(
            s.find_routed("ses_a", None).unwrap().turn,
            "turn_1",
            "by the thread session"
        );
        assert_eq!(
            s.find_routed("ses_h", Some("turn_1")).unwrap().session,
            "ses_a"
        );
        assert!(s.find_routed("ses_other", None).is_none());
        s.moved("turn_3");
        assert_eq!(s.find_routed("ses_h", None).unwrap().turn, "turn_1");
    }

    /// An ask is answered by an option's key or by any live thread's slug it
    /// was asked over; `other` alone, or an unknown word, names the choices.
    #[test]
    fn an_ask_takes_a_key_or_a_threads_slug() {
        use crate::thread_route::AskChoice;
        use theseus_protocol::thread_route::AskOption;
        let s = State::default();
        let opt = |choice: Choice, key: &str| AskChoice {
            choice,
            option: AskOption {
                key: key.into(),
                label: key.into(),
            },
        };
        let ask = |id: &str| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            s.ask(
                id,
                Pending {
                    home: "ses_h".into(),
                    text: "x".into(),
                    options: vec![
                        opt(Choice::Thread("a".into()), "1"),
                        opt(Choice::Other, "other"),
                        opt(Choice::Home, "home"),
                    ],
                    slugs: vec!["a".into(), "e-five".into()],
                    tx,
                },
            );
            rx
        };
        let who = || Answerer::from("the CLI");
        let _rx = ask("ask_1");
        let err = s.answer("ask_1", "other", who()).unwrap_err();
        assert_eq!(
            err,
            "\"other\" is not an option: 1, home, or a thread's slug"
        );
        assert!(s.answer("ask_1", "nope", who()).is_err());
        let (_, c) = s.answer("ask_1", "E-Five", who()).unwrap();
        assert_eq!(c, Choice::Thread("e-five".into()));
        let _rx = ask("ask_2");
        assert_eq!(
            s.answer("ask_2", "1", who()).unwrap().1,
            Choice::Thread("a".into())
        );
    }

    /// A handoff is the session's and for its message only; taken once.
    #[test]
    fn a_handoff_is_taken_once_by_the_message_it_was_made_for() {
        let s = State::default();
        let h = |text: &str| Handoff {
            turn_id: "turn_x".into(),
            text: text.into(),
            early: None,
            row: None,
            at: Instant::now(),
        };
        s.hand_off("ses_a", h("one"));
        assert_eq!(s.turn_id("ses_a").as_deref(), Some("turn_x"));
        assert!(s.take_handoff("ses_a", "two").is_none(), "another message");
        assert!(s.take_handoff("ses_b", "one").is_none(), "another session");
        assert!(s.take_handoff("ses_a", "one").is_some());
        assert!(s.take_handoff("ses_a", "one").is_none(), "once");
        s.hand_off("ses_a", h("one"));
        s.clear_handoff("ses_a");
        assert!(s.turn_id("ses_a").is_none());
    }
}
