//! From a motif to a tool spec (§2.5): the skeleton the miner wrote, as the
//! flow spec R6 defines, with a placeholder name until the writer names it.
//! Nothing here calls a model, and a spec is built only through
//! [`crate::flow::spec`]'s types, never by a copy of its shape.
//!
//! The mapping, and what it drops:
//! - **Steps.** The motif's core steps, and any optional step an arm's `then`
//!   names (a flow has no optional step: the arms decide what runs). A
//!   `proc.run` step's argv is its program and subcommand words, then its
//!   flags, then its slots in order; another tool's arguments are read back
//!   from its shape's text (`tool[key=$REF flag=true]`, the keys sorted).
//! - **Slots.** An input slot is an input, typed (`ref`, `path`, `num`,
//!   `str`). A constant stays only when it passes the shape check
//!   (`mine::read::constant_ok`), the daemon's scrubber leaves it as it is,
//!   and it is not a path outside the repository; otherwise the slot
//!   becomes an input of its own and the value is not kept anywhere. A slot
//!   bound to an earlier step's slot takes that slot's input or constant; one
//!   bound to an earlier step's *result* has no flow binding, so it is an
//!   input too (the model names it).
//! - **Branches.** An arm's result class (`err:1+conflict+rerere-left`) is
//!   read as the flow class that is most specific in it (`class::CLASSES`);
//!   two arms of one class keep the one seen more often. Where an arm ends
//!   in an exit or a hand-back, `on` says so; where it goes on, `goto:`.
//!   The miner reads `rerere-left` and `rerere-resolved` off a merge's own
//!   text, but the flow runtime only produces them from `git rerere status`:
//!   so where arms differ by them the skeleton gets a step of that program
//!   after the branching step (the owner's example in §2.5 has it), the
//!   conflict class goes to it, and its two classes go where the arms go. It
//!   is a *synthetic* step ([`Skeleton::synthetic`]): no recording ran it.
//! - **Evidence.** The counts and costs the skeleton carries; never an id.
//!
//! A kind other than a flow has no spec of this shape: its writer gets a
//! brief ([`super::brief`]).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use crate::flow::class::CLASSES;
use crate::flow::spec::{Exit, Input, InputType, Scope, Spec, Step};
use crate::rsi::mine::read::constant_ok;
use crate::rsi::mine::{self, Motif, SlotKind, SlotType};

/// The name a skeleton carries until the writer gives it one: the motif's own
/// digest, so two nights name one motif alike.
pub fn placeholder_name(m: &Motif) -> String {
    let hex = m.digest.strip_prefix("sha256:").unwrap_or(&m.digest);
    format!("mint_{}", &hex[..hex.len().min(12)])
}

/// A flow input's type for a slot's.
fn input_type(t: SlotType) -> InputType {
    match t {
        SlotType::Path => InputType::Path,
        SlotType::Reference => InputType::Ref,
        SlotType::Num => InputType::Num,
        SlotType::UrlHost | SlotType::Crate | SlotType::Str => InputType::Str,
    }
}

/// The flow class an arm's result class stands for: the most specific one
/// the class names.
fn class_of(on: &str) -> &'static str {
    let words: Vec<&str> = on.split('+').collect();
    let has = |w: &str| words.contains(&w);
    for c in ["conflict", "test-fail"] {
        if has(c) {
            return CLASSES.iter().find(|x| **x == c).copied().unwrap_or("err");
        }
    }
    if words.iter().any(|w| w.starts_with("err")) {
        "err"
    } else {
        "ok"
    }
}

/// What a step's argument at slot `k` is in the spec.
struct Binder<'a> {
    motif: &'a Motif,
    scrub: &'a dyn Fn(&str) -> String,
    inputs: BTreeMap<String, Input>,
    /// Each input's source: the step's id and the slot it was read from.
    from: BTreeMap<String, (String, usize)>,
    /// (step index, slot) -> the `$name` or constant already decided.
    decided: BTreeMap<(usize, usize), Value>,
}

