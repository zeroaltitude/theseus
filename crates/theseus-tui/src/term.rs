//! The terminal's modes the loop turns on, and puts back on every way out
//! (theseus-8hcg): focus events (the session in focus gets no notice while
//! the terminal has focus, design §2.9) and bracketed paste, so a paste
//! arrives as one event and never as keys. A pasted `q` is not the quit key,
//! and a pasted `y` answers no question.
//!
//! Raw mode and the alternate screen are ratatui's (`try_init`, its panic
//! hook, `try_restore`); these two are the TUI's own. The loop writes `enter`
//! when it starts and `leave` when it ends, on a quit, an error, or a
//! signal; a panic writes `leave` from the hook `on_panic` installs.

use std::io::Write;

use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
};

/// The bytes that turn the TUI's modes on.
pub fn enter() -> Vec<u8> {
    let mut out = Vec::new();
    let _ = crossterm::queue!(out, EnableFocusChange, EnableBracketedPaste);
    out
}

/// The bytes that turn them off again.
pub fn leave() -> Vec<u8> {
    let mut out = Vec::new();
    let _ = crossterm::queue!(out, DisableBracketedPaste, DisableFocusChange);
    out
}

/// On a panic, write `leave` to `out()`, then run the hook before it
/// (ratatui's, which puts back raw mode and the alternate screen).
pub fn on_panic(out: fn() -> Box<dyn Write>) {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut w = out();
        let _ = w.write_all(&leave()).and_then(|()| w.flush());
        before(info);
    }));
}
