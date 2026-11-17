//! The candidate: the config's text with named changes applied, its comments
//! and order kept (issue 22, line 1 step 2).
//!
//! The workspace has no format-preserving TOML editor, and this adds none: the
//! changes are made on the file's own text, at the places the `toml` crate's
//! spans name (a value to replace or a key to remove) or at the end of a
//! table's section (a key to add). **Then the result is checked against the
//! changes**: it is parsed again, and the tree it parses to must equal the
//! original tree with the same changes applied to it as data. A change the
//! text surgery got wrong, or a layout it does not understand (a dotted key, an
//! inline table), is therefore refused with words, never written as something
//! the owner did not see.

use serde_json::Value as Json;
use toml::de::{DeTable, DeValue};
use toml::Value;

/// One named change: a setting's path and its new value, or its removal.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Set(Vec<String>, Json),
    Remove(Vec<String>),
}

impl Change {
    pub fn path(&self) -> &[String] {
        match self {
            Change::Set(p, _) | Change::Remove(p) => p,
        }
    }
}

/// A setting's path as written, `self.budget.usd` or `policy.tools."proc.run"`:
/// bare keys and quoted keys, separated by dots.
pub fn parse_path(s: &str) -> Result<Vec<String>, String> {
    let bad = || {
        format!("{s:?} is not a setting's path: write keys joined by dots, such as self.budget.usd")
    };
    let mut out = Vec::new();
    let mut chars = s.trim().chars().peekable();
    loop {
        let mut key = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some('\\') => key.push(chars.next().ok_or_else(bad)?),
                    Some(c) => key.push(c),
                    None => return Err(bad()),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == '.' {
                    break;
                }
                key.push(c);
                chars.next();
            }
            if key.is_empty()
                || !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err(bad());
            }
        }
        out.push(key);
        match chars.next() {
            None => return Ok(out),
            Some('.') => {}
            Some(_) => return Err(bad()),
        }
    }
}

