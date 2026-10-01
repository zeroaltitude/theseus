//! Is an address block public? A block is private only when all of it lies in private space: RFC 1918,
//! the shared range of RFC 6598, loopback, and link-local; for IPv6, unique-local, link-local, and
//! loopback. Anything else reaches beyond private space, so `0.0.0.0/1` and a single internet address
//! are public, as `0.0.0.0/0` is.

use std::net::{Ipv4Addr, Ipv6Addr};

const PRIVATE_V4: [(u32, u8); 6] = [
    (0x0a00_0000, 8),  // 10.0.0.0/8
    (0xac10_0000, 12), // 172.16.0.0/12
    (0xc0a8_0000, 16), // 192.168.0.0/16
    (0x6440_0000, 10), // 100.64.0.0/10, shared address space
    (0x7f00_0000, 8),  // 127.0.0.0/8
    (0xa9fe_0000, 16), // 169.254.0.0/16
];

const PRIVATE_V6: [(u128, u8); 3] = [
    (0xfc00_u128 << 112, 7),  // fc00::/7, unique local
    (0xfe80_u128 << 112, 10), // fe80::/10, link local
    (1, 128),                 // ::1
];

/// `Some(true)` when the block reaches beyond private space, `Some(false)` when all of it is private,
/// and `None` when the text is not an address or a CIDR block.
pub fn is_public(text: &str) -> Option<bool> {
    let text = text.trim();
    let (addr, len) = match text.split_once('/') {
        Some((a, l)) => (a, Some(l.parse::<u8>().ok()?)),
        None => (text, None),
    };
    if let Ok(v4) = addr.parse::<Ipv4Addr>() {
        let len = len.unwrap_or(32);
        if len > 32 {
            return None;
        }
        return Some(!v4_private(u32::from(v4), len));
    }
    let v6 = addr.parse::<Ipv6Addr>().ok()?;
    let len = len.unwrap_or(128);
    if len > 128 {
        return None;
    }
    let bits = u128::from(v6);
    // An IPv4-mapped block (::ffff:0:0/96) is its IPv4 block.
    if len >= 96 && bits >> 32 == 0xffff {
        return Some(!v4_private(bits as u32, len - 96));
    }
    Some(
        !PRIVATE_V6
            .iter()
            .any(|&(net, plen)| len >= plen && mask128(bits, plen) == net),
    )
}

fn v4_private(bits: u32, len: u8) -> bool {
    PRIVATE_V4
        .iter()
        .any(|&(net, plen)| len >= plen && mask32(bits, plen) == net)
}

fn mask32(bits: u32, len: u8) -> u32 {
    if len == 0 {
        0
    } else {
        bits & (u32::MAX << (32 - len))
    }
}

fn mask128(bits: u128, len: u8) -> u128 {
    if len == 0 {
        0
    } else {
        bits & (u128::MAX << (128 - len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_space_is_not_public_and_the_rest_is() {
        for private in [
            "10.0.0.0/8",
            "10.42.0.0/16",
            "172.16.0.0/12",
            "172.31.255.0/24",
            "192.168.1.7",
            "100.64.0.0/10",
            "127.0.0.1/32",
            "169.254.169.254/32",
            "fd00:1234::/48",
            "fe80::1/128",
            "::1/128",
            "::ffff:10.0.0.0/104",
        ] {
            assert_eq!(is_public(private), Some(false), "{private}");
        }
        for public in [
            "0.0.0.0/0",
            "0.0.0.0/1",
            "128.0.0.0/1",
            "10.0.0.0/7",
            "172.0.0.0/8",
            "203.0.113.5/32",
            "198.51.100.7",
            "8.8.8.8/32",
            "::/0",
            "2001:db8::/32",
            "::ffff:0:0/96",
        ] {
            assert_eq!(is_public(public), Some(true), "{public}");
        }
        for nonsense in ["", "anywhere", "10.0.0.0/33", "1.2.3/8", "::/129"] {
            assert_eq!(is_public(nonsense), None, "{nonsense}");
        }
    }
}
