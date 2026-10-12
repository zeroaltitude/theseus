//! The weights mapped and hashed once (theseus-agqn): a load holds the
//! tensors and not the file beside them, and a file is hashed once per
//! signature.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::mapped;
use crate::weights::{read_safetensors, sha256_file, LoadError};

/// A safetensors file of `tensors` f32 tensors of `elems` each, written
/// through a small buffer (so the test's own heap never holds it): element
/// `j` of tensor `k` is `k * 1000 + j % 1000`.
fn write_file(path: &Path, tensors: usize, elems: usize) {
    let mut header = String::from("{");
    for k in 0..tensors {
        let (b, e) = (k * elems * 4, (k + 1) * elems * 4);
        if k > 0 {
            header.push(',');
        }
        header.push_str(&format!(
            "\"t{k:02}\":{{\"dtype\":\"F32\",\"shape\":[{elems}],\"data_offsets\":[{b},{e}]}}"
        ));
    }
    header.push('}');
    while header.len() % 8 != 0 {
        header.push(' ');
    }
    let mut f = std::io::BufWriter::with_capacity(1 << 16, File::create(path).unwrap());
    f.write_all(&(header.len() as u64).to_le_bytes()).unwrap();
    f.write_all(header.as_bytes()).unwrap();
    for k in 0..tensors {
        for j in 0..elems {
            f.write_all(&((k * 1000 + j % 1000) as f32).to_le_bytes())
                .unwrap();
        }
    }
    f.flush().unwrap();
}

/// A line of `/proc/self/status`, in bytes.
fn status_bytes(key: &str) -> u64 {
    let s = fs::read_to_string("/proc/self/status").unwrap();
    s.lines()
        .find_map(|l| {
            let v = l.strip_prefix(key)?.strip_prefix(':')?;
            v.trim().trim_end_matches(" kB").trim().parse::<u64>().ok()
        })
        .unwrap_or(0)
        * 1024
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim() {
    // SAFETY: no pointers.
    unsafe { libc::malloc_trim(0) };
}
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim() {}

/// The weights mapped: a load's peak resident set grows by the tensors it
/// copies and a window of the file, not by the file as well (read into the
/// heap whole, or mapped and left resident, it grew by twice the tensors).
#[test]
fn a_load_holds_the_tensors_and_not_the_file_beside_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.safetensors");
    let (tensors, elems) = (16usize, 1usize << 20);
    write_file(&path, tensors, elems);
    let want = sha256_file(&path).unwrap();
    let size = (tensors * elems * 4) as u64;
    trim();
    // Reset the peak (VmHWM) to what is resident now.
    fs::write("/proc/self/clear_refs", "5").unwrap();
    let (base, anon0) = (status_bytes("VmHWM"), status_bytes("RssAnon"));
    let loaded = read_safetensors(&path, &want).unwrap();
    let (peak, anon, file) = (
        status_bytes("VmHWM"),
        status_bytes("RssAnon"),
        status_bytes("RssFile"),
    );
    eprintln!(
        "tensors {} MiB · peak +{:.1} MiB · anonymous +{:.1} MiB · file-backed resident {:.1} MiB",
        size >> 20,
        (peak - base) as f64 / 1048576.0,
        (anon.saturating_sub(anon0)) as f64 / 1048576.0,
        file as f64 / 1048576.0
    );
    assert_eq!(loaded.len(), tensors);
    let t = loaded["t03"].to_vec1::<f32>().unwrap();
    assert_eq!(
        (t[0], t[999], t[1000], t.len()),
        (3000.0, 3999.0, 3000.0, elems)
    );
    assert!(
        peak - base < size + size / 4,
        "a load's peak grew by {} MiB for {} MiB of tensors: the file was held beside them",
        (peak - base) >> 20,
        size >> 20
    );
}

/// The hash once per signature: a second load of the same file hashes
/// nothing; a touched or replaced one is hashed again, and a refused file's
/// signature is refused again without a pass.
#[test]
fn a_file_is_hashed_once_per_signature() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.safetensors");
    write_file(&path, 3, 4096);
    let want = sha256_file(&path).unwrap();
    let first = read_safetensors(&path, &want).unwrap();
    assert_eq!(mapped::hashes_of(&path), 1);
    let again = read_safetensors(&path, &want).unwrap();
    assert_eq!(
        mapped::hashes_of(&path),
        1,
        "the same file was hashed again"
    );
    for (name, t) in &first {
        assert_eq!(
            t.to_vec1::<f32>().unwrap(),
            again[name].to_vec1::<f32>().unwrap()
        );
    }
    // Touched: another mtime, hashed again.
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    read_safetensors(&path, &want).unwrap();
    assert_eq!(mapped::hashes_of(&path), 2);
    // Replaced by another file: hashed, and refused; then refused unhashed.
    let other = dir.path().join("other");
    write_file(&other, 3, 4097);
    fs::rename(&other, &path).unwrap();
    assert!(matches!(
        read_safetensors(&path, &want),
        Err(LoadError::Refused { .. })
    ));
    assert_eq!(mapped::hashes_of(&path), 3);
    assert!(matches!(
        read_safetensors(&path, &want),
        Err(LoadError::Refused { .. })
    ));
    assert_eq!(mapped::hashes_of(&path), 3);
}

/// A file that does not read as a checkpoint is named by its hash.
#[test]
fn a_file_that_is_no_checkpoint_is_named_by_its_hash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.safetensors");
    fs::write(&path, b"not a checkpoint at all").unwrap();
    let got = sha256_file(&path).unwrap();
    match read_safetensors(&path, "00") {
        Err(LoadError::Refused { got: g, .. }) => assert_eq!(g, got),
        other => panic!("{:?}", other.map(|t| t.len())),
    }
    match read_safetensors(&path, &got) {
        Err(LoadError::Other(_)) => {}
        other => panic!("{:?}", other.map(|t| t.len())),
    }
}
