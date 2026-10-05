//! `citation.v1` (M6 step 31b, consolidation's check): a synthesis's
//! sentences and the sources they cite, each numbered from 1. One Noul per
//! sentence and cited source, "the source supports the sentence": the first
//! ten pairs under `supports`, the next ten under `supports_more` (a per-item
//! Noul asks at most ten). The core hands only a synthesis that passed the
//! deterministic checks (every sentence cites, every cited number is a
//! source), and its sources are the cluster's own nodes; each is scrubbed
//! here and clipped, like every string a state holds.

use super::*;

/// The most pairs one check asks about: twenty.
pub const CITATION_PAIRS: usize = 20;
/// A source's text at most, in characters.
const SOURCE_CHARS: usize = 1500;
/// A sentence at most, in characters.
const SENTENCE_CHARS: usize = 600;
/// Pairs per question: a per-item Noul asks at most ten.
const PER_SOURCE: usize = 10;

/// One sentence of a synthesis, and the sources it cites (numbers from 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitedSentence {
    pub text: String,
    pub cites: Vec<usize>,
}

/// `citation.v1`'s input: the sentences, and the sources by number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitationInput {
    pub sentences: Vec<CitedSentence>,
    /// The sources' texts: `[1]` is the first.
    pub sources: Vec<String>,
}

impl CitationInput {
    /// Each (sentence, source) pair to ask about, both from 1, in order;
    /// a source a sentence cites twice is asked once.
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (i, s) in self.sentences.iter().enumerate() {
            for &c in &s.cites {
                if (1..=self.sources.len()).contains(&c) && !out.contains(&(i + 1, c)) {
                    out.push((i + 1, c));
                }
            }
        }
        out
    }
}

/// The key a pair's answer comes back about: `s<sentence>:<source>`.
pub fn pair_key(sentence: usize, source: usize) -> String {
    format!("s{sentence}:{source}")
}

pub fn citation(i: &CitationInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("citation", CITATION_VERSION, cap, scrub);
    let sentences: Vec<Value> = i
        .sentences
        .iter()
        .enumerate()
        .map(|(n, s)| json!({"sentence": n + 1, "text": c.clip(s.text.trim(), SENTENCE_CHARS)}))
        .collect();
    b.list_head("sentences", 10, share(cap, 25), sentences, 0)
        .cut_if("sentences", c.cut());
    let sources: Vec<Value> = i
        .sources
        .iter()
        .enumerate()
        .map(|(n, t)| json!({"source": n + 1, "text": c.clip(t.trim(), SOURCE_CHARS)}))
        .collect();
    b.list_head("sources", 8, share(cap, 70), sources, 0)
        .cut_if("sources", c.cut());
    let state = b.build();
    let kept = |field: &str, key: &str| -> Vec<usize> {
        state.value()[field]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v[key].as_u64())
                    .map(|n| n as usize)
                    .collect()
            })
            .unwrap_or_default()
    };
    let (have_s, have_c) = (kept("sentences", "sentence"), kept("sources", "source"));
    // Only pairs whose sentence and source the state kept are asked.
    let pairs: Vec<(usize, usize)> = i
        .pairs()
        .into_iter()
        .filter(|(s, c)| have_s.contains(s) && have_c.contains(c))
        .take(CITATION_PAIRS)
        .collect();
    let item = |(s, src): &(usize, usize)| Item {
        key: pair_key(*s, *src),
        text: format!("source {src} and sentence {s}"),
    };
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::Pairs,
        pairs.iter().take(PER_SOURCE).map(item).collect(),
    );
    dynamic.sources.insert(
        Source::MorePairs,
        pairs.iter().skip(PER_SOURCE).map(item).collect(),
    );
    Prepared {
        state: Arc::new(state),
        dynamic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::by_name;
    use crate::state::NoScrub;

    fn input(sentences: usize, cites: &[usize]) -> CitationInput {
        CitationInput {
            sentences: (0..sentences)
                .map(|i| CitedSentence {
                    text: format!("The kestrel relay fact number {i}."),
                    cites: cites.to_vec(),
                })
                .collect(),
            sources: vec![
                "port 7714".into(),
                "logs to /var/log/kestrel".into(),
                "restarts at 3".into(),
            ],
        }
    }

    /// One Noul a pair, keyed `s<sentence>:<source>`, ten under each
    /// question, at most twenty; a pair cited twice is asked once.
    #[test]
    fn each_sentence_and_cited_source_is_one_noul() {
        let p = by_name("citation.v1").unwrap();
        let prepared = prepare(&p, &Input::Citation(input(2, &[1, 3, 3])), &NoScrub).unwrap();
        let asked = p.ask(&prepared.dynamic);
        let keys: Vec<_> = asked.iter().map(|q| q.about.clone().unwrap()).collect();
        assert_eq!(keys, ["s1:1", "s1:3", "s2:1", "s2:3"]);
        assert_eq!(asked[0].id, "supports.1");
        assert!(prepared.state.tokens <= p.state_cap_tokens);
        let many = prepare(&p, &Input::Citation(input(9, &[1, 2, 3])), &NoScrub).unwrap();
        let asked = p.ask(&many.dynamic);
        assert_eq!(asked.len(), 20, "27 pairs, twenty asked");
        assert_eq!(asked[10].id, "supports_more.1");
    }
}
