//! The chat's key map, in one place. A later row (chat-ask, chat-sessions,
//! rewind, context-meter) adds its keys here and its action to [`Action`],
//! and handles the action in `state.rs`; nothing else reads a `KeyEvent`.
//!
//! What the map assumes of the terminal: Enter sends; a newline is Ctrl-J
//! everywhere, Alt-Enter, Shift-Enter where the terminal reports the
//! modifier (the kitty keyboard protocol's terminals), or `\` then Enter.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Submit,
    Newline,
    Insert(char),
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    WordLeft,
    WordRight,
    KillToStart,
    KillToEnd,
    KillWord,
    /// Ctrl-C: clear the line; on an empty one, leave on the second.
    Interrupt,
    /// Ctrl-D on an empty line.
    EndOfInput,
    Tab,
    Escape,
    /// Two Escapes that reached the terminal in one read, which crossterm
    /// reports as Alt-Esc: the rewind's Esc Esc (theseus-23rk).
    DoubleEscape,
    /// Ctrl-O: the folded paste, whole.
    ShowPaste,
    /// Ctrl-T: the tasks, as a tree.
    Tasks,
    /// Ctrl-Enter (where the terminal reports it): stop the turn and send
    /// the queued message now.
    SendNow,
    /// Shift+Tab: the conversation's next mode, ask → edits → plan → auto
    /// (theseus-lyxy).
    CycleMode,
}

/// What a one-key answer to a question does (theseus-wy6v.1). It works only
/// while the question has the focus and the input line is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Yes,
    /// `n`: no, then an optional note and Enter.
    No,
    /// `v`: the whole call, in the transcript.
    Whole,
    /// `a` (a command's prefix) or `e` (edits in this project): yes, and
    /// the question's grant offer. A question that offers the other, or none,
    /// ignores the key.
    Grant,
    /// `1`..`6` (the index is 0..5): the nth choice of a task's `ask`
    /// question (theseus-mnd9g). A question without choices ignores the key.
    Choice(u8),
}

/// The question's keys, one table: a later row (modes) adds a row.
pub const ASK_KEYS: &[(char, Ask)] = &[
    ('y', Ask::Yes),
    ('n', Ask::No),
    ('v', Ask::Whole),
    ('a', Ask::Grant),
    ('e', Ask::Grant),
    ('1', Ask::Choice(0)),
    ('2', Ask::Choice(1)),
    ('3', Ask::Choice(2)),
    ('4', Ask::Choice(3)),
    ('5', Ask::Choice(4)),
    ('6', Ask::Choice(5)),
];

/// The answer a character is, if it is one.
pub fn ask_key(c: char) -> Option<(char, Ask)> {
    let c = c.to_ascii_lowercase();
    ASK_KEYS
        .iter()
        .find(|(k, _)| *k == c)
        .map(|(k, a)| (*k, *a))
}

pub fn map(k: KeyEvent) -> Option<Action> {
    if k.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
    Some(match k.code {
        KeyCode::Enter if ctrl => Action::SendNow,
        KeyCode::Enter if shift || alt => Action::Newline,
        KeyCode::Enter => Action::Submit,
        KeyCode::Char(c) if ctrl => match c.to_ascii_lowercase() {
            'j' => Action::Newline,
            'c' => Action::Interrupt,
            'd' => Action::EndOfInput,
            'a' => Action::Home,
            'e' => Action::End,
            'b' => Action::Left,
            'f' => Action::Right,
            'p' => Action::Up,
            'n' => Action::Down,
            'u' => Action::KillToStart,
            'k' => Action::KillToEnd,
            'w' => Action::KillWord,
            'o' => Action::ShowPaste,
            't' => Action::Tasks,
            'h' => Action::Backspace,
            _ => return None,
        },
        KeyCode::Char('b') if alt => Action::WordLeft,
        KeyCode::Char('f') if alt => Action::WordRight,
        KeyCode::Char(c) if !alt => Action::Insert(c),
        KeyCode::Backspace if alt => Action::KillWord,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Delete => Action::Delete,
        KeyCode::Left if ctrl || alt => Action::WordLeft,
        KeyCode::Right if ctrl || alt => Action::WordRight,
        KeyCode::Left => Action::Left,
        KeyCode::Right => Action::Right,
        KeyCode::Up => Action::Up,
        KeyCode::Down => Action::Down,
        KeyCode::Home => Action::Home,
        KeyCode::End => Action::End,
        KeyCode::Tab => Action::Tab,
        KeyCode::Esc if alt => Action::DoubleEscape,
        KeyCode::BackTab => Action::CycleMode,
        KeyCode::Esc => Action::Escape,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> Option<Action> {
        map(KeyEvent::new(code, mods))
    }

    #[test]
    fn enter_sends_and_the_newlines_are_newlines() {
        let none = KeyModifiers::NONE;
        assert_eq!(key(KeyCode::Enter, none), Some(Action::Submit));
        assert_eq!(
            key(KeyCode::Enter, KeyModifiers::SHIFT),
            Some(Action::Newline)
        );
        assert_eq!(
            key(KeyCode::Enter, KeyModifiers::ALT),
            Some(Action::Newline)
        );
        assert_eq!(
            key(KeyCode::Char('j'), KeyModifiers::CONTROL),
            Some(Action::Newline)
        );
    }

    #[test]
    fn ctrl_enter_sends_now_and_the_question_keys_are_one_table() {
        assert_eq!(
            key(KeyCode::BackTab, KeyModifiers::SHIFT),
            Some(Action::CycleMode)
        );
        assert_eq!(
            key(KeyCode::Enter, KeyModifiers::CONTROL),
            Some(Action::SendNow)
        );
        assert_eq!(ask_key('Y'), Some(('y', Ask::Yes)));
        assert_eq!(ask_key('e'), Some(('e', Ask::Grant)));
        assert_eq!(ask_key('w'), None);
        let mut keys: Vec<char> = ASK_KEYS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), ASK_KEYS.len());
    }

    #[test]
    fn a_release_is_no_key() {
        let mut k = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        k.kind = KeyEventKind::Release;
        assert_eq!(map(k), None);
    }

    #[test]
    fn a_capital_is_a_character_and_ctrl_c_interrupts() {
        assert_eq!(
            key(KeyCode::Char('Q'), KeyModifiers::SHIFT),
            Some(Action::Insert('Q'))
        );
        assert_eq!(
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some(Action::Interrupt)
        );
    }
}
