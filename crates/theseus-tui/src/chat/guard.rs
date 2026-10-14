//! The terminal's modes the chat turns on, and puts back on every way out:
//! raw mode, bracketed paste (a paste is one event, never keys, theseus-8hcg)
//! and, where the terminal is known to report them, the keyboard protocol's
//! modifiers on Enter (Shift-Enter). Put back on a normal exit, an error, a
//! panic (a hook), and by `Drop`; a signal ends the loop, which exits
//! normally.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};

/// The keyboard flags were pushed, so they are popped.
static PUSHED: AtomicBool = AtomicBool::new(false);

/// A terminal that reports Shift-Enter once asked (the kitty keyboard
/// protocol's). Asking one that does not know it prints nothing, but waiting
/// for its answer would cost the start, so only the known ones are asked.
pub fn reports_modifiers() -> bool {
    std::env::var_os("KITTY_WINDOW_ID").is_some()
        || std::env::var_os("WEZTERM_EXECUTABLE").is_some()
        || std::env::var("TERM_PROGRAM").is_ok_and(|t| matches!(t.as_str(), "ghostty" | "WezTerm"))
        || std::env::var("TERM").is_ok_and(|t| t.contains("kitty"))
}

/// The bytes that turn the chat's modes on.
pub fn enter_bytes(keyboard: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = crossterm::queue!(out, EnableBracketedPaste);
    if keyboard {
        let _ = crossterm::queue!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    out
}

/// The bytes that turn them off again, and show the cursor.
pub fn leave_bytes(keyboard: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if keyboard {
        let _ = crossterm::queue!(out, PopKeyboardEnhancementFlags);
    }
    let _ = crossterm::queue!(out, DisableBracketedPaste, crossterm::cursor::Show);
    out
}

/// Raw mode on, the modes written; holds them until dropped.
pub struct Guard {
    done: bool,
}

impl Guard {
    pub fn enter() -> std::io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let keyboard = reports_modifiers();
        PUSHED.store(keyboard, Ordering::SeqCst);
        let mut out = std::io::stdout();
        out.write_all(&enter_bytes(keyboard))?;
        out.flush()?;
        on_panic();
        Ok(Self { done: false })
    }

    /// Put the terminal back. Safe to call twice.
    pub fn restore(&mut self) {
        if !std::mem::replace(&mut self.done, true) {
            put_back(&mut std::io::stdout());
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        // On a panic the hook alone puts the terminal back, as in a release
        // build, whose panic aborts and runs no drop: a dev build that
        // unwound through here would hide a hook that did not work.
        if !std::thread::panicking() {
            self.restore();
        }
    }
}

fn put_back(out: &mut dyn Write) {
    let _ = out.write_all(&leave_bytes(PUSHED.load(Ordering::SeqCst)));
    let _ = out.flush();
    let _ = crossterm::terminal::disable_raw_mode();
}

/// On a panic, put the terminal back, then run the hook before it.
fn on_panic() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        put_back(&mut std::io::stdout());
        // The message goes on a fresh line, in a cooked terminal.
        let _ = std::io::stdout().write_all(b"\r\n");
        before(info);
    }));
}
