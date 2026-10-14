//! `theseus status --watch --notify HOW` (theseus-f0bi9): pings from the
//! watch, by the same notify module the TUI uses. Off by default: a watch runs
//! in a status bar, whose stdout is the bar's text, so a ping never goes to
//! stdout. A bell or OSC sequence goes to the controlling terminal, and a
//! toast is a Windows process the worker starts.
//!
//! The watch keeps each execution's latest view and its session's name (one
//! `session.list {ids}` the first time a session moves), feeds `Notices`, and
//! wakes for the soonest notice that is due: the only timer a watch has, and
//! only with `--notify`.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use theseus_client::notify::{Delivery, Notices, Notifier};
use theseus_client::{names, Conn};
use theseus_protocol::notices::{self, Viewer};
use theseus_protocol::{method, ExecutionView, SessionListParams, SessionListResult};

pub struct Alerts {
    how: Delivery,
    notices: Notices,
    notifier: Notifier,
    tty: Option<std::fs::File>,
    views: HashMap<String, ExecutionView>,
    names: HashMap<String, String>,
}

impl Alerts {
    /// `None` for `off`: a watch that is not asked to ping holds none of this.
    pub fn new(how: Delivery) -> Option<Self> {
        if how == Delivery::Off {
            return None;
        }
        let tty = || {
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/tty")
                .ok()
        };
        let rings = tty();
        let notifier = Notifier::new(Arc::new(move |b| {
            if let Some(mut t) = rings.as_ref() {
                let _ = t.write_all(b);
            }
        }));
        Some(Self::with(how, notifier, tty()))
    }

    /// The same over a notifier and a terminal of the caller's (tests).
    pub fn with(how: Delivery, notifier: Notifier, tty: Option<std::fs::File>) -> Self {
        Self {
            how,
            notices: Notices::default(),
            notifier,
            tty,
            views: HashMap::new(),
            names: HashMap::new(),
        }
    }

    /// The board read from a snapshot: remembered, with no notice.
    pub fn saw(&mut self, snap: Vec<ExecutionView>) {
        self.views.clear();
        for v in snap {
            let name = self.names.get(&v.session_id).cloned().unwrap_or_default();
            self.notices.saw(&v, &name);
            self.views.insert(v.execution_id.clone(), v);
        }
    }

    /// Whether a name for this view's session is still to be read.
    pub fn needs_name(&self, v: &ExecutionView) -> bool {
        !self.names.contains_key(&v.session_id)
    }

    pub fn name(&mut self, sid: &str, name: String) {
        self.names.insert(sid.to_string(), name);
    }

    /// A view moved live.
    pub fn moved(&mut self, v: ExecutionView, now_ms: u64, hm: &dyn Fn(u64) -> String) {
        let name = self.names.get(&v.session_id).cloned().unwrap_or_default();
        self.notices.moved(&v, &name, now_ms, hm);
        self.views.insert(v.execution_id.clone(), v);
    }

    /// The soonest time a notice is due, in ms since the epoch.
    pub fn next_due(&self) -> Option<u64> {
        self.notices.next_due()
    }

    /// Carry out the notices due by `now_ms`.
    pub fn fire(
        &mut self,
        now_ms: u64,
        displayed: &HashMap<String, u64>,
        hm: &dyn Fn(u64) -> String,
    ) {
        let by_session = |sid: &str| {
            self.views
                .values()
                .filter(|v| v.session_id == sid)
                .max_by_key(|v| v.position)
                .cloned()
        };
        let viewer = |sid: &str| Viewer {
            focused: None,
            seen: by_session(sid)
                .map_or(0, |v| displayed.get(&v.execution_id).copied().unwrap_or(0)),
            away_ms: 0,
            quiet: false,
            sound: true,
        };
        let fired = self.notices.take_due(now_ms, &by_session, &viewer, hm);
        for f in fired {
            if let notices::Delivery::Ping { sound } = f.delivery {
                let bytes = self.notifier.deliver(self.how, &f.title, &f.line, sound);
                if let (Some(tty), false) = (self.tty.as_mut(), bytes.is_empty()) {
                    let _ = tty.write_all(&bytes);
                }
            }
        }
    }
}

/// A session's name, read once: `session.list {ids}`. Notifications that
/// arrive first are handed to `early`, to be applied after.
pub async fn read_name(
    conn: &mut Conn,
    sid: &str,
    early: &mut Vec<(String, Value)>,
) -> Result<String> {
    let params = serde_json::to_value(SessionListParams {
        ids: Some(vec![sid.to_string()]),
        ..Default::default()
    })?;
    let v = conn
        .call(method::SESSION_LIST, params, |m, p| {
            early.push((m.to_string(), p.clone()));
        })
        .await?;
    let r: SessionListResult = serde_json::from_value(v)?;
    Ok(r.sessions
        .iter()
        .find(|s| names::words(s.kind, s.label.as_deref(), s.title.as_deref()).is_some())
        .map(names::of)
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use theseus_client::notify::{Env, Spawner};

    #[derive(Default)]
    struct Argvs(Mutex<Vec<Vec<String>>>);

    impl Spawner for Argvs {
        fn run(&self, argv: &[String]) -> std::io::Result<()> {
            self.0.lock().unwrap().push(argv.to_vec());
            Ok(())
        }
    }

    fn unword(w: &str) -> String {
        let hex = &w[1..];
        let bytes: Vec<u8> = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        String::from_utf8(bytes).unwrap()
    }

    fn asking(position: u64, at: u64) -> ExecutionView {
        serde_json::from_value(serde_json::json!({
            "position": position, "at_ms": at,
            "execution_id": "exe_tide", "session_id": "ses_tide", "kind": "task",
            "state": "waiting", "turns": 1,
            "pending": [{"correlation_id": "act_tide", "tool": "fs.write",
                         "reason": "write regatta/paint-log.md"}],
            "attention": {"level": "needs_you", "label": "confirm", "since_ms": at},
        }))
        .unwrap()
    }

    /// A watch with `--notify toast` toasts a question that stays, titled by
    /// the task, once its second has passed; and nothing before.
    #[test]
    fn a_question_in_a_watch_is_a_toast_after_its_second() {
        let spawner = Arc::new(Argvs::default());
        let notifier = Notifier::with(
            Box::new(Env::default),
            spawner.clone(),
            Arc::new(|_| {}),
            Arc::new(|_| {}),
        );
        let mut a = Alerts::with(Delivery::Toast, notifier, None);
        let t0 = 1_759_300_000_000;
        let hm = theseus_protocol::utc_hm;
        a.saw(vec![]);
        a.name("ses_tide", "Tide report".into());
        a.moved(asking(5, t0), t0, &hm);
        a.fire(t0 + 500, &HashMap::new(), &hm);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            spawner.0.lock().unwrap().is_empty(),
            "not before its second"
        );
        assert_eq!(a.next_due(), Some(t0 + 1_000));
        a.fire(t0 + 1_000, &HashMap::new(), &hm);
        for _ in 0..200 {
            if !spawner.0.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let runs = spawner.0.lock().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(unword(&runs[0][5]), "Tide report");
        assert!(unword(&runs[0][6]).starts_with("● Tide report asks: fs.write"));
        assert_eq!(unword(&runs[0][7]), "sound");
    }

    #[test]
    fn off_holds_no_alerts() {
        assert!(Alerts::new(Delivery::Off).is_none());
    }
}
