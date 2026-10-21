//! What a binary file is, by its first bytes (theseus-53ujg.2): the fact
//! `fs_read` gives for a file it cannot show, instead of "text only".

/// A binary's kind: `desc` reads after "a" or "an" (`ELF 64-bit PIE
/// executable, x86-64`), `image` is an image `fs_read` does not show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kind {
    pub desc: String,
    pub image: bool,
}

impl Kind {
    fn new(desc: impl Into<String>) -> Option<Kind> {
        Some(Kind {
            desc: desc.into(),
            image: false,
        })
    }
    fn picture(name: &str, size: Option<(u64, u64)>) -> Option<Kind> {
        let desc = match size {
            Some((w, h)) => format!("{name} image, {w}×{h}"),
            None => format!("{name} image"),
        };
        Some(Kind { desc, image: true })
    }

    /// `an ELF …`, `a PPM image, …`.
    pub fn with_article(&self) -> String {
        let an = ["ELF", "ICO", "SQLite", "xz"]
            .iter()
            .any(|w| self.desc.starts_with(w));
        format!("{} {}", if an { "an" } else { "a" }, self.desc)
    }
}

/// `864015` as `864,015`.
pub fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn u16_at(b: &[u8], at: usize, big: bool) -> Option<u16> {
    let v: [u8; 2] = b.get(at..at + 2)?.try_into().ok()?;
    Some(if big {
        u16::from_be_bytes(v)
    } else {
        u16::from_le_bytes(v)
    })
}
fn u32_at(b: &[u8], at: usize, big: bool) -> Option<u32> {
    let v: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
    Some(if big {
        u32::from_be_bytes(v)
    } else {
        u32::from_le_bytes(v)
    })
}
fn u64_at(b: &[u8], at: usize, big: bool) -> Option<u64> {
    let v: [u8; 8] = b.get(at..at + 8)?.try_into().ok()?;
    Some(if big {
        u64::from_be_bytes(v)
    } else {
        u64::from_le_bytes(v)
    })
}

/// What `bytes` is, when its first bytes say so.
pub fn kind(b: &[u8]) -> Option<Kind> {
    if b.starts_with(b"\x7fELF") {
        return elf(b);
    }
    if b.starts_with(b"MZ") {
        if let Some(k) = pe(b) {
            return Some(k);
        }
    }
    if let Some(k) = mach_o(b) {
        return Some(k);
    }
    if b.starts_with(b"\xca\xfe\xba\xbe") {
        return Kind::new(format!("Java class file, version {}", u16_at(b, 6, true)?));
    }
    if let Some(k) = netpbm(b) {
        return Some(k);
    }
    if b.starts_with(b"BM") {
        return bmp(b);
    }
    if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        return tiff(b);
    }
    if b.starts_with(&[0, 0, 1, 0]) {
        return ico(b);
    }
    let fixed: [(&[u8], &str); 6] = [
        (b"SQLite format 3\0", "SQLite database"),
        (b"\x1f\x8b\x08", "gzip-compressed data"),
        (b"\xfd7zXZ\0", "xz-compressed data"),
        (b"\x28\xb5\x2f\xfd", "zstd-compressed data"),
        (b"7z\xbc\xaf\x27\x1c", "7-Zip archive"),
        (b"\0asm\x01\0\0\0", "WebAssembly module"),
    ];
    for (magic, what) in fixed {
        if b.starts_with(magic) {
            return Kind::new(what);
        }
    }
    if b.starts_with(b"BZh") && b.get(3).is_some_and(|c| (b'1'..=b'9').contains(c)) {
        return Kind::new("bzip2-compressed data");
    }
    // Python 3's bytecode: a magic number, then CR LF.
    if b.len() >= 16
        && b[2..4] == *b"\r\n"
        && u16_at(b, 0, false).is_some_and(|m| (3000..4000).contains(&m))
    {
        return Kind::new("Python bytecode");
    }
    None
}

