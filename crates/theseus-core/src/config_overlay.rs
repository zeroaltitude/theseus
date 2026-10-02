//! The operator's overlay on the config template (theseus-dxgb).
//!
//! The template (`Config::EXAMPLE_TOML`, which `theseusd example-config`
//! prints) is public, so it carries placeholders where a deployment's own
//! values go: its vault's references and its people's ids (theseus-8d1b). An
//! operator who keeps a deployment as a copy of the template keeps those
//! values in an overlay, a private TOML document of only what differs, at
//! `~/.config/theseus/template-overlay.toml` (or any file `--overlay` names),
//! and `theseusd example-config` prints the template with it in place.
//!
//! Every line of the template is kept as it was but those the overlay sets.
//! For each key the overlay sets, in its table:
//! - a live line for that key gets the overlay's value, and keeps its
//!   trailing comment at the column it had;
//! - else the first commented line for it (`# key = …`) is switched on with
//!   the overlay's value, and so is its table's header when that is
//!   commented too (`# [approval]`);
//! - else the key is added after the table's last key line, and a table the
//!   template does not have at all is added at the end.
//!
//! The result is parsed and validated as a config before it is returned, so
//! a misspelt key fails here, named, and not at the daemon's next start.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};

use crate::Config;

/// Where `theseusd example-config` finds the operator's overlay when no
/// `--overlay` names one: with a file here it prints the template with it in
/// place, and without one, the template alone (`--plain` forces that).
pub const DEFAULT_PATH: &str = "~/.config/theseus/template-overlay.toml";

/// A table's path (`["policy", "aws"]`) and the values the overlay sets in it.
type Wanted = BTreeMap<Vec<String>, BTreeMap<String, toml::Value>>;

/// One table of the template: its path, its header's line (None for the lines
/// before the first header), whether that header is commented, and its lines.
struct Section {
    path: Vec<String>,
    header: Option<usize>,
    commented: bool,
    lines: std::ops::Range<usize>,
}

/// The template with the overlay's values in place, checked to load as a
/// config.
pub fn render(template: &str, overlay: &str) -> Result<String> {
    let text = render_lines(template, overlay)?;
    Config::parse(&text).context("the template with the overlay does not load as a config")?;
    Ok(text)
}

/// `render` without the config's own checks: the line-by-line work, and a
/// check that every value the overlay sets is where it sets it.
fn render_lines(template: &str, overlay: &str) -> Result<String> {
    let doc: toml::Table = overlay.parse().context("parsing the overlay as TOML")?;
    let mut wanted = Wanted::new();
    flatten(&doc, &mut Vec::new(), &mut wanted)?;
    let mut lines: Vec<String> = template.lines().map(str::to_string).collect();
    let sections = sections(&lines);
    let owners = owners(&sections, lines.len());
    // Insertions, by the line they follow, applied last to last so the
    // indices stay true; and tables the template lacks, for the end.
    let mut after: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut tail: Vec<String> = Vec::new();
    for (path, keys) in &wanted {
        let Some(s) = pick(&sections, path) else {
            tail.push(String::new());
            tail.push(format!("[{}]", dotted(path)));
            tail.extend(keys.iter().map(|(k, v)| format!("{} = {v}", key(k))));
            continue;
        };
        let last_key = s
            .lines
            .clone()
            .rev()
            .find(|&i| key_line(&lines[i]).is_some())
            .or(s.header);
        for (k, v) in keys {
            // A live line is the table's in TOML's terms (under its last live
            // header); a commented one, the picked section's own.
            let live = (0..lines.len()).find(|&i| {
                sections[owners[i]].path == *path
                    && matches!(key_line(&lines[i]), Some((ref n, false)) if n == k)
            });
            let commented = || {
                s.lines
                    .clone()
                    .find(|&i| matches!(key_line(&lines[i]), Some((ref n, true)) if n == k))
            };
            match live.or_else(commented) {
                Some(i) => lines[i] = set_value(&lines[i], k, v),
                None => {
                    let at = last_key.unwrap_or_else(|| s.lines.start.saturating_sub(1));
                    after
                        .entry(at)
                        .or_default()
                        .push(format!("{} = {v}", key(k)));
                }
            }
        }
        if let (true, Some(h)) = (s.commented, s.header) {
            lines[h] = uncomment(&lines[h]);
        }
    }
    for (at, add) in after.into_iter().rev() {
        let at = (at + 1).min(lines.len());
        lines.splice(at..at, add);
    }
    lines.extend(tail);
    let mut text = lines.join("\n");
    if template.ends_with('\n') {
        text.push('\n');
    }
    landed(&text, &wanted)?;
    Ok(text)
}

