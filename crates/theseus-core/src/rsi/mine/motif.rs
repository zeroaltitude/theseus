//! Finding recurring sequences (§2.3).
//!
//! 1. **Segment.** A run's calls are split into stretches at the calls the
//!    model authored and the calls that are never mined. The creative work
//!    stays with the model; a tool encapsulates the scaffolding around it.
//! 2. **Count.** Every contiguous n-gram of 3 to 16 shapes in a stretch is
//!    counted once per run, so a loop inside one run does not inflate support.
//!    A gram can only be frequent when its (n − 1)-prefix is, which bounds the
//!    work to the frequent grams. Only **closed** patterns are kept: a gram
//!    with the same support as an extension of it (one call longer, either
//!    side) is dropped.
//! 3. **Support.** The thresholds are `[self.mint]`'s: occurrences, sessions
//!    and days in one scope; for the bench, trials and tasks, and a bench-born
//!    motif must be generic.
//! 4. **Align** the occurrences ([`super::align`]), keep the core and optional
//!    steps, classify the slots ([`super::slots`]), branch on result class, and
//!    cost the occurrences ([`super::cost`]).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::align::{align, Aligned};
use super::cost;
use super::shape::{Shape, SlotType};
use super::slots::{classify, SlotCall, SlotKind};
use super::{
    Arm, Branch, Exit, Input, Motif, Occurrence, Params, Run, Step, Support, Vocab, BENCH_TASKS,
    BENCH_TRIALS, CORE_SHARE, MAX_LEN, MIN_LEN, OPTIONAL_SHARE,
};

/// Calls kept before a seed's first call, and after its last, in a window.
const LEFT: usize = 3;
const RIGHT: usize = 16;
/// The steps an arm lists after its branching step.
const ARM_STEPS: usize = 8;
/// Arms kept at a branch.
const ARMS: usize = 6;

/// How a stretch ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// The run's calls ended.
    Natural,
    /// A call the model authored followed: the model resumes there.
    Authored,
    /// A call that is never mined followed.
    Never,
}

#[derive(Debug, Clone, Copy)]
struct Stretch {
    run: usize,
    start: usize,
    end: usize,
    ends: End,
}

fn stretches(runs: &[&Run]) -> Vec<Stretch> {
    let mut out = Vec::new();
    for (r, run) in runs.iter().enumerate() {
        let mut start = 0;
        for (i, c) in run.calls.iter().enumerate() {
            if c.cuts() {
                if i > start {
                    out.push(Stretch {
                        run: r,
                        start,
                        end: i,
                        ends: if c.flags & super::AUTHORED != 0 {
                            End::Authored
                        } else {
                            End::Never
                        },
                    });
                }
                start = i + 1;
            }
        }
        if run.calls.len() > start {
            out.push(Stretch {
                run: r,
                start,
                end: run.calls.len(),
                ends: End::Natural,
            });
        }
    }
    out
}

fn key(tokens: &[u64]) -> u64 {
    tokens
        .iter()
        .fold(0xcbf2_9ce4_8422_2325 ^ tokens.len() as u64, |h, t| {
            (h ^ t).wrapping_mul(0x0100_0000_01b3).rotate_left(5)
        })
}

#[derive(Debug, Clone, Copy)]
struct Gram {
    support: u32,
    last_run: u32,
    /// Where it was first seen: the stretch and the offset in it.
    at: (u32, u32),
}

/// The tokens of a stretch.
fn tokens(runs: &[&Run], s: &Stretch) -> Vec<u64> {
    runs[s.run].calls[s.start..s.end]
        .iter()
        .map(|c| c.shape)
        .collect()
}

/// The closed frequent grams of every length, as (length, key, first place).
fn closed_grams(toks: &[Vec<u64>], st: &[Stretch], min_runs: u32) -> Vec<(usize, u64, (u32, u32))> {
    let mut freq: Vec<HashMap<u64, Gram>> = Vec::new();
    for n in MIN_LEN..=MAX_LEN {
        let prev = freq.last();
        let mut m: HashMap<u64, Gram> = HashMap::new();
        for (si, t) in toks.iter().enumerate() {
            if t.len() < n {
                continue;
            }
            let run = st[si].run as u32;
            for off in 0..=t.len() - n {
                // Frequent only if the prefix is.
                if let Some(p) = prev {
                    if !p.contains_key(&key(&t[off..off + n - 1])) {
                        continue;
                    }
                }
                let g = m.entry(key(&t[off..off + n])).or_insert(Gram {
                    support: 0,
                    last_run: u32::MAX,
                    at: (si as u32, off as u32),
                });
                if g.last_run != run {
                    g.last_run = run;
                    g.support += 1;
                }
            }
        }
        m.retain(|_, g| g.support >= min_runs);
        if m.is_empty() {
            break;
        }
        freq.push(m);
    }
    let mut open: HashSet<(usize, u64)> = HashSet::new();
    for (ix, m) in freq.iter().enumerate().skip(1) {
        let n = MIN_LEN + ix;
        for g in m.values() {
            let t = &toks[g.at.0 as usize][g.at.1 as usize..g.at.1 as usize + n];
            for part in [&t[..n - 1], &t[1..]] {
                let k = key(part);
                if freq[ix - 1].get(&k).is_some_and(|p| p.support == g.support) {
                    open.insert((n - 1, k));
                }
            }
        }
    }
    let mut out = Vec::new();
    for (ix, m) in freq.iter().enumerate() {
        let n = MIN_LEN + ix;
        for (k, g) in m {
            if !open.contains(&(n, *k)) {
                out.push((n, *k, g.at));
            }
        }
    }
    out.sort_unstable();
    out
}

