//! `families.v1` (theseus-xlri5): the message in focus, trimmed, and the tool
//! families its request hides, numbered from 1, each with what it holds and up
//! to three of its tools (the wire name and the first sentence of its
//! description). One Noul per family, "answering the message needs a tool of
//! it": the first ten under `relevant`, the next ten under `relevant_more` (a
//! per-item Noul asks at most ten). Every string is scrubbed here and clipped,
//! like every string a state holds.
//!
//! Its dynamic items are the families the state kept, each keyed by the
//! family's name and named by its number: a family the cap left out has no
//! Noul, so Jev is never asked about a family it cannot see.

use super::*;

/// The most families one request asks about: twenty.
pub const FAMILIES_MAX: usize = 20;
/// A family's tools shown at most.
pub const FAMILY_TOOLS: usize = 3;
/// Families per question: a per-item Noul asks at most ten.
const PER_QUESTION: usize = 10;
/// A tool's line at most, in characters.
const TOOL_CHARS: usize = 160;
/// The message at most, in characters, before the state's own cap.
const MESSAGE_CHARS: usize = 2000;

/// One tool of a family: its wire name and what it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyTool {
    pub name: String,
    pub does: String,
}

/// One hidden family: its name (what the answer comes back about), what it
/// holds, and some of its tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyItem {
    pub key: String,
    #[serde(default)]
    pub holds: String,
    #[serde(default)]
    pub tools: Vec<FamilyTool>,
}

/// `families.v1`'s input: the message, and the hidden families.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamiliesInput {
    pub message: String,
    #[serde(default)]
    pub families: Vec<FamilyItem>,
}

pub fn families(i: &FamiliesInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let fams = &i.families[..i.families.len().min(FAMILIES_MAX)];
    let mut b = StateBuilder::new("families", FAMILIES_VERSION, cap, scrub);
    b.text(
        "message",
        10,
        share(cap, 25),
        Keep::Both,
        &c.clip(i.message.trim(), MESSAGE_CHARS),
    )
    .cut_if("message", c.cut());
    let items: Vec<Value> = fams
        .iter()
        .enumerate()
        .map(|(n, f)| {
            let tools: Vec<Value> = f
                .tools
                .iter()
                .take(FAMILY_TOOLS)
                .map(|t| json!({"tool": c.clip(&t.name, 80), "does": c.clip(t.does.trim(), TOOL_CHARS)}))
                .collect();
            json!({
                "family": n + 1,
                "name": c.clip(&f.key, 60),
                "holds": c.clip(f.holds.trim(), TOOL_CHARS),
                "tools": tools,
            })
        })
        .collect();
    b.list_head(
        "families",
        8,
        share(cap, 70),
        items,
        left_out(&i.families, fams.len()),
    )
    .cut_if("families", c.cut());
    let state = b.build();
    let kept: Vec<usize> = state.value()["families"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v["family"].as_u64())
                .map(|n| n as usize)
                .filter(|n| (1..=fams.len()).contains(n))
                .collect()
        })
        .unwrap_or_default();
    let item = |&n: &usize| Item {
        key: fams[n - 1].key.clone(),
        text: n.to_string(),
    };
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::Families,
        kept.iter().take(PER_QUESTION).map(item).collect(),
    );
    dynamic.sources.insert(
        Source::MoreFamilies,
        kept.iter().skip(PER_QUESTION).map(item).collect(),
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

    fn input(n: usize, tools: usize) -> FamiliesInput {
        FamiliesInput {
            message: "  which cloud account am I on?  ".into(),
            families: (0..n)
                .map(|i| FamilyItem {
                    key: format!("fam-{i}"),
                    holds: "who the account is, and any API call".into(),
                    tools: (0..tools)
                        .map(|t| FamilyTool {
                            name: format!("fam_{i}_tool_{t}"),
                            does: "Ask the account who it is.".into(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// One Noul a family, keyed by its name, ten under each question, at most twenty; a family
    /// shows at most three tools.
    #[test]
    fn each_family_is_asked_once_with_three_of_its_tools() {
        let p = by_name("families.v1").unwrap();
        let prepared = prepare(&p, &Input::Families(input(24, 5)), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["message"], "which cloud account am I on?");
        assert_eq!(v["families"][0]["tools"].as_array().unwrap().len(), 3);
        let asked = p.ask(&prepared.dynamic);
        assert_eq!(asked.len(), FAMILIES_MAX);
        assert_eq!(asked[0].id, "relevant.1");
        assert_eq!(asked[0].about.as_deref(), Some("fam-0"));
        assert_eq!(asked[10].id, "relevant_more.1");
        assert_eq!(asked[10].about.as_deref(), Some("fam-10"));
        assert!(prepared.state.tokens <= p.state_cap_tokens);
        // Nothing hidden: nothing asked.
        let none = prepare(&p, &Input::Families(input(0, 0)), &NoScrub).unwrap();
        assert!(p.ask(&none.dynamic).is_empty());
    }
}
