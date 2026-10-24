//! The repository is public: no text file in it names an old organisation
//! (theseus-cuv2o). The names are held only as digests (`names::OLD`), so the
//! scan reports a file and a line, never a word.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::names::names_an_old_name;

// .beads is the local issue tracker: git-ignored, never published.
const SKIP: [&str; 6] = ["vendor", "target", "node_modules", ".git", "dist", ".beads"];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            if !SKIP.iter().any(|s| entry.file_name() == *s) {
                files(&entry.path(), out);
            }
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
}

/// The `file:line` of each line of `paths` that names an old name.
fn scan(paths: &[PathBuf]) -> Vec<String> {
    let mut clean = HashSet::new();
    let mut found = Vec::new();
    for path in paths {
        // A binary file does not read as text.
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        // The whole file first; only a file that hits is read by line.
        if !names_an_old_name(&text, &mut clean) {
            continue;
        }
        for (i, line) in text.lines().enumerate() {
            if names_an_old_name(line, &mut clean) {
                found.push(format!("{}:{}", path.display(), i + 1));
            }
        }
    }
    found
}

#[test]
fn no_text_file_in_the_repository_names_an_old_organisation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    files(&root, &mut paths);
    paths.sort();
    // Debug code hashes about 6 MB of words a second: the files go to a
    // thread per core.
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let mut found: Vec<String> = std::thread::scope(|s| {
        let work: Vec<_> = paths
            .chunks(paths.len().div_ceil(threads).max(1))
            .map(|chunk| s.spawn(|| scan(chunk)))
            .collect();
        work.into_iter().flat_map(|w| w.join().unwrap()).collect()
    });
    found.sort();
    assert_eq!(found, Vec::<String>::new(), "files naming an old name");
}
