//! A reply in the DM that answers the question it replies to (theseus-suvpe).
//!
//! The courier tells `Asked` which questions a needs-you write holds when it
//! plans it, and which message Discord gave it when it lands. A message that
//! replies to one of those, and whose whole text is one of the short forms
//! (`yes`, `no`, `approve 2`, `no all`, an ask's number or its choice's text),
//! is `work.answer` as the replier, with their `DiscordOrigin`, so the core
//! judges it as it judges a press. Any other text is an ordinary message, a
//! turn as ever: `yes, but wait` is no answer. The map is in memory, so after
//! a restart a reply to an older card is an ordinary message.
//!
//! A counted answer is acknowledged by the card's settle and a ✅ on the
//! reply; a refused one tells the replier, in a reply to their message.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use theseus_protocol::answer::{ConfirmListParams, WorkAnswerParams};
use theseus_protocol::{method, ConfirmListResult, DiscordOrigin};
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::channel::message::AllowedMentions;
use twilight_model::channel::Message;

use super::super::Shared;

/// How many messages a reply can find.
const KEPT: usize = 256;

/// One question a message holds: its id, and its choices when it is an ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub question: String,
    pub choices: Vec<String>,
}

#[derive(Default)]
struct State {
    /// What a write holds, by its key, until it lands.
    planned: HashMap<String, Vec<Entry>>,
    /// What a landed message holds, oldest first.
    landed: VecDeque<(u64, Vec<Entry>)>,
}

/// The needs-you messages a reply can answer.
#[derive(Default)]
pub(crate) struct Asked(Mutex<State>);

impl Asked {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The write under `key` holds `entries`: one for a card, the cards' in
    /// their numbers for a header.
    pub(crate) fn plan(&self, key: &str, entries: Vec<Entry>) {
        self.state().planned.insert(key.to_string(), entries);
    }

    /// A write landed as `message`.
    pub(crate) fn landed(&self, message: u64, key: &str) {
        let mut s = self.state();
        let Some(entries) = s.planned.remove(key) else {
            return;
        };
        s.landed.retain(|(m, _)| *m != message);
        s.landed.push_back((message, entries));
        while s.landed.len() > KEPT {
            s.landed.pop_front();
        }
    }

    fn of(&self, message: u64) -> Option<Vec<Entry>> {
        let s = self.state();
        s.landed
            .iter()
            .find(|(m, _)| *m == message)
            .map(|(_, e)| e.clone())
    }
}

/// What a reply to a needs-you message says.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Said {
    /// Not an answer: an ordinary message.
    Ordinary,
    /// This option for each of these questions (indexes into the entries).
    Answer(String, Vec<usize>),
    /// This option for every open question that is not an ask.
    All(String),
    /// A bare yes or no to a header: whichever is the one open question.
    Bare(String),
}

/// A short form's option: `yes y approve ok` and `no n decline`.
fn verb(word: &str) -> Option<&'static str> {
    match word {
        "yes" | "y" | "approve" | "ok" => Some("approve"),
        "no" | "n" | "decline" => Some("decline"),
        _ => None,
    }
}

/// Read a reply's whole text against what the message holds.
pub(crate) fn read(text: &str, entries: &[Entry]) -> Said {
    let t = text.trim().trim_end_matches(['.', '!']).to_lowercase();
    let words: Vec<&str> = t.split_whitespace().collect();
    if let [one] = entries {
        if !one.choices.is_empty() {
            let n = words.first().and_then(|w| w.parse::<usize>().ok());
            let by_text = one
                .choices
                .iter()
                .position(|c| c.trim().to_lowercase() == t);
            return match (n.filter(|n| (1..=one.choices.len()).contains(n)), by_text) {
                (Some(n), _) if words.len() == 1 => Said::Answer(n.to_string(), vec![0]),
                (_, Some(i)) => Said::Answer((i + 1).to_string(), vec![0]),
                _ => Said::Ordinary,
            };
        }
        return match words.as_slice() {
            [w] => verb(w).map_or(Said::Ordinary, |o| Said::Answer(o.into(), vec![0])),
            _ => Said::Ordinary,
        };
    }
    match words.as_slice() {
        [w] => verb(w).map_or(Said::Ordinary, |o| Said::Bare(o.into())),
        [w, "all"] => verb(w).map_or(Said::Ordinary, |o| Said::All(o.into())),
        [w, n] => match (verb(w), n.parse::<usize>()) {
            (Some(o), Ok(n)) if (1..=entries.len()).contains(&n) => {
                Said::Answer(o.into(), vec![n - 1])
            }
            _ => Said::Ordinary,
        },
        _ => Said::Ordinary,
    }
}

impl Shared {
    /// Whether `m` answers a question it replies to; the answer is made off
    /// the gateway's path.
    pub(in super::super) fn answers_question(self: &Arc<Self>, m: &Message) -> bool {
        let to = m
            .reference
            .as_ref()
            .and_then(|r| r.message_id)
            .or(m.referenced_message.as_ref().map(|r| r.id));
        let Some(entries) = to.and_then(|id| self.replies.asked().of(id.get())) else {
            return false;
        };
        let said = read(&m.content, &entries);
        if said == Said::Ordinary {
            return false;
        }
        let (shared, m) = (self.clone(), m.clone());
        tokio::spawn(async move { shared.answer_by_reply(&m, &entries, said).await });
        true
    }

