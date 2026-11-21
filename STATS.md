# STATS

The size of Theseus's code and of what it builds. Written by `scripts/stats.py`; do not edit it by hand.

Counted at commit cfbddc7c, over every file git tracks:

- **Code**: a line that is not blank and not only comment (code with a comment after it is code). **Test code**: files under a `tests/` or `test/` directory, files named `tests…`, `test_…` or `*.test.*`, and Rust's `#[cfg(test)] mod … { … }` blocks.
- **Comments**: `//`, `///`, `//!` and `/* … */` (nested, in Rust) in Rust, TypeScript, JavaScript, C and CSS; `#` lines in Python and shell, and Python's `"""` strings that begin a line. A marker inside a string literal is not a comment.
- **Not counted**: blank lines, and files that are not code (Markdown, TOML, JSON, data). `vendor/` is third-party and kept out of the total.

## Lines

| Where | Code | Test code | Comments |
|---|---:|---:|---:|
| bench/ | 69,588 | 59,556 | 16,876 |
| cockpit/ | 47,860 | 14,913 | 21,073 |
| crates/ (in no crate) | 93 | 62 | 159 |
| crates/theseus | 39,729 | 26,133 | 7,499 |
| crates/theseus-aws | 3,179 | 2,303 | 549 |
| crates/theseus-aws-catalog | 5,994 | 1,394 | 644 |
| crates/theseus-aws-guard | 2,857 | 1,522 | 323 |
| crates/theseus-core | 276,050 | 279,315 | 94,411 |
| crates/theseus-discord | 40,085 | 35,755 | 10,900 |
| crates/theseus-exam | 5,328 | 2,921 | 1,211 |
| crates/theseus-files | 2,318 | 591 | 296 |
| crates/theseus-follow | 651 | 853 | 283 |
| crates/theseus-google | 3,672 | 664 | 316 |
| crates/theseus-index | 13,526 | 5,216 | 2,995 |
| crates/theseus-judge | 12,869 | 11,060 | 4,440 |
| crates/theseus-kernel | 13,004 | 13,824 | 5,585 |
| crates/theseus-lsp | 3,175 | 1,450 | 694 |
| crates/theseus-mcp | 4,256 | 1,641 | 663 |
| crates/theseus-memory | 1,962 | 2,778 | 998 |
| crates/theseus-ontology | 3,546 | 4,106 | 956 |
| crates/theseus-protocol | 21,087 | 8,297 | 8,475 |
| crates/theseus-root-install | 696 | 440 | 122 |
| crates/theseus-sandbox | 2,891 | 3,616 | 1,642 |
| crates/theseus-sim | 18,142 | 3,616 | 3,914 |
| crates/theseus-store | 5,683 | 5,757 | 2,632 |
| crates/theseus-tools | 4,869 | 3,315 | 1,113 |
| crates/theseus-tui | 13,529 | 12,944 | 3,270 |
| crates/theseus-voice | 6,131 | 6,580 | 2,022 |
| crates/theseusd | 5,257 | 32,120 | 5,458 |
| infra/ | 554 | 1,422 | 203 |
| scripts/ | 10,213 | 5,584 | 3,418 |
| **Total** | **638,794** | **549,748** | **203,140** |
| vendor/ (third-party, not in the total) | 12,827 | 1,058 | 3,154 |

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
