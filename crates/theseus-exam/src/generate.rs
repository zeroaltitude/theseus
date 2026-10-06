//! Generated pasts (exam-v2): many sessions from templates and seeds, with
//! dates spread over months, so an exam can hold hundreds of sessions and
//! many near-duplicates of its gold without writing each by hand.
//!
//! A block is a template session and the number of sessions to make from it:
//!
//! ```toml
//! [[item.generate]]            # or [[background]]: sessions no item owns
//! key = "d"                    # keys d01, d02, … (zero-padded to the count)
//! count = 16
//! seed = 7
//! from = "2026-03-02 09:00"    # local times, at the exam's offset
//! to = "2026-09-20 18:00"
//! places = ["discord DM", "cli"]
//! [item.generate.vars]         # each session draws one value of each
//! box = ["canary", "perf"]
//! [[item.generate.node]]
//! who = "zeroaltitude"
//! text = "Is the replay worker on {box} still up?"
//! [[item.generate.node]]
//! who = "theseus"
//! after_min = 2                # minutes after the node before it (default 1)
//! text = "Yes, {box}'s replay worker answers."
//! ```
//!
//! - **Dates** are stratified: the span is cut into `count` equal slots and
//!   session i starts at a seeded minute inside slot i, so the sessions spread
//!   over the whole span, in order.
//! - **Variables**: `{name}` is replaced by the session's draw of `name`
//!   (from the block's `vars`, then the exam's `[vars]`); `{i}` is the
//!   session's index from 1 and `{date}` its local date. Only `{` + a name +
//!   `}` is a placeholder, so a tool's JSON input keeps its braces. A name
//!   with no values is an error that names it.
//! - **Draws** come in a fixed order per session (the slot's minute, the place,
//!   then every variable by name), from SplitMix64 seeded by `seed`, so the
//!   expansion is a function of the exam file alone, and its digest covers it.
//!
//! What a block makes is ordinary `PastSession`s, validated and written like
//! hand-written ones: generation adds no kind and no schema.

use std::collections::BTreeMap;

use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;

use crate::item::{PastNode, PastSession};
use crate::rng::Rng;
use crate::time::{format_local, parse_local};

/// A variable's name and the values a session draws one of.
pub type Vars = BTreeMap<String, Vec<String>>;

