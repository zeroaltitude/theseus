//! The sweep's questions about names (theseus-63tfk.1; the design's sections
//! 3.1 and 3.2). Jev writes no name: every answer is a Choice among names code
//! mined (`theseus_judge::mine`: paths, hosts, capitalised terms), and acts
//! only at the act band. Same money, mark and not-again rules as `judged.rs`
//! (`Run::paid`, `Run::left`, 30 days).
//!
//! - **Rename** (`name_it.v1`, [`Run::renames`]): a category a machine wrote
//!   whose name has gone stale, at most [`RENAMES_MAX`] a run. Stale is
//!   [`stale_names`]: at least [`RENAME_MIN`] titled sessions hold it, its
//!   name is in none of their titles, and one mined name is in at least half
//!   of them (and three); the names in two or more titles are the options,
//!   most shared first. `none` keeps the name. The write keeps the old name
//!   in the description (`Ontology::plan_rename`).
//! - **`same_kind`** ([`Run::kind_hints`]): before `mint::plan` clusters the
//!   `other` answers, pairs of unlisted mined names that no session shares and
//!   that together would make a cluster are asked, at most [`KIND_PAIRS_MAX`]
//!   a run, largest union first. A yes joins the two names' sessions.
//! - **The kind's name** (`name_it.v1`, same method): a cluster that will mint
//!   and has two names or more is asked which of them names the kind. Jev's pick
//!   names it; `none`, a pick below the band, or no answer leaves the most
//!   shared name, as before.

use std::collections::BTreeMap;

use serde_json::json;
use theseus_judge::builders::{KindSide, NameItInput, SameKindInput, SWEEP_TITLES};
use theseus_judge::mine::{mine, Source, Text};
use theseus_judge::Input;
use theseus_ontology::sweep::fold;
use theseus_ontology::{CategoryId, Ontology};

use super::judged::{acts, named, noul, Asked, Run, KIND_PACK, NAME_PACK};
use super::mint::{self, Hints};
use crate::rpc::Core;

/// The categories one run asks to rename at most.
pub const RENAMES_MAX: usize = 3;
/// The pairs of names one run asks `same_kind` about at most.
pub const KIND_PAIRS_MAX: usize = 20;
/// A rename is asked about with this many titled sessions at least.
pub const RENAME_MIN: usize = 5;
/// The newest sessions a rename reads titles of.
const SAMPLE: usize = 50;
/// The names a choice offers at most.
const NAMES: usize = 10;

/// The names mined from one title.
fn mined(title: &str) -> Vec<String> {
    mine(
        &Text {
            message: title,
            ..Text::default()
        },
        &[Source::Paths, Source::Hosts, Source::Capitalised],
    )
}

/// The names to offer for renaming `name`, from the titles of the sessions
/// that hold it: none unless the name has gone stale (see the module doc).
pub fn stale_names(name: &str, titles: &[&str]) -> Option<Vec<String>> {
    if titles.len() < RENAME_MIN {
        return None;
    }
    let own = fold(name);
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for title in titles {
        if fold(title).contains(&own) {
            return None;
        }
        let mut folds: Vec<String> = Vec::new();
        for n in mined(title) {
            let f = fold(&n);
            if !f.is_empty() && !folds.contains(&f) {
                counts.entry(f.clone()).or_insert((n, 0)).1 += 1;
                folds.push(f);
            }
        }
    }
    let mut shared: Vec<(usize, String)> = counts
        .into_values()
        .filter(|(_, n)| *n >= 2)
        .map(|(spelled, n)| (n, spelled))
        .collect();
    shared.sort_by(|(na, a), (nb, b)| nb.cmp(na).then_with(|| a.cmp(b)));
    let best = shared.first().map_or(0, |(n, _)| *n);
    (best >= 3 && best * 2 >= titles.len())
        .then(|| shared.into_iter().map(|(_, s)| s).take(NAMES).collect())
}

