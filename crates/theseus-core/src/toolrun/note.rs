//! The tools note's posture line (theseus-rwpbg.3, T3).
//!
//! Said as exceptions: the posture most tools share once, and by name only the
//! tools whose posture differs. A line that named every tool under its posture
//! cost 325 tokens in every private request, over tools the request does not
//! carry. The model reads the line by wire names, the ones it calls. Enforcement
//! is the policy's; the note only tells.

use crate::policy::Posture;

/// What the note says when it names no exception.
const ALL_BUT: &str = "every other tool";

/// `tools` (wire name, posture) as the note's line: `open: a, b; approve: c; every other tool: notify`.
pub(crate) fn say(tools: &[(String, Posture)]) -> String {
    let share = |p: Posture| tools.iter().filter(|(_, q)| *q == p).count();
    let Some(common) = Posture::ALL
        .into_iter()
        .max_by_key(|p| (share(*p), std::cmp::Reverse(*p as u8)))
    else {
        return String::new();
    };
    if tools.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Posture::ALL
        .into_iter()
        .filter(|p| *p != common)
        .filter_map(|p| {
            let names: Vec<&str> = tools
                .iter()
                .filter(|(_, q)| *q == p)
                .map(|(n, _)| n.as_str())
                .collect();
            (!names.is_empty()).then(|| format!("{}: {}", p.as_str(), names.join(", ")))
        })
        .collect();
    let rest = if parts.is_empty() {
        "every tool"
    } else {
        ALL_BUT
    };
    parts.push(format!("{rest}: {}", common.as_str()));
    parts.join("; ")
}

/// The posture the line `say` made gives each of `names`: the inverse a test holds it to.
#[cfg(test)]
pub(crate) fn read<'a>(
    line: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Vec<(String, Posture)> {
    let mut named = std::collections::BTreeMap::new();
    let mut common = None;
    for part in line.split("; ") {
        let (head, list) = part.split_once(": ").expect("a posture and its tools");
        let posture = Posture::parse(list).filter(|_| head.starts_with("every"));
        if let Some(p) = posture {
            common = Some(p);
            continue;
        }
        let p = Posture::parse(head).expect("a posture");
        for n in list.split(", ") {
            named.insert(n.to_string(), p);
        }
    }
    names
        .into_iter()
        .map(|n| {
            (
                n.to_string(),
                named.get(n).copied().or(common).expect("a posture"),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools(list: &[(&str, Posture)]) -> Vec<(String, Posture)> {
        list.iter().map(|(n, p)| (n.to_string(), *p)).collect()
    }

    #[test]
    fn the_commonest_posture_is_said_once_and_the_rest_by_name() {
        use Posture::*;
        let t = tools(&[
            ("fs_read", Open),
            ("fs_edit", Notify),
            ("proc_run", Notify),
            ("aws_call", Approve),
            ("web_search", Notify),
        ]);
        assert_eq!(
            say(&t),
            "open: fs_read; approve: aws_call; every other tool: notify"
        );
        assert_eq!(say(&tools(&[("a", Open), ("b", Open)])), "every tool: open");
        assert_eq!(say(&[]), "");
        // A tie goes to the milder posture, so the line is the same however the registry is ordered.
        assert_eq!(
            say(&tools(&[("a", Open), ("b", Approve)])),
            "approve: b; every other tool: open"
        );
    }

    #[test]
    fn what_is_said_reads_back_as_each_toolss_posture() {
        use Posture::*;
        let t = tools(&[("a", Open), ("b", Notify), ("c", Notify), ("d", Approve)]);
        let line = say(&t);
        let back = read(&line, t.iter().map(|(n, _)| n.as_str()));
        assert_eq!(back, t);
    }
}
