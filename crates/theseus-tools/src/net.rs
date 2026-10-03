//! Which addresses are not public (DD5, spec §3.9), for every client that
//! must refuse them: the web tools' resolver, and L1's egress proxy (M4
//! design §2.4), which runs in the job wrapper. Moved here from
//! `theseus-core/src/web/net.rs` by 18b, so the wrapper can use it without
//! the core; since 18c the core's resolver reads it from here too.
//!
//! Also an egress list's entry, `Allow` (18b, moved here by 18c): the
//! proxy matches a `CONNECT` against it, `proc.run`'s plan checks a call's
//! `sandbox.egress` with it, and the gate asks whether the operator's list
//! covers a call's.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// One entry of an egress list: `host:port`, with a glob on the host
/// (`*.crates.io:443`) and the exact port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allow {
    host: String,
    port: u16,
}

impl std::str::FromStr for Allow {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (host, port) = split_host_port(s)
            .ok_or_else(|| format!("{s:?} is not host:port, as github.com:443"))?;
        Ok(Self { host, port })
    }
}

impl std::fmt::Display for Allow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.host.contains(':') {
            write!(f, "[{}]:{}", self.host, self.port)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

impl Allow {
    pub fn permits(&self, host: &str, port: u16) -> bool {
        self.port == port && glob(&self.host, host)
    }

    /// Whether every name `other` reaches, this reaches too: the same entry,
    /// or a plain name (no `*`) this one's glob matches. A glob is covered
    /// only by itself, so `*:443` is never taken as inside `*.crates.io:443`.
    pub fn covers(&self, other: &Allow) -> bool {
        self == other || (!other.host.contains('*') && self.permits(&other.host, other.port))
    }
}

/// `name:port` or `[v6]:port`, the name lowercased and without a final dot.
pub fn split_host_port(s: &str) -> Option<(String, u16)> {
    let (host, port) = match s.strip_prefix('[') {
        Some(rest) => rest.split_once("]:")?,
        None => {
            let (h, p) = s.rsplit_once(':')?;
            if h.contains(':') {
                return None;
            }
            (h, p)
        }
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let port: u16 = port.parse().ok()?;
    let name = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '*' | '_' | ':');
    (!host.is_empty() && port != 0 && host.chars().all(name)).then_some((host, port))
}

/// `*` matches any run of characters, dots included (so `*.crates.io` is
/// every name under crates.io, and not crates.io itself); anything else
/// matches itself, without regard to case.
pub fn glob(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.as_bytes(), name.as_bytes());
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, ni));
            pi += 1;
        } else if pi < p.len() && p[pi].eq_ignore_ascii_case(&n[ni]) {
            pi += 1;
            ni += 1;
        } else if let Some((s, m)) = star {
            pi = s + 1;
            ni = m + 1;
            star = Some((s, m + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

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

    /// The one classification (18c: the core's resolver reads this one, so
    /// there is no second copy to keep in step).
    #[test]
    fn each_kind_of_address_that_is_not_public_is_named() {
        let kind = |s: &str| private_kind(s.parse().unwrap());
        for (ip, want) in [
            ("127.0.0.1", "a loopback address"),
            ("127.8.9.10", "a loopback address"),
            ("10.0.0.1", "a private address"),
            ("172.16.5.4", "a private address"),
            ("192.168.1.1", "a private address"),
            ("169.254.169.254", "a link-local address"),
            ("0.0.0.0", "an unspecified address"),
            ("100.101.102.103", "a shared address"),
            ("224.0.0.1", "a multicast or reserved address"),
            ("255.255.255.255", "a multicast or reserved address"),
            ("192.0.2.7", "a reserved address"),
            ("198.18.0.1", "a reserved address"),
            ("::1", "a loopback address"),
            ("::", "an unspecified address"),
            ("fd12:3456::1", "a unique-local address"),
            ("fd00:ec2::254", "a unique-local address"),
            ("fe80::1", "a link-local address"),
            ("::ffff:127.0.0.1", "a loopback address"),
            ("::ffff:169.254.169.254", "a link-local address"),
            ("64:ff9b::10.0.0.1", "a private address"),
            ("2002:7f00:1::", "a loopback address"),
            ("::10.0.0.1", "a private address"),
            ("ff02::1", "a multicast address"),
            ("2001:db8::1", "a documentation address"),
        ] {
            assert_eq!(kind(ip), Some(want), "{ip}");
        }
        for ip in [
            "93.184.215.14",
            "1.1.1.1",
            "172.32.0.1",
            "100.128.0.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
            "2002:0808:0808::",
        ] {
            assert_eq!(kind(ip), None, "{ip}");
        }
    }

    #[test]
    fn a_glob_is_on_the_host_and_the_port_is_exact() {
        let a: Allow = "*.crates.io:443".parse().unwrap();
        assert!(a.permits("index.crates.io", 443));
        assert!(a.permits("a.b.crates.io", 443));
        assert!(!a.permits("crates.io", 443));
        assert!(!a.permits("index.crates.io", 80));
        assert!(!a.permits("evilcrates.io", 443));
        let a: Allow = "GitHub.com.:443".parse().unwrap();
        assert!(a.permits("github.com", 443));
        assert!(!a.permits("api.github.com", 443));
        assert_eq!(a.to_string(), "github.com:443");
        let a: Allow = "[2606:4700::1111]:443".parse().unwrap();
        assert!(a.permits("2606:4700::1111", 443));
        assert_eq!(a.to_string(), "[2606:4700::1111]:443");
        for bad in [
            "github.com",
            "github.com:0",
            ":443",
            "2606:4700::1111:443",
            "x:y",
            "a b.test:443",
            "a/b.test:443",
            "https://github.com:443",
        ] {
            assert!(bad.parse::<Allow>().is_err(), "{bad}");
        }
        assert!(glob("*", "anything.at.all"));
        assert!(glob("a*b*c", "aXXbYYc"));
        assert!(!glob("a*b*c", "aXXbYY"));
    }

    /// The gate's question for a call's extra hosts (18c): whether the
    /// operator's entry already reaches every name the call's does.
    #[test]
    fn an_entry_covers_itself_and_the_plain_names_its_glob_matches() {
        let a = |s: &str| s.parse::<Allow>().unwrap();
        assert!(a("*.crates.io:443").covers(&a("index.crates.io:443")));
        assert!(a("*.crates.io:443").covers(&a("*.crates.io:443")));
        assert!(a("github.com:443").covers(&a("GitHub.com:443")));
        assert!(!a("*.crates.io:443").covers(&a("crates.io:443")));
        assert!(!a("*.crates.io:443").covers(&a("index.crates.io:80")));
        assert!(!a("*.crates.io:443").covers(&a("*.index.crates.io:443")));
        assert!(!a("*.crates.io:443").covers(&a("*:443")));
        assert!(!a("github.com:443").covers(&a("api.github.com:443")));
    }
}