/// One occurrence of a seed: a window of a run.
struct Window {
    run: usize,
    stretch: usize,
    lo: usize,
    hi: usize,
    ends: End,
}

fn short_id(seen: &mut BTreeMap<String, u32>, base: String) -> String {
    let n = seen.entry(base.clone()).or_default();
    *n += 1;
    if *n == 1 {
        base
    } else {
        format!("{base}{n}")
    }
}

/// A step's short name: the subcommand words of a program, or the tool.
fn step_name(s: &Shape) -> String {
    if s.tool == "proc.run" && !s.head.is_empty() {
        let words: Vec<&str> = s.head.split(' ').collect();
        let own = if ["git", "gh", "docker", "kubectl", "bd", "systemctl"].contains(&words[0])
            && words.len() > 1
        {
            &words[1..]
        } else {
            &words[..]
        };
        return own.join("_").replace('-', "_");
    }
    s.tool.replace(['.', ':'], "_")
}

/// The words of a result class that name what led to a hand-back.
fn handback_word(class: &str) -> String {
    let mut parts = class.split('+');
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        first.replace(':', "-")
    } else {
        rest.join("-")
    }
}

/// The motifs of one scope.
pub fn find(scope: &str, runs: &[&Run], vocab: &Vocab, p: &Params) -> Vec<Motif> {
    let bench = scope.starts_with("bench:");
    let min_runs = if bench {
        BENCH_TRIALS
    } else {
        p.min_occurrences
    };
    let st = stretches(runs);
    let toks: Vec<Vec<u64>> = st.iter().map(|s| tokens(runs, s)).collect();
    let grams = closed_grams(&toks, &st, min_runs);
    // The seeds, by length: their keys.
    let mut want: BTreeMap<usize, HashMap<u64, usize>> = BTreeMap::new();
    let mut seeds: Vec<Vec<u64>> = Vec::new();
    for (n, k, at) in &grams {
        let t = &toks[at.0 as usize][at.1 as usize..at.1 as usize + n];
        let distinct: BTreeSet<&u64> = t.iter().collect();
        if distinct.len() < 2 {
            continue;
        }
        want.entry(*n).or_default().insert(*k, seeds.len());
        seeds.push(t.to_vec());
    }
    // Their windows: the first occurrence in each run.
    let mut found: Vec<BTreeMap<usize, Window>> =
        (0..seeds.len()).map(|_| BTreeMap::new()).collect();
    for (si, t) in toks.iter().enumerate() {
        let s = &st[si];
        for (n, keys) in &want {
            if t.len() < *n {
                continue;
            }
            for off in 0..=t.len() - n {
                let Some(&ix) = keys.get(&key(&t[off..off + n])) else {
                    continue;
                };
                if seeds[ix] != t[off..off + n] || found[ix].contains_key(&s.run) {
                    continue;
                }
                let lo = off.saturating_sub(LEFT);
                let hi = (off + n + RIGHT).min(t.len());
                found[ix].insert(
                    s.run,
                    Window {
                        run: s.run,
                        stretch: si,
                        lo: s.start + lo,
                        hi: s.start + hi,
                        ends: if hi == t.len() { s.ends } else { End::Natural },
                    },
                );
            }
        }
    }
    let mut motifs: Vec<(Motif, BTreeSet<usize>)> = Vec::new();
    for (ix, seed) in seeds.iter().enumerate() {
        let wins: Vec<&Window> = found[ix].values().collect();
        let sessions: BTreeSet<&str> = wins.iter().map(|w| runs[w.run].session.as_str()).collect();
        let days: BTreeSet<&str> = wins.iter().map(|w| runs[w.run].day.as_str()).collect();
        let tasks: BTreeSet<&str> = wins
            .iter()
            .map(|w| runs[w.run].task.as_str())
            .filter(|t| !t.is_empty())
            .collect();
        let enough = if bench {
            wins.len() as u32 >= BENCH_TRIALS && tasks.len() as u32 >= BENCH_TASKS
        } else {
            wins.len() as u32 >= p.min_occurrences
                && sessions.len() as u32 >= p.min_sessions
                && days.len() as u32 >= p.min_days
        };
        if !enough {
            continue;
        }
        let support = Support {
            occurrences: wins.len() as u32,
            sessions: sessions.len() as u32,
            days: days.len() as u32,
            tasks: tasks.len() as u32,
        };
        if let Some(m) = build(scope, seed, &wins, runs, vocab, support, p) {
            motifs.push((m, wins.iter().map(|w| w.stretch).collect()));
        }
    }
    // Best first; a motif whose stretches another, better one holds is that one.
    motifs.sort_by(|a, b| {
        b.0.support
            .occurrences
            .cmp(&a.0.support.occurrences)
            .then_with(|| b.0.seed.len().cmp(&a.0.seed.len()))
            .then_with(|| a.0.digest.cmp(&b.0.digest))
    });
    let mut kept: Vec<(Motif, BTreeSet<usize>)> = Vec::new();
    for (m, stretches_of) in motifs {
        if !kept.iter().any(|(_, held)| stretches_of.is_subset(held)) {
            kept.push((m, stretches_of));
        }
    }
    kept.into_iter().map(|(m, _)| m).collect()
}

