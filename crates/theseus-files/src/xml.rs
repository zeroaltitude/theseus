//! Just enough XML to read Office, OpenDocument, and EPUB files
//! (theseus-c9l6): tags with their attributes, and text with its entities
//! decoded. Their XML is written by programs, so a small scanner reads it;
//! what it does not understand (a DTD, a processing instruction, a comment)
//! it skips. No dependency.

use std::borrow::Cow;

/// One piece of a document, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Item<'a> {
    /// `<name a="1">`, or `<name/>` when `empty`.
    Open {
        name: &'a str,
        attrs: Vec<(&'a str, Cow<'a, str>)>,
        empty: bool,
    },
    /// `</name>`.
    Close { name: &'a str },
    /// Text between tags, entities decoded; CDATA as it is.
    Text(Cow<'a, str>),
}

impl Item<'_> {
    /// A tag's name without its prefix: `w:p` is `p`.
    pub fn local(name: &str) -> &str {
        name.rsplit_once(':').map_or(name, |(_, l)| l)
    }
}

/// The value of attribute `key` (with its prefix, or without one).
pub fn attr<'b>(attrs: &'b [(&str, Cow<'_, str>)], key: &str) -> Option<&'b str> {
    attrs
        .iter()
        .find(|(k, _)| *k == key || Item::local(k) == key)
        .map(|(_, v)| v.as_ref())
}

/// Every item of `xml`, in order. Malformed input ends the walk where it
/// stops making sense; what came before is kept.
pub fn items(xml: &str) -> Vec<Item<'_>> {
    let mut out = Vec::new();
    let b = xml.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let end = memchr(b'<', &b[i..]).map_or(b.len(), |n| i + n);
            let text = &xml[i..end];
            if !text.is_empty() {
                out.push(Item::Text(unescape(text)));
            }
            i = end;
            continue;
        }
        let rest = &xml[i..];
        if let Some(r) = rest.strip_prefix("<!--") {
            i += 4 + r.find("-->").map_or(r.len(), |n| n + 3);
        } else if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let n = r.find("]]>").unwrap_or(r.len());
            out.push(Item::Text(Cow::Borrowed(&r[..n])));
            i += 9 + (n + 3).min(r.len());
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            i += rest.find('>').map_or(rest.len(), |n| n + 1);
        } else {
            let Some(n) = tag_end(rest) else { break };
            let tag = &rest[1..n];
            i += n + 1;
            if let Some(name) = tag.strip_prefix('/') {
                out.push(Item::Close { name: name.trim() });
            } else {
                let empty = tag.ends_with('/');
                let tag = tag.trim_end_matches('/');
                let (name, attrs) = parse_tag(tag);
                out.push(Item::Open { name, attrs, empty });
            }
        }
    }
    out
}

fn memchr(c: u8, b: &[u8]) -> Option<usize> {
    b.iter().position(|x| *x == c)
}

/// Where a tag that starts `s` ends: its `>`, outside quoted values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (n, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(n),
            _ => {}
        }
    }
    None
}

fn parse_tag(tag: &str) -> (&str, Vec<(&str, Cow<'_, str>)>) {
    let tag = tag.trim();
    let name_end = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
    let name = &tag[..name_end];
    let mut attrs = Vec::new();
    let mut rest = &tag[name_end..];
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(close) = after[1..].find(q) else {
            break;
        };
        attrs.push((key, unescape(&after[1..1 + close])));
        rest = &after[close + 2..];
    }
    (name, attrs)
}

/// Text with its entities decoded: the five of XML, numeric ones, and the
/// few of HTML an EPUB's XHTML uses most.
pub fn unescape(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let Some(semi) = after.find(';').filter(|n| *n <= 10) else {
            out.push('&');
            rest = after;
            continue;
        };
        let name = &after[..semi];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "copy" => Some('©'),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16)
                .ok()
                .and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => {
                out.push('&');
                out.push_str(name);
                out.push(';');
            }
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_attributes_and_text_come_out_in_order() {
        let x = r#"<?xml version="1.0"?><!-- a note --><w:p w:val='H1' a="x &amp; y"><w:t>Tide &lt;high&gt;</w:t><w:br/><![CDATA[raw <b>]]></w:p>"#;
        let got = items(x);
        assert_eq!(
            got[0],
            Item::Open {
                name: "w:p",
                attrs: vec![("w:val", "H1".into()), ("a", "x & y".into())],
                empty: false
            }
        );
        assert_eq!(got[2], Item::Text("Tide <high>".into()));
        assert_eq!(
            got[4],
            Item::Open {
                name: "w:br",
                attrs: vec![],
                empty: true
            }
        );
        assert_eq!(got[5], Item::Text("raw <b>".into()));
        assert_eq!(got[6], Item::Close { name: "w:p" });
        assert_eq!(Item::local("w:p"), "p");
        if let Item::Open { attrs, .. } = &got[0] {
            assert_eq!(attr(attrs, "val"), Some("H1"));
        }
        assert_eq!(
            unescape("a&#8217;s &#x41; &mdash; &bogus; & b"),
            "a’s A — &bogus; & b"
        );
    }

    #[test]
    fn a_torn_document_keeps_what_came_before() {
        let got = items("<a>one</a><b x=\"unclosed");
        assert_eq!(got.len(), 3);
        assert_eq!(got[1], Item::Text("one".into()));
    }
}
