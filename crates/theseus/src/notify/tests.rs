//! The notify module (theseus-f0bi9): `auto` by environment, the toast's
//! command line (fixed script, text that never meets it), one in flight and
//! one waiting, and a missing `powershell.exe`. Words and bursts are the
//! TUI's `tests_notice` over `Notices`.

use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;

fn env(vars: &[(&str, &str)], osrelease: &str) -> Env {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Env::from_vars(
        &|k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
        osrelease,
    )
}

/// Records every argv it is run with; waits on `gate` when it has one.
#[derive(Default)]
struct Fake {
    runs: Mutex<Vec<Vec<String>>>,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
    started: Mutex<Option<mpsc::Sender<()>>>,
    missing: bool,
}

impl Spawner for Fake {
    fn run(&self, argv: &[String]) -> io::Result<()> {
        self.runs.lock().unwrap().push(argv.to_vec());
        if let Some(tx) = self.started.lock().unwrap().as_ref() {
            let _ = tx.send(());
        }
        if let Some(rx) = self.gate.lock().unwrap().as_ref() {
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }
        if self.missing {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(())
    }
}

type Heard = Arc<Mutex<Vec<String>>>;
type Rang = Arc<Mutex<Vec<Vec<u8>>>>;

fn notifier(fake: &Arc<Fake>, wsl: bool) -> (Notifier, Heard, Rang) {
    let heard = Heard::default();
    let rang = Rang::default();
    let (h, r) = (heard.clone(), rang.clone());
    let n = Notifier::with(
        Box::new(move || Env { wsl, osc9: false }),
        fake.clone(),
        Arc::new(move |b| r.lock().unwrap().push(b.to_vec())),
        Arc::new(move |m| h.lock().unwrap().push(m.to_string())),
    );
    (n, heard, rang)
}

/// Wait for `cond` the way a test waits on a thread: a bounded poll.
fn until(what: &str, cond: impl Fn() -> bool) {
    for _ in 0..500 {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("never saw {what}");
}

fn decode(w: &str) -> String {
    assert!(w.starts_with('x'), "{w}");
    let hex = &w[1..];
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn auto_picks_the_toast_under_wsl_and_the_bell_outside() {
    let wsl_var = env(&[("WSL_DISTRO_NAME", "Ubuntu")], "");
    let wsl_kernel = env(&[], "5.15.153.1-microsoft-standard-WSL2\n");
    let plain = env(&[], "6.8.0-45-generic\n");
    let kitty = env(&[("KITTY_WINDOW_ID", "3")], "6.8.0-45-generic");
    assert_eq!(Delivery::Auto.resolve(&wsl_var), Delivery::Toast);
    assert_eq!(Delivery::Auto.resolve(&wsl_kernel), Delivery::Toast);
    assert_eq!(Delivery::Auto.resolve(&plain), Delivery::Bell);
    assert_eq!(Delivery::Auto.resolve(&kitty), Delivery::Osc9);
    // WSL wins over a terminal that has OSC 9: a toast reaches a terminal behind other windows.
    let both = env(&[("WSL_DISTRO_NAME", "Ubuntu"), ("WT_SESSION", "x")], "");
    assert_eq!(Delivery::Auto.resolve(&both), Delivery::Toast);
    // A named kind is itself.
    assert_eq!(Delivery::Bell.resolve(&wsl_var), Delivery::Bell);
    assert_eq!(Delivery::Toast.resolve(&plain), Delivery::Toast);
    assert_eq!(Delivery::parse("auto"), Some(Delivery::Auto));
    assert_eq!(Delivery::parse("toast"), Some(Delivery::Toast));
    assert_eq!(Delivery::parse("loud"), None);
}

#[test]
fn the_environment_is_read_at_the_first_ping_not_before() {
    let fake = Arc::new(Fake::default());
    let probes = Arc::new(Mutex::new(0));
    let p = probes.clone();
    let n = Notifier::with(
        Box::new(move || {
            *p.lock().unwrap() += 1;
            Env::default()
        }),
        fake,
        Arc::new(|_| {}),
        Arc::new(|_| {}),
    );
    assert_eq!(
        *probes.lock().unwrap(),
        0,
        "making a notifier probes nothing"
    );
    n.deliver(Delivery::Auto, "t", "x", true);
    n.deliver(Delivery::Auto, "t", "y", true);
    assert_eq!(*probes.lock().unwrap(), 1, "and the first ping probes once");
}

#[test]
fn a_toast_runs_a_fixed_script_with_the_text_as_safe_words() {
    let fake = Arc::new(Fake::default());
    let (n, _, rang) = notifier(&fake, true);
    let hostile = "Tide \"report\" `whoami` $(calc) '; Remove-Item *";
    let body = "● Tide asks: fs.write create \"a b\".txt `x` $(y)?";
    let bytes = n.deliver(Delivery::Auto, hostile, body, true);
    assert!(bytes.is_empty(), "a toast writes nothing to the terminal");
    until("the run", || !fake.runs.lock().unwrap().is_empty());
    let runs = fake.runs.lock().unwrap();
    let argv = &runs[0];
    assert_eq!(
        argv[..4],
        [
            "powershell.exe",
            "-NoProfile",
            "-NonInteractive",
            "-Command"
        ]
    );
    assert_eq!(
        argv[4], SCRIPT,
        "the script is the constant, whatever the text"
    );
    assert_eq!(argv.len(), 8, "the title, the body and the sound, apart");
    // The three words hold letters and digits alone: nothing a shell or a
    // PowerShell parser could read as more than a word.
    for w in &argv[5..] {
        assert!(w.chars().all(|c| c.is_ascii_alphanumeric()), "{w}");
        assert!(
            !w.chars().all(|c| c.is_ascii_digit()),
            "never a number: {w}"
        );
    }
    assert_eq!(decode(&argv[5]), hostile);
    assert_eq!(decode(&argv[6]), body);
    assert_eq!(decode(&argv[7]), "sound");
    assert!(rang.lock().unwrap().is_empty());
}

#[test]
fn a_ping_without_sound_asks_for_a_silent_toast_and_a_blank_title_is_theseus() {
    let fake = Arc::new(Fake::default());
    let (n, _, _) = notifier(&fake, true);
    n.deliver(Delivery::Toast, "  ", "Tide finished", false);
    until("the run", || !fake.runs.lock().unwrap().is_empty());
    let argv = fake.runs.lock().unwrap()[0].clone();
    assert_eq!(decode(&argv[5]), "Theseus");
    assert_eq!(decode(&argv[7]), "silent");
}

#[test]
fn control_characters_and_length_are_cut_from_the_words() {
    let t = Toast {
        title: "a\u{1b}[31mb\nc".into(),
        body: "é".repeat(1000),
        sound: true,
    };
    let argv = toast_argv(&t);
    assert_eq!(decode(&argv[5]), "a [31mb c");
    assert_eq!(decode(&argv[6]).chars().count(), 300);
}

#[test]
fn deliver_never_waits_on_the_toast_one_runs_and_a_newer_replaces_the_one_waiting() {
    let (gate_tx, gate_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let fake = Arc::new(Fake::default());
    *fake.gate.lock().unwrap() = Some(gate_rx);
    *fake.started.lock().unwrap() = Some(started_tx);
    let (n, _, _) = notifier(&fake, true);
    let began = std::time::Instant::now();
    for i in 1..=10 {
        n.deliver(Delivery::Toast, "Tide", &format!("notice {i}"), false);
    }
    // The spawner is parked on its gate, and all ten calls came back.
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "{:?}",
        began.elapsed()
    );
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(fake.runs.lock().unwrap().len(), 1, "one in flight");
    gate_tx.send(()).unwrap();
    gate_tx.send(()).unwrap();
    until("the queued one", || fake.runs.lock().unwrap().len() == 2);
    let runs = fake.runs.lock().unwrap();
    let bodies: Vec<String> = runs.iter().map(|a| decode(&a[6])).collect();
    assert_eq!(
        bodies,
        ["notice 1", "notice 10"],
        "the newest replaced the rest"
    );
}

#[test]
fn a_missing_powershell_rings_the_bell_once_and_says_so_once() {
    let fake = Arc::new(Fake {
        missing: true,
        ..Fake::default()
    });
    let (n, heard, rang) = notifier(&fake, true);
    n.deliver(Delivery::Toast, "Tide", "first", true);
    until("the fallback", || !rang.lock().unwrap().is_empty());
    until("it is known", || n.toaster.missing());
    assert_eq!(rang.lock().unwrap().as_slice(), [b"\x07".to_vec()]);
    assert_eq!(heard.lock().unwrap().len(), 1);
    assert!(heard.lock().unwrap()[0].contains("powershell.exe"));
    // After that a ping is the bell at once, with no second try and no second word.
    let bytes = n.deliver(Delivery::Toast, "Tide", "second", true);
    assert_eq!(bytes, b"\x07");
    assert_eq!(fake.runs.lock().unwrap().len(), 1, "no second try");
    assert_eq!(heard.lock().unwrap().len(), 1, "said once");
    let quiet = n.deliver(Delivery::Toast, "Tide", "third", false);
    assert!(quiet.is_empty(), "a ping without sound rings nothing");
}

#[test]
fn the_terminal_kinds_are_bytes_and_off_is_nothing() {
    let fake = Arc::new(Fake::default());
    let (n, _, _) = notifier(&fake, false);
    assert_eq!(n.deliver(Delivery::Auto, "T", "x", true), b"\x07");
    assert_eq!(
        n.deliver(Delivery::Osc9, "T", "a\x1bb", false),
        b"\x1b]9;theseus: ab\x07"
    );
    assert!(n.deliver(Delivery::Off, "T", "x", true).is_empty());
    assert!(fake.runs.lock().unwrap().is_empty(), "no toast outside WSL");
}

#[test]
fn every_kind_the_config_allows_parses_and_the_names_list_them() {
    for kind in theseus_protocol::notices::NOTIFY_KINDS {
        assert!(Delivery::parse(kind).is_some(), "{kind}");
        assert!(Delivery::NAMES.contains(kind), "{kind}");
    }
}
