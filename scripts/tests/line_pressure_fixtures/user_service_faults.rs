
fn faults<'a>() -> [Fault<'a>; 12] {
    [
        (
            "no user manager",
            |r| r.set("system-running", "offline\n"),
            &[
                "FAIL  systemd: no user manager answers",
                "on WSL: put systemd=true under [boot]",
            ],
        ),
        (
            "no token named",
            |_| {},
            &["FAIL  token file: none named", "then add: --op-token-file"],
        ),
        (
            "token mode 0644",
            |r| r.token(0o644),
            &[
                "FAIL  token file: ",
                "mode 0644 is looser than 0600: chmod 600 ",
            ],
        ),
        (
            "token mode 0200",
            |r| r.token(0o200),
            &["mode 0200 does not let you read it: chmod 600 "],
        ),
        (
            "token missing",
            |r| std::fs::remove_file(r.token_path()).unwrap(),
            &["does not exist", "make it with mode 0600"],
        ),
        (
            "token a directory",
            |r| {
                std::fs::remove_file(r.token_path()).unwrap();
                std::fs::create_dir(r.token_path()).unwrap();
            },
            &["is not a regular file (directory): name the file itself"],
        ),
        (
            "token empty",
            |r| std::fs::write(r.token_path(), "").unwrap(),
            &["it is empty: put the 1Password"],
        ),
        (
            "token another user's",
            |r| r.stat_says("regular file|0|600|40"),
            &["owned by uid 0, not by you (", "sudo chown "],
        ),
        (
            "no theseusd",
            |r| std::fs::remove_file(r.home().join("bin/theseusd")).unwrap(),
            &["FAIL  theseusd: not on PATH"],
        ),
        (
            "no op",
            |r| std::fs::remove_file(r.home().join("bin/op")).unwrap(),
            &["FAIL  op: the 1Password CLI is not on PATH"],
        ),
        (
            "config not a vault reference",
            |_| {},
            // The plan makes a relative path absolute, and the check reads it from the plan.
            &[
                "FAIL  config: ",
                "/not-a-ref is not a readable file (THESEUS_CONFIG names it)",
                "export THESEUS_CONFIG=op://<vault>/<item>/notesPlain",
            ],
        ),
        (
            "config reference malformed",
            |_| {},
            &["FAIL  config: op://only-a-vault is not op://<vault>/<item>/<field>"],
        ),
    ]
}
