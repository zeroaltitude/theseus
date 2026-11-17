//! The code an authenticator app shows: TOTP, RFC 6238 over HOTP, RFC 4226,
//! with HMAC-SHA1, six digits and thirty-second steps, which is what every
//! authenticator app implements by default. About forty lines and the RFC's
//! own vectors, so no TOTP crate stands between the owner's phone and this
//! check.
//!
//! Pure: the time is passed in, so a test never reads the clock, and a secret
//! never leaves this module except as the bytes its caller handed in.

use hmac::{Hmac, Mac};
use sha1::Sha1;
use zeroize::Zeroize;

/// The secret's size: 20 random bytes, the length of SHA-1's own output and
/// the size RFC 4226 recommends.
pub const SECRET_BYTES: usize = 20;

/// Digits in a code.
pub const DIGITS: u32 = 6;

/// Seconds in a step.
pub const STEP_SECS: u64 = 30;

/// A code is checked against this many steps either side of the present one:
/// a phone's clock a half-minute off still works, and no more.
pub const WINDOW: u64 = 1;

/// The secret bytes, wiped when dropped, and never printed: its `Debug` says
/// nothing of the value, so a stray `{:?}` of anything that holds one (a
/// state, a result, an error) leaks nothing.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret([u8; SECRET_BYTES]);

impl Secret {
    pub fn from_bytes(bytes: [u8; SECRET_BYTES]) -> Self {
        Self(bytes)
    }

    pub(super) fn bytes(&self) -> &[u8; SECRET_BYTES] {
        &self.0
    }

    /// The secret as an authenticator app takes it typed: base32, upper
    /// case, no padding.
    pub fn base32(&self) -> String {
        base32(&self.0)
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// HOTP (RFC 4226 §5.3): HMAC-SHA1 of the counter, dynamically truncated to
/// `digits` decimal digits.
fn hotp(key: &[u8], counter: u64, digits: u32) -> u32 {
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(key).expect("HMAC takes a key of any size");
    mac.update(&counter.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let at = usize::from(h[h.len() - 1] & 0x0f);
    let bin = u32::from_be_bytes([h[at] & 0x7f, h[at + 1], h[at + 2], h[at + 3]]);
    bin % 10u32.pow(digits)
}

/// The step `unix_secs` falls in.
pub fn step_at(unix_secs: u64) -> u64 {
    unix_secs / STEP_SECS
}

/// The code for `step`, six digits, zero-padded.
pub fn code_at_step(secret: &Secret, step: u64) -> String {
    format!(
        "{:0width$}",
        hotp(secret.bytes(), step, DIGITS),
        width = DIGITS as usize
    )
}

/// Why a code was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// Not six digits, or the right shape and not any code of the window.
    Wrong,
    /// A code of the window, but of a step already used or an earlier one.
    Used,
}

/// Check `given` against the steps around `unix_secs`. The accepted step is
/// returned, so the caller stores it: a code for that step or an earlier one
/// is `Used` from then on, which is what makes a code single-use.
///
/// Spaces are ignored (an app shows `123 456`). Every candidate is compared
/// in constant time and all of them are compared, so the time taken says
/// nothing of which digit was wrong.
pub fn verify(
    secret: &Secret,
    given: &str,
    unix_secs: u64,
    last_step: Option<u64>,
) -> Result<u64, Mismatch> {
    let given: Vec<u8> = given.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if given.len() != DIGITS as usize || !given.iter().all(u8::is_ascii_digit) {
        return Err(Mismatch::Wrong);
    }
    let now = step_at(unix_secs);
    let mut fresh = None;
    let mut used = false;
    for step in now.saturating_sub(WINDOW)..=now.saturating_add(WINDOW) {
        let want = code_at_step(secret, step);
        if !equal(want.as_bytes(), &given) {
            continue;
        }
        match last_step {
            Some(last) if step <= last => used = true,
            _ => fresh = fresh.max(Some(step)),
        }
    }
    match (fresh, used) {
        (Some(step), _) => Ok(step),
        (None, true) => Err(Mismatch::Used),
        (None, false) => Err(Mismatch::Wrong),
    }
}

/// Equal bytes, in time that depends on the length alone.
fn equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// RFC 4648 base32, upper case, no padding: how an authenticator app is told
/// a secret by hand.
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buf = (buf << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((buf >> bits) & 31) as usize]));
        }
        buf &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((buf << (5 - bits)) & 31) as usize]));
    }
    out
}

