//! Where a web call may connect (DD5, spec §3.9).
//!
//! - A URL whose host is a loopback, private, link-local, unique-local, or
//!   other non-public address, or `localhost`, waits for the operator's
//!   approval at the gate (`private_host`), and so does every redirect hop
//!   to one.
//! - A public name that *resolves* to such an address is refused at connect
//!   (`Dns`), so a DNS answer cannot take a call past the gate. The check is
//!   on the very answer the connection uses: nothing is resolved twice, so a
//!   rebinding name has no second answer to give.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use reqwest::Url;

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

/// Why `url`'s host waits for approval, or None when it is public: an
/// address that is not public, or `localhost`.
pub fn private_host(url: &Url) -> Option<String> {
    // The URL parser has already read every spelling of an address
    // (`2130706433`, `0x7f.1`, `127.1`) as the address itself.
    let host = url.host_str()?;
    if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        let ip: Ipv6Addr = v6.parse().ok()?;
        return v6_kind(ip).map(|k| format!("[{ip}] is {k}"));
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return v4_kind(ip).map(|k| format!("{ip} is {k}"));
    }
    let d = host.trim_end_matches('.').to_ascii_lowercase();
    (d == "localhost" || d.ends_with(".localhost")).then(|| format!("{d} is this machine"))
}

/// `private_host` for a URL as a string; one that does not parse is not
/// judged here (its tool's plan refuses it).
pub fn private_url(url: &str) -> Option<String> {
    private_host(&Url::parse(url).ok()?)
}

/// A name resolved to an address that is not public: the connection is
/// refused, and the result says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub host: String,
    pub ip: IpAddr,
    pub kind: &'static str,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} resolves to {}, {}", self.host, self.ip, self.kind)
    }
}

impl std::error::Error for Refused {}

/// The resolver a web client connects through.
#[derive(Debug, Clone, Default)]
pub struct Dns {
    /// Refuse a name any of whose addresses is not public. Off only for the
    /// client that reaches the private host an operator approved.
    pub check: bool,
    /// Tests: names answered here instead of by the system's resolver.
    pub hosts: BTreeMap<String, Vec<IpAddr>>,
    /// Tests: addresses taken as public (a test's server on 127.0.0.1).
    pub public: Vec<IpAddr>,
}

impl Dns {
    pub fn checked() -> Self {
        Self {
            check: true,
            ..Self::default()
        }
    }
}

impl reqwest::dns::Resolve for Dns {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().trim_end_matches('.').to_ascii_lowercase();
        let fixed = self.hosts.get(&host).cloned();
        let (check, public) = (self.check, self.public.clone());
        Box::pin(async move {
            let ips: Vec<IpAddr> = match fixed {
                Some(ips) => ips,
                None => tokio::net::lookup_host((host.as_str(), 0))
                    .await?
                    .map(|a| a.ip())
                    .collect(),
            };
            if check {
                let bad = ips.iter().find_map(|ip| {
                    private_kind(*ip)
                        .filter(|_| !public.contains(ip))
                        .map(|kind| (*ip, kind))
                });
                if let Some((ip, kind)) = bad {
                    return Err(Box::new(Refused { host, ip, kind }) as _);
                }
            }
            let addrs: reqwest::dns::Addrs =
                Box::new(ips.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addrs)
        })
    }
}

/// The refusal behind a failed request, if the resolver refused it.
pub fn refused_in(e: &(dyn std::error::Error + 'static)) -> Option<Refused> {
    let mut at = Some(e);
    while let Some(e) = at {
        if let Some(r) = e.downcast_ref::<Refused>() {
            return Some(r.clone());
        }
        at = e.source();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_url_names_a_private_host_in_every_spelling() {
        for (url, why) in [
            ("http://127.0.0.1:7433/", "127.0.0.1 is a loopback address"),
            ("http://localhost/", "localhost is this machine"),
            ("http://LOCALHOST./x", "localhost is this machine"),
            (
                "http://app.localhost:3000/",
                "app.localhost is this machine",
            ),
            ("http://[::1]/", "[::1] is a loopback address"),
            ("http://10.0.0.1/", "10.0.0.1 is a private address"),
            (
                "http://169.254.169.254/latest/meta-data/",
                "169.254.169.254 is a link-local address",
            ),
            // The URL parser reads each of these as 127.0.0.1.
            ("http://2130706433/", "127.0.0.1 is a loopback address"),
            ("http://0x7f.1/", "127.0.0.1 is a loopback address"),
            ("http://127.1/", "127.0.0.1 is a loopback address"),
            (
                "http://[::ffff:7f00:1]/",
                "[::ffff:127.0.0.1] is a loopback address",
            ),
            ("http://0.0.0.0:7433/", "0.0.0.0 is an unspecified address"),
        ] {
            assert_eq!(private_url(url).as_deref(), Some(why), "{url}");
        }
        for url in [
            "https://doc.rust-lang.org/std/",
            "https://example.com/localhost",
            "http://localhost.example.com/",
            "https://93.184.215.14/",
        ] {
            assert_eq!(private_url(url), None, "{url}");
        }
    }

    #[tokio::test]
    async fn a_name_that_resolves_to_a_private_address_is_refused() {
        use reqwest::dns::Resolve;
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        let pub_ip: IpAddr = "93.184.215.14".parse().unwrap();
        let mut dns = Dns::checked();
        dns.hosts.insert("rebind.test".into(), vec![lo]);
        dns.hosts.insert("mixed.test".into(), vec![pub_ip, lo]);
        dns.hosts.insert("fine.test".into(), vec![pub_ip]);
        let name = |n: &str| n.parse::<reqwest::dns::Name>().unwrap();
        for n in ["rebind.test", "mixed.test", "REBIND.test."] {
            let e = match dns.resolve(name(n)).await {
                Ok(_) => panic!("{n} resolved"),
                Err(e) => e,
            };
            assert_eq!(
                refused_in(e.as_ref()),
                Some(Refused {
                    host: n.trim_end_matches('.').to_ascii_lowercase(),
                    ip: lo,
                    kind: "a loopback address"
                })
            );
        }
        let ok: Vec<SocketAddr> = dns.resolve(name("fine.test")).await.unwrap().collect();
        assert_eq!(ok, vec![SocketAddr::new(pub_ip, 0)]);
        // Taken as public, as a test's server is; unchecked, as the approved client is.
        dns.public = vec![lo];
        assert!(dns.resolve(name("rebind.test")).await.is_ok());
        let open = Dns {
            check: false,
            ..dns.clone()
        };
        assert!(open.resolve(name("mixed.test")).await.is_ok());
    }
}