/// For each line, the section whose table it belongs to in TOML's terms: the
/// last live header's (a commented header changes nothing for TOML).
fn owners(sections: &[Section], n: usize) -> Vec<usize> {
    let mut out = vec![0; n];
    let mut live = 0;
    for (si, s) in sections.iter().enumerate() {
        if !s.commented {
            live = si;
        }
        if let Some(h) = s.header {
            out[h] = live;
        }
        for i in s.lines.clone() {
            out[i] = live;
        }
    }
    out
}

/// The overlay's values by table, every table at any depth; an array of
/// tables has no place in a line-by-line render.
fn flatten(t: &toml::Table, path: &mut Vec<String>, out: &mut Wanted) -> Result<()> {
    for (k, v) in t {
        match v {
            toml::Value::Table(sub) => {
                path.push(k.clone());
                flatten(sub, path, out)?;
                path.pop();
            }
            toml::Value::Array(a) if !a.is_empty() && a.iter().all(toml::Value::is_table) => {
                bail!(
                    "the overlay sets [[{}]], an array of tables: put it in the config itself",
                    dotted(&[path.as_slice(), std::slice::from_ref(k)].concat())
                )
            }
            _ => {
                out.entry(path.clone())
                    .or_default()
                    .insert(k.clone(), v.clone());
            }
        }
    }
    Ok(())
}

/// The template's tables, in order, each from its header (live or commented)
/// to the next header.
fn sections(lines: &[String]) -> Vec<Section> {
    let mut out = Vec::new();
    let mut cur = Section {
        path: Vec::new(),
        header: None,
        commented: false,
        lines: 0..0,
    };
    for (i, l) in lines.iter().enumerate() {
        if let Some((path, commented)) = header(l) {
            cur.lines.end = i;
            out.push(cur);
            cur = Section {
                path,
                header: Some(i),
                commented,
                lines: i + 1..i + 1,
            };
        }
    }
    cur.lines.end = lines.len();
    out.push(cur);
    out
}

/// The section for `path`: its live table when the template has one, else
/// its first commented one.
fn pick<'a>(sections: &'a [Section], path: &[String]) -> Option<&'a Section> {
    let mut with = sections.iter().filter(|s| s.path == path);
    let first = with.next()?;
    if !first.commented {
        return Some(first);
    }
    Some(with.find(|s| !s.commented).unwrap_or(first))
}

/// A table header's path, and whether it is commented: `[a.b]` or `# [a.b]`,
/// with nothing after it but a comment. Prose that starts with a bracket
/// (`# [tools].approve_paths, …`) is not a header.
fn header(line: &str) -> Option<(Vec<String>, bool)> {
    let (body, commented) = match line.strip_prefix('#') {
        Some(rest) => (rest.trim_start(), true),
        None => (line.trim_start(), false),
    };
    let inner = body.strip_prefix('[')?;
    if inner.starts_with('[') {
        return None;
    }
    let close = inner.find(']')?;
    let rest = inner[close + 1..].trim_start();
    if !(rest.is_empty() || rest.starts_with('#')) {
        return None;
    }
    let doc: toml::Table = format!("[{}]\n", &inner[..close]).parse().ok()?;
    let mut path = Vec::new();
    let mut t = &doc;
    while let Some((k, toml::Value::Table(sub))) = t.iter().next() {
        path.push(k.clone());
        t = sub;
    }
    (!path.is_empty()).then_some((path, commented))
}