    /// The open questions among `entries`, as the core lists them for the
    /// replier.
    async fn open_among(&self, entries: &[Entry], who: &DiscordOrigin) -> Vec<usize> {
        let listed = self
            .rpc
            .call::<_, ConfirmListResult>(
                method::CONFIRM_LIST,
                ConfirmListParams {
                    discord: Some(who.clone()),
                },
            )
            .await;
        let open: Vec<String> = listed
            .map(|l| l.confirms.into_iter().map(|c| c.correlation_id).collect())
            .unwrap_or_default();
        (0..entries.len())
            .filter(|i| open.contains(&entries[*i].question))
            .collect()
    }

    async fn answer_by_reply(&self, m: &Message, entries: &[Entry], said: Said) {
        let origin = DiscordOrigin {
            user_id: m.author.id.to_string(),
            channel_id: m.channel_id.to_string(),
            guild_id: m.guild_id.map(|g| g.to_string()),
        };
        let author = format!("discord:{}", m.author.name);
        let (option, targets) = match said {
            Said::Answer(o, t) => (o, t),
            Said::All(o) => {
                let open = self.open_among(entries, &origin).await;
                let t: Vec<usize> = open
                    .into_iter()
                    .filter(|i| entries[*i].choices.is_empty())
                    .collect();
                (o, t)
            }
            Said::Bare(o) => match self.open_among(entries, &origin).await.as_slice() {
                [one] => (o, vec![*one]),
                open => {
                    let list: Vec<String> = open.iter().map(|i| (i + 1).to_string()).collect();
                    let text = match list.is_empty() {
                        true => "None of these questions is open now.".to_string(),
                        false => format!(
                            "More than one is open ({}): reply `approve 2` or `no 1`, or \
                             `yes all`.",
                            list.join(", ")
                        ),
                    };
                    self.tell_replier(m, &text).await;
                    return;
                }
            },
            Said::Ordinary => return,
        };
        let mut refused = Vec::new();
        for i in &targets {
            let r = self
                .rpc
                .call::<_, serde_json::Value>(
                    method::WORK_ANSWER,
                    WorkAnswerParams {
                        question_id: entries[*i].question.clone(),
                        option: option.clone(),
                        author: Some(author.clone()),
                        discord: Some(origin.clone()),
                        ..Default::default()
                    },
                )
                .await;
            if let Err(e) = r {
                let why = e.data.get("why").and_then(serde_json::Value::as_str);
                refused.push(why.unwrap_or(&e.message).to_string());
            }
        }
        match (refused.first(), targets.len() > refused.len()) {
            (Some(why), _) => {
                self.tell_replier(m, &format!("🔐 Your answer did not count: {why}"))
                    .await;
            }
            (None, true) => {
                let ok = RequestReactionType::Unicode { name: "\u{2705}" };
                let _ = self.http.create_reaction(m.channel_id, m.id, &ok).await;
            }
            (None, false) => {
                self.tell_replier(m, "Nothing there is open to answer.")
                    .await
            }
        }
    }

    /// A line to the replier, as a reply to their message.
    async fn tell_replier(&self, m: &Message, text: &str) {
        let none = AllowedMentions::default();
        let sent = self
            .http
            .create_message(m.channel_id)
            .reply(m.id)
            .content(text)
            .allowed_mentions(Some(&none))
            .await;
        if let Err(e) = sent {
            self.board.error("reply to an answer", None, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Vec<Entry> {
        vec![Entry {
            question: "act_a".into(),
            choices: vec![],
        }]
    }

    fn header() -> Vec<Entry> {
        (0..3)
            .map(|i| Entry {
                question: format!("act_{i}"),
                choices: vec![],
            })
            .collect()
    }

    #[test]
    fn a_card_takes_the_short_forms_whole_and_nothing_else() {
        for yes in ["yes", " Y ", "approve", "OK.", "ok!"] {
            assert_eq!(read(yes, &card()), Said::Answer("approve".into(), vec![0]));
        }
        for no in ["no", "n", "Decline"] {
            assert_eq!(read(no, &card()), Said::Answer("decline".into(), vec![0]));
        }
        // "trust" is no short form any more (R6, theseus-gyjyo.8).
        assert_eq!(read("trust", &card()), Said::Ordinary);
        for other in [
            "yes, but wait",
            "yes please do",
            "no way",
            "ok then",
            "maybe",
            "",
        ] {
            assert_eq!(read(other, &card()), Said::Ordinary, "{other}");
        }
    }

    #[test]
    fn an_ask_takes_its_number_or_its_choices_text() {
        let ask = vec![Entry {
            question: "act_q".into(),
            choices: vec!["Careful".into(), "Ship it".into()],
        }];
        assert_eq!(read("2", &ask), Said::Answer("2".into(), vec![0]));
        assert_eq!(read("ship it", &ask), Said::Answer("2".into(), vec![0]));
        assert_eq!(read("careful", &ask), Said::Answer("1".into(), vec![0]));
        assert_eq!(read("3", &ask), Said::Ordinary);
        assert_eq!(read("yes", &ask), Said::Ordinary);
    }

    #[test]
    fn a_header_takes_a_number_all_or_a_bare_verb() {
        assert_eq!(
            read("approve 2", &header()),
            Said::Answer("approve".into(), vec![1])
        );
        assert_eq!(
            read("no 1", &header()),
            Said::Answer("decline".into(), vec![0])
        );
        assert_eq!(read("yes all", &header()), Said::All("approve".into()));
        assert_eq!(read("no all", &header()), Said::All("decline".into()));
        assert_eq!(read("yes", &header()), Said::Bare("approve".into()));
        assert_eq!(read("approve 4", &header()), Said::Ordinary);
        assert_eq!(read("approve 2 and 3", &header()), Said::Ordinary);
    }
}
