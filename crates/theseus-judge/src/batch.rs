//! Batching (design §2.2; spec §3.7, "serial depth, not call count, bounds
//! latency"). At one decision point, packs whose states are byte-identical
//! and that pin the same model go out in one request, with question ids
//! namespaced `<pack>/<question>` (ids are never shown to Jev, so this changes
//! nothing it sees; verified accepted 2026-09-30). Packs with different
//! states go out as separate requests, concurrently.

use std::sync::Arc;

use crate::client::{Answer, Request, Response};
use crate::pack::{Asked, Pack};
use crate::state::BuiltState;

/// One pack's share of a decision point: its state and its questions. `tag`
/// is the caller's own index, carried through untouched.
#[derive(Debug, Clone)]
pub struct Part {
    pub tag: usize,
    pub pack: Arc<Pack>,
    pub state: Arc<BuiltState>,
    pub asked: Vec<Asked>,
}

/// One request and the parts it carries, in order.
#[derive(Debug, Clone)]
pub struct Batch {
    pub request: Request,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BatchError {
    #[error("nothing to batch")]
    Empty,
    #[error("the states of {a} and {b} differ; they go out as separate requests")]
    StatesDiffer { a: String, b: String },
    #[error("{a} and {b} pin different models")]
    ModelsDiffer { a: String, b: String },
    #[error("{0} appears twice in one request")]
    Duplicate(String),
}

pub fn namespaced(pack: &str, question: &str) -> String {
    format!("{pack}/{question}")
}

/// One request for parts that share a state and a model.
pub fn merge(parts: Vec<Part>) -> Result<Batch, BatchError> {
    let first = parts.first().ok_or(BatchError::Empty)?;
    let (state, model, first_name) = (
        first.state.clone(),
        first.pack.jev_model.clone(),
        first.pack.name(),
    );
    let mut names = Vec::with_capacity(parts.len());
    for p in &parts {
        let name = p.pack.name();
        if p.state.json != state.json {
            return Err(BatchError::StatesDiffer {
                a: first_name,
                b: name,
            });
        }
        if p.pack.jev_model != model {
            return Err(BatchError::ModelsDiffer {
                a: first_name,
                b: name,
            });
        }
        if names.contains(&name) {
            return Err(BatchError::Duplicate(name));
        }
        names.push(name);
    }
    let questions = parts
        .iter()
        .zip(&names)
        .flat_map(|(p, name)| {
            p.asked
                .iter()
                .map(move |a| (namespaced(name, &a.id), a.question.clone()))
        })
        .collect();
    Ok(Batch {
        request: Request {
            state: state.json.clone(),
            model,
            questions,
        },
        parts,
    })
}

/// Groups parts into as few requests as the rules allow, keeping the
/// parts' order within each, and the groups in order of first appearance.
pub fn plan(parts: Vec<Part>) -> Vec<Batch> {
    let mut groups: Vec<Vec<Part>> = Vec::new();
    for p in parts {
        let home = groups.iter_mut().find(|g| {
            g[0].state.json == p.state.json
                && g[0].pack.jev_model == p.pack.jev_model
                && g.iter().all(|q| q.pack.name() != p.pack.name())
        });
        match home {
            Some(g) => g.push(p),
            None => groups.push(vec![p]),
        }
    }
    groups
        .into_iter()
        .map(|g| merge(g).expect("a planned group shares its state and model"))
        .collect()
}

/// Each part's answers, ids back to the pack's own, in the part's order.
/// `None` for an answer the response lacks (a parsed response never does).
pub fn split(batch: &Batch, response: &Response) -> Vec<Vec<(Asked, Option<Answer>)>> {
    batch
        .parts
        .iter()
        .map(|p| {
            let name = p.pack.name();
            p.asked
                .iter()
                .map(|a| {
                    (
                        a.clone(),
                        response.answer(&namespaced(&name, &a.id)).cloned(),
                    )
                })
                .collect()
        })
        .collect()
}

/// `total` split in proportion to `weights`, by largest remainder, so the
/// shares sum to `total` exactly (the cost of one call, split between the
/// judgments it carried, by their question counts).
pub fn shares(total: u64, weights: &[usize]) -> Vec<u64> {
    let sum: u128 = weights.iter().map(|&w| w as u128).sum();
    if sum == 0 {
        return vec![0; weights.len()];
    }
    let exact: Vec<(u64, u128)> = weights
        .iter()
        .map(|&w| {
            let num = total as u128 * w as u128;
            ((num / sum) as u64, num % sum)
        })
        .collect();
    let mut out: Vec<u64> = exact.iter().map(|&(q, _)| q).collect();
    let mut left = total - out.iter().sum::<u64>();
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|&a, &b| exact[b].1.cmp(&exact[a].1).then(a.cmp(&b)));
    for i in order {
        if left == 0 {
            break;
        }
        out[i] += 1;
        left -= 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Answer, Usage};
    use crate::pack::{by_name, Dynamic};
    use crate::state::{NoScrub, StateBuilder};

