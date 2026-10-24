//! `watch --interactive`'s threads (theseus-j0yh): `/t` lists the live
//! threads, `/t N` or `/t NAME` switches to one (`/t home` back), the status
//! line names the current thread, and a message sent in a thread carries its
//! tag. A switch moves the watch to the thread's session; a thread with no
//! session yet opens one at its first message (`turn.submit {thread}`), and
//! the watch moves there when the reply comes.
//!
//! A watch that never had a current thread (no `THESEUS_THREAD`, nothing
//! remembered for this terminal) sends no `thread.*` request at all.

use anyhow::Result;
use serde_json::json;
use theseus_client::current::{self, Pick};
use theseus_client::render::thread as words;
use theseus_client::render::time::fmt_hm;
use theseus_client::Conn;
use theseus_protocol::threads::{ThreadListParams, ThreadListResult, ThreadNameParams, ThreadRow};
use theseus_protocol::{
    method, Notification, SessionListResult, SessionWaitResult, TurnSubmitResult,
};

use super::{call, reread, Watch};
use crate::herdr::now_ms;

/// What a line is, when it addresses threads: `Some("")` for `/t` alone.
pub(super) fn command(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("/t")?;
    match rest.chars().next() {
        None => Some(""),
        Some(c) if c.is_whitespace() => Some(rest.trim()),
        Some(_) => None,
    }
}

/// The columns the status line may use.
fn width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(80)
}

/// The current thread at a start without a session named: the one this
/// terminal chose, read once. Its session, when it has one, is the one to
/// watch.
pub(super) async fn at_start(conn: &mut Conn, early: &mut Vec<Notification>) -> Option<ThreadRow> {
    let slug = current::of_terminal()?;
    let p = serde_json::to_value(ThreadNameParams {
        slug: slug.clone(),
        ..Default::default()
    })
    .ok()?;
    match call(conn, method::THREAD_SHOW, p, early).await {
        Ok(v) => serde_json::from_value(v).ok(),
        Err(e) => {
            eprintln!("theseus: the current thread `{slug}` is gone ({e}); watching without one");
            current::remember(None);
            None
        }
    }
}

impl Watch {
    /// The status line, on stderr.
    pub(super) fn print_status(&self) {
        let line = words::status_line(self.thread.as_ref(), now_ms(), width().saturating_sub(2));
        eprintln!("[{line}]");
    }

    /// The reply's tag, when the message goes to a thread.
    pub(super) fn print_tag(&self) {
        if let Some(t) = &self.thread {
            eprintln!("  {}", words::reply_tag(&t.thread.title));
        }
    }

    async fn list(&mut self, conn: &mut Conn) -> Result<()> {
        let mut early = Vec::new();
        let v = call(
            conn,
            method::THREAD_LIST,
            serde_json::to_value(ThreadListParams::default())?,
            &mut early,
        )
        .await?;
        for n in early {
            self.on_note(conn, n).await?;
        }
        self.listed = serde_json::from_value::<ThreadListResult>(v)?.threads;
        Ok(())
    }

    /// `/t`, `/t N`, `/t NAME`, `/t home`. A refusal is a line on stderr.
    pub(super) async fn thread_cmd(&mut self, conn: &mut Conn, arg: &str) -> Result<()> {
        self.printer.settle();
        if let Err(e) = self.list(conn).await {
            eprintln!("  threads: {e}");
            return Ok(());
        }
        if arg.is_empty() {
            let now = now_ms();
            let here = self.thread.as_ref().map(|t| t.thread.slug.as_str());
            if self.listed.is_empty() {
                eprintln!("  no live threads: `theseus thread new SLUG` makes one");
            }
            for (i, r) in self.listed.iter().enumerate() {
                let when = fmt_hm(r.thread.last_active_ms);
                let cur = here == Some(r.thread.slug.as_str());
                eprintln!("  {}", words::picker_line(i + 1, r, &when, now, cur));
            }
            eprintln!("  /t N or /t NAME switches; /t home goes back");
            self.print_status();
            return Ok(());
        }
        match current::pick(arg, &self.listed) {
            Err(e) => eprintln!("  {e}"),
            Ok(Pick::Home) => self.go_home(conn).await?,
            Ok(Pick::Thread(r)) => {
                let r = r.clone();
                self.go_thread(conn, r).await?;
            }
        }
        Ok(())
    }

    async fn go_thread(&mut self, conn: &mut Conn, r: ThreadRow) -> Result<()> {
        current::remember(Some(&r.thread.slug));
        match r.thread.session_id.clone() {
            Some(sid) => {
                self.thread = Some(r);
                self.open_thread = None;
                self.rebind(conn, sid).await?;
            }
            None => {
                eprintln!(
                    "  {} has no session yet: your next message opens it",
                    r.thread.title
                );
                self.open_thread = Some(r.thread.slug.clone());
                self.thread = Some(r);
            }
        }
        self.print_status();
        Ok(())
    }