/// A path as the owner reads it: dotted, a key with anything odd quoted.
pub fn dotted(path: &[String]) -> String {
    path.iter()
        .map(|k| match bare(k) {
            true => k.clone(),
            false => Value::String(k.clone()).to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn bare(k: &str) -> bool {
    !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The text with every change made, or why it cannot be made faithfully.
pub fn apply(text: &str, changes: &[Change]) -> Result<String, String> {
    let original: Value =
        toml::from_str::<Value>(text).map_err(|e| format!("the file does not parse: {e}"))?;
    let mut expect = original;
    let mut out = text.to_string();
    for c in changes {
        out = match c {
            Change::Set(p, v) => {
                let v = to_toml(v).map_err(|e| format!("{}: {e}", dotted(p)))?;
                set_tree(&mut expect, p, v.clone())?;
                set_text(&out, p, &v)?
            }
            Change::Remove(p) => {
                remove_tree(&mut expect, p)?;
                remove_text(&out, p)?
            }
        };
    }
    let mut got: Value = toml::from_str(&out).map_err(|e| {
        format!("the edited file would not parse ({e}): edit this setting in the file by hand")
    })?;
    // An empty table is the same as none: a removal leaves a bare header.
    prune(&mut got);
    prune(&mut expect);
    if got != expect {
        return Err(
            "the change could not be made in the file's own layout without altering something \
             else: edit this setting in the file by hand"
                .into(),
        );
    }
    Ok(out)
}

fn prune(v: &mut Value) {
    if let Value::Table(t) = v {
        for (_, c) in t.iter_mut() {
            prune(c);
        }
        let empty: Vec<String> = t
            .iter()
            .filter(|(_, c)| matches!(c, Value::Table(c) if c.is_empty()))
            .map(|(k, _)| k.clone())
            .collect();
        for k in empty {
            t.remove(&k);
        }
    }
}

fn to_toml(v: &Json) -> Result<Value, String> {
    Value::try_from(v).map_err(|e| format!("that value cannot be written in TOML: {e}"))
}

fn set_tree(root: &mut Value, path: &[String], v: Value) -> Result<(), String> {
    let (last, parents) = path.split_last().ok_or("a change names no setting")?;
    let mut cur = root;
    for k in parents {
        let t = cur
            .as_table_mut()
            .ok_or_else(|| format!("{} is not a table", dotted(path)))?;
        cur = t
            .entry(k.clone())
            .or_insert_with(|| Value::Table(Default::default()));
    }
    let t = cur
        .as_table_mut()
        .ok_or_else(|| format!("{} is under something that is not a table", dotted(path)))?;
    t.insert(last.clone(), v);
    Ok(())
}

fn remove_tree(root: &mut Value, path: &[String]) -> Result<(), String> {
    let (last, parents) = path.split_last().ok_or("a change names no setting")?;
    let mut cur = root;
    for k in parents {
        cur = cur.get_mut(k.as_str()).ok_or_else(|| {
            format!(
                "{} is not set in the file, so there is nothing to remove",
                dotted(path)
            )
        })?;
    }
    cur.as_table_mut()
        .and_then(|t| t.remove(last))
        .map(drop)
        .ok_or_else(|| {
            format!(
                "{} is not set in the file, so there is nothing to remove",
                dotted(path)
            )
        })
}

/// Where a path is in the parsed text.
enum Found {
    /// A value that is not a table: the spans of its key and of itself.
    Leaf {
        key: std::ops::Range<usize>,
        value: std::ops::Range<usize>,
    },
    Table,
    /// Not there; how many of the path's keys are.
    Absent {
        present: usize,
    },
}

fn find(text: &str, path: &[String]) -> Result<Found, String> {
    let doc = DeTable::parse(text).map_err(|e| format!("the file does not parse: {e}"))?;
    let mut cur: &DeTable<'_> = doc.get_ref();
    for (i, seg) in path.iter().enumerate() {
        let hit = cur.iter().find(|(k, _)| {
            let name: &str = k.get_ref();
            name == seg
        });
        let Some((k, v)) = hit else {
            return Ok(Found::Absent { present: i });
        };
        let last = i + 1 == path.len();
        match v.get_ref() {
            DeValue::Table(t) if !last => cur = t,
            DeValue::Table(_) => return Ok(Found::Table),
            _ if last => {
                return Ok(Found::Leaf {
                    key: k.span(),
                    value: v.span(),
                })
            }
            _ => {
                return Err(format!(
                    "{} is under a value that is not a table",
                    dotted(path)
                ))
            }
        }
    }
    Err("a change names no setting".into())
}

fn key_text(k: &str) -> String {
    match bare(k) {
        true => k.to_string(),
        false => Value::String(k.to_string()).to_string(),
    }
}

fn set_text(text: &str, path: &[String], v: &Value) -> Result<String, String> {
    match find(text, path)? {
        Found::Leaf { value, .. } => {
            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..value.start]);
            out.push_str(&v.to_string());
            out.push_str(&text[value.end..]);
            Ok(out)
        }
        Found::Table => match v {
            Value::Table(_) => append_table(&remove_text(text, path)?, path, v),
            _ => Err(format!(
                "{} is a table of settings: name the setting inside it, or give a whole table",
                dotted(path)
            )),
        },
        Found::Absent { present } => match v {
            Value::Table(_) => append_table(text, path, v),
            _ => add_key(text, path, v, present),
        },
    }
}

/// A new table at `path`, at the end of the file.
fn append_table(text: &str, path: &[String], v: &Value) -> Result<String, String> {
    let mut wrapped = v.clone();
    for k in path.iter().rev() {
        let mut t = toml::Table::new();
        t.insert(k.clone(), wrapped);
        wrapped = Value::Table(t);
    }
    let body =
        toml::to_string(&wrapped).map_err(|e| format!("that table cannot be written: {e}"))?;
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&body);
    Ok(out)
}

/// A new key in a table the file already has, or a new table for it.
fn add_key(text: &str, path: &[String], v: &Value, present: usize) -> Result<String, String> {
    let (last, parent) = path.split_last().ok_or("a change names no setting")?;
    if present + 1 < path.len() {
        // The table it goes in is not there: a table of its own, at the end.
        return append_table(text, path, v);
    }
    let line = format!("{} = {}\n", key_text(last), v);
    let at = section_end(text, parent).ok_or_else(|| {
        format!(
            "the table [{}] is written in a form that cannot be added to here: edit this setting \
             in the file by hand",
            dotted(parent)
        )
    })?;
    let mut out = String::with_capacity(text.len() + line.len() + 1);
    out.push_str(&text[..at]);
    if at > 0 && !text[..at].ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&line);
    out.push_str(&text[at..]);
    Ok(out)
}