fn elf(b: &[u8]) -> Option<Kind> {
    let wide = match b.get(4)? {
        1 => false,
        2 => true,
        _ => return None,
    };
    let big = match b.get(5)? {
        1 => false,
        2 => true,
        _ => return None,
    };
    let (ty, machine) = (u16_at(b, 16, big)?, u16_at(b, 18, big)?);
    let arch = match machine {
        3 => "x86",
        8 => "MIPS",
        20 => "PowerPC",
        21 => "PowerPC64",
        22 => "S/390",
        40 => "ARM",
        62 => "x86-64",
        183 => "ARM aarch64",
        243 => "RISC-V",
        258 => "LoongArch",
        _ => "",
    };
    let what = match ty {
        1 => "object",
        2 => "executable",
        3 if has_interp(b, wide, big) => "PIE executable",
        3 => "shared object",
        4 => "core dump",
        _ => return None,
    };
    let bits = if wide { 64 } else { 32 };
    let arch = if arch.is_empty() {
        String::new()
    } else {
        format!(", {arch}")
    };
    Kind::new(format!("ELF {bits}-bit {what}{arch}"))
}

/// Whether a program header is `PT_INTERP`: a dynamic executable names its loader.
fn has_interp(b: &[u8], wide: bool, big: bool) -> bool {
    let (off, size, num) = if wide {
        (u64_at(b, 32, big), u16_at(b, 54, big), u16_at(b, 56, big))
    } else {
        (
            u32_at(b, 28, big).map(u64::from),
            u16_at(b, 42, big),
            u16_at(b, 44, big),
        )
    };
    let (Some(off), Some(size), Some(num)) = (off, size, num) else {
        return false;
    };
    (0..u64::from(num)).any(|i| {
        i.checked_mul(u64::from(size))
            .and_then(|d| off.checked_add(d))
            .and_then(|at| usize::try_from(at).ok())
            .and_then(|at| u32_at(b, at, big))
            == Some(3)
    })
}

fn pe(b: &[u8]) -> Option<Kind> {
    let at = usize::try_from(u32_at(b, 0x3c, false)?).ok()?;
    if b.get(at..at + 4)? != b"PE\0\0" {
        return None;
    }
    let arch = match u16_at(b, at + 4, false)? {
        0x14c => ", x86",
        0x8664 => ", x86-64",
        0xaa64 => ", ARM64",
        _ => "",
    };
    let dll = u16_at(b, at + 22, false)? & 0x2000 != 0;
    Kind::new(format!(
        "PE {}{arch}",
        if dll { "DLL" } else { "executable" }
    ))
}

fn mach_o(b: &[u8]) -> Option<Kind> {
    let magic = u32_at(b, 0, true)?;
    let wide = matches!(magic, 0xfeed_facf | 0xcffa_edfe);
    match magic {
        0xfeed_face | 0xfeed_facf | 0xcefa_edfe | 0xcffa_edfe => {
            Kind::new(format!("Mach-O {}-bit", if wide { 64 } else { 32 }))
        }
        // The universal header shares Java's magic: its count is small.
        0xcafe_babe if u32_at(b, 4, true)? < 45 => Kind::new("Mach-O universal binary"),
        _ => None,
    }
}

/// The binary netpbm images, P4 to P7; P1 to P3 are text and stay text.
fn netpbm(b: &[u8]) -> Option<Kind> {
    if b.first() != Some(&b'P') || !b.get(2).is_some_and(u8::is_ascii_whitespace) {
        return None;
    }
    let name = match b.get(1)? {
        b'4' => "PBM",
        b'5' => "PGM",
        b'6' => "PPM",
        b'7' => "PAM",
        _ => return None,
    };
    if b[1] == b'7' {
        let head = String::from_utf8_lossy(&b[..b.len().min(256)]);
        let field = |key: &str| {
            head.lines()
                .find_map(|l| l.strip_prefix(key)?.trim().parse::<u64>().ok())
        };
        return Kind::picture(name, field("WIDTH").zip(field("HEIGHT")));
    }
    let mut at = 2;
    let mut number = || {
        loop {
            match *b.get(at)? {
                b'#' => {
                    while *b.get(at)? != b'\n' {
                        at += 1
                    }
                }
                c if c.is_ascii_whitespace() => at += 1,
                _ => break,
            }
        }
        let start = at;
        while b.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        std::str::from_utf8(&b[start..at]).ok()?.parse::<u64>().ok()
    };
    let size = number().zip(number());
    Kind::picture(name, size)
}