    async fn go_home(&mut self, conn: &mut Conn) -> Result<()> {
        current::remember(None);
        self.thread = None;
        self.open_thread = None;
        if let Some(sid) = self.home_sid.clone() {
            self.rebind(conn, sid).await?;
        } else {
            // Started in a thread: home is the newest session no thread holds.
            let mut early = Vec::new();
            let v = call(conn, method::SESSION_LIST, json!({ "n": 50 }), &mut early).await?;
            let held: Vec<&str> = self
                .listed
                .iter()
                .filter_map(|r| r.thread.session_id.as_deref())
                .collect();
            let home = serde_json::from_value::<SessionListResult>(v)?
                .sessions
                .into_iter()
                .find(|s| !held.contains(&s.session_id.as_str()))
                .map(|s| s.session_id);
            match home {
                Some(sid) => {
                    self.home_sid = Some(sid.clone());
                    self.rebind(conn, sid).await?;
                }
                None => eprintln!("  there is no home session yet: your next message opens one"),
            }
        }
        self.print_status();
        Ok(())
    }

    /// Watch `sid` in place of the session watched, reading it whole as the
    /// start does.
    pub(super) async fn rebind(&mut self, conn: &mut Conn, sid: String) -> Result<()> {
        if sid == self.sid {
            return Ok(());
        }
        let mut early = Vec::new();
        let _ = call(
            conn,
            method::SESSION_UNWATCH,
            json!({ "session_id": self.sid }),
            &mut early,
        )
        .await;
        call(
            conn,
            method::SESSION_WATCH,
            json!({ "session_id": sid }),
            &mut early,
        )
        .await?;
        let read: SessionWaitResult = serde_json::from_value(
            call(conn, method::SESSION_WAIT, reread(&sid), &mut early).await?,
        )?;
        let info = serde_json::from_value::<SessionListResult>(
            call(
                conn,
                method::SESSION_LIST,
                json!({ "ids": [sid] }),
                &mut early,
            )
            .await?,
        )?
        .sessions
        .into_iter()
        .find(|s| s.session_id == sid);
        self.sid = sid;
        self.questions.clear();
        self.asked = None;
        self.resolved.clear();
        if let Some(i) = info {
            self.profile = i.profile;
            self.title_asked_at = i.turns;
            self.profile_read_at = i.turns;
        }
        self.apply_read(read);
        for n in early {
            self.on_note(conn, n).await?;
        }
        self.ask();
        Ok(())
    }

    /// What a response left to do that needs the connection: move to the
    /// session a thread's first message opened, and read the thread again for
    /// the status line.
    pub(super) async fn follow(&mut self, conn: &mut Conn) -> Result<()> {
        if let Some(sid) = self.rebind_to.take() {
            self.rebind(conn, sid).await?;
        }
        if std::mem::take(&mut self.status_due) {
            if let Some(slug) = self.thread.as_ref().map(|t| t.thread.slug.clone()) {
                let p = serde_json::to_value(ThreadNameParams {
                    slug,
                    ..Default::default()
                })?;
                let id = conn.send(method::THREAD_SHOW, p).await?;
                self.pending.insert(id, super::Purpose::ThreadRow);
            }
        }
        Ok(())
    }

    /// A turn the watch sent to open a thread's session has answered: the
    /// session it opened is the one to watch.
    pub(super) fn opened(&mut self, result: Option<serde_json::Value>) {
        if self.open_thread.take().is_none() {
            return;
        }
        if let Some(r) = result.and_then(|v| serde_json::from_value::<TurnSubmitResult>(v).ok()) {
            self.rebind_to = Some(r.session_id);
            self.status_due = true;
        }
    }

    /// The thread's row, read again: the status line shows it.
    pub(super) fn thread_row(&mut self, result: Option<serde_json::Value>) {
        if let Some(r) = result.and_then(|v| serde_json::from_value::<ThreadRow>(v).ok()) {
            self.thread = Some(r);
            self.print_status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slash_t_line_addresses_threads_and_nothing_else_does() {
        assert_eq!(command("/t"), Some(""));
        assert_eq!(command("  /t  "), Some(""));
        assert_eq!(command("/t 2"), Some("2"));
        assert_eq!(command("/t ex-ai pr"), Some("ex-ai pr"));
        assert_eq!(command("/tide"), None);
        assert_eq!(command("t 2"), None);
        assert_eq!(command("what is /t"), None);
    }
}
