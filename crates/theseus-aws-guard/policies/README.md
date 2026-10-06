# The guardrails' AWS side, generated

Every file here is generated from `../guardrails.toml` by the crate's own test (`tests/policies.rs`), which
also fails when a file is out of step with the list. Never edit one by hand: change the list, then run

```bash
THESEUS_GUARD_WRITE=1 cargo test -p theseus-aws-guard --test policies
```

| file | what it is | where it goes |
|---|---|---|
| `theseus-guard-limits.json` | the guardrails' denies | a managed policy in the foundation stack, passed as a session policy to every work and job session |
| `theseus-guard-iac.json`, `theseus-guard-iac-2.json` | the IaC-only denies (two policies: one cannot hold them) | the same |
| `theseus-boundary.json` | allow-all less both guards, compacted to fit one policy | the permissions boundary of every hand role |
| `theseus-scp-guardrails.json`, `theseus-scp-guardrails-2.json` | the guardrails as service control policies | **for the owner's management account, to attach to this account. Nothing here applies them.** |

The guards and the boundary deny every guarded action to whatever carries them. The SCPs deny the same actions
across the account, except that an entry marked `deny-except-deployer` lets `theseus-cfn-deployer` through, so
a reviewed stack can still change what it guards. An SCP never applies to the management account itself.
