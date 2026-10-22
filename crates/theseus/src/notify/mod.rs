//! Notifying the owner from a terminal client (theseus-f0bi9): one module
//! every terminal client calls, so a ping means the same in each.
//!
//! - [`Notices`] (in `notices`): the policy's notices (`theseus_protocol::notices`)
//!   with their second of debounce and the burst fold ("3 questions need
//!   you"). The TUI and `theseus status --watch` feed it views.
//! - [`Delivery`]: how a ping reaches the owner: `auto`, `toast`, `bell`,
//!   `osc9`, `osc777`, `off`. `auto` is the toast under WSL, OSC 9 in a
//!   terminal that has it, else the bell ([`Delivery::resolve`], over an
//!   [`Env`] a test injects).
//! - [`Notifier`]: carries a ping out. A terminal ping is bytes for the
//!   caller to write; a toast goes to a worker thread (`toast`), so the
//!   caller never waits on `powershell.exe`.
//!
//! Nothing here probes the machine at start: the environment is read at the
//! first ping, and `powershell.exe` is found by trying it.

use std::sync::{Arc, OnceLock};

mod notices;
#[cfg(test)]
mod tests;
mod toast;

pub use notices::{Fired, Notices, DEBOUNCE_MS};
pub use toast::{toast_argv, Powershell, Sink, Spawner, Toast, Toaster, SCRIPT};

/// How a ping reaches the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delivery {
    /// A toast under WSL, OSC 9 in a terminal that has it, else the bell.
    #[default]
    Auto,
    /// A Windows toast, through `powershell.exe` (WSL).
    Toast,
    /// The terminal's bell.
    Bell,
    /// OSC 9, a desktop notification (iTerm2, Windows Terminal, kitty).
    Osc9,
    /// OSC 777, a desktop notification with a title (rxvt, foot, Ghostty).
    Osc777,
    Off,
}

impl Delivery {
    pub const NAMES: &'static str = "auto, toast, bell, osc9, osc777, or off";

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "auto" => Delivery::Auto,
            "toast" => Delivery::Toast,
            "bell" => Delivery::Bell,
            "osc9" => Delivery::Osc9,
            "osc777" => Delivery::Osc777,
            "off" => Delivery::Off,
            _ => return None,
        })
    }

    /// What `auto` is here; any other kind is itself.
    pub fn resolve(self, env: &Env) -> Delivery {
        match self {
            Delivery::Auto if env.wsl => Delivery::Toast,
            Delivery::Auto if env.osc9 => Delivery::Osc9,
            Delivery::Auto => Delivery::Bell,
            other => other,
        }
    }

    /// The bytes that deliver `text` to the terminal: none for a toast or
    /// off, and no bell for a ping without sound. No control character of
    /// the text reaches the terminal's sequence.
    pub fn bytes(self, text: &str, sound: bool) -> Vec<u8> {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        match self {
            // `auto` here is the bell, as an unresolved caller would want.
            Delivery::Bell | Delivery::Auto if sound => b"\x07".to_vec(),
            Delivery::Osc9 => format!("\x1b]9;theseus: {clean}\x07").into_bytes(),
            Delivery::Osc777 => format!("\x1b]777;notify;theseus;{clean}\x07").into_bytes(),
            _ => Vec::new(),
        }
    }
}

/// What `auto` reads of the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Env {
    pub wsl: bool,
    /// The terminal is one known to show OSC 9 as a notification.
    pub osc9: bool,
}

impl Env {
    /// This process's environment and `/proc/sys/kernel/osrelease`.
    pub fn detect() -> Self {
        let osrelease = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        Self::from_vars(&|k| std::env::var(k).ok(), &osrelease)
    }

    /// The same over injected variables and an `osrelease` text.
    pub fn from_vars(get: &dyn Fn(&str) -> Option<String>, osrelease: &str) -> Self {
        let set = |k: &str| get(k).is_some_and(|v| !v.is_empty());
        let rel = osrelease.to_ascii_lowercase();
        let program = get("TERM_PROGRAM").unwrap_or_default();
        Self {
            wsl: set("WSL_DISTRO_NAME") || rel.contains("microsoft") || rel.contains("wsl"),
            osc9: set("WT_SESSION")
                || set("ITERM_SESSION_ID")
                || set("KITTY_WINDOW_ID")
                || matches!(program.as_str(), "iTerm.app" | "WezTerm" | "ghostty"),
        }
    }
}

/// Carries pings out. Cheap to make: the environment is read at the first
/// ping and the worker thread starts at the first toast.
pub struct Notifier {
    env: OnceLock<Env>,
    detect: Box<dyn Fn() -> Env + Send + Sync>,
    toaster: Toaster,
}

impl Notifier {
    /// Toasts through `powershell.exe`; a toast that can't start rings
    /// `fallback` (the terminal's bell bytes) once and says so through `say`: stderr for a
    /// command, the notice line for a screen that owns the terminal.
    pub fn new(fallback: Sink, say: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        Self::with(Box::new(Env::detect), Arc::new(Powershell), fallback, say)
    }

    /// The same over an injected environment, spawner and voice (tests).
    pub fn with(
        detect: Box<dyn Fn() -> Env + Send + Sync>,
        spawner: Arc<dyn Spawner>,
        fallback: Sink,
        say: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        Self {
            env: OnceLock::new(),
            detect,
            toaster: Toaster::new(spawner, fallback, say),
        }
    }

    /// What `requested` comes to on this machine.
    pub fn resolve(&self, requested: Delivery) -> Delivery {
        requested.resolve(self.env.get_or_init(|| (self.detect)()))
    }

    /// Carry one ping out: the bytes for the terminal, empty for a toast
    /// (queued for the worker; this never waits on it) and for off. `title`
    /// names the task; `text` is the notice's line.
    pub fn deliver(&self, requested: Delivery, title: &str, text: &str, sound: bool) -> Vec<u8> {
        match self.resolve(requested) {
            Delivery::Toast if self.toaster.missing() => Delivery::Bell.bytes(text, sound),
            Delivery::Toast => {
                self.toaster.submit(Toast {
                    title: title.to_string(),
                    body: text.to_string(),
                    sound,
                });
                Vec::new()
            }
            d => d.bytes(text, sound),
        }
    }
}
