//! What the next voice turn is told about the last one (theseus-qb8o): the
//! session holds each reply's whole text as if it had all been heard, so a
//! reply cut short, one never voiced, and what the speaker said while
//! Theseus spoke ride in one bracketed line before the next voice turn's
//! input. Pure string work on the engine's facts: no model call, nothing
//! stored but the turn's input.

use std::collections::BTreeMap;

use theseus_voice::{CutWhy, Event, HeardAs, Over, Spoken, TurnId, Utterance};

/// A voice turn's first words, as a note names its reply: about this many
/// characters.
const FIRST_WORDS: usize = 40;
/// A quote in a note: about this many characters.
const QUOTE: usize = 80;
/// The newest notes the line carries.
const MAX_NOTES: usize = 3;
/// The line's length bound, in characters.
const MAX_LINE: usize = 400;
/// The voice turns whose first words are kept: a call's recent ones.
const KEPT_TURNS: usize = 32;

/// The call's notes: each voice turn's first words, and what waits for the
/// next voice turn.
#[derive(Debug, Default)]
pub(crate) struct Notes {
    firsts: BTreeMap<TurnId, String>,
    waiting: Vec<String>,
}

impl Notes {
    /// A voice turn began: its first words, to name its reply by.
    pub(crate) fn turn(&mut self, id: TurnId, utterances: &[Utterance]) {
        let words: Vec<&str> = utterances
            .iter()
            .map(|u| u.text.trim())
            .filter(|t| !t.is_empty())
            .collect();
        self.firsts.insert(id, clip(&words.join(" "), FIRST_WORDS));
        while self.firsts.len() > KEPT_TURNS {
            self.firsts.pop_first();
        }
    }

    /// What `what` is called in a note: `your reply to "<first words>"`, or
    /// `your report`.
    fn named(&self, what: Spoken) -> String {
        match what {
            Spoken::Reply(turn) => match self.firsts.get(&turn) {
                Some(first) => format!("your reply to \"{first}\""),
                None => "your reply".into(),
            },
            Spoken::Report | Spoken::Acknowledgment => "your report".into(),
        }
    }

    /// A reply or report cut short (the engine's `Cut`; any other event is
    /// none of this). One cut at the call's end needs no note: the call has
    /// no next turn.
    pub(crate) fn cut(&mut self, e: &Event) {
        let Event::Cut {
            what,
            why,
            sentences,
            heard,
            into,
            last_heard,
            cut,
        } = e
        else {
            return;
        };
        let (sentences, heard) = (*sentences, *heard);
        let named = self.named(*what);
        // A report cut short comes back at the next pause; a reply doesn't.
        let rest = match what {
            Spoken::Reply(_) => "the rest was not said",
            _ => "the rest comes back at the next pause",
        };
        let started = !into.is_zero();
        let cut = quote(cut);
        let note = match why {
            CutWhy::CallEnded => return,
            CutWhy::Superseded => {
                format!("{named} was not said: they kept talking before it began")
            }
            CutWhy::Words if heard == 0 && !started => {
                format!("{named} ({sentences} sentences) was never said aloud")
            }
            CutWhy::Words => {
                let heard_part = match (heard, last_heard) {
                    (0, _) | (_, None) => {
                        format!("they heard none of it whole (0 of {sentences} sentences)")
                    }
                    (h, Some(l)) => {
                        format!("they heard \"{}\" ({h} of {sentences} sentences)", quote(l))
                    }
                };
                let cut_part = match started {
                    true => format!("you were saying \"{cut}\" when they spoke, and {rest}"),
                    false => format!("\"{cut}\" was next, and {rest}"),
                };
                format!("they cut in on {named}: {heard_part}; {cut_part}")
            }
        };
        self.waiting.push(note);
    }

    /// An utterance heard: a backchannel or a request to go on, said while
    /// Theseus spoke, is no turn of its own, so the next one is told.
    pub(crate) fn heard(&mut self, u: &Utterance) {
        if matches!(u.heard_as, HeardAs::Backchannel | HeardAs::Resume) {
            self.waiting
                .push(format!("while you spoke they said \"{}\"", quote(&u.text)));
        }
    }

    /// The line before a voice turn's input, `[Voice: …]`, and the notes
    /// drained: the newest that fit, and a tag for each of `utterances` said
    /// before a reply being prepared had been spoken. `None` with no notes.
    pub(crate) fn line(&mut self, utterances: &[Utterance]) -> Option<String> {
        let mut notes = std::mem::take(&mut self.waiting);
        for u in utterances {
            if let Some(Over::Preparing { turn }) = &u.over {
                let named = self.named(Spoken::Reply(*turn));
                notes.push(format!("they said this before {named} had been spoken"));
            }
        }
        bounded(notes)
    }
}

/// `[Voice: ` the newest notes that fit (at most `MAX_NOTES`, the line under
/// `MAX_LINE` characters, the newest always), oldest first, joined by `; `,
/// then `]`.
fn bounded(notes: Vec<String>) -> Option<String> {
    const OPEN: &str = "[Voice: ";
    let mut kept: Vec<String> = Vec::new();
    let mut len = OPEN.chars().count() + 1;
    for note in notes.into_iter().rev().take(MAX_NOTES) {
        let n = note.chars().count() + if kept.is_empty() { 0 } else { 2 };
        if !kept.is_empty() && len + n >= MAX_LINE {
            break;
        }
        len += n;
        kept.push(note);
    }
    if kept.is_empty() {
        return None;
    }
    kept.reverse();
    let mut line = format!("{OPEN}{}", kept.join("; "));
    if line.chars().count() >= MAX_LINE {
        line = clip(&line, MAX_LINE - 2);
    }
    line.push(']');
    Some(line)
}

/// `text` as a quote: clipped at about `QUOTE` characters, on a word.
fn quote(text: &str) -> String {
    clip(text.trim(), QUOTE)
}

/// `text`, its whitespace collapsed, clipped on a word boundary to at most
/// `max` characters, an ellipsis ending what was clipped.
pub(crate) fn clip(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let mut out = String::new();
    for word in text.split(' ') {
        let more = if out.is_empty() { 0 } else { 1 } + word.chars().count();
        if out.chars().count() + more + 1 > max {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.is_empty() {
        // One word longer than `max`: cut inside it.
        out = text.chars().take(max - 1).collect();
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quote_clips_on_a_word_with_an_ellipsis() {
        assert_eq!(clip("short words", 40), "short words");
        assert_eq!(clip("  spaced \n out  ", 40), "spaced out");
        let long = "the quick brown fox jumps over the lazy dog again and again";
        let c = clip(long, 20);
        assert_eq!(c, "the quick brown fox…");
        assert!(c.chars().count() <= 20);
        assert_eq!(clip("abcdefghijklmnop", 6), "abcde…");
    }
}