fn bmp(b: &[u8]) -> Option<Kind> {
    let dib = u32_at(b, 14, false)?;
    let size = match dib {
        12 => Some((
            u64::from(u16_at(b, 18, false)?),
            u64::from(u16_at(b, 20, false)?),
        )),
        40 | 52 | 56 | 64 | 108 | 124 => {
            let w = i32::from_le_bytes(b.get(18..22)?.try_into().ok()?);
            let h = i32::from_le_bytes(b.get(22..26)?.try_into().ok()?);
            Some((u64::from(w.unsigned_abs()), u64::from(h.unsigned_abs())))
        }
        _ => return None,
    };
    Kind::picture("BMP", size)
}

fn tiff(b: &[u8]) -> Option<Kind> {
    let big = b[0] == b'M';
    let ifd = usize::try_from(u32_at(b, 4, big)?).ok()?;
    let count = usize::from(u16_at(b, ifd, big)?);
    let (mut w, mut h) = (None, None);
    for i in 0..count.min(64) {
        let e = ifd + 2 + i * 12;
        let (tag, ty) = (u16_at(b, e, big)?, u16_at(b, e + 2, big)?);
        let v = match ty {
            3 => u64::from(u16_at(b, e + 8, big)?),
            4 => u64::from(u32_at(b, e + 8, big)?),
            _ => continue,
        };
        match tag {
            256 => w = Some(v),
            257 => h = Some(v),
            _ => {}
        }
    }
    Kind::picture("TIFF", w.zip(h))
}