/// A header line's path: `[a.b]` or `[[a.b]]`, with its array-ness.
fn header(line: &str) -> Option<(bool, Vec<String>)> {
    let t = line.trim();
    let (array, rest) = match t.strip_prefix("[[") {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('[')?),
    };
    let mut in_quote = false;
    let mut esc = false;
    let mut end = None;
    for (i, c) in rest.char_indices() {
        match (c, in_quote, esc) {
            ('\\', true, false) => esc = true,
            ('"', _, false) => in_quote = !in_quote,
            (']', false, _) => {
                end = Some(i);
                break;
            }
            _ => esc = false,
        }
    }
    parse_path(&rest[..end?]).ok().map(|p| (array, p))
}

/// Byte offsets of each line and the line itself.
fn lines(text: &str) -> Vec<(usize, &str)> {
    let mut at = 0;
    text.split_inclusive('\n')
        .map(|l| {
            let o = at;
            at += l.len();
            (o, l)
        })
        .collect()
}

/// Where a new key of the table `parent` goes: after the last key or value
/// line of its section. `None` when the file has no header for it (it is
/// written as dotted keys or an inline table).
fn section_end(text: &str, parent: &[String]) -> Option<usize> {
    let ls = lines(text);
    let start = match parent.is_empty() {
        true => Some(0),
        false => ls
            .iter()
            .position(|(_, l)| header(l).is_some_and(|(a, p)| !a && p == parent))
            .map(|i| i + 1),
    }?;
    let mut end = ls.get(start).map_or(text.len(), |(o, _)| *o);
    for (o, l) in ls.iter().skip(start) {
        if header(l).is_some() {
            break;
        }
        let t = l.trim();
        if !t.is_empty() && !t.starts_with('#') {
            end = o + l.len();
        }
    }
    Some(end)
}

fn remove_text(text: &str, path: &[String]) -> Result<String, String> {
    match find(text, path)? {
        Found::Leaf { key, value } => {
            let start = text[..key.start].rfind('\n').map_or(0, |i| i + 1);
            let end = text[value.end..]
                .find('\n')
                .map_or(text.len(), |i| value.end + i + 1);
            let before = &text[start..key.start];
            let after = text[value.end..end].trim();
            if !before.trim().is_empty() || !(after.is_empty() || after.starts_with('#')) {
                return Err(format!(
                    "{} shares its line with something else: remove it in the file by hand",
                    dotted(path)
                ));
            }
            Ok(format!("{}{}", &text[..start], &text[end..]))
        }
        Found::Table => {
            let mut out = String::new();
            let mut removing = false;
            for (_, l) in lines(text) {
                if let Some((_, p)) = header(l) {
                    removing = p.starts_with(path);
                }
                if !removing {
                    out.push_str(l);
                }
            }
            Ok(out)
        }
        Found::Absent { .. } => Err(format!(
            "{} is not set in the file, so there is nothing to remove",
            dotted(path)
        )),
    }
}

#[cfg(test)]
mod tests;
