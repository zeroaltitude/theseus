//! Health's line per terminal (theseus-n88g.4): a program on a pty that a
//! session drives with `term.*`; `HealthResult` is in lib.rs.

use serde::{Deserialize, Serialize};

/// One open terminal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TerminalInfo {
    /// Its id, as `term.open` gave it (`t3`).
    pub id: String,
    pub session_id: String,
    /// Its program, by its file name.
    pub program: String,
    pub pid: u32,
    pub rows: u16,
    pub cols: u16,
    pub opened_at_unix_ms: u64,
    /// Its program still runs.
    pub running: bool,
    /// The listed program that makes its screen outside text, when one does
    /// (`[policy] external_programs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub external: Option<String>,
    /// Bytes its program wrote, and bytes typed into it.
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub last_output_unix_ms: u64,
}

/// Health's line for a terminal: `terminal t1 (python3) · ses_… · pid 4242 ·
/// 24x80 · running · open 3 min · 1,204 B out, 40 B in`, and its listed
/// program when its screen is outside text.
pub fn health_line(t: &TerminalInfo, now_ms: u64) -> String {
    let open_s = now_ms.saturating_sub(t.opened_at_unix_ms) / 1000;
    let open = match open_s {
        s if s < 120 => format!("{s} s"),
        s if s < 7200 => format!("{} min", s / 60),
        s => format!("{} h", s / 3600),
    };
    format!(
        "terminal {} ({}) · {} · pid {} · {}x{} · {} · open {open} · {} B out, {} B in{}",
        t.id,
        t.program,
        t.session_id,
        t.pid,
        t.rows,
        t.cols,
        if t.running {
            "running"
        } else {
            "its program has ended"
        },
        t.bytes_out,
        t.bytes_in,
        t.external
            .as_deref()
            .map(|p| format!(" · outside text ({p} is listed)"))
            .unwrap_or_default(),
    )
}

/// A `term.*` call's line, from its input, for the tool lines on the CLI and
/// Discord: `python3 -q` (an open), `t1 "print(1)⏎" Ctrl-C` (a send), `t1
/// until ">>>"` (a read), `t1` (a close). None for any other tool.
pub fn summary(tool: &str, input: &serde_json::Value) -> Option<String> {
    let s = |k: &str| input.get(k).and_then(serde_json::Value::as_str);
    let words = |k: &str| -> Vec<String> {
        input
            .get(k)
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let quoted = |t: &str| {
        let shown: String = t.replace('\n', "⏎").chars().take(60).collect();
        let more = if t.chars().count() > 60 { "…" } else { "" };
        format!("{shown:?}{more}")
    };
    let id = s("terminal").unwrap_or("?");
    Some(match tool {
        "term.open" => words("argv").join(" "),
        "term.send" => {
            let mut parts = vec![id.to_string()];
            parts.extend(s("text").filter(|t| !t.is_empty()).map(quoted));
            parts.extend(words("keys"));
            parts.join(" ")
        }
        "term.read" => match (
            s("until"),
            input.get("quiet_ms").and_then(serde_json::Value::as_u64),
        ) {
            (Some(u), _) => format!("{id} until {}", quoted(u)),
            (None, Some(q)) => format!("{id} until quiet {q} ms"),
            _ => id.to_string(),
        },
        "term.close" => id.to_string(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_terminal_calls_line_names_its_terminal_and_keys() {
        let line = |t: &str, i| super::summary(t, &i).unwrap();
        assert_eq!(
            line("term.open", json!({"argv": ["python3", "-q"]})),
            "python3 -q"
        );
        assert_eq!(
            line(
                "term.send",
                json!({"terminal": "t1", "text": "print(1)\n", "keys": ["Ctrl-C"]})
            ),
            "t1 \"print(1)⏎\" Ctrl-C"
        );
        assert_eq!(
            line("term.read", json!({"terminal": "t2", "until": ">>>"})),
            "t2 until \">>>\""
        );
        assert_eq!(
            line("term.read", json!({"terminal": "t2", "quiet_ms": 200})),
            "t2 until quiet 200 ms"
        );
        assert_eq!(line("term.close", json!({"terminal": "t3"})), "t3");
        assert_eq!(super::summary("proc.run", &json!({})), None);
        let t = super::TerminalInfo {
            id: "t1".into(),
            session_id: "ses_a".into(),
            program: "python3".into(),
            pid: 4242,
            rows: 24,
            cols: 80,
            opened_at_unix_ms: 1_000,
            running: true,
            external: Some("gh".into()),
            bytes_out: 1204,
            bytes_in: 40,
            last_output_unix_ms: 2_000,
        };
        assert_eq!(
            super::health_line(&t, 181_000),
            "terminal t1 (python3) · ses_a · pid 4242 · 24x80 · running · open 3 min · 1204 B \
             out, 40 B in · outside text (gh is listed)"
        );
    }
}
