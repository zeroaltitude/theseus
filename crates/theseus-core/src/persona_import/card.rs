//! The specialization card, drafted from the files with no model: a few
//! lines (about 60 tokens) of what the persona is for and what it knows. It
//! is what route.v5's persona question reads, so it is marked a draft until
//! the owner has read it.

/// The most headings the draft names.
const HEADINGS: usize = 6;
/// The draft's cap, in characters (`CARD_MAX` is 600; room is left to edit).
const MAX: usize = 420;

/// Role lines in an identity file, in the order a draft prefers them:
/// `- **Role:** …`, `Role: …`, `## Role` and the line under it.
const ROLE_KEYS: [&str; 6] = [
    "role",
    "focus",
    "specialization",
    "specialty",
    "creature",
    "vibe",
];

/// The draft from the identity file's text and the agents file's text; none
/// when neither says anything to draw from.
pub fn draft(identity: Option<&str>, agents: Option<&str>) -> Option<String> {
    let role = identity.and_then(role_line);
    let heads = agents.map(headings).unwrap_or_default();
    let mut out = String::new();
    if let Some(r) = &role {
        out.push_str(r.trim_end_matches('.'));
        out.push('.');
    }
    if !heads.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str("Areas: ");
        out.push_str(&heads.join("; "));
        out.push('.');
    }
    if out.is_empty() {
        return None;
    }
    Some(cut(&out))
}

fn cut(s: &str) -> String {
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let t: String = s.chars().take(MAX - 1).collect();
    format!("{}…", t.trim_end())
}

/// The first role-like line of an identity file, as plain words.
pub fn role_line(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for key in ROLE_KEYS {
        for (i, l) in lines.iter().enumerate() {
            let t = l.trim().trim_start_matches(['-', '*', ' ']).trim();
            let plain = t.replace("**", "");
            let lower = plain.to_lowercase();
            if let Some(rest) = lower.strip_prefix(key) {
                let rest_at = plain.len() - rest.len();
                let tail = plain[rest_at..].trim_start();
                if let Some(v) = tail.strip_prefix(':') {
                    let v = v.trim();
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                    if let Some(n) = lines.get(i + 1) {
                        let n = n.trim();
                        if !n.is_empty() && !n.starts_with('#') {
                            return Some(n.to_string());
                        }
                    }
                }
            }
            // `## Role` and the line under it.
            if t.starts_with('#') && t.trim_start_matches('#').trim().eq_ignore_ascii_case(key) {
                if let Some(n) = lines.get(i + 1).map(|n| n.trim()) {
                    if !n.is_empty() && !n.starts_with('#') {
                        return Some(n.to_string());
                    }
                }
            }
        }
    }
    None
}

/// The first headings of level 1 and 2, as plain words, once each.
pub fn headings(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut fenced = false;
    for l in text.lines() {
        if l.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        if fenced {
            continue;
        }
        let t = l.trim_end();
        let level = t.len() - t.trim_start_matches('#').len();
        if !(1..=2).contains(&level) || !t[level..].starts_with(' ') {
            continue;
        }
        let name = t[level..].trim().replace(['*', '`'], "").trim().to_string();
        if !name.is_empty() && !out.iter().any(|h| h.eq_ignore_ascii_case(&name)) {
            out.push(name);
        }
        if out.len() == HEADINGS {
            break;
        }
    }
    out
}
