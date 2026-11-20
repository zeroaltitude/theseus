//! What the stand-in holds back before it is applied: the daemon's edits of messages, and an interaction's
//! completion, so a test can put two writers of one card in either order. A child of `fake_discord`.

use std::time::Duration;

use super::FakeDiscord;

/// What the service has not applied yet (`FakeDiscord::withhold_edits`).
#[derive(Default)]
pub(super) struct Withhold {
    pub(super) edits: bool,
    pub(super) completions: bool,
    pub(super) waiting: usize,
}

impl FakeDiscord {
    /// Withhold the bot's edits of messages (a card's settle) before they are applied, until lifted (`false`), or a
    /// minute passes: the other order of two writers to a card.
    pub fn withhold_edits(&self, on: bool) {
        self.state.lock().unwrap().withhold.edits = on;
    }

    /// Withhold an interaction's completion (the edit of its response, a follow-up) before it is applied, until lifted
    /// (`false`), or a minute passes. The acknowledgement is not withheld: Discord's three seconds are the binding's.
    pub fn withhold_completions(&self, on: bool) {
        self.state.lock().unwrap().withhold.completions = on;
    }

    /// How many requests wait for a withhold now.
    pub fn withheld(&self) -> usize {
        self.state.lock().unwrap().withhold.waiting
    }

    /// Wait while `is_on` says the withhold stands.
    pub(super) fn wait_withheld(&self, is_on: impl Fn(&Withhold) -> bool) {
        let mut st = self.state.lock().unwrap();
        if !is_on(&st.withhold) {
            return;
        }
        st.withhold.waiting += 1;
        drop(st);
        for _ in 0..60_000 {
            if !is_on(&self.state.lock().unwrap().withhold) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.state.lock().unwrap().withhold.waiting -= 1;
    }
}
