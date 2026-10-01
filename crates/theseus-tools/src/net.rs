//! Which addresses are not public (DD5, spec §3.9), for every client that
//! must refuse them: the web tools' resolver, and L1's egress proxy (M4
//! design §2.4), which runs in the job wrapper.
//!
//! Moved here from `theseus-core/src/web/net.rs` by 18b, so the wrapper can
//! use it without the core. Until 18c points the core at this module, the
//! two copies are kept word for word by a test below.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// What kind of non-public address `ip` is, or None for a public one.
pub fn private_kind(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(v4) => v4_kind(v4),
        IpAddr::V6(v6) => v6_kind(v6),
    }
}

fn v4_kind(ip: Ipv4Addr) -> Option<&'static str> {
    let [a, b, c, _] = ip.octets();
    Some(if ip.is_loopback() {
        "a loopback address"
    } else if ip.is_private() {
        "a private address"
    } else if ip.is_link_local() {
        // 169.254.169.254 is a cloud's metadata service.
        "a link-local address"
    } else if a == 0 {
        // 0.0.0.0 reaches this machine.
        "an unspecified address"
    } else if a == 100 && (b & 0xc0) == 64 {
        // 100.64.0.0/10: a carrier's NAT, and a tailnet's addresses.
        "a shared address"
    } else if a >= 224 {
        "a multicast or reserved address"
    } else if (a, b, c) == (192, 0, 0)
        || (a, b, c) == (192, 0, 2)
        || (a, b, c) == (198, 51, 100)
        || (a, b, c) == (203, 0, 113)
        || (a == 198 && (b & 0xfe) == 18)
    {
        "a reserved address"
    } else {
        return None;
    })
}

fn v6_kind(ip: Ipv6Addr) -> Option<&'static str> {
    let s = ip.segments();
    let embedded =
        |hi: u16, lo: u16| v4_kind(Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo)));
    // An IPv4 address inside IPv6 is judged as itself: mapped (::ffff:a.b.c.d),
    // NAT64 (64:ff9b::a.b.c.d), 6to4 (2002:aabb:ccdd::), compatible (::a.b.c.d).
    if let Some(v4) = ip.to_ipv4_mapped() {
        return v4_kind(v4);
    }
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return embedded(s[6], s[7]);
    }
    if s[0] == 0x2002 {
        return embedded(s[1], s[2]);
    }
    Some(if ip.is_loopback() {
        "a loopback address"
    } else if ip.is_unspecified() {
        "an unspecified address"
    } else if s[..6] == [0; 6] {
        return embedded(s[6], s[7]);
    } else if (s[0] & 0xfe00) == 0xfc00 {
        "a unique-local address"
    } else if (s[0] & 0xffc0) == 0xfe80 {
        "a link-local address"
    } else if (s[0] & 0xffc0) == 0xfec0 {
        "a site-local address"
    } else if (s[0] & 0xff00) == 0xff00 {
        "a multicast address"
    } else if s[0] == 0x2001 && s[1] == 0x0db8 {
        "a documentation address"
    } else {
        return None;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One function's text, from its signature to its closing brace.
    fn function<'a>(src: &'a str, signature: &str) -> &'a str {
        let at = src.find(signature).unwrap_or_else(|| panic!("{signature}"));
        let end = src[at..].find("\n}\n").expect("the function's end") + at + 3;
        &src[at..end]
    }

    #[test]
    fn the_classification_is_the_cores_word_for_word() {
        let core = include_str!("../../theseus-core/src/web/net.rs");
        let ours = include_str!("net.rs");
        for signature in [
            "pub fn private_kind(ip: IpAddr)",
            "fn v4_kind(ip: Ipv4Addr)",
            "fn v6_kind(ip: Ipv6Addr)",
        ] {
            assert_eq!(
                function(core, signature),
                function(ours, signature),
                "{signature} differs from theseus-core's: change both, or point the core here"
            );
        }
    }

    #[test]
    fn each_kind_of_address_that_is_not_public_is_named() {
        let kind = |s: &str| private_kind(s.parse().unwrap());
        for (ip, want) in [
            ("127.0.0.1", "a loopback address"),
            ("10.0.0.1", "a private address"),
            ("169.254.169.254", "a link-local address"),
            ("0.0.0.0", "an unspecified address"),
            ("100.101.102.103", "a shared address"),
            ("::1", "a loopback address"),
            ("fd00:ec2::254", "a unique-local address"),
            ("::ffff:169.254.169.254", "a link-local address"),
        ] {
            assert_eq!(kind(ip), Some(want), "{ip}");
        }
        for ip in ["1.1.1.1", "2606:4700::1111", "::ffff:8.8.8.8"] {
            assert_eq!(kind(ip), None, "{ip}");
        }
    }
}