/// The address an authenticator app takes by scan or paste. `issuer` and
/// `account` name the entry in the app; both are plain words, never a
/// person's or a host's name.
pub fn otpauth_uri(secret: &Secret, issuer: &str, account: &str) -> String {
    format!(
        "otpauth://totp/{issuer}:{account}?secret={}&issuer={issuer}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECS}",
        secret.base32()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238's own test secret for SHA-1 (appendix B): the ASCII digits
    /// 1 to 0, twice over.
    fn rfc() -> Secret {
        Secret::from_bytes(*b"12345678901234567890")
    }

    /// RFC 6238 appendix B's SHA-1 vectors, which are eight digits; a
    /// six-digit code is the same truncation mod 10^6, so each vector's last
    /// six digits are the six-digit code at that time.
    #[test]
    fn the_rfcs_sha1_vectors_hold() {
        let vectors: [(u64, u32); 6] = [
            (59, 94_287_082),
            (1_111_111_109, 7_081_804),
            (1_111_111_111, 14_050_471),
            (1_234_567_890, 89_005_924),
            (2_000_000_000, 69_279_037),
            (20_000_000_000, 65_353_130),
        ];
        for (t, eight) in vectors {
            assert_eq!(hotp(rfc().bytes(), step_at(t), 8), eight, "8 digits at {t}");
            assert_eq!(
                code_at_step(&rfc(), step_at(t)),
                format!("{:06}", eight % 1_000_000),
                "6 digits at {t}"
            );
        }
    }

    /// RFC 4226 appendix D's counters 0 to 9, the HOTP values the TOTP
    /// vectors stand on.
    #[test]
    fn the_hotp_vectors_hold() {
        let want = [
            755_224, 287_082, 359_152, 969_429, 338_314, 254_676, 287_922, 162_583, 399_871,
            520_489,
        ];
        for (counter, code) in want.into_iter().enumerate() {
            assert_eq!(
                hotp(rfc().bytes(), counter as u64, 6),
                code,
                "counter {counter}"
            );
        }
    }

    /// The window is the present step and one either side, and nothing more.
    #[test]
    fn a_code_is_taken_one_step_either_side_and_no_further() {
        let t = 1_111_111_111;
        let now = step_at(t);
        for (step, ok) in [
            (now - 2, false),
            (now - 1, true),
            (now, true),
            (now + 1, true),
            (now + 2, false),
        ] {
            let code = code_at_step(&rfc(), step);
            assert_eq!(
                verify(&rfc(), &code, t, None).is_ok(),
                ok,
                "step {} from {now}",
                step as i64 - now as i64
            );
        }
    }

    /// A code is used once: the step accepted is the one to store, and from
    /// then on that step's code, and every earlier one's, is `Used`; a later
    /// step's still goes.
    #[test]
    fn a_code_is_single_use() {
        let t = 1_111_111_111;
        let now = step_at(t);
        let code = code_at_step(&rfc(), now);
        assert_eq!(verify(&rfc(), &code, t, None), Ok(now));
        assert_eq!(verify(&rfc(), &code, t, Some(now)), Err(Mismatch::Used));
        let earlier = code_at_step(&rfc(), now - 1);
        assert_eq!(verify(&rfc(), &earlier, t, Some(now)), Err(Mismatch::Used));
        let later = code_at_step(&rfc(), now + 1);
        assert_eq!(verify(&rfc(), &later, t, Some(now)), Ok(now + 1));
    }

    /// Anything that is not six digits is wrong without being compared; spaces
    /// an app shows are ignored.
    #[test]
    fn a_code_is_six_digits_and_spaces_do_not_count() {
        let t = 59;
        let code = code_at_step(&rfc(), step_at(t));
        let spaced = format!("{} {}", &code[..3], &code[3..]);
        assert_eq!(verify(&rfc(), &spaced, t, None), Ok(step_at(t)));
        for bad in [
            "",
            "12345",
            "1234567",
            "12345a",
            "١٢٣٤٥٦",
            &format!("{code}0"),
        ] {
            assert_eq!(
                verify(&rfc(), bad, t, None),
                Err(Mismatch::Wrong),
                "{bad:?}"
            );
        }
    }

    /// RFC 4648 §10's vectors, unpadded.
    #[test]
    fn base32_is_rfc_4648s() {
        for (plain, coded) in [
            ("", ""),
            ("f", "MY"),
            ("fo", "MZXQ"),
            ("foo", "MZXW6"),
            ("foob", "MZXW6YQ"),
            ("fooba", "MZXW6YTB"),
            ("foobar", "MZXW6YTBOI"),
        ] {
            assert_eq!(base32(plain.as_bytes()), coded, "{plain:?}");
        }
        assert_eq!(rfc().base32(), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
    }

    #[test]
    fn the_address_names_the_secret_the_issuer_and_the_shape() {
        let uri = otpauth_uri(&rfc(), "Theseus", "owner");
        assert_eq!(
            uri,
            "otpauth://totp/Theseus:owner?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Theseus&algorithm=SHA1&digits=6&period=30"
        );
    }

    /// A stray `{:?}` of a secret, or of anything that holds one, says nothing.
    #[test]
    fn a_secret_prints_as_nothing() {
        let shown = format!("{:?}", rfc());
        assert_eq!(shown, "Secret(..)");
        assert!(!shown.contains("GEZDG") && !shown.contains("1234"));
    }
}
