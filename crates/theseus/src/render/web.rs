//! Health's `web:` line (theseus-jxau): what the web port refused, the owner
//! check, and the dev origin, in the words of the cockpit's header
//! indicator. Apart from `render.rs`, whose length the shape budget caps.

use theseus_protocol::WebStatus;

use super::{push, Line, Tag};

/// The line, loud (the bad tag) where the owner check is off. Health carries
/// no word for a web UI that is switched off, so it reads as `web: ok`.
pub(super) fn push_health(o: &mut Vec<Line>, w: &WebStatus) {
    let tag = match w.peer_unchecked {
        Some(_) => Tag::Bad,
        None => Tag::Plain,
    };
    push(o, tag, &web_line(w));
}

/// `web: refused host 3, origin 1, peer 2; dev origin localhost:5173 (served
/// 4)`, or `web: ok` when nothing was refused and the dev origin is off; the
/// owner check's absence is said first: `web: OWNER CHECK OFF (reason): any
/// local user is served; ...`.
pub fn web_line(w: &WebStatus) -> String {
    let mut parts = Vec::new();
    if let Some(why) = &w.peer_unchecked {
        parts.push(format!(
            "OWNER CHECK OFF ({why}): a local process of any user is served"
        ));
    }
    let refused = [
        ("host", w.refused_host),
        ("origin", w.refused_origin),
        ("peer", w.refused_peer),
    ];
    if refused.iter().any(|(_, n)| *n > 0) {
        parts.push(format!(
            "refused {}",
            refused
                .iter()
                .map(|(what, n)| format!("{what} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(dev) = &w.dev_origin {
        parts.push(format!("dev origin {dev} (served {})", w.dev_origin_served));
    }
    match parts.is_empty() {
        true => "web: ok".to_string(),
        false => format!("web: {}", parts.join("; ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_web_line_says_refusals_the_dev_origin_and_the_owner_check() {
        let mut w = WebStatus::default();
        assert_eq!(web_line(&w), "web: ok");
        w.refused_host = 3;
        w.refused_origin = 1;
        w.refused_peer = 2;
        assert_eq!(web_line(&w), "web: refused host 3, origin 1, peer 2");
        w.dev_origin = Some("localhost:5173".into());
        w.dev_origin_served = 4;
        assert_eq!(
            web_line(&w),
            "web: refused host 3, origin 1, peer 2; dev origin localhost:5173 (served 4)"
        );
        w.refused_host = 0;
        w.refused_origin = 0;
        w.refused_peer = 0;
        assert_eq!(web_line(&w), "web: dev origin localhost:5173 (served 4)");
        w.dev_origin = None;
        w.peer_unchecked = Some("no table of socket owners here".into());
        assert_eq!(
            web_line(&w),
            "web: OWNER CHECK OFF (no table of socket owners here): a local process of any \
             user is served"
        );
    }

    #[test]
    fn the_owner_checks_absence_is_loud_and_the_rest_is_quiet() {
        let mut quiet = Vec::new();
        push_health(&mut quiet, &WebStatus::default());
        assert_eq!(quiet.len(), 1);
        assert_eq!(quiet[0].tag, Tag::Plain);
        let mut loud = Vec::new();
        let w = WebStatus {
            peer_unchecked: Some("invented reason".into()),
            ..WebStatus::default()
        };
        push_health(&mut loud, &w);
        assert_eq!(loud[0].tag, Tag::Bad);
    }
}
