//! Whether Gmail itself authenticated a message's sender (theseus-g4lit).
//!
//! A `From` header is the sender's own claim, so a trusted address in it
//! proves nothing. Gmail's server prepends an `Authentication-Results` header
//! on arrival (authserv-id `mx.google.com`), above anything the sender wrote,
//! so the **first** such header is Gmail's and any lower one is the sender's.
//! A message is authenticated when that header says DMARC passed for the
//! From's domain, or DKIM passed for a domain aligned with it. Alignment is
//! DMARC's relaxed rule: the two domains have the same organizational domain,
//! the registrable domain under the public suffix list (the `psl` crate's
//! compiled-in copy, private section included, so `github.io` is a suffix and
//! `a.github.io` and `b.github.io` are two organizations). A name with no
//! organizational domain, a public suffix itself, aligns with nothing. A
//! top-level domain the list does not know follows its default rule (`*`).
//! Anything else (no header, another server's, a fail, a misaligned pass) is
//! not: fail closed.

use serde_json::Value;

use crate::google::mime;

/// The authserv-id of Gmail's own receiving servers.
const GMAIL: &str = "mx.google.com";

/// Whether Gmail's own `Authentication-Results` in `payload` passes `from`'s
/// domain by DMARC, or by DKIM aligned with it.
pub fn authenticated(payload: &Value, from: &str) -> bool {
    let Some(domain) = mime::address_of(from)
        .and_then(|a| a.rsplit_once('@').map(|(_, d)| d.to_string()))
        .filter(|d| !d.is_empty())
    else {
        return false;
    };
    let Some(header) = mime::header(payload, "Authentication-Results") else {
        return false;
    };
    let header = without_comments(&header);
    let mut clauses = header.split(';');
    let id = clauses.next().and_then(|c| c.split_whitespace().next());
    if !id.is_some_and(|id| id.eq_ignore_ascii_case(GMAIL)) {
        return false;
    }
    clauses.any(|clause| {
        let mut words = clause.split_whitespace();
        let Some((method, result)) = words.next().and_then(|w| w.split_once('=')) else {
            return false;
        };
        if !result.eq_ignore_ascii_case("pass") {
            return false;
        }
        let prop = |name: &str| {
            clause
                .split_whitespace()
                .filter_map(|w| w.split_once('='))
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim_matches('"').to_ascii_lowercase())
        };
        match method.to_ascii_lowercase().as_str() {
            "dmarc" => prop("header.from").is_some_and(|d| d == domain),
            "dkim" => prop("header.d")
                .or_else(|| prop("header.i").map(|i| i.rsplit('@').next().unwrap_or("").into()))
                .is_some_and(|d| aligned(&d, &domain)),
            _ => false,
        }
    })
}

/// Relaxed alignment: both names have an organizational domain, and it is the
/// same one. Case and a trailing dot do not matter.
fn aligned(a: &str, b: &str) -> bool {
    let org =
        |d: &str| psl::domain_str(&d.trim_end_matches('.').to_ascii_lowercase()).map(str::to_owned);
    matches!((org(a), org(b)), (Some(x), Some(y)) if x == y)
}

/// `text` with its parenthesised comments (nesting allowed) taken out.
fn without_comments(text: &str) -> String {
    let (mut out, mut depth) = (String::new(), 0u32);
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const FROM: &str = "Dana <dana@example.test>";

    fn payload(headers: &[&str]) -> Value {
        let h: Vec<_> = headers
            .iter()
            .map(|v| json!({"name": "Authentication-Results", "value": v}))
            .collect();
        json!({ "headers": h })
    }

    fn passes(headers: &[&str]) -> bool {
        authenticated(&payload(headers), FROM)
    }

    #[test]
    fn authentication_results_are_read_as_gmail_wrote_them() {
        let dmarc = "mx.google.com; dkim=pass header.i=@other.test header.s=s1; \
                     dmarc=pass (p=NONE sp=NONE dis=NONE) header.from=example.test";
        assert!(passes(&[dmarc]), "DMARC pass for the From's domain");
        // DKIM alone, aligned: equal, and a subdomain either way.
        assert!(passes(&[
            "mx.google.com; dkim=pass header.d=example.test header.s=s"
        ]));
        assert!(passes(&[
            "mx.google.com; dkim=pass header.d=mail.example.test"
        ]));
        assert!(authenticated(
            &payload(&["mx.google.com; dkim=pass header.d=example.test"]),
            "dana@mail.example.test"
        ));
        // header.i stands in when header.d is absent.
        assert!(passes(&[
            "mx.google.com; dkim=pass header.i=@example.test header.s=s1"
        ]));
        // An unaligned pass, a bare suffix, a fail, a none, a DMARC for another domain.
        assert!(!passes(&[
            "mx.google.com; dkim=pass header.d=evilexample.test"
        ]));
        assert!(!passes(&[
            "mx.google.com; dkim=pass header.d=elsewhere.test"
        ]));
        assert!(!passes(&["mx.google.com; dkim=fail header.d=example.test"]));
        assert!(!passes(&[
            "mx.google.com; dmarc=none header.from=example.test"
        ]));
        assert!(!passes(&[
            "mx.google.com; dmarc=pass header.from=elsewhere.test"
        ]));
        assert!(!passes(&[
            "mx.google.com; spf=pass smtp.mailfrom=example.test"
        ]));
        // Only the first header is Gmail's; the sender's below it count for nothing.
        let forged = "mx.google.com; dkim=pass header.d=example.test";
        assert!(!passes(&[
            "mx.google.com; dkim=fail header.d=example.test",
            forged
        ]));
        assert!(!passes(&[
            "evil.example.test; dkim=pass header.d=example.test",
            forged
        ]));
        // No header, no address, another server's id.
        assert!(!passes(&[]));
        assert!(!authenticated(&payload(&[forged]), "no address here"));
        assert!(!passes(&[
            "mx.example.test; dkim=pass header.d=example.test"
        ]));
        // Case and comments do not matter.
        assert!(passes(&[
            "MX.Google.COM; DKIM=PASS (good signature) Header.D=Example.Test"
        ]));
        assert!(!passes(&[
            "mx.google.com; dkim=fail (was pass) header.d=example.test"
        ]));
        assert!(!passes(&[
            "mx.google.com (dkim=pass header.d=example.test)"
        ]));
    }

    #[test]
    fn a_pass_for_a_public_suffix_never_aligns() {
        let at = |d: &str, from: &str| {
            authenticated(
                &payload(&[&format!("mx.google.com; dkim=pass header.d={d}")]),
                from,
            )
        };
        assert!(!at("co.uk", "dana@shop.example.co.uk"));
        assert!(!at("github.io", "dana@site.github.io"));
        assert!(!at("other.github.io", "dana@site.github.io"));
        assert!(at("mail.example.co.uk", "dana@example.co.uk"));
    }

    #[test]
    fn hosts_of_one_organization_align() {
        let at = |d: &str, from: &str| {
            authenticated(
                &payload(&[&format!("mx.google.com; dkim=pass header.d={d}")]),
                from,
            )
        };
        assert!(at("news.example.test", "dana@shop.example.test"));
        assert!(!at("example.test.evil.test", "dana@example.test"));
        assert!(at("Example.Test.", "dana@shop.example.test"));
    }
}
