//! The Windows toast from WSL (theseus-f0bi9): `powershell.exe` run with a
//! fixed script, on a worker thread of its own.
//!
//! **The text never meets the script.** `powershell.exe -Command` joins its
//! trailing arguments with spaces and parses the result as code, so a title
//! passed as an argument would be code if it held a space and a quote. So the
//! title, the body and the sound go as three words that are letters and
//! digits alone (`x` and the text's UTF-8 as hex, [`word`]), which the
//! script decodes and sets as a node's text, never parsing it. Whatever the
//! text holds, the command line is the same fixed script and three safe
//! words.
//!
//! At most one toast runs and one waits; a newer replaces the one waiting.
//! A toast is killed at five seconds. A machine with no `powershell.exe`
//! rings the bell once, says so on stderr once, and rings the bell for each
//! ping after.

use std::io;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a toast may run before it is killed.
pub const KILL_AFTER: Duration = Duration::from_secs(5);
/// The longest title and body the toast shows, in characters.
const TITLE_MAX: usize = 80;
const BODY_MAX: usize = 300;

/// The fixed script. Its three arguments are [`word`]s. The toast's text is
/// set with `InnerText`, so XML needs no escaping, and the app id is
/// Windows PowerShell's own, which a toast may carry without registering
/// one.
pub const SCRIPT: &str = r#"& { param([string]$t,[string]$b,[string]$s)
$ErrorActionPreference = 'Stop'
$d = { param($h) $h = $h.Substring(1); $n = [int]($h.Length / 2); $o = New-Object byte[] $n; for ($i = 0; $i -lt $n; $i++) { $o[$i] = [Convert]::ToByte($h.Substring(2 * $i, 2), 16) }; [Text.Encoding]::UTF8.GetString($o) }
[void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]
[void][Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime]
$x = New-Object Windows.Data.Xml.Dom.XmlDocument
$x.LoadXml('<toast><visual><binding template="ToastGeneric"><text/><text/></binding></visual><audio silent="true"/></toast>')
$n = $x.GetElementsByTagName('text')
$n.Item(0).InnerText = (& $d $t)
$n.Item(1).InnerText = (& $d $b)
if ((& $d $s) -eq 'sound') { $x.SelectSingleNode('//audio').SetAttribute('silent', 'false') }
$app = '{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe'
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($app).Show([Windows.UI.Notifications.ToastNotification]::new($x))
}"#;

/// Where bytes for the terminal go when a toast can't: the caller's writer.
pub type Sink = Arc<dyn Fn(&[u8]) + Send + Sync>;

/// One toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub title: String,
    pub body: String,
    pub sound: bool,
}

/// A word the script decodes: `x` and the text's UTF-8 as lower-case hex,
/// control characters dropped and the text cut to `max` characters. Letters
/// and digits alone, and never a number (the `x`).
fn word(text: &str, max: usize) -> String {
    let clean: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect();
    let mut w = String::with_capacity(1 + clean.len() * 2);
    w.push('x');
    for b in clean.trim().bytes() {
        w.push_str(&format!("{b:02x}"));
    }
    w
}

/// The command a toast runs: the program, then its arguments.
pub fn toast_argv(t: &Toast) -> Vec<String> {
    let title = if t.title.trim().is_empty() {
        "Theseus"
    } else {
        &t.title
    };
    let sound = if t.sound { "sound" } else { "silent" };
    vec![
        "powershell.exe".into(),
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-Command".into(),
        SCRIPT.into(),
        word(title, TITLE_MAX),
        word(&t.body, BODY_MAX),
        word(sound, TITLE_MAX),
    ]
}

/// Runs a toast's command to its end (or its kill). Blocking: the worker
/// thread calls it, never the caller of `Toaster::submit`.
pub trait Spawner: Send + Sync {
    fn run(&self, argv: &[String]) -> io::Result<()>;
}

/// The real thing: the command with no input and no output, killed at
/// [`KILL_AFTER`].
pub struct Powershell;

impl Spawner for Powershell {
    fn run(&self, argv: &[String]) -> io::Result<()> {
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let start = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            if start.elapsed() >= KILL_AFTER {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[derive(Default)]
struct State {
    running: bool,
    queued: Option<Toast>,
}

struct Shared {
    state: Mutex<State>,
    missing: AtomicBool,
    spawner: Arc<dyn Spawner>,
    fallback: Sink,
    say: Arc<dyn Fn(&str) + Send + Sync>,
}

/// The toast worker: one in flight, one waiting.
pub struct Toaster {
    shared: Arc<Shared>,
}

impl Toaster {
    pub fn new(
        spawner: Arc<dyn Spawner>,
        fallback: Sink,
        say: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                missing: AtomicBool::new(false),
                spawner,
                fallback,
                say,
            }),
        }
    }

    /// Whether `powershell.exe` was tried and is not there.
    pub fn missing(&self) -> bool {
        self.shared.missing.load(Ordering::Relaxed)
    }

    /// Hand a toast to the worker and return at once: it starts one if none
    /// runs, else it replaces the one waiting.
    pub fn submit(&self, t: Toast) {
        {
            let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.running {
                st.queued = Some(t);
                return;
            }
            st.running = true;
        }
        let shared = Arc::clone(&self.shared);
        let started = std::thread::Builder::new()
            .name("theseus-toast".into())
            .spawn(move || work(&shared, t));
        if started.is_err() {
            // No thread to be had: the next toast may try again.
            let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            st.running = false;
        }
    }
}

fn work(shared: &Shared, first: Toast) {
    let mut t = first;
    loop {
        if let Err(e) = shared.spawner.run(&toast_argv(&t)) {
            if e.kind() == io::ErrorKind::NotFound {
                give_up(shared, &t);
            }
        }
        let mut st = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        match st.queued.take() {
            Some(next) if !shared.missing.load(Ordering::Relaxed) => t = next,
            _ => {
                st.running = false;
                return;
            }
        }
    }
}

/// No `powershell.exe`: say so once, and ring the bell for this ping.
fn give_up(shared: &Shared, t: &Toast) {
    if !shared.missing.swap(true, Ordering::Relaxed) {
        (shared.say)(
            "theseus: no powershell.exe on PATH, so no Windows toasts; \
             ringing the terminal's bell instead (set --notify bell to stop looking)",
        );
        if t.sound {
            (shared.fallback)(b"\x07");
        }
    }
}