impl Binder<'_> {
    /// A new input for slot `k` of step `ix`, named by the step and slot.
    fn promote(&mut self, ix: usize, k: usize, ty: SlotType) -> Value {
        let step = &self.motif.steps[ix];
        let base = format!("{}_{}", step.id.replace('-', "_").to_lowercase(), k);
        let mut name = base.clone();
        let mut n = 2;
        while self.inputs.contains_key(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        self.inputs.insert(
            name.clone(),
            Input {
                ty: input_type(ty),
                default: None,
            },
        );
        self.from.insert(name.clone(), (step.id.clone(), k));
        json!(format!("${name}"))
    }

    fn named_input(&mut self, ix: usize, k: usize, ty: SlotType) -> Option<Value> {
        let step = &self.motif.steps[ix];
        let i = self
            .motif
            .inputs
            .iter()
            .find(|i| i.step == step.id && i.slot == k)?;
        let name = i.name.clone();
        let from = (step.id.clone(), k);
        self.inputs.entry(name.clone()).or_insert(Input {
            ty: input_type(ty),
            default: None,
        });
        self.from.entry(name.clone()).or_insert(from);
        Some(json!(format!("${name}")))
    }

    /// The value of slot `k` of step `ix`.
    fn slot(&mut self, ix: usize, k: usize) -> Value {
        if let Some(v) = self.decided.get(&(ix, k)) {
            return v.clone();
        }
        let s = &self.motif.steps[ix].slots[k];
        let ty = s.ty;
        let v = match (s.kind, &s.constant, &s.bound) {
            (SlotKind::Constant, Some(c), _) if self.constant_ok(c) => json!(c),
            (SlotKind::Bound, _, Some(mine::Bound::Slot { step, slot })) if *step < ix => {
                self.slot(*step, *slot)
            }
            _ => self
                .named_input(ix, k, ty)
                .unwrap_or_else(|| self.promote(ix, k, ty)),
        };
        self.decided.insert((ix, k), v.clone());
        v
    }

    /// The constant's survival test (§2.5, Hygiene).
    fn constant_ok(&self, c: &str) -> bool {
        constant_ok(c, "")
            && (self.scrub)(c) == c
            && !c.starts_with('/')
            && !c.starts_with('~')
            && !c.starts_with('$')
    }
}

/// One step's arguments.
fn args_of(b: &mut Binder<'_>, ix: usize) -> Result<Value, String> {
    let step = b.motif.steps[ix].clone();
    if step.shape.tool == "proc.run" {
        let mut argv: Vec<Value> = step.shape.head.split(' ').map(|w| json!(w)).collect();
        let mut slot = 0usize;
        for f in &step.shape.flags {
            argv.push(json!(f.trim_end_matches('=')));
            if f.ends_with('=') && slot < step.slots.len() {
                argv.push(b.slot(ix, slot));
                slot += 1;
            }
        }
        while slot < step.slots.len() {
            argv.push(b.slot(ix, slot));
            slot += 1;
        }
        return Ok(json!({ "argv": argv }));
    }
    // Another tool: `tool[key=$REF flag=true]`, the keys in sorted order.
    let inner = step
        .shape
        .text
        .split_once('[')
        .and_then(|(_, r)| r.rsplit_once(']'))
        .map(|(i, _)| i)
        .ok_or_else(|| format!("step {}: its shape does not read", step.id))?;
    let (mut args, mut slot) = (Map::new(), 0usize);
    for part in inner.split(' ').filter(|p| !p.is_empty()) {
        let (k, v) = part
            .split_once('=')
            .ok_or_else(|| format!("step {}: {part:?} is not key=value", step.id))?;
        if v.starts_with('$') {
            args.insert(k.into(), b.slot(ix, slot));
            slot += 1;
        } else {
            args.insert(k.into(), json!(v == "true"));
        }
    }
    Ok(Value::Object(args))
}

/// The steps the flow keeps: the core ones, and any an arm goes on to.
fn kept(m: &Motif) -> Vec<usize> {
    let named: BTreeSet<&str> = m
        .branches
        .iter()
        .flat_map(|b| {
            b.arms
                .iter()
                .flat_map(|a| a.then.iter().map(String::as_str))
        })
        .collect();
    (0..m.steps.len())
        .filter(|i| m.steps[*i].core || named.contains(m.steps[*i].id.as_str()))
        .collect()
}

/// A flow spec with where each of its inputs was read from, for binding the
/// recorded instances' values to it.
#[derive(Debug, Clone)]
pub struct Skeleton {
    pub spec: Spec,
    /// The steps the skeleton added that no recording ran, each with the step
    /// whose result they read.
    pub synthetic: BTreeMap<String, String>,
    /// Input name to the step's id and the slot it came from.
    pub from: BTreeMap<String, (String, usize)>,
}