fn ico(b: &[u8]) -> Option<Kind> {
    let n = u16_at(b, 4, false)?;
    let at = usize::try_from(u32_at(b, 18, false)?).ok()?;
    // A directory entry's reserved byte is zero, and its image lies in the file.
    if n == 0 || n > 64 || *b.get(9)? != 0 || at >= b.len() {
        return None;
    }
    let dim = |c: u8| if c == 0 { 256 } else { u64::from(c) };
    Kind::picture("ICO", Some((dim(b[6]), dim(b[7]))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64-bit little-endian ELF header of `ty`, with `phdrs` program header types.
    fn elf64(ty: u16, machine: u16, phdrs: &[u32]) -> Vec<u8> {
        let mut b = vec![0u8; 64 + 56 * phdrs.len()];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[5] = 1;
        b[16..18].copy_from_slice(&ty.to_le_bytes());
        b[18..20].copy_from_slice(&machine.to_le_bytes());
        b[32..40].copy_from_slice(&64u64.to_le_bytes());
        b[54..56].copy_from_slice(&56u16.to_le_bytes());
        b[56..58].copy_from_slice(&(phdrs.len() as u16).to_le_bytes());
        for (i, p) in phdrs.iter().enumerate() {
            b[64 + 56 * i..][..4].copy_from_slice(&p.to_le_bytes());
        }
        b
    }

    fn desc(b: &[u8]) -> String {
        kind(b).unwrap().desc
    }

    #[test]
    fn an_elf_is_told_by_its_type_and_its_loader() {
        // PT_LOAD, PT_INTERP, PT_DYNAMIC
        assert_eq!(
            desc(&elf64(3, 62, &[1, 3, 2])),
            "ELF 64-bit PIE executable, x86-64"
        );
        assert_eq!(
            desc(&elf64(3, 183, &[1, 2])),
            "ELF 64-bit shared object, ARM aarch64"
        );
        assert_eq!(desc(&elf64(2, 62, &[1])), "ELF 64-bit executable, x86-64");
        assert_eq!(desc(&elf64(1, 62, &[])), "ELF 64-bit object, x86-64");
        assert_eq!(
            kind(&elf64(3, 62, &[1, 3])).unwrap().with_article(),
            "an ELF 64-bit PIE executable, x86-64"
        );
    }

    #[test]
    fn a_program_header_offset_near_the_top_of_u64_is_no_loader() {
        // e_phoff is the file's own data: a crafted one must not overflow.
        let mut b = elf64(3, 62, &[1, 3]);
        b[32..40].copy_from_slice(&(u64::MAX - 8).to_le_bytes());
        assert_eq!(desc(&b), "ELF 64-bit shared object, x86-64");
    }

    #[test]
    fn a_netpbm_image_has_its_size_and_the_text_forms_stay_text() {
        let mut ppm = b"P6\n# a comment\n720 400\n255\n".to_vec();
        ppm.extend([0u8; 12]);
        let k = kind(&ppm).unwrap();
        assert_eq!(k.desc, "PPM image, 720×400");
        assert!(k.image);
        assert_eq!(k.with_article(), "a PPM image, 720×400");
        assert_eq!(desc(b"P5 3 2 255 \0\0\0\0\0\0"), "PGM image, 3×2");
        assert_eq!(
            desc(b"P7\nWIDTH 8\nHEIGHT 4\nDEPTH 4\nMAXVAL 255\nENDHDR\n"),
            "PAM image, 8×4"
        );
        for text in [&b"P1\n3 3\n"[..], b"P2\n3 3\n255\n", b"P3\n3 3\n255\n"] {
            assert_eq!(kind(text), None);
        }
    }

    #[test]
    fn other_binaries_are_named_by_their_first_bytes() {
        assert_eq!(desc(b"SQLite format 3\0\x10\0\x01\x01"), "SQLite database");
        assert_eq!(desc(b"\x1f\x8b\x08\0\0\0\0\0"), "gzip-compressed data");
        assert_eq!(desc(b"\xfd7zXZ\0\0\x04"), "xz-compressed data");
        assert_eq!(desc(b"\x28\xb5\x2f\xfd\x04\x58"), "zstd-compressed data");
        assert_eq!(desc(b"BZh91AY&SY"), "bzip2-compressed data");
        assert_eq!(desc(b"7z\xbc\xaf\x27\x1c\0\x04"), "7-Zip archive");
        assert_eq!(desc(b"\0asm\x01\0\0\0\x01\x04"), "WebAssembly module");
        assert_eq!(
            desc(b"\xca\xfe\xba\xbe\0\0\0\x34"),
            "Java class file, version 52"
        );
        assert_eq!(
            desc(b"\xca\xfe\xba\xbe\0\0\0\x02rest"),
            "Mach-O universal binary"
        );
        assert_eq!(desc(b"\xcf\xfa\xed\xfe\x07\0\0\x01"), "Mach-O 64-bit");
        let mut pyc = vec![0u8; 16];
        pyc[..2].copy_from_slice(&3571u16.to_le_bytes());
        pyc[2..4].copy_from_slice(b"\r\n");
        assert_eq!(desc(&pyc), "Python bytecode");
        let mut bmp = b"BM".to_vec();
        bmp.extend([0u8; 12]);
        bmp.extend(40u32.to_le_bytes());
        bmp.extend(640i32.to_le_bytes());
        bmp.extend((-480i32).to_le_bytes());
        assert_eq!(desc(&bmp), "BMP image, 640×480");
        assert_eq!(kind(b"just some bytes\0\x01"), None);
    }

    #[test]
    fn sizes_read_with_thousands_marks() {
        assert_eq!(grouped(864_015), "864,015");
        assert_eq!(grouped(412_608), "412,608");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(0), "0");
    }
}
