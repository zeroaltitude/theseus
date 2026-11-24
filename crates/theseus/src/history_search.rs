//! `theseus history` with a search (issue 44): `--when`, `--from`, `--to`, `--label`, `--words` and `--place`
//! read the history tool's one read (`history.read`), the lines the model and an MCP client read, with the
//! vault's values substituted by the daemon. With no place named, a search reads every place; `--before` takes
//! the cursor a page printed. With none of them, `theseus history` is a session's transcript, as before.

use std::io;

use anyhow::{anyhow, bail, Result};
use theseus_client::{render, resolve, Conn};
use theseus_protocol::memory::{HistoryCursor, HistoryReadParams, HistoryReadResult};
use theseus_protocol::method;

use crate::cmd::output;
use crate::print;

/// What a search names: the history tool's own arguments.
#[derive(Debug, Default, Clone)]
struct Search {
    when: Option<String>,
    from: Option<String>,
    to: Option<String>,
    label: Option<String>,
    words: Option<String>,
    place: Option<String>,
}

impl Search {
    /// Whether it names nothing: then `theseus history` reads a session's transcript.
    fn is_empty(&self) -> bool {
        self.flags().is_empty()
    }

    fn flags(&self) -> Vec<(&'static str, &str)> {
        let named = [
            ("--when", &self.when),
            ("--from", &self.from),
            ("--to", &self.to),
            ("--label", &self.label),
            ("--words", &self.words),
            ("--place", &self.place),
        ];
        (named.into_iter())
            .filter_map(|(k, v)| Some((k, v.as_deref()?)))
            .collect()
    }

    /// The command that reads this search again: `theseus history`, the session it named, and its flags.
    fn again(&self, session: Option<&str>) -> String {
        let mut out = String::from("theseus history");
        if let Some(s) = session {
            out.push(' ');
            out.push_str(&quoted(s));
        }
        for (k, v) in self.flags() {
            out.push_str(&format!(" {k} {}", quoted(v)));
        }
        out
    }
}

/// `v` as a shell word: as it is when it is plain, else in single quotes.
fn quoted(v: &str) -> String {
    let plain = !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.:/@+,".contains(c));
    match plain {
        true => v.to_string(),
        false => format!("'{}'", v.replace('\'', "'\\''")),
    }
}

/// A search's `--before`: the cursor a page printed, `<at_ms>:<turn id>`.
fn cursor(s: &str) -> Result<HistoryCursor> {
    let bad = || {
        anyhow!("--before {s:?}: a search's page takes the cursor its last line printed (<ms>:<turn id>)")
    };
    let (at, turn) = s.split_once(':').ok_or_else(bad)?;
    Ok(HistoryCursor {
        at_ms: at.parse().map_err(|_| bad())?,
        turn_id: turn.to_string(),
    })
}

/// `theseus history` with a search: one page of `history.read`, anchored at `session` when one is named (a bare
/// page's place; a search names every place unless `--place` names one).
async fn search(
    conn: &mut Conn,
    json: bool,
    session: Option<String>,
    s: Search,
    before: Option<String>,
) -> Result<()> {
    let session_id = match session {
        Some(given) => Some(resolve::session(conn, &given).await?),
        None => None,
    };
    let p = HistoryReadParams {
        place: s.place.clone(),
        session_id: session_id.clone(),
        label: s.label.clone(),
        words: s.words.clone(),
        before: before.as_deref().map(cursor).transpose()?,
        open: None,
        when: s.when.clone(),
        from: s.from.clone(),
        to: s.to.clone(),
        client: None,
    };
    let v = conn.request(method::HISTORY_READ, p).await?;
    let again = s.again(session_id.as_deref());
    output(json, v, |r: HistoryReadResult| {
        print::lines(
            &mut io::stdout().lock(),
            &render::history::read_lines(&r, &again, s.label.as_deref()),
        )?;
        Ok(())
    })
}

/// `theseus history`: a search when it names one, else a session's transcript (`cmd::history`), whose `--before`
/// is a position.
pub(crate) async fn cmd(conn: &mut Conn, json: bool, cmd: crate::Cmd) -> Result<()> {
    let crate::Cmd::History {
        session,
        n,
        after,
        before,
        full,
        when,
        from,
        to,
        label,
        words,
        place,
    } = cmd
    else {
        unreachable!("main.rs hands history_search::cmd only `theseus history`")
    };
    let s = Search {
        when,
        from,
        to,
        label,
        words,
        place,
    };
    if s.is_empty() {
        let before = position(before)?;
        return crate::cmd::history(conn, json, session, (n, after, before), full).await;
    }
    if n.is_some() || after.is_some() || full {
        bail!("-n, --after and --full read a session's transcript, not a search");
    }
    search(conn, json, session, s, before).await
}

/// `theseus history`'s session page's `--before`: a position, a number.
fn position(before: Option<String>) -> Result<Option<u64>> {
    match before {
        None => Ok(None),
        Some(b) => match b.parse() {
            Ok(n) => Ok(Some(n)),
            Err(_) => bail!(
                "--before {b:?}: a session's page takes a position (a number); a search's cursor goes with \
                 its search's flags (--when, --label, …)"
            ),
        },
    }
}