/// The kept columns as steps: a short name each, core or optional.
fn steps_of(al: &Aligned, cols: &[usize], vocab: &Vocab) -> Option<Vec<Step>> {
    let mut names: BTreeMap<String, u32> = BTreeMap::new();
    let mut steps: Vec<Step> = Vec::new();
    for &c in cols {
        let shape = vocab.shape(al.shapes[c])?.clone();
        let presence = al.presence(c);
        steps.push(Step {
            id: short_id(&mut names, step_name(&shape)),
            shape,
            core: presence >= CORE_SHARE,
            presence: (presence * 100.0).round() / 100.0,
            slots: vec![],
        });
    }
    Some(steps)
}

/// The place classes of the windows, their evidence (positions), and when
/// the newest ran.
fn evidence_of(wins: &[&Window], runs: &[&Run]) -> (Vec<String>, Vec<Occurrence>, u64) {
    let mut places: Vec<String> = wins.iter().map(|w| runs[w.run].place.clone()).collect();
    places.sort();
    places.dedup();
    let occurrences = wins
        .iter()
        .map(|w| {
            let calls = &runs[w.run].calls[w.lo..w.hi];
            Occurrence {
                session: runs[w.run].session.clone(),
                turn: runs[w.run].turn.clone(),
                day: runs[w.run].day.clone(),
                first: calls.first().map_or(0, |c| c.pos),
                last: calls.last().map_or(0, |c| c.pos),
            }
        })
        .collect();
    let last_ms = wins.iter().map(|w| runs[w.run].last_ms).max().unwrap_or(0);
    (places, occurrences, last_ms)
}

/// The slots of every step, classified across the occurrences.
fn bind_slots(
    steps: &mut [Step],
    al: &Aligned,
    cols: &[usize],
    wins: &[&Window],
    runs: &[&Run],
    vocab: &Vocab,
) {
    let types: Vec<Vec<SlotType>> = steps
        .iter()
        .map(|s| s.shape.slots.iter().map(|x| x.ty).collect())
        .collect();
    let columns: Vec<Vec<Option<SlotCall<'_>>>> = cols
        .iter()
        .map(|c| {
            al.rows
                .iter()
                .enumerate()
                .map(|(w, row)| {
                    row[*c].map(|j| {
                        let call = &runs[wins[w].run].calls[wins[w].lo + j];
                        SlotCall {
                            digests: &call.slots,
                            outs: &call.outs,
                        }
                    })
                })
                .collect()
        })
        .collect();
    for (s, b) in steps
        .iter_mut()
        .zip(classify(&types, &columns, &|d| vocab.constant(d)))
    {
        s.slots = b;
    }
}

/// A bench-born motif with a task's own path or string is a tool that helps
/// only a benchmark.
fn not_generic(steps: &[Step]) -> bool {
    steps.iter().any(|s| {
        s.slots.iter().any(|b| {
            b.kind == SlotKind::Constant
                && matches!(b.ty, SlotType::Path | SlotType::Str | SlotType::UrlHost)
        })
    })
}