/// Where a step's arms go: its `on` table, and the `git rerere status` step
/// that follows it when its arms differ by what rerere did.
fn on_of(
    m: &Motif,
    s: &mine::Step,
    next: Option<&str>,
) -> (BTreeMap<String, String>, Option<Step>) {
    let Some(br) = m.branches.iter().find(|br| br.at == s.id) else {
        return (BTreeMap::new(), None);
    };
    let mut arms: Vec<&mine::Arm> = br.arms.iter().collect();
    arms.sort_by_key(|a| std::cmp::Reverse(a.count));
    let jump = |a: &mine::Arm| match (&a.exit, a.then.first()) {
        (mine::Exit::Ok, Some(n)) => format!("goto:{n}"),
        (mine::Exit::Ok, None) => "exit:ok".into(),
        (e, _) => e.word(),
    };
    let (mut on, mut rerere_on) = (BTreeMap::new(), BTreeMap::new());
    for a in arms {
        let words: Vec<&str> = a.on.split('+').collect();
        if words.iter().any(|w| w.starts_with("rerere-")) {
            let class = if words.contains(&"rerere-left") {
                "rerere-left"
            } else {
                "rerere-resolved"
            };
            rerere_on
                .entry(class.to_string())
                .or_insert_with(|| jump(a));
        } else {
            on.entry(class_of(&a.on).to_string())
                .or_insert_with(|| jump(a));
        }
    }
    if !rerere_on.is_empty() {
        let mut rid = "rerere".to_string();
        let mut n = 2;
        while m.steps.iter().any(|x| x.id == rid) {
            rid = format!("rerere{n}");
            n += 1;
        }
        on.insert("conflict".into(), format!("goto:{rid}"));
        let after = Step {
            id: rid,
            tool: "proc.run".into(),
            args: json!({"argv": ["git", "rerere", "status"]}),
            on: rerere_on,
            max: None,
        };
        return (on, Some(after));
    }
    // An `ok` that goes to the next step is what falling through does.
    if next.is_some_and(|n| on.get("ok") == Some(&format!("goto:{n}"))) {
        on.remove("ok");
    }
    (on, None)
}

/// The exits: `ok`, and a hand-back for each arm that ends in one.
fn exits_of(m: &Motif) -> BTreeMap<String, Exit> {
    let mut exits = BTreeMap::new();
    for a in m.branches.iter().flat_map(|br| &br.arms) {
        if let mine::Exit::Handback(w) = &a.exit {
            exits.insert(
                format!("handback:{w}"),
                Exit::Hand {
                    state: vec!["conflict_files".into(), "rerere_resolved".into()],
                    text: None,
                },
            );
        }
    }
    exits.insert("ok".into(), Exit::Text("done".into()));
    exits
}

/// The flow spec a motif's skeleton makes, named `name` (a placeholder when
/// none yet), sealed. `scrub` is the daemon's scrubber: a constant it would
/// change becomes an input. Fails, saying why, when the skeleton is not a
/// valid flow (a bench motif has no repository; a goto that loops without
/// a bound; a tool a flow may not name).
pub fn flow_spec(
    m: &Motif,
    name: &str,
    scrub: &dyn Fn(&str) -> String,
) -> Result<Skeleton, String> {
    if m.bench_born || !m.scope.starts_with("r:") {
        return Err(format!(
            "the motif's scope {:?} is not a repository: a flow is offered only where its \
             motif was seen",
            m.scope
        ));
    }
    let keep = kept(m);
    let mut b = Binder {
        motif: m,
        scrub,
        inputs: BTreeMap::new(),
        from: BTreeMap::new(),
        decided: BTreeMap::new(),
    };
    let (mut steps, mut synthetic) = (Vec::new(), BTreeMap::new());
    for (at, &ix) in keep.iter().enumerate() {
        let s = &m.steps[ix];
        let next = keep.get(at + 1).map(|n| m.steps[*n].id.as_str());
        let (on, after) = on_of(m, s, next);
        steps.push(Step {
            id: s.id.clone(),
            tool: s.shape.tool.clone(),
            args: args_of(&mut b, ix)?,
            on,
            max: None,
        });
        if let Some(a) = after {
            synthetic.insert(a.id.clone(), s.id.clone());
            steps.push(a);
        }
    }
    let evidence = match m.spec()["evidence"].clone() {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    let spec = Spec {
        v: crate::flow::spec::VERSION,
        kind: "flow".into(),
        name: name.into(),
        description: String::new(),
        scope: Scope {
            repos: vec![m.scope.clone()],
            places: m.places.clone(),
        },
        inputs: b.inputs.clone(),
        steps,
        exits: exits_of(m),
        evidence,
        digest: None,
    };
    spec.validate()?;
    Ok(Skeleton {
        spec: spec.sealed(),
        synthetic,
        from: b.from,
    })
}