/// The placeholders every block has.
pub const BUILT_IN: [&str; 2] = ["i", "date"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generate {
    /// The sessions' key prefix: lower-case letters, digits, and `-`.
    pub key: String,
    pub count: usize,
    pub seed: u64,
    pub from: String,
    pub to: String,
    pub places: Vec<String>,
    #[serde(default)]
    pub vars: Vars,
    #[serde(rename = "node")]
    pub nodes: Vec<TemplateNode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateNode {
    /// Minutes after the node before it; ignored on the first node.
    #[serde(default = "one")]
    pub after_min: u32,
    pub who: String,
    pub text: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub input: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

fn one() -> u32 {
    1
}

/// `text` with each `{name}` replaced by `vals[name]`; an unknown name is an
/// error.
pub fn fill(text: &str, vals: &BTreeMap<&str, String>) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let name_len = after
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
            .unwrap_or(after.len());
        let name = &after[..name_len];
        let is_placeholder = !name.is_empty()
            && after[name_len..].starts_with('}')
            && name.starts_with(|c: char| c.is_ascii_lowercase());
        if is_placeholder {
            let Some(v) = vals.get(name) else {
                bail!("{{{name}}} names no variable");
            };
            out.push_str(v);
            rest = &after[name_len + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    Ok(out)
}

impl Generate {
    /// The block's sessions. `shared` is the exam's `[vars]`, under the
    /// block's own.
    pub fn expand(&self, shared: &Vars, offset_min: i32) -> Result<Vec<PastSession>> {
        let at = || format!("generated block {:?}", self.key);
        ensure!(
            !self.key.is_empty()
                && self
                    .key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{}: a key prefix is lower-case letters, digits, and -",
            at()
        );
        ensure!(self.count > 0, "{}: count is 0", at());
        ensure!(!self.places.is_empty(), "{}: no places", at());
        ensure!(!self.nodes.is_empty(), "{}: no nodes", at());
        let from = parse_local(&self.from, offset_min).with_context(at)?;
        let to = parse_local(&self.to, offset_min).with_context(at)?;
        ensure!(from < to, "{}: `from` is not before `to`", at());
        let span_min = (to - from) / 60_000;
        let slot = span_min / self.count as u64;
        ensure!(
            slot >= 1,
            "{}: {} sessions do not fit in {span_min} minutes",
            at(),
            self.count
        );
        let mut vars: BTreeMap<&str, &Vec<String>> = BTreeMap::new();
        for (k, v) in shared.iter().chain(&self.vars) {
            ensure!(
                !BUILT_IN.contains(&k.as_str()),
                "{}: {{{k}}} is built in",
                at()
            );
            ensure!(!v.is_empty(), "{}: variable {k} has no values", at());
            vars.insert(k, v);
        }
        let width = self.count.to_string().len().max(2);
        let mut rng = Rng::new(self.seed);
        let mut out = Vec::with_capacity(self.count);
        for i in 0..self.count {
            let start = from + (i as u64 * slot + rng.below(slot)) * 60_000;
            let place = self.places[rng.below(self.places.len() as u64) as usize].clone();
            let mut vals: BTreeMap<&str, String> = vars
                .iter()
                .map(|(k, v)| (*k, v[rng.below(v.len() as u64) as usize].clone()))
                .collect();
            vals.insert("i", (i + 1).to_string());
            vals.insert("date", format_local(start, offset_min)[..10].to_string());
            let key = format!("{}{:0width$}", self.key, i + 1);
            let mut t = start;
            let mut nodes = Vec::with_capacity(self.nodes.len());
            for (k, n) in self.nodes.iter().enumerate() {
                if k > 0 {
                    t += u64::from(n.after_min) * 60_000;
                }
                let f =
                    |s: &str| fill(s, &vals).with_context(|| format!("{}, node {}", at(), k + 1));
                nodes.push(PastNode {
                    at: format_local(t, offset_min),
                    who: n.who.clone(),
                    text: f(&n.text)?,
                    tool: n.tool.clone(),
                    input: n.input.as_deref().map(f).transpose()?,
                    status: n.status.clone(),
                    external: None,
                    volatile: false,
                });
            }
            out.push(PastSession { key, place, nodes });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MST: i32 = -420;

    fn block(src: &str) -> Generate {
        toml::from_str(src).unwrap()
    }

    const B: &str = r#"
key = "d"
count = 12
seed = 7
from = "2026-03-01 00:00"
to = "2026-09-01 00:00"
places = ["discord DM", "cli"]
[vars]
box = ["canary", "perf", "qa"]
[[node]]
who = "zeroaltitude"
text = "Is the replay worker on {box} up? (#{i}, {date})"
[[node]]
who = "tool"
after_min = 3
tool = "proc.run"
input = '{"argv": ["probe", "{box}"]}'
text = "{box}: up"
"#;

    /// The same block makes the same sessions; another seed makes others.
    #[test]
    fn expansion_is_a_function_of_the_block() {
        let a = block(B).expand(&Vars::new(), MST).unwrap();
        let b = block(B).expand(&Vars::new(), MST).unwrap();
        let fmt = |v: &[PastSession]| format!("{v:?}");
        assert_eq!(fmt(&a), fmt(&b));
        let c = block(&B.replace("seed = 7", "seed = 8"))
            .expand(&Vars::new(), MST)
            .unwrap();
        assert_ne!(fmt(&a), fmt(&c));
        // Keys are zero-padded and unique; every draw is one of the values.
        let keys: Vec<&str> = a.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys[0], "d01");
        assert_eq!(keys[11], "d12");
        for s in &a {
            assert!(["discord DM", "cli"].contains(&s.place.as_str()));
            let t = &s.nodes[0].text;
            assert!(
                ["canary", "perf", "qa"]
                    .iter()
                    .any(|b| t.starts_with(&format!("Is the replay worker on {b} up?"))),
                "{t}"
            );
            // A session's variable is one draw: the tool's input and result agree.
            let b = t.split(' ').nth(5).unwrap();
            assert_eq!(s.nodes[1].text, format!("{b}: up"));
            assert_eq!(
                s.nodes[1].input.as_deref(),
                Some(format!("{{\"argv\": [\"probe\", \"{b}\"]}}").as_str())
            );
        }
        // Over 12 sessions, more than one box and more than one place.
        let boxes: std::collections::BTreeSet<&str> =
            a.iter().map(|s| s.nodes[1].text.as_str()).collect();
        assert!(boxes.len() > 1);
    }

    /// Stratified dates: session i starts inside the i-th of `count` equal
    /// slots of the span, so the sessions are in order and spread over all of
    /// it; each node follows the one before by its `after_min`.
    #[test]
    fn dates_spread_over_the_span_in_order() {
        let g = block(B);
        let s = g.expand(&Vars::new(), MST).unwrap();
        let from = parse_local(&g.from, MST).unwrap();
        let to = parse_local(&g.to, MST).unwrap();
        let slot = (to - from) / 12;
        for (i, x) in s.iter().enumerate() {
            let t0 = parse_local(&x.nodes[0].at, MST).unwrap();
            let t1 = parse_local(&x.nodes[1].at, MST).unwrap();
            assert!(
                t0 >= from + i as u64 * slot && t0 < from + (i as u64 + 1) * slot,
                "{i}: {}",
                x.nodes[0].at
            );
            assert_eq!(t1 - t0, 3 * 60_000);
            assert!(x.nodes[0]
                .text
                .ends_with(&format!("(#{}, {})", i + 1, &x.nodes[0].at[..10])));
        }
        // March to September: the span is months, and the sessions use it.
        let first = &s[0].nodes[0].at;
        let last = &s[11].nodes[0].at;
        assert!(
            first.starts_with("2026-03") && last.starts_with("2026-08"),
            "{first} {last}"
        );
    }

    #[test]
    fn placeholders_are_names_in_braces_and_nothing_else() {
        let vals: BTreeMap<&str, String> = [("box", "qa".to_string())].into_iter().collect();
        assert_eq!(fill("{box}/{box}", &vals).unwrap(), "qa/qa");
        // JSON's braces, a capital anywhere in the name, a space: all literal.
        assert_eq!(
            fill("{\"a\": {}} {Box} {bOx} { box}", &vals).unwrap(),
            "{\"a\": {}} {Box} {bOx} { box}"
        );
        assert_eq!(fill("{box", &vals).unwrap(), "{box");
        let e = fill("on {bx}", &vals).unwrap_err().to_string();
        assert!(e.contains("{bx} names no variable"), "{e}");
    }

    #[test]
    fn faults_in_a_block_are_refused_and_named() {
        let err = |src: String| format!("{:#}", block(&src).expand(&Vars::new(), MST).unwrap_err());
        for (from, to, says) in [
            ("count = 12", "count = 0", "count is 0"),
            ("key = \"d\"", "key = \"d.x\"", "key prefix"),
            (
                "places = [\"discord DM\", \"cli\"]",
                "places = []",
                "no places",
            ),
            (
                "to = \"2026-09-01 00:00\"",
                "to = \"2026-03-01 00:00\"",
                "not before",
            ),
            (
                "to = \"2026-09-01 00:00\"",
                "to = \"2026-03-01 00:05\"",
                "do not fit",
            ),
            (
                "box = [\"canary\", \"perf\", \"qa\"]",
                "box = []",
                "has no values",
            ),
            (
                "box = [\"canary\", \"perf\", \"qa\"]",
                "i = [\"x\"]",
                "built in",
            ),
            ("on {box} up", "on {bocks} up", "{bocks} names no variable"),
            (
                "from = \"2026-03-01 00:00\"",
                "from = \"March\"",
                "not a time",
            ),
        ] {
            assert!(B.contains(from), "{from}");
            let e = err(B.replacen(from, to, 1));
            assert!(e.contains(says), "{to}: {e}");
        }
        // The exam's shared variables fill in, under the block's own.
        let shared: Vars = [("box".to_string(), vec!["edge".to_string()])]
            .into_iter()
            .collect();
        let no_own = B.replace("box = [\"canary\", \"perf\", \"qa\"]", "");
        let s = block(&no_own).expand(&shared, MST).unwrap();
        assert!(s.iter().all(|x| x.nodes[1].text == "edge: up"));
        let s = block(B).expand(&shared, MST).unwrap();
        assert!(s.iter().all(|x| x.nodes[1].text != "edge: up"));
    }
}