    fn state(text: &str) -> Arc<BuiltState> {
        let mut b = StateBuilder::new("t", 1, 100, &NoScrub);
        b.scalar("event", text);
        Arc::new(b.build())
    }

    /// Two "packs" from the test pack: the same questions, another name.
    fn part(tag: usize, version: u32, s: &Arc<BuiltState>) -> Part {
        let mut p = (*by_name("probe.v1").unwrap()).clone();
        p.version = version;
        let asked = p.ask(&Dynamic::default());
        Part {
            tag,
            pack: Arc::new(p),
            state: s.clone(),
            asked,
        }
    }

    #[test]
    fn packs_sharing_a_state_merge_with_namespaced_ids() {
        let s = state("one");
        let b = merge(vec![part(0, 1, &s), part(1, 2, &s)]).unwrap();
        let ids: Vec<&str> = b
            .request
            .questions
            .iter()
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(ids.len(), 8);
        assert!(ids.contains(&"probe.v1/cause") && ids.contains(&"probe.v2/cause"));
        assert_eq!(b.request.state, s.json);
        assert_eq!(b.request.model, "jev-1.13.0");
    }

    #[test]
    fn states_that_differ_are_refused_and_planned_apart() {
        let (a, b) = (state("one"), state("two"));
        assert_eq!(
            merge(vec![part(0, 1, &a), part(1, 2, &b)]).unwrap_err(),
            BatchError::StatesDiffer {
                a: "probe.v1".into(),
                b: "probe.v2".into()
            }
        );
        // A different model refuses as well.
        let mut other = part(1, 2, &a);
        let mut p = (*other.pack).clone();
        p.jev_model = "jev-1.14.0".into();
        other.pack = Arc::new(p);
        assert!(matches!(
            merge(vec![part(0, 1, &a), other]),
            Err(BatchError::ModelsDiffer { .. })
        ));
        assert_eq!(
            merge(vec![part(0, 1, &a), part(1, 1, &a)]).unwrap_err(),
            BatchError::Duplicate("probe.v1".into())
        );
        assert_eq!(merge(vec![]).unwrap_err(), BatchError::Empty);
        // The plan: v1 and v2 on `one` together, v3 on `two` alone, and a
        // second v1 on `one` in a request of its own.
        let plan = plan(vec![
            part(0, 1, &a),
            part(1, 3, &b),
            part(2, 2, &a),
            part(3, 1, &a),
        ]);
        let tags: Vec<Vec<usize>> = plan
            .iter()
            .map(|b| b.parts.iter().map(|p| p.tag).collect())
            .collect();
        assert_eq!(tags, vec![vec![0, 2], vec![1], vec![3]]);
    }

    #[test]
    fn answers_split_back_to_each_pack_under_its_own_ids() {
        let s = state("one");
        let b = merge(vec![part(0, 1, &s), part(1, 2, &s)]).unwrap();
        let answers = b
            .request
            .questions
            .iter()
            .enumerate()
            .map(|(i, (id, _))| {
                (
                    id.clone(),
                    Answer::Noul {
                        noul: i as f64 / 10.0,
                    },
                )
            })
            .collect();
        let r = Response {
            model: "jev-1.13.0".into(),
            usage: Usage::default(),
            answers,
        };
        let split = split(&b, &r);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0][0].0.id, "cause");
        assert_eq!(split[1][0].0.id, "cause");
        assert_eq!(split[0][0].1, Some(Answer::Noul { noul: 0.0 }));
        assert_eq!(split[1][0].1, Some(Answer::Noul { noul: 0.4 }));
    }

    #[test]
    fn shares_sum_to_the_whole() {
        assert_eq!(shares(10, &[1, 1, 1]), vec![4, 3, 3]);
        assert_eq!(shares(93, &[5, 1]), vec![78, 15]);
        assert_eq!(shares(0, &[2, 3]), vec![0, 0]);
        assert_eq!(shares(7, &[0, 0]), vec![0, 0]);
        for total in [1u64, 2, 97, 1_000_003] {
            let s = shares(total, &[3, 7, 1, 9]);
            assert_eq!(s.iter().sum::<u64>(), total);
        }
    }
}
