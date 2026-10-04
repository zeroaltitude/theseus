//! Named keys, as the bytes a terminal sends for them (an xterm's, in its
//! normal cursor mode).

/// The bytes for a named key, or None for a name it does not know. Names
/// are matched without regard to case: `Enter`, `Tab`, `Escape` (`Esc`),
/// `Backspace`, `Delete`, `Insert`, `Space`, `Up`, `Down`, `Left`, `Right`,
/// `Home`, `End`, `PageUp`, `PageDown`, `F1`–`F12`, and `Ctrl-<key>` (also
/// `C-<key>` and `^<key>`) for a letter or one of `@[\]^_ ?`.
pub fn bytes(name: &str) -> Option<Vec<u8>> {
    let lower = name.trim().to_ascii_lowercase();
    let fixed: &[u8] = match lower.as_str() {
        "enter" | "return" | "cr" => b"\r",
        "tab" => b"\t",
        "escape" | "esc" => b"\x1b",
        "backspace" | "bs" => b"\x7f",
        "delete" | "del" => b"\x1b[3~",
        "insert" | "ins" => b"\x1b[2~",
        "space" => b" ",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "pageup" | "pgup" => b"\x1b[5~",
        "pagedown" | "pgdn" => b"\x1b[6~",
        "f1" => b"\x1bOP",
        "f2" => b"\x1bOQ",
        "f3" => b"\x1bOR",
        "f4" => b"\x1bOS",
        "f5" => b"\x1b[15~",
        "f6" => b"\x1b[17~",
        "f7" => b"\x1b[18~",
        "f8" => b"\x1b[19~",
        "f9" => b"\x1b[20~",
        "f10" => b"\x1b[21~",
        "f11" => b"\x1b[23~",
        "f12" => b"\x1b[24~",
        _ => b"",
    };
    if !fixed.is_empty() {
        return Some(fixed.to_vec());
    }
    let key = ["ctrl-", "ctrl+", "c-", "^"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))?;
    let mut cs = key.chars();
    let (Some(c), None) = (cs.next(), cs.next()) else {
        return None;
    };
    match c {
        'a'..='z' => Some(vec![c as u8 - b'a' + 1]),
        '@' | ' ' => Some(vec![0]),
        '[' => Some(vec![0x1b]),
        '\\' => Some(vec![0x1c]),
        ']' => Some(vec![0x1d]),
        '^' => Some(vec![0x1e]),
        '_' => Some(vec![0x1f]),
        '?' => Some(vec![0x7f]),
        _ => None,
    }
}

/// Text as typed: each newline is the Enter key (CR), as a terminal sends
/// it; the line discipline makes it a newline for a program that reads
/// lines.
pub fn text(t: &str) -> Vec<u8> {
    t.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
}
