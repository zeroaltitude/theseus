# STATS

The size of Theseus's code and of what it builds. Written by `scripts/stats.py`; do not edit it by hand.

Counted at commit d5a50e71, over every file git tracks:

- **Code**: a line that is not blank and not only comment (code with a comment after it is code). **Test code**: a file in a directory, or with a name (before its first dot), that is `test`, `tests`, `fixtures`, `benches`, `test_…`, `tests_…`, `…_test` or `…_tests`; files named `*.test.*`; and Rust's `#[cfg(test)] mod … { … }` blocks.
- **Comments**: `//`, `///`, `//!` and `/* … */` (nested, in Rust) in Rust, TypeScript, JavaScript, C and CSS; in Python, comment tokens and triple-quoted strings that begin a statement (read with Python's own tokenizer); in shell, a `#` that begins a word outside quotes. A marker inside a string literal is not a comment.
- **Not counted**: blank lines, and files that are not code (Markdown, TOML, JSON, data). `vendor/` is third-party and kept out of the total.

## Lines

| Where | Code | Test code | Comments |
|---|---:|---:|---:|
| bench/ | 67,988 | 57,882 | 16,508 |
| cockpit/ | 46,970 | 14,492 | 20,908 |
| crates/ (in no crate) | 93 | 62 | 159 |
| crates/theseus | 35,066 | 28,372 | 7,318 |
| crates/theseus-aws | 3,179 | 2,303 | 549 |
| crates/theseus-aws-catalog | 5,994 | 1,394 | 644 |
| crates/theseus-aws-guard | 2,857 | 1,522 | 323 |
| crates/theseus-core | 264,456 | 278,157 | 92,073 |
| crates/theseus-discord | 32,101 | 40,024 | 10,504 |
| crates/theseus-exam | 5,289 | 2,872 | 1,196 |
| crates/theseus-files | 2,318 | 591 | 296 |
| crates/theseus-follow | 651 | 853 | 283 |
| crates/theseus-google | 3,672 | 664 | 316 |
| crates/theseus-index | 12,386 | 6,501 | 3,025 |
| crates/theseus-judge | 13,128 | 11,195 | 4,514 |
| crates/theseus-kernel | 13,004 | 13,824 | 5,585 |
| crates/theseus-lsp | 3,153 | 1,472 | 694 |
| crates/theseus-mcp | 4,256 | 1,641 | 663 |
| crates/theseus-memory | 750 | 786 | 374 |
| crates/theseus-ontology | 3,546 | 4,106 | 956 |
| crates/theseus-protocol | 21,131 | 7,924 | 8,425 |
| crates/theseus-root-install | 696 | 440 | 122 |
| crates/theseus-sandbox | 2,891 | 3,616 | 1,642 |
| crates/theseus-sim | 18,152 | 3,616 | 3,917 |
| crates/theseus-store | 5,758 | 5,757 | 2,648 |
| crates/theseus-tools | 4,869 | 3,315 | 1,113 |
| crates/theseus-tui | 12,591 | 12,106 | 3,043 |
| crates/theseus-voice | 6,131 | 6,580 | 2,022 |
| crates/theseusd | 5,253 | 32,060 | 5,483 |
| infra/ | 554 | 1,432 | 193 |
| scripts/ | 10,025 | 6,896 | 3,440 |
| **Total** | **608,908** | **552,455** | **198,936** |
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
