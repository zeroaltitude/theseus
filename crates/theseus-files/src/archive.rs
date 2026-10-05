//! Archives (theseus-c9l6): a zip's, a tar's, or a gzipped tar's members
//! listed, and one member read out on request. A tar is read as a stream,
//! never unpacked whole, so a gzip bomb costs only the reading time; what is
//! read out stops at its cap; and a member's path is kept only when it is
//! relative and climbs nowhere, so whatever writes it stays inside its
//! directory.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::doc::{Doc, Section, MAX_LISTED};
use crate::kind::Kind;

/// One member of an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub path: String,
    pub size: u64,
    pub dir: bool,
}

/// A member's path as a relative path that climbs nowhere, or `None`: an
/// absolute path, one with `..`, or an empty one is never written.
pub fn safe_path(member: &str) -> Option<PathBuf> {
    let p = Path::new(member.trim_start_matches("./"));
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// An archive's members, at most [`MAX_LISTED`] of them, and how many more.
pub fn members(bytes: &[u8], kind: Kind) -> Result<(Vec<Member>, u64), String> {
    let mut out = Vec::new();
    let mut more = 0u64;
    let mut keep = |m: Member| {
        if out.len() < MAX_LISTED {
            out.push(m);
        } else {
            more += 1;
        }
    };
    match kind {
        Kind::Zip => {
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes))
                .map_err(|e| format!("it could not be read as a zip ({e})"))?;
            for i in 0..z.len() {
                let f = z
                    .by_index_raw(i)
                    .map_err(|e| format!("its member {i} could not be read ({e})"))?;
                keep(Member {
                    path: f.name().to_string(),
                    size: f.size(),
                    dir: f.is_dir(),
                });
            }
        }
        Kind::Tar => walk_tar(bytes, &mut |m, _| {
            keep(m);
            Ok(Walk::Skip)
        })?,
        Kind::TarGz => walk_tar(flate2::read::GzDecoder::new(bytes), &mut |m, _| {
            keep(m);
            Ok(Walk::Skip)
        })?,
        _ => return Err(format!("a {} is not an archive", kind.noun())),
    }
    Ok((out, more))
}

/// An archive's one section: its members, a line each, with their sizes.
pub fn listing(bytes: &[u8], kind: Kind) -> Result<Doc, String> {
    let (list, more) = members(bytes, kind)?;
    let files = list.iter().filter(|m| !m.dir).count();
    let mut text = list
        .iter()
        .map(|m| match m.dir {
            true => format!("{}/", m.path.trim_end_matches('/')),
            false => format!("{}  ({} bytes)", m.path, m.size),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if more > 0 {
        text.push_str(&format!("\n… and {more} more members, not listed"));
    }
    Ok(Doc {
        sections: vec![Section {
            label: format!("contents: {files} files"),
            text,
            images: vec![],
        }],
        cut: None,
    })
}

/// One member's bytes, at most `max` of them. `Err` says why not: no such
/// member, a directory, over the cap, a path that climbs out.
pub fn extract(bytes: &[u8], kind: Kind, member: &str, max: u64) -> Result<Vec<u8>, String> {
    if safe_path(member).is_none() {
        return Err(format!(
            "`{member}` is not a path inside the archive that can be read out (absolute, or with ..)"
        ));
    }
    let want = member.trim_start_matches("./").trim_end_matches('/');
    let over = |size: u64| {
        format!("its member {member} is {size} bytes, over the {max} a member is read out to")
    };
    match kind {
        Kind::Zip => {
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes))
                .map_err(|e| format!("it could not be read as a zip ({e})"))?;
            let mut f = z
                .by_name(want)
                .map_err(|_| format!("it has no member {member}"))?;
            if f.is_dir() {
                return Err(format!("{member} is a directory"));
            }
            if f.size() > max {
                return Err(over(f.size()));
            }
            let mut out = Vec::new();
            (&mut f)
                .take(max + 1)
                .read_to_end(&mut out)
                .map_err(|e| format!("its member {member} could not be read ({e})"))?;
            if out.len() as u64 > max {
                return Err(over(out.len() as u64));
            }
            Ok(out)
        }
        Kind::Tar | Kind::TarGz => {
            let mut found: Option<Vec<u8>> = None;
            let mut f = |m: Member, data: &mut dyn Read| -> Result<Walk, String> {
                if m.path.trim_start_matches("./").trim_end_matches('/') != want {
                    return Ok(Walk::Skip);
                }
                if m.dir {
                    return Err(format!("{member} is a directory"));
                }
                if m.size > max {
                    return Err(over(m.size));
                }
                let mut out = Vec::with_capacity(m.size as usize);
                data.take(m.size)
                    .read_to_end(&mut out)
                    .map_err(|e| format!("its member {member} could not be read ({e})"))?;
                found = Some(out);
                Ok(Walk::Stop)
            };
            match kind {
                Kind::Tar => walk_tar(bytes, &mut f)?,
                _ => walk_tar(flate2::read::GzDecoder::new(bytes), &mut f)?,
            }
            found.ok_or_else(|| format!("it has no member {member}"))
        }
        _ => Err(format!("a {} is not an archive", kind.noun())),
    }
}

/// What a tar's walk does after a member.
enum Walk {
    /// Go on to the next member (its data, if the visitor left it, is skipped).
    Skip,
    Stop,
}

