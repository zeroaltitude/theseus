# STATS

The size of Theseus's code and of what it builds. Written by `scripts/stats.py`; do not edit it by hand.

Counted at the merge of 0ed1ff1e into 6fc09050, over every file git tracks:

- **Code**: a line that is not blank and not only comment (code with a comment after it is code). **Test code**: a file in a directory, or with a name (before its first dot), that is `test`, `tests`, `fixtures`, `benches`, `test_…`, `tests_…`, `…_test` or `…_tests`; files named `*.test.*`; and Rust's `#[cfg(test)] mod … { … }` blocks.
- **Comments**: `//`, `///`, `//!` and `/* … */` (nested, in Rust) in Rust, TypeScript, JavaScript, C and CSS; in Python, comment tokens and triple-quoted strings that begin a statement (read with Python's own tokenizer); in shell, a `#` that begins a word outside quotes. A marker inside a string literal is not a comment.
- **Not counted**: blank lines, and files that are not code (Markdown, TOML, JSON, data). `vendor/` is third-party and kept out of the total.

## Lines

| Where | Code | Test code | Comments |
|---|---:|---:|---:|
| bench/ | 68,903 | 58,713 | 16,545 |
| cockpit/ | 46,940 | 14,832 | 20,790 |
| crates/ (in no crate) | 93 | 62 | 159 |
| crates/theseus | 34,922 | 28,434 | 7,264 |
| crates/theseus-aws | 3,179 | 2,303 | 549 |
| crates/theseus-aws-catalog | 5,994 | 1,394 | 644 |
| crates/theseus-aws-guard | 2,857 | 1,522 | 323 |
| crates/theseus-core | 262,779 | 286,039 | 92,171 |
| crates/theseus-discord | 32,170 | 40,360 | 10,531 |
| crates/theseus-exam | 5,442 | 3,242 | 1,256 |
| crates/theseus-files | 2,377 | 682 | 297 |
| crates/theseus-follow | 657 | 875 | 283 |
| crates/theseus-google | 3,676 | 682 | 316 |
| crates/theseus-index | 12,444 | 6,553 | 3,023 |
| crates/theseus-judge | 13,216 | 11,236 | 4,457 |
| crates/theseus-kernel | 13,063 | 13,949 | 5,585 |
| crates/theseus-lsp | 3,189 | 1,496 | 695 |
| crates/theseus-mcp | 4,331 | 1,677 | 675 |
| crates/theseus-memory | 721 | 754 | 367 |
| crates/theseus-ontology | 3,618 | 4,160 | 960 |
| crates/theseus-protocol | 20,875 | 7,906 | 8,308 |
| crates/theseus-root-install | 696 | 440 | 122 |
| crates/theseus-sandbox | 2,891 | 3,616 | 1,642 |
| crates/theseus-sim | 18,163 | 3,672 | 3,924 |
| crates/theseus-store | 5,841 | 5,993 | 2,669 |
| crates/theseus-tools | 4,869 | 3,315 | 1,113 |
| crates/theseus-tui | 12,972 | 12,708 | 3,055 |
| crates/theseus-voice | 6,144 | 6,651 | 2,023 |
| crates/theseusd | 5,316 | 32,372 | 5,535 |
| infra/ | 554 | 1,432 | 193 |
| scripts/ | 10,008 | 7,080 | 3,417 |
| **Total** | **608,900** | **564,150** | **198,891** |
| vendor/ (third-party, not in the total) | 12,719 | 1,166 | 3,154 |

## Build sizes

### What each crate builds

From the `release` build of 773c6dba (an optimised build; a library's newest `.rlib` when the build directory holds several). An `.rlib` carries metadata and code for link-time optimisation, so it is 

| Crate | Library (.rlib) | Executables |
|---|---:|---|
| crates/theseus | 6.76 MB | theseus 11.21 MB |
| crates/theseus-aws | 1.75 MB |  |
| crates/theseus-aws-catalog | 8.26 MB |  |
| crates/theseus-aws-guard | 4.68 MB |  |
| crates/theseus-core | 273.68 MB |  |
| crates/theseus-discord | 68.06 MB |  |
| crates/theseus-exam | not built |  |
| crates/theseus-files | 3.58 MB |  |
| crates/theseus-follow | 0.26 MB |  |
| crates/theseus-google | 1.53 MB |  |
| crates/theseus-index | 9.46 MB | theseus-index 7.64 MB |
| crates/theseus-judge | 24.59 MB |  |
| crates/theseus-kernel | 13.10 MB |  |
| crates/theseus-lsp | 5.01 MB |  |
| crates/theseus-mcp | 5.12 MB |  |
| crates/theseus-memory | 1.06 MB |  |
| crates/theseus-ontology | 4.29 MB |  |
| crates/theseus-protocol | 74.85 MB |  |
| crates/theseus-root-install | not built |  |
| crates/theseus-sandbox | 2.61 MB |  |
| crates/theseus-sim | 4.88 MB | theseus-sim 40.47 MB |
| crates/theseus-store | 5.09 MB |  |
| crates/theseus-tools | 6.29 MB |  |
| crates/theseus-tui | — | theseus-tui 5.04 MB |
| crates/theseus-voice | 5.34 MB |  |
| crates/theseusd | — | theseusd 82.03 MB |

### Executables that are installed and run

| Executable | Size | Installed from |
|---|---:|---|
| theseus | 11.53 MB | 9840525d |
| theseus-index | 7.64 MB | 9840525d |
| theseus-tui | 5.27 MB | 9840525d |
| theseusd | 79.54 MB | 9840525d |
| theseus-root-install | 0.36 MB | an unrecorded commit |
| **Total** | **104.34 MB** | |
