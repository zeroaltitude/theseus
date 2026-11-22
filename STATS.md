# STATS

The size of Theseus's code and of what it builds. Written by `scripts/stats.py`; do not edit it by hand.

Counted at commit 662dc611, over every file git tracks:

- **Code**: a line that is not blank and not only comment (code with a comment after it is code). **Test code**: a file in a directory, or with a name (before its first dot), that is `test`, `tests`, `fixtures`, `benches`, `test_…`, `tests_…`, `…_test` or `…_tests`; files named `*.test.*`; and Rust's `#[cfg(test)] mod … { … }` blocks.
- **Comments**: `//`, `///`, `//!` and `/* … */` (nested, in Rust) in Rust, TypeScript, JavaScript, C and CSS; in Python, comment tokens and triple-quoted strings that begin a statement (read with Python's own tokenizer); in shell, a `#` that begins a word outside quotes. A marker inside a string literal is not a comment.
- **Not counted**: blank lines, and files that are not code (Markdown, TOML, JSON, data). `vendor/` is third-party and kept out of the total.

## Lines

| Where | Code | Test code | Comments |
|---|---:|---:|---:|
| bench/ | 69,755 | 59,593 | 16,672 |
| cockpit/ | 47,773 | 14,842 | 21,093 |
| crates/ (in no crate) | 93 | 62 | 159 |
| crates/theseus | 36,238 | 29,046 | 7,484 |
| crates/theseus-aws | 3,179 | 2,303 | 549 |
| crates/theseus-aws-catalog | 5,994 | 1,394 | 644 |
| crates/theseus-aws-guard | 2,857 | 1,522 | 323 |
| crates/theseus-core | 270,011 | 290,058 | 94,454 |
| crates/theseus-discord | 33,865 | 41,549 | 10,876 |
| crates/theseus-exam | 5,298 | 2,873 | 1,196 |
| crates/theseus-files | 2,318 | 591 | 296 |
| crates/theseus-follow | 651 | 853 | 283 |
| crates/theseus-google | 3,672 | 664 | 316 |
| crates/theseus-index | 12,386 | 6,501 | 3,025 |
| crates/theseus-judge | 13,059 | 11,099 | 4,475 |
| crates/theseus-kernel | 13,004 | 13,824 | 5,585 |
| crates/theseus-lsp | 3,153 | 1,472 | 694 |
| crates/theseus-mcp | 4,256 | 1,641 | 663 |
| crates/theseus-memory | 965 | 1,659 | 573 |
| crates/theseus-ontology | 3,546 | 4,106 | 956 |
| crates/theseus-protocol | 21,284 | 8,266 | 8,499 |
| crates/theseus-root-install | 696 | 440 | 122 |
| crates/theseus-sandbox | 2,891 | 3,616 | 1,642 |
| crates/theseus-sim | 18,142 | 3,616 | 3,914 |
| crates/theseus-store | 5,729 | 5,757 | 2,641 |
| crates/theseus-tools | 4,869 | 3,315 | 1,113 |
| crates/theseus-tui | 13,421 | 12,859 | 3,239 |
| crates/theseus-voice | 6,131 | 6,580 | 2,022 |
| crates/theseusd | 5,254 | 31,926 | 5,443 |
| infra/ | 554 | 1,432 | 193 |
| scripts/ | 10,014 | 5,759 | 3,366 |
| **Total** | **621,058** | **569,218** | **202,510** |
| vendor/ (third-party, not in the total) | 12,719 | 1,166 | 3,154 |

## Build sizes

### What each crate builds

From the `release` build of 98fa4df8 (an optimised build; a library's newest `.rlib` when the build directory holds several). An `.rlib` carries metadata and code for link-time optimisation, so it is far larger than what its crate adds to an executable: `scripts/size-breakdown.py` gives that.

| Crate | Library (.rlib) | Executables |
|---|---:|---|
| crates/theseus | 6.80 MB | theseus 11.53 MB |
| crates/theseus-aws | 1.75 MB |  |
| crates/theseus-aws-catalog | 8.26 MB |  |
| crates/theseus-aws-guard | 4.68 MB |  |
| crates/theseus-core | 268.24 MB |  |
| crates/theseus-discord | 69.79 MB |  |
| crates/theseus-exam | not built |  |
| crates/theseus-files | 3.58 MB |  |
| crates/theseus-follow | 0.26 MB |  |
| crates/theseus-google | 1.53 MB |  |
| crates/theseus-index | 9.42 MB | theseus-index 7.64 MB |
| crates/theseus-judge | 24.53 MB |  |
| crates/theseus-kernel | 13.10 MB |  |
| crates/theseus-lsp | 5.01 MB |  |
| crates/theseus-mcp | 5.12 MB |  |
| crates/theseus-memory | 2.09 MB |  |
| crates/theseus-ontology | 4.29 MB |  |
| crates/theseus-protocol | 74.95 MB |  |
| crates/theseus-root-install | 0.22 MB | theseus-root-install 0.36 MB |
| crates/theseus-sandbox | 2.61 MB |  |
| crates/theseus-sim | 4.88 MB | theseus-sim 40.35 MB |
| crates/theseus-store | 5.00 MB |  |
| crates/theseus-tools | 6.29 MB |  |
| crates/theseus-tui | — | theseus-tui 5.27 MB |
| crates/theseus-voice | 5.34 MB |  |
| crates/theseusd | — | theseusd 83.19 MB |

### Executables that are installed and run

| Executable | Size | Installed from |
|---|---:|---|
| theseus | 11.53 MB | 9840525d |
| theseus-index | 7.64 MB | 9840525d |
| theseus-tui | 5.27 MB | 9840525d |
| theseusd | 79.54 MB | 9840525d |
| theseus-root-install | 0.36 MB | an unrecorded commit |
| **Total** | **104.34 MB** | |
