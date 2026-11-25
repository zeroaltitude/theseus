# STATS

The size of Theseus's code and of what it builds. Written by `scripts/stats.py`; do not edit it by hand.

Counted at commit 409364c3, over every file git tracks:

- **Code**: a line that is not blank and not only comment (code with a comment after it is code). **Test code**: a file in a directory, or with a name (before its first dot), that is `test`, `tests`, `fixtures`, `benches`, `test_…`, `tests_…`, `…_test` or `…_tests`; files named `*.test.*`; and Rust's `#[cfg(test)] mod … { … }` blocks.
- **Comments**: `//`, `///`, `//!` and `/* … */` (nested, in Rust) in Rust, TypeScript, JavaScript, C and CSS; in Python, comment tokens and triple-quoted strings that begin a statement (read with Python's own tokenizer); in shell, a `#` that begins a word outside quotes. A marker inside a string literal is not a comment.
- **Not counted**: blank lines, and files that are not code (Markdown, TOML, JSON, data). `vendor/` is third-party and kept out of the total.

## Lines

| Where | Code | Test code | Comments |
|---|---:|---:|---:|
| bench/ | 68,934 | 58,761 | 16,607 |
| cockpit/ | 46,995 | 14,892 | 20,988 |
| crates/ (in no crate) | 93 | 62 | 159 |
| crates/theseus | 35,627 | 29,708 | 7,522 |
| crates/theseus-aws | 3,179 | 2,303 | 549 |
| crates/theseus-aws-catalog | 5,996 | 1,394 | 644 |
| crates/theseus-aws-guard | 2,861 | 1,522 | 323 |
| crates/theseus-core | 268,700 | 307,543 | 96,114 |
| crates/theseus-discord | 32,796 | 42,833 | 10,562 |
| crates/theseus-exam | 5,403 | 3,237 | 1,247 |
| crates/theseus-files | 2,377 | 682 | 297 |
| crates/theseus-follow | 657 | 875 | 283 |
| crates/theseus-google | 3,676 | 682 | 316 |
| crates/theseus-index | 12,447 | 6,578 | 3,027 |
| crates/theseus-judge | 13,307 | 11,642 | 4,536 |
| crates/theseus-kernel | 13,220 | 14,248 | 5,653 |
| crates/theseus-lsp | 3,189 | 1,496 | 695 |
| crates/theseus-mcp | 4,272 | 1,658 | 668 |
| crates/theseus-memory | 722 | 770 | 372 |
| crates/theseus-ontology | 3,618 | 4,160 | 960 |
| crates/theseus-protocol | 21,018 | 7,969 | 8,435 |
| crates/theseus-root-install | 696 | 440 | 122 |
| crates/theseus-sandbox | 2,891 | 3,616 | 1,642 |
| crates/theseus-sim | 18,165 | 3,692 | 3,927 |
| crates/theseus-store | 5,841 | 5,993 | 2,678 |
| crates/theseus-tools | 4,918 | 3,315 | 1,128 |
| crates/theseus-tui | 13,116 | 12,986 | 3,122 |
| crates/theseus-voice | 6,144 | 6,651 | 2,023 |
| crates/theseusd | 5,340 | 33,188 | 5,687 |
| infra/ | 554 | 1,432 | 193 |
| scripts/ | 10,150 | 9,198 | 3,682 |
| **Total** | **616,902** | **593,526** | **204,161** |
| vendor/ (third-party, not in the total) | 12,719 | 1,166 | 3,154 |

## Build sizes

### What each crate builds

From the `release` build of 90dabfa9 (an optimised build; a library's newest `.rlib` when the build directory holds several). An `.rlib` carries metadata and code for link-time optimisation, so it is far larger than what its crate adds to an executable: `scripts/size-breakdown.py` gives that.

| Crate | Library (.rlib) | Executables |
|---|---:|---|
| crates/theseus | 6.62 MB | theseus 11.33 MB |
| crates/theseus-aws | 1.75 MB |  |
| crates/theseus-aws-catalog | 8.26 MB |  |
| crates/theseus-aws-guard | 4.68 MB |  |
| crates/theseus-core | 275.65 MB |  |
| crates/theseus-discord | 68.84 MB |  |
| crates/theseus-exam | not built |  |
| crates/theseus-files | 3.62 MB |  |
| crates/theseus-follow | 0.26 MB |  |
| crates/theseus-google | 1.53 MB |  |
| crates/theseus-index | 9.46 MB | theseus-index 7.64 MB |
| crates/theseus-judge | 24.66 MB |  |
| crates/theseus-kernel | 13.15 MB |  |
| crates/theseus-lsp | 5.06 MB |  |
| crates/theseus-mcp | 5.11 MB |  |
| crates/theseus-memory | 1.05 MB |  |
| crates/theseus-ontology | 4.31 MB |  |
| crates/theseus-protocol | 73.85 MB |  |
| crates/theseus-root-install | 0.22 MB | theseus-root-install 0.36 MB |
| crates/theseus-sandbox | 2.61 MB |  |
| crates/theseus-sim | 4.88 MB | theseus-sim 40.48 MB |
| crates/theseus-store | 5.11 MB |  |
| crates/theseus-tools | 6.31 MB |  |
| crates/theseus-tui | — | theseus-tui 5.03 MB |
| crates/theseus-voice | 5.35 MB |  |
| crates/theseusd | — | theseusd 82.84 MB |

### Executables that are installed and run

| Executable | Size | Installed from |
|---|---:|---|
| theseus | 11.53 MB | 9840525d |
| theseus-index | 7.64 MB | 9840525d |
| theseus-tui | 5.27 MB | 9840525d |
| theseusd | 79.54 MB | 9840525d |
| theseus-root-install | 0.36 MB | an unrecorded commit |
| **Total** | **104.34 MB** | |
