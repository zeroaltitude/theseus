# theseus-root-install

The helper that does one thing (issue 22; `docs/design/self-install.md` is the design, and says what the root-owned
directory protects and what this gives up): `theseus-root-install <commit>` copies the four programs of a commit from the
fixed staging directory into `/usr/local/lib/theseus/<commit8>/`, root-owned, and changes nothing else. Standard library and
`libc` only. Run by the daemon through `sudo -n` after the owner's yes; installed once by the administrator
(`scripts/root-install.sh`), never by an ordinary install.

Key modules: `lib.rs` (the argument, `Config`, `run`), `install.rs` (copy, check, rename), `stage.rs` (trusted
directories, the staging directory's files), `stamp.rs` (the build stamp), `sys.rs` (the system calls, by descriptor).
Read by: `theseus-core`'s `rsi/join/root_install` (it calls the program, not the crate).

- **Never run what is copied.** Running `--version` of an owner-written program as root hands the owner root, so the
  commit check reads the stamp out of the bytes of the copy. It catches a mistake, not an attacker.
- **The real paths are compiled in** (`Config::real`); `main.rs` names no other configuration and reads no part of its
  environment, and `tests::only_main_reads_the_process_and_only_to_clear_it` holds the sources to it. Any new input is a
  new way for the owner's program to steer a root program: do not add one.
- **Every name after the first open is resolved by descriptor with `O_NOFOLLOW`**, and every check reads the open
  descriptor or the copy, never a name twice.
- Tests run as the user, against directories they own (`Config { real: false, .. }`); `tests/real_program.rs` runs the
  real binary. What no test can show without root: the ownership of results, and refusing a file someone else owns.