fn build(
    scope: &str,
    seed: &[u64],
    wins: &[&Window],
    runs: &[&Run],
    vocab: &Vocab,
    support: Support,
    p: &Params,
) -> Option<Motif> {
    let windows: Vec<Vec<u64>> = wins
        .iter()
        .map(|w| {
            runs[w.run].calls[w.lo..w.hi]
                .iter()
                .map(|c| c.shape)
                .collect()
        })
        .collect();
    let al = align(&windows);
    // The kept columns: core and optional.
    let cols: Vec<usize> = (0..al.shapes.len())
        .filter(|c| al.presence(*c) >= OPTIONAL_SHARE)
        .collect();
    let mut steps = steps_of(&al, &cols, vocab)?;
    if steps.is_empty() {
        return None;
    }
    bind_slots(&mut steps, &al, &cols, wins, runs, vocab);
    let bench_born = scope.starts_with("bench:");
    if bench_born && not_generic(&steps) {
        return None;
    }
    let inputs = inputs_of(&steps);
    let branches = branches(&al, &cols, &steps, wins, runs, vocab);
    let parts: Vec<_> = wins
        .iter()
        .map(|w| cost::occurrence(runs[w.run], w.lo..w.hi))
        .collect();
    let (cost, saving) = cost::mean(&parts, p.window_days);
    let seed_shapes: Vec<Shape> = seed
        .iter()
        .filter_map(|id| vocab.shape(*id).cloned())
        .collect();
    if seed_shapes.len() != seed.len() {
        return None;
    }
    let (places, occurrences, last_ms) = evidence_of(wins, runs);
    let mut m = Motif {
        scope: scope.to_string(),
        digest: String::new(),
        bench_born,
        places,
        seed: seed_shapes,
        steps,
        branches,
        inputs,
        support,
        cost,
        saving,
        last_ms,
        occurrences,
    };
    m.digest = m.spec()["digest"].as_str().unwrap_or_default().to_string();
    Some(m)
}

/// The tool's inputs: each INPUT slot, named by its type.
fn inputs_of(steps: &[Step]) -> Vec<Input> {
    let mut out: Vec<Input> = Vec::new();
    let mut count: BTreeMap<&'static str, u32> = BTreeMap::new();
    for s in steps {
        for (k, b) in s.slots.iter().enumerate() {
            if b.kind != SlotKind::Input {
                continue;
            }
            let base = b.ty.input_name();
            let n = count.entry(base).or_default();
            *n += 1;
            out.push(Input {
                name: if *n == 1 {
                    base.to_string()
                } else {
                    format!("{base}{n}")
                },
                ty: b.ty,
                step: s.id.clone(),
                slot: k,
            });
        }
    }
    out
}

/// Branches (§2.3.4): at each core step whose occurrences differ in result
/// class, one arm for each class, with the steps that came next for most of
/// the occurrences and how they ended.
fn branches(
    al: &Aligned,
    cols: &[usize],
    steps: &[Step],
    wins: &[&Window],
    runs: &[&Run],
    vocab: &Vocab,
) -> Vec<Branch> {
    let mut out = Vec::new();
    for (k, step) in steps.iter().enumerate() {
        if !step.core {
            continue;
        }
        // Occurrence -> (class, steps after this one, how its window ended).
        let mut by_class: BTreeMap<String, Vec<(Vec<String>, End)>> = BTreeMap::new();
        for (w, row) in al.rows.iter().enumerate() {
            let Some(j) = row[cols[k]] else { continue };
            let call = &runs[wins[w].run].calls[wins[w].lo + j];
            let class = vocab.result(call.res).to_string();
            let then: Vec<String> = cols[k + 1..]
                .iter()
                .enumerate()
                .filter(|(_, c)| row[**c].is_some())
                .map(|(i, _)| steps[k + 1 + i].id.clone())
                .take(ARM_STEPS)
                .collect();
            by_class
                .entry(class)
                .or_default()
                .push((then, wins[w].ends));
        }
        if by_class.len() < 2 {
            continue;
        }
        let mut arms: Vec<Arm> = by_class
            .into_iter()
            .map(|(on, seen)| {
                let mut tails: BTreeMap<&Vec<String>, u32> = BTreeMap::new();
                for (t, _) in &seen {
                    *tails.entry(t).or_default() += 1;
                }
                let modal = tails
                    .iter()
                    .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                    .map(|(t, _)| (*t).clone())
                    .unwrap_or_default();
                let ends: Vec<End> = seen
                    .iter()
                    .filter(|(t, _)| *t == modal)
                    .map(|(_, e)| *e)
                    .collect();
                let hand = ends.iter().filter(|e| **e == End::Authored).count() * 2 > ends.len();
                let exit = if hand {
                    Exit::Handback(handback_word(&on))
                } else if on.starts_with("err") && modal.is_empty() {
                    Exit::Err(handback_word(&on))
                } else {
                    Exit::Ok
                };
                Arm {
                    on,
                    then: modal,
                    count: seen.len() as u32,
                    exit,
                }
            })
            .collect();
        arms.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.on.cmp(&b.on)));
        arms.truncate(ARMS);
        out.push(Branch {
            at: step.id.clone(),
            arms,
        });
    }
    out
}

#[cfg(test)]
mod tests;
