//! The line that names the configured profiles (issue 37): "use mythos for this" is a request a
//! conversation can only act on if something in its first request says what `mythos` is.
//!
//! The tools that ask a profile or hand it work are deferred (issue 28), so nothing the request
//! carries named the profiles. The line rides in the turn's tail, beside the sentences of the
//! place (`self_card::told`), under exactly the conditions the tools have: the same wall
//! (`profile_tool::refusal`: the owner's own private place, never a task or a shared place) and
//! a place that offers the tool, so no conversation is told of a thing it would be refused. It
//! reads the names from `Config::all_profiles`, as the tools' own list is made, so a profile
//! added or removed is in the next turn's line.

use super::{refusal, ASK};
use crate::ceiling::PlaceView;
use crate::toolrun::{ToolRuntime, TurnCtx};

/// The names the line gives before it says how many more there are.
pub const SHOWN: usize = 12;

/// The line for `names` (in the order given), or none for one profile or none.
pub fn line<'a>(names: impl ExactSizeIterator<Item = &'a str>) -> Option<String> {
    let total = names.len();
    if total < 2 {
        return None;
    }
    let shown: Vec<&str> = names.take(SHOWN).collect();
    let more = match total - shown.len() {
        0 => String::new(),
        n => format!(", and {n} more"),
    };
    Some(format!(
        "Model profiles configured here: {}{more}. You can hand one a piece of work or ask it one \
         question (search the tools for `profile`).",
        shown.join(", ")
    ))
}

/// What the turn is told of the profiles, when its `tc` may use the tools that take them.
pub(crate) fn told(
    cfg: &crate::Config,
    tools: &ToolRuntime,
    place: PlaceView,
    tc: &TurnCtx<'_>,
) -> Option<String> {
    let usable = tools.registry.get(ASK).is_some()
        && place.offered(ASK, &tools.public_roots)
        && refusal(tc, ASK).is_none();
    usable
        .then(|| line(cfg.all_profiles().keys().map(String::as_str)))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(n: usize) -> Option<String> {
        let names: Vec<String> = (0..n).map(|i| format!("p{i}")).collect();
        line(names.iter().map(String::as_str))
    }

    #[test]
    fn one_profile_or_none_has_no_line() {
        assert_eq!(said(0), None);
        assert_eq!(said(1), None);
        assert!(said(2)
            .unwrap()
            .starts_with("Model profiles configured here: p0, p1. "));
    }

    #[test]
    fn a_long_list_is_cut_and_says_how_many_more() {
        assert!(!said(SHOWN).unwrap().contains("more"));
        let cut = said(SHOWN + 3).unwrap();
        assert!(
            cut.contains(&format!("p{}, and 3 more. ", SHOWN - 1)),
            "{cut}"
        );
        assert!(!cut.contains(&format!("p{SHOWN}")), "{cut}");
    }
}