/// A key line's key, and whether it is commented: `key = value` or
/// `# key = value`, with nothing after the value but a comment, the value
/// being one TOML value. Prose that happens to hold an `=` is not one.
fn key_line(line: &str) -> Option<(String, bool)> {
    let (body, commented) = match line.strip_prefix("# ") {
        Some(rest) => (rest, true),
        None if line.starts_with('#') => return None,
        None => (line, false),
    };
    let (name, value) = body.split_once('=')?;
    let name = name.trim();
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let quoted = name.len() >= 2 && name.starts_with('"') && name.ends_with('"');
    if !(bare || quoted) {
        return None;
    }
    let end = value_end(value)?;
    let rest = value[end..].trim_start();
    if !(rest.is_empty() || rest.starts_with('#')) {
        return None;
    }
    let doc: toml::Table = format!("{name} = {}\n", value[..end].trim()).parse().ok()?;
    let (k, _) = doc.into_iter().next()?;
    Some((k, commented))
}

/// Where the TOML value that starts `text` (after its `=`) ends: one string,
/// array, inline table, or bare word. None for a multi-line string, which
/// never sits on one line of the template.
fn value_end(text: &str) -> Option<usize> {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    let start = i;
    let mut depth = 0usize;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => {
                let q = b[i];
                if text[i..].starts_with("\"\"\"") || text[i..].starts_with("'''") {
                    return None;
                }
                i += 1;
                while i < b.len() && b[i] != q {
                    if q == b'"' && b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
                if depth == 0 {
                    return Some(i.min(b.len()));
                }
            }
            b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b']' | b'}' => {
                depth = depth.checked_sub(1)?;
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b' ' | b'\t' | b'#' if depth == 0 => break,
            _ => i += 1,
        }
    }
    (i > start && depth == 0).then_some(i)
}

/// `line` (a key line, live or commented) with `value` in place and on, its
/// trailing comment kept at the column it had.
fn set_value(line: &str, k: &str, value: &toml::Value) -> String {
    let set = format!("{} = {value}", key(k));
    let body = line.strip_prefix("# ").unwrap_or(line);
    let offset = line.len() - body.len();
    let comment = body
        .split_once('=')
        .and_then(|(_, v)| value_end(v).map(|end| (v, end)))
        .and_then(|(v, end)| {
            let rest = &v[end..];
            let at = rest.find('#')?;
            rest[..at]
                .trim()
                .is_empty()
                .then(|| (line.len() - rest.len() + at, &rest[at..]))
        });
    match comment {
        None => set,
        Some((column, text)) => {
            let column = column.max(offset);
            let pad = if set.len() < column {
                column - set.len()
            } else {
                3
            };
            format!("{set}{}{text}", " ".repeat(pad))
        }
    }
}

/// A commented header switched on: `# [approval]` is `[approval]`.
fn uncomment(line: &str) -> String {
    line.strip_prefix('#')
        .map_or_else(|| line.to_string(), |rest| rest.trim_start().to_string())
}

/// A key as TOML writes it: bare when it can be, else quoted.
fn key(k: &str) -> String {
    let bare = !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        k.to_string()
    } else {
        toml::Value::String(k.to_string()).to_string()
    }
}

/// A table's path as its header writes it: `catalog."claude-fable-5"`.
fn dotted(path: &[String]) -> String {
    path.iter().map(|k| key(k)).collect::<Vec<_>>().join(".")
}

/// The rendered text holds every value the overlay sets, where it sets it.
fn landed(text: &str, wanted: &Wanted) -> Result<()> {
    let doc: toml::Table = text.parse().context("parsing the rendered template")?;
    for (path, keys) in wanted {
        let mut t = &doc;
        for p in path {
            match t.get(p) {
                Some(toml::Value::Table(sub)) => t = sub,
                _ => bail!("the rendered template has no [{}]", dotted(path)),
            }
        }
        for (k, v) in keys {
            if t.get(k) != Some(v) {
                bail!(
                    "the rendered template does not hold the overlay's {}.{}",
                    dotted(path),
                    key(k)
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