impl Run<'_> {
    /// The categories Jev renamed at the act band, with the name it picked.
    pub(super) async fn renames(&mut self, o: &Ontology) -> Vec<(CategoryId, String)> {
        let mut out = Vec::new();
        let mut asked = 0;
        for id in o.rename_candidates() {
            if asked >= RENAMES_MAX || !self.going() {
                break;
            }
            let key = format!("rename {id}");
            let Some(category) = o
                .category(&id)
                .filter(|_| !self.mark.asked.contains_key(&key))
            else {
                continue;
            };
            let mut held = self.core.held_by(o, &id).sessions;
            held.truncate(SAMPLE);
            let titles: Vec<&str> = held.iter().map(|(_, t)| t.as_str()).collect();
            let Some(names) = stale_names(&category.name, &titles) else {
                continue;
            };
            let input = NameItInput {
                kind: id.kind().to_string(),
                current: Some(category.name.clone()),
                description: category.description.clone(),
                titles: titles
                    .iter()
                    .take(SWEEP_TITLES)
                    .map(|t| t.to_string())
                    .collect(),
                names: names.clone(),
            };
            if asked > 0 {
                theseus_store::pressure::quiet(theseus_store::pressure::BOUND).await;
            }
            let context = json!({"category": id});
            match self.ask(NAME_PACK, Input::NameIt(input), context).await {
                Asked::Answered(j, cost) => {
                    asked += 1;
                    self.paid(key, cost);
                    if acts(&j) {
                        if let Some(name) = named(&j, "name", &names) {
                            out.push((id, name));
                        }
                    }
                }
                Asked::Over(need) => {
                    tracing::info!(
                        need,
                        "ontology: the sweep's day cap would pass; no more renames"
                    );
                    break;
                }
                Asked::Off => break,
            }
        }
        out
    }

    /// One call of the sweep: `input` to `pack`, with what the cap leaves.
    async fn ask(&self, pack: &str, input: Input, context: serde_json::Value) -> Asked {
        let judge = &self.core.runner.judge;
        judge.judge_sweep(pack, input, context, self.left()).await
    }

    /// One side of a `same_kind` pair or a kind's name: the titles of the
    /// sessions that said `names`.
    fn titles_of(&self, sessions: &[&str]) -> Vec<String> {
        sessions
            .iter()
            .filter_map(|s| self.core.titled(s))
            .map(|(_, _, title)| title)
            .take(SWEEP_TITLES)
            .collect()
    }

    /// What Jev says of the `other` answers `said`: which pairs of names are
    /// one kind, and what to call each kind that will mint.
    pub(super) async fn kind_hints(&mut self, o: &Ontology, said: &[mint::Said]) -> Hints {
        let folds = mint::folds(said);
        let mut hints = Hints {
            same: self.same_kinds(o, &folds).await,
            ..Hints::default()
        };
        self.kind_names(o, &folds, &mut hints).await;
        hints.spent = self.spent;
        hints
    }

    /// The pairs of names Jev called one kind at the act band.
    async fn same_kinds(&mut self, o: &Ontology, folds: &mint::Folds<'_>) -> Vec<(String, String)> {
        let side = |run: &Self, f: &str| {
            let (spelled, sessions) = &folds[f];
            let ids: Vec<&str> = sessions.keys().copied().collect();
            KindSide {
                names: vec![spelled.clone()],
                titles: run.titles_of(&ids),
            }
        };
        let mut same = Vec::new();
        let mut asked = 0;
        for (a, b) in mint::kind_pairs(o, folds) {
            if asked >= KIND_PAIRS_MAX || !self.going() {
                break;
            }
            let key = format!("kind {a} {b}");
            if self.mark.asked.contains_key(&key) {
                continue;
            }
            let input = SameKindInput {
                first: side(self, &a),
                second: side(self, &b),
            };
            if asked > 0 {
                theseus_store::pressure::quiet(theseus_store::pressure::BOUND).await;
            }
            match self
                .ask(KIND_PACK, Input::SameKind(input), json!({"a": a, "b": b}))
                .await
            {
                Asked::Answered(j, cost) => {
                    asked += 1;
                    self.paid(key, cost);
                    if acts(&j) && noul(&j, "same_kind", true) {
                        same.push((a, b));
                    }
                }
                Asked::Over(need) => {
                    tracing::info!(
                        need,
                        "ontology: the sweep's day cap would pass; no more name pairs"
                    );
                    break;
                }
                Asked::Off => break,
            }
        }
        same
    }

    /// The name Jev picked for each cluster that will mint, in `hints.named`.
    async fn kind_names(&mut self, o: &Ontology, folds: &mint::Folds<'_>, hints: &mut Hints) {
        for cluster in mint::leads(o, folds, &hints.same) {
            let names = cluster.spelled(folds);
            let key = format!("kind-name {}", cluster.key);
            if names.len() < 2 || !self.going() || self.mark.asked.contains_key(&key) {
                continue;
            }
            let ids: Vec<&str> = cluster.sessions.keys().copied().collect();
            let input = NameItInput {
                kind: "new kind".into(),
                current: None,
                description: String::new(),
                titles: self.titles_of(&ids),
                names: names.iter().take(NAMES).cloned().collect(),
            };
            let context = json!({"cluster": cluster.key});
            match self.ask(NAME_PACK, Input::NameIt(input), context).await {
                Asked::Answered(j, cost) => {
                    self.paid(key, cost);
                    if let Some(name) = acts(&j).then(|| named(&j, "name", &names)).flatten() {
                        hints.named.insert(cluster.key.clone(), name);
                    }
                }
                Asked::Over(need) => {
                    tracing::info!(
                        need,
                        "ontology: the sweep's day cap would pass; no more kind names"
                    );
                    break;
                }
                Asked::Off => break,
            }
        }
    }
}

impl Core {
    /// What Jev says of the window's `other` answers, before the plan mints:
    /// nothing with the sweep's model part off.
    pub(super) async fn onto_mint_hints(&self, now_ms: u64) -> anyhow::Result<Hints> {
        let Some(mut run) = self.judged_run(now_ms)? else {
            return Ok(Hints::default());
        };
        let Some(said) = self.onto_said(now_ms)? else {
            return Ok(Hints::default());
        };
        let o = self.runner.ontology.snapshot(&self.store)?;
        let hints = run.kind_hints(&o, &said).await;
        run.keep()?;
        Ok(hints)
    }
}
