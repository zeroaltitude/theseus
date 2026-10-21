//! `fs.read` of a binary it does not show says what the file is (theseus-53ujg.2).

use serde_json::json;

use crate::{fs::Read, Tool, ToolCtx};

fn read(d: &tempfile::TempDir, name: &str, bytes: &[u8]) -> crate::ToolOutput {
    std::fs::write(d.path().join(name), bytes).unwrap();
    Read.run(&json!({"path": name}), &ToolCtx::for_tests(d.path()))
        .unwrap()
}

/// A 64-bit little-endian x86-64 ELF of `ty`, with program headers of `phdrs`' types.
fn elf(ty: u16, phdrs: &[u32]) -> Vec<u8> {
    let mut b = vec![0u8; 64 + 56 * phdrs.len()];
    b[..4].copy_from_slice(b"\x7fELF");
    b[4] = 2;
    b[5] = 1;
    b[16..18].copy_from_slice(&ty.to_le_bytes());
    b[18..20].copy_from_slice(&62u16.to_le_bytes());
    b[32..40].copy_from_slice(&64u64.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&(phdrs.len() as u16).to_le_bytes());
    for (i, p) in phdrs.iter().enumerate() {
        b[64 + 56 * i..][..4].copy_from_slice(&p.to_le_bytes());
    }
    b
}

#[test]
fn fs_read_names_a_binary_kind() {
    let d = tempfile::tempdir().unwrap();
    // A screendump's PPM under a .png name, with a zero byte in its first 8 KiB.
    let mut ppm = b"P6\n720 400\n255\n".to_vec();
    ppm.resize(720 * 400 * 3 + ppm.len(), 0);
    let o = read(&d, "s.png", &ppm);
    let path = d.path().join("s.png");
    assert_eq!(
        o.text,
        format!(
            "{} is a PPM image, 720×400, 864,015 bytes (fs_read shows PNG, JPEG, GIF and WebP images)",
            path.display()
        )
    );
    assert_eq!(o.meta["kind"], "PPM image, 720×400");
    assert_eq!(o.meta["binary"], true);

    let pie = elf(3, &[1, 3, 2]);
    let o = read(&d, "main", &pie);
    assert!(
        o.text.ends_with(&format!(
            "is an ELF 64-bit PIE executable, x86-64, {} bytes",
            pie.len()
        )),
        "{}",
        o.text
    );
    let lib = elf(3, &[1, 2]);
    let o = read(&d, "libx.so", &lib);
    assert!(
        o.text.contains("is an ELF 64-bit shared object, x86-64"),
        "{}",
        o.text
    );
    assert!(!o.text.contains("PNG"), "an ELF is no image: {}", o.text);

    let mut db = b"SQLite format 3\0".to_vec();
    db.resize(4096, 0);
    let o = read(&d, "app.db", &db);
    assert!(
        o.text.contains("is an SQLite database, 4,096 bytes"),
        "{}",
        o.text
    );

    // Bytes no table knows keep today's sentence.
    let o = read(&d, "blob.bin", &[1, 2, 0, 3, 4]);
    assert!(
        o.text
            .ends_with("is a binary file (5 bytes); fs_read returns text only."),
        "{}",
        o.text
    );
    assert_eq!(o.meta.get("kind"), None);
}