/// Walk a tar's members as a stream: each header, then the visitor, which
/// may read the member's data. GNU long names and pax paths are honored.
fn walk_tar<R: Read>(
    mut r: R,
    visit: &mut dyn FnMut(Member, &mut dyn Read) -> Result<Walk, String>,
) -> Result<(), String> {
    let mut block = [0u8; 512];
    let mut long_name: Option<String> = None;
    loop {
        if !read_block(&mut r, &mut block)? {
            return Ok(());
        }
        if block.iter().all(|b| *b == 0) {
            return Ok(());
        }
        let size =
            octal(&block[124..136]).ok_or("it could not be read as a tar (a header's size)")?;
        let kind = block[156];
        let padded = size.div_ceil(512) * 512;
        // A long name, or a pax header: their data names the next member.
        if kind == b'L' || kind == b'x' {
            let mut data = Vec::new();
            (&mut r)
                .take(padded)
                .read_to_end(&mut data)
                .map_err(|e| format!("it could not be read as a tar ({e})"))?;
            data.truncate(size as usize);
            let text = String::from_utf8_lossy(&data);
            long_name = match kind {
                b'L' => Some(text.trim_end_matches('\0').to_string()),
                _ => text
                    .lines()
                    .find_map(|l| l.split_once(" path=").map(|(_, p)| p.to_string()))
                    .or(long_name),
            };
            continue;
        }
        let name = |r: std::ops::Range<usize>| {
            let f = &block[r];
            let end = f.iter().position(|b| *b == 0).unwrap_or(f.len());
            String::from_utf8_lossy(&f[..end]).into_owned()
        };
        let path = long_name.take().unwrap_or_else(|| {
            let prefix = name(345..500);
            let base = name(0..100);
            if &block[257..262] == b"ustar" && !prefix.is_empty() {
                format!("{prefix}/{base}")
            } else {
                base
            }
        });
        let m = Member {
            path,
            size,
            dir: kind == b'5',
        };
        let regular = matches!(kind, b'0' | 0 | b'5');
        let mut data = (&mut r).take(padded);
        let walk = if regular {
            visit(m, &mut data)?
        } else {
            Walk::Skip
        };
        if let Walk::Stop = walk {
            return Ok(());
        }
        // Whatever of the member the visitor left is read past.
        std::io::copy(&mut data, &mut std::io::sink())
            .map_err(|e| format!("it could not be read as a tar ({e})"))?;
    }
}

/// One 512-byte block, or `false` at a clean end.
fn read_block<R: Read>(r: &mut R, block: &mut [u8; 512]) -> Result<bool, String> {
    let mut got = 0;
    while got < 512 {
        match r.read(&mut block[got..]) {
            Ok(0) if got == 0 => return Ok(false),
            Ok(0) => return Err("it could not be read as a tar (it ends inside a header)".into()),
            Ok(n) => got += n,
            Err(e) => return Err(format!("it could not be read as a tar ({e})")),
        }
    }
    Ok(true)
}

/// A header's number: octal digits, or GNU's base-256 when its high bit is set.
fn octal(f: &[u8]) -> Option<u64> {
    if f.first().is_some_and(|b| b & 0x80 != 0) {
        return Some(f[1..].iter().fold(0u64, |n, b| (n << 8) | u64::from(*b)));
    }
    let s = String::from_utf8_lossy(f);
    let t = s.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    if t.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(t, 8).ok()
}

/// A tar of `files` for tests: each a regular member with its bytes.
pub fn sample_tar(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data) in files {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        h[156] = b'0';
        h[257..263].copy_from_slice(b"ustar\0");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zip_and_a_tar_are_listed_and_one_member_is_read_out() {
        let zip =
            crate::doc::sample_zip(&[("src/main.rs", "fn main() {}"), ("README.md", "# Tides")]);
        let d = listing(&zip, Kind::Zip).unwrap();
        assert_eq!(d.sections[0].label, "contents: 2 files");
        assert_eq!(
            d.sections[0].text,
            "src/main.rs  (12 bytes)\nREADME.md  (7 bytes)"
        );
        assert_eq!(
            extract(&zip, Kind::Zip, "README.md", 100).unwrap(),
            b"# Tides"
        );
        assert!(extract(&zip, Kind::Zip, "README.md", 3)
            .unwrap_err()
            .contains("over the 3"));
        assert_eq!(
            extract(&zip, Kind::Zip, "nope", 100).unwrap_err(),
            "it has no member nope"
        );

        let tar = sample_tar(&[("logs/tide.log", b"high 06:12\n"), ("notes.txt", b"pilot")]);
        let (list, more) = members(&tar, Kind::Tar).unwrap();
        assert_eq!(more, 0);
        assert_eq!(
            list.iter()
                .map(|m| (m.path.as_str(), m.size))
                .collect::<Vec<_>>(),
            vec![("logs/tide.log", 11), ("notes.txt", 5)]
        );
        assert_eq!(
            extract(&tar, Kind::Tar, "notes.txt", 100).unwrap(),
            b"pilot"
        );

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar).unwrap();
        let tgz = gz.finish().unwrap();
        assert_eq!(
            extract(&tgz, Kind::TarGz, "logs/tide.log", 100).unwrap(),
            b"high 06:12\n"
        );
    }

    #[test]
    fn a_path_that_climbs_out_is_never_read_out() {
        assert_eq!(safe_path("a/b.txt"), Some(PathBuf::from("a/b.txt")));
        assert_eq!(safe_path("./a"), Some(PathBuf::from("a")));
        assert!(safe_path("../etc/passwd").is_none());
        assert!(safe_path("/etc/passwd").is_none());
        assert!(safe_path("a/../../b").is_none());
        assert!(safe_path("").is_none());
        let zip = crate::doc::sample_zip(&[("../evil.sh", "rm")]);
        assert!(extract(&zip, Kind::Zip, "../evil.sh", 100)
            .unwrap_err()
            .contains("with .."));
    }
}
