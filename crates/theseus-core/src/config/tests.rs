use super::*;
use crate::policy::Posture;

/// Consideration 5 of review 2: `~` and `$VAR` without `shellexpand`. What
/// does not resolve is left as written.
#[test]
fn a_path_expands_home_and_variables_as_a_shell_would() {
    let env = |name: &str| match name {
        "HOME" => Some("/home/invented".to_string()),
        "XDG_STATE" => Some("/var/invented".to_string()),
        "EMPTY" => Some(String::new()),
        _ => None,
    };
    for (p, want) in [
        ("~", "/home/invented"),
        ("~/.theseus", "/home/invented/.theseus"),
        ("~other/x", "~other/x"),
        ("a/~/b", "a/~/b"),
        ("$XDG_STATE/theseus", "/var/invented/theseus"),
        ("${XDG_STATE}x/y", "/var/inventedx/y"),
        ("~/$XDG_STATE", "/home/invented//var/invented"),
        ("$UNSET/x", "$UNSET/x"),
        ("${UNSET}/x", "${UNSET}/x"),
        ("x$EMPTY/y", "x/y"),
        ("cost: $5 and $", "cost: $5 and $"),
        ("${unclosed", "${unclosed"),
        ("${}", "${}"),
        ("$$HOME", "$/home/invented"),
        ("/plain/path", "/plain/path"),
        ("é$HOME/中", "é/home/invented/中"),
    ] {
        assert_eq!(expand_with(p, env), want, "{p}");
    }
    assert_eq!(
        expand_with("~/x", |_| None),
        "~/x",
        "no HOME: left as written"
    );
    assert_eq!(
        expand_with("~/x", |_| Some(String::new())),
        "~/x",
        "an empty HOME: left as written"
    );
}

#[test]
fn example_template_parses_and_validates() {
    let cfg = Config::example();
    cfg.validate().unwrap();
    assert_eq!(cfg.model.live, "sonnet");
    assert!(cfg.profiles.contains_key("glm"));
    assert!(cfg.providers.contains_key("zai"));
    assert_eq!(cfg.telemetry.otlp_endpoint, None);
}

/// The template's draft guidance is commented (theseus-a1wvq.12): with no `cn_draft` in any class, nothing drafts.
#[test]
fn the_templates_draft_guidance_is_off_by_default() {
    let c = &Config::example().routing.classes;
    for class in [
        &c.trivial,
        &c.quick,
        &c.chat,
        &c.sophisticated,
        &c.deep_coding,
        &c.routine_coding,
        &c.review,
    ] {
        assert!(class.cn_draft.is_none() && class.final_review.is_empty());
    }
}

/// theseus-0hufe: the template's three caps of a response's calls are the
/// defaults a config without them gets, so neither drifts from the other
/// (a planted default of 8 for `parallel_runs` passed every test before).
#[test]
fn the_templates_parallel_caps_are_the_defaults() {
    let t = Config::example().tools;
    assert_eq!(t.parallel_runs, default_parallel_runs());
    assert_eq!(t.parallel_reads, default_parallel_reads());
    assert_eq!(t.parallel_writes, default_parallel_writes());
    let d = ToolsConfig::default();
    assert_eq!(
        (d.parallel_runs, d.parallel_reads, d.parallel_writes),
        (t.parallel_runs, t.parallel_reads, t.parallel_writes)
    );
}

/// theseus-7m7w.3: `[tools] job_wait_max_secs` is 1,800 in the template and
/// by default, at least `proc_sync_secs` and at most the tool's own deadline.
#[test]
fn job_wait_max_secs_has_a_default_and_a_range() {
    let t = Config::example().tools;
    assert_eq!(t.job_wait_max_secs, default_job_wait_max_secs());
    assert_eq!(t.job_wait_max_secs, 1800);
    assert_eq!(ToolsConfig::default().job_wait_max_secs, 1800);
    let mut cfg = Config::example();
    for (secs, sync, ok) in [
        (60, 60, true),
        (3600, 60, true),
        (59, 60, false),
        (3601, 60, false),
        (1800, 3000, false),
        (3000, 3000, true),
    ] {
        cfg.tools.job_wait_max_secs = secs;
        cfg.tools.proc_sync_secs = sync;
        let r = cfg.validate();
        assert_eq!(r.is_ok(), ok, "{secs} with sync {sync}: {r:?}");
        if let Err(e) = r {
            assert!(format!("{e:#}").contains("job_wait_max_secs"), "{e:#}");
        }
    }
}

/// theseus-53ujg.6: `[tools] job_wait_beat_secs` is 600 in the template and by
/// default; 0 is off, else it is at most the tool's deadline, whatever
/// `proc_sync_secs` and `job_wait_max_secs` say (a beat past the longest wait
/// never fires).
#[test]
fn job_wait_beat_secs_has_a_default_and_a_range() {
    let t = Config::example().tools;
    assert_eq!(t.job_wait_beat_secs, default_job_wait_beat_secs());
    assert_eq!(t.job_wait_beat_secs, 600);
    assert_eq!(ToolsConfig::default().job_wait_beat_secs, 600);
    let mut cfg = Config::example();
    for (beat, ok) in [(0, true), (1, true), (3600, true), (3601, false)] {
        cfg.tools.job_wait_beat_secs = beat;
        let r = cfg.validate();
        assert_eq!(r.is_ok(), ok, "beat {beat}: {r:?}");
        if let Err(e) = r {
            assert!(format!("{e:#}").contains("job_wait_beat_secs"), "{e:#}");
        }
    }
    // The default is valid beside the bench's 900 s foreground run and a short cap.
    cfg.tools.job_wait_beat_secs = default_job_wait_beat_secs();
    (cfg.tools.proc_sync_secs, cfg.tools.job_wait_max_secs) = (900, 1800);
    cfg.validate().unwrap();
    (cfg.tools.proc_sync_secs, cfg.tools.job_wait_max_secs) = (60, 60);
    cfg.validate().unwrap();
}

/// theseus-n88g.1 (D6): a secret may come from outside the vault, from
/// the daemon's environment or a private file, and a malformed entry is
/// refused by its name.
#[test]
fn a_secret_may_come_from_the_environment_or_a_file() {
    let mut cfg = Config::example();
    cfg.secrets
        .insert("anthropic_api_key".into(), "env:ANTHROPIC_API_KEY".into());
    cfg.secrets
        .insert("zai_api_key".into(), "file:~/.config/invented/zai".into());
    cfg.validate().unwrap();
    for (bad, why) in [
        ("file:zai.txt", "absolute path"),
        ("env:", "variable's name"),
        ("env:ZAI-KEY", "variable's name"),
        (
            "vault:zai",
            "not a valid op:// reference, env:NAME, or file:PATH",
        ),
    ] {
        cfg.secrets.insert("zai_api_key".into(), bad.into());
        let e = format!("{:#}", cfg.validate().unwrap_err());
        assert!(
            e.contains("secrets.zai_api_key") && e.contains(why),
            "{bad}: {e}"
        );
    }
}

/// Every commented-out parameter must be a real parameter with a valid
/// value: un-comment them all and the result must still parse under
/// deny_unknown_fields. This is what stops the template from lying.
#[test]
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
fn example_template_uncommented_still_parses() {
    let mut out = String::new();
    for line in Config::EXAMPLE_TOML.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("# ") {
            let looks_like_toml = rest.starts_with('[')
                || rest
                    .split_once('=')
                    .map(|(k, _)| {
                        let k = k.trim();
                        // A quoted key: `"proc.run" = …`, `"server/tool" = …`.
                        let quoted = k.len() > 2
                            && k.starts_with('"')
                            && k.ends_with('"')
                            && !k[1..k.len() - 1].contains('"');
                        quoted
                            || (!k.is_empty()
                                && k.chars()
                                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                    })
                    .unwrap_or(false);
            if looks_like_toml {
                out.push_str(rest);
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    let cfg = Config::from_toml(&out).unwrap_or_else(|e| {
            panic!("un-commented template must parse (a documented key is stale or its value invalid): {e}\n{out}")
        });
    assert_eq!(
        cfg.telemetry.otlp_endpoint.as_deref(),
        Some("http://127.0.0.1:4318")
    );
    assert!(cfg.providers["zai"].timeouts.is_some());
    assert!(cfg.model.system.is_some());
    // The secrets from outside the vault (theseus-n88g.1) are real entries.
    assert_eq!(cfg.secrets["ci_runner_key"], "env:CI_RUNNER_KEY");
    assert_eq!(
        cfg.secrets["ci_deploy_key"],
        "file:~/.config/theseus/deploy-key"
    );
    // The 1-hour cache TTL (theseus-ev1), on the implicit profile and on
    // the Sonnet one; the GLM one says none, so inherits it (theseus-ezeg).
    assert_eq!(cfg.cache.ttl, CacheTtl::OneHour);
    assert_eq!(cfg.profiles["sonnet"].cache_ttl, Some(CacheTtl::OneHour));
    assert_eq!(cfg.profile("glm").unwrap().ttl(), CacheTtl::OneHour);
    let new = &cfg.catalog["some-new-model"];
    assert_eq!(
        (new.cache_write_1h_per_mtok, new.caches),
        (Some(6.0), Some(true))
    );
    // The commented tool lines and the [policy.mcp] example are real too.
    assert_eq!(cfg.policy.tools["proc.run"], Posture::Approve);
    assert_eq!(cfg.policy.tools["http.fetch"], Posture::Notify);
    assert_eq!(cfg.policy.tools["task.create"], Posture::Notify);
    assert_eq!(cfg.policy.tools["wake.at"], Posture::Notify);
    assert_eq!(cfg.policy.tools["aws.call"], Posture::Approve);
    assert_eq!(cfg.policy.tools["aws.stack.apply"], Posture::Approve);
    assert_eq!(cfg.policy.tools["extend.propose"], Posture::Notify);
    assert_eq!(cfg.policy.tools["channel.post"], Posture::Notify);
    // wake.move and wake.cancel (theseus-lherl), one (pw1q.8), routine.create (theseus-pvm5c),
    // `watch` (f2r2b), project.attach (theseus-n5ui2), google.call (batvg.1), gmail.* (five,
    // batvg.2), drive.* (four, batvg.3), sheets.* and slides.* (three, batvg.4) and calendar.*
    // (two, batvg.5), task.wait (theseus-z9mze.2) and task.fan_out (theseus-z9mze.3).
    assert_eq!(cfg.policy.tools.len(), 76);
    // The AWS account's table, and [policy.aws]'s lines (rows 29 and 30, C1 and C2).
    let a = &cfg.aws.accounts["111122223333"];
    assert_eq!(a.credentials, AwsCredentialNames::default());
    assert_eq!(a.region, "us-west-2");
    assert_eq!(a.allowed_regions(), ["us-west-2", "us-east-1"]);
    assert_eq!(a.endpoint.as_deref(), Some("http://127.0.0.1:4566"));
    assert_eq!(a.owner_role.as_deref(), Some("theseus-owner"));
    assert_eq!(a.deployment(), "theseus-desktop");
    assert_eq!(a.monthly_budget_usd, Some(50));
    let net = a
        .hands_network
        .as_ref()
        .expect("the template's existing network");
    assert_eq!((net.subnets.len(), net.security_group.is_some()), (2, true));
    assert_eq!(
        cfg.broker.programs["aws"].aws_account.as_deref(),
        Some("111122223333")
    );
    assert_eq!(cfg.policy.aws["write"], Posture::Notify);
    assert_eq!(cfg.policy.aws["run"], Posture::Notify);
    assert_eq!(cfg.policy.aws["read"], Posture::Open);
    assert_eq!(cfg.policy.aws["ec2"], Posture::Notify);
    assert_eq!(cfg.policy.aws["s3:ListBuckets"], Posture::Approve);
    assert_eq!(cfg.policy.mcp["some-server"], Posture::Notify);
    assert_eq!(cfg.policy.mcp["some-server/read-only-tool"], Posture::Open);
    assert_eq!(
        cfg.policy.mcp["some-server/dangerous-tool"],
        Posture::Approve
    );
    // The broker's example grant and posture are real (theseus-dcy).
    assert_eq!(cfg.broker.programs["gh"].env["GH_TOKEN"], "github_token");
    assert_eq!(cfg.broker.secrets["github_token"].posture, Posture::Notify);
    // [voice] and its key's line are real too (rows 77 and 78).
    assert!(cfg.voice.enabled && cfg.secrets.contains_key(&cfg.voice.key_secret));
    // [mcp_server] and its key's line are real too (step 41b).
    assert!(cfg.mcp_server.enabled && cfg.secrets.contains_key(&cfg.mcp_server.key_secret));
    assert_eq!(cfg.mcp_server.posture_floor, Posture::Approve);
    crate::sandbox::the_templates_sandbox_section(&cfg.sandbox);
    memory::the_templates_memory_section(&cfg.memory);
    judge::the_templates_judge_section(&cfg);
    routing::the_templates_routing_section(&cfg);
    lsp::the_templates_lsp_section(&cfg.lsp);
    crate::broker::the_templates_broker_section(&cfg);
    crate::broker::the_templates_harness_only_keys(&cfg);
    mcp::the_templates_mcp_section(&cfg);
}

/// theseus-8d1b: the template and the default config name no one's
/// vault. Every `op://` reference the template holds, in a value or a
/// comment, names its vault and item by placeholders (`<your vault>`),
/// and the default config is a local file.
#[test]
fn the_template_and_the_default_name_no_ones_vault() {
    assert!(!DEFAULT_CONFIG.contains("op://"), "{DEFAULT_CONFIG}");
    let placeholder = |s: &str| s.starts_with('<') && s.ends_with('>');
    let mut refs = 0;
    for (at, _) in Config::EXAMPLE_TOML.match_indices("op://") {
        let rest = &Config::EXAMPLE_TOML[at + "op://".len()..];
        // Prose that names the scheme ("an op:// reference").
        if rest.starts_with(char::is_whitespace) {
            continue;
        }
        let mut parts = rest.split('/');
        let (vault, item) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        assert!(
            placeholder(vault) && placeholder(item),
            "a reference that names a vault or an item: op://{vault}/{item}/…"
        );
        refs += 1;
    }
    assert!(refs >= 8, "{refs} references");
    for (name, r) in &Config::example().secrets {
        let r = SecretRef::parse(r).unwrap();
        assert_eq!(r.vault, "<your vault>", "{name}");
    }
}

/// The template's [policy.tools] names every tool in the registry, one
/// line each (set or commented), and nothing else, so it cannot drift
/// from the tools as they are added.
#[test]
fn example_template_lists_every_tool_under_policy_tools() {
    let listed: Vec<String> = Config::EXAMPLE_TOML
        .lines()
        .skip_while(|l| l.trim() != "[policy.tools]")
        .skip(1)
        .take_while(|l| !l.starts_with('[') && !l.starts_with("# ["))
        .filter_map(|l| {
            let l = l.strip_prefix("# ").unwrap_or(l).trim_start();
            let (name, rest) = l.strip_prefix('"')?.split_once('"')?;
            rest.trim_start().starts_with('=').then(|| name.to_string())
        })
        .collect();
    let tools: Vec<String> = theseus_tools::default_registry()
        .all()
        .map(|t| t.name().to_string())
        .chain(crate::web::NAMES.map(String::from))
        .chain(crate::task::NAMES.map(String::from))
        .chain(crate::task_graph::tools::NAMES.map(String::from))
        .chain(crate::wake::NAMES.map(String::from))
        .chain(crate::routine_tool::NAMES.map(String::from))
        .chain(crate::project_tool::NAMES.map(String::from))
        .chain(crate::glide::NAMES.map(String::from))
        .chain(crate::extend::NAMES.map(String::from))
        .chain(crate::aws::NAMES.map(String::from))
        .chain(crate::google::NAMES.map(String::from))
        .chain(crate::term::NAMES.map(String::from))
        .chain(crate::lsp::NAMES.map(String::from))
        .chain(crate::memory_lookup::NAMES.map(String::from))
        .chain(crate::rsi::pr::NAMES.map(String::from))
        .collect();
    for t in &tools {
        assert!(
            listed.contains(t),
            "the template's [policy.tools] must list `{t}`: {listed:?}"
        );
    }
    for l in &listed {
        assert!(tools.contains(l), "[policy.tools] lists `{l}`, not a tool");
    }
    assert_eq!(listed.len(), tools.len(), "one line per tool: {listed:?}");
    // Reads stay quiet out of the box; everything else inherits (an AWS
    // call's posture is [policy.aws]'s).
    let set = Config::example().policy.tools;
    let reads = [
        "fs.read",
        "fs.glob",
        "fs.grep",
        "fs.list",
        "git.diff",
        "git.log",
        "text.diff",
        "aws.describe",
        "term.read",
        "term.close",
        "job.read",
        "job.wait",
        "memory.lookup",
    ];
    for r in reads {
        assert_eq!(set.get(r), Some(&Posture::Open), "{r}");
    }
    assert_eq!(set.len(), reads.len(), "{set:?}");
}

/// The template's [policy] is the config to set (theseus-8az): notify
/// everywhere, reads open, the default approve lists, no retired keys, a
/// commented [policy.mcp], and no word of a deny.
#[test]
fn example_template_is_the_policy_to_set() {
    let (cfg, warnings) = Config::parse(Config::EXAMPLE_TOML).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(cfg.policy.enforcement, Posture::Notify);
    assert_eq!(cfg.policy.allow_argv, default_allow_argv());
    assert_eq!(cfg.policy.approve_argv, default_approve_argv());
    assert_eq!(cfg.tools.approve_paths, default_approve_paths());
    assert!(cfg.policy.mcp.is_empty(), "[policy.mcp] stays commented");
    assert!(cfg.mcp.is_empty(), "[mcp.servers] stays commented");
    let section: Vec<&str> = Config::EXAMPLE_TOML
        .lines()
        .skip_while(|l| !l.contains("------ policy"))
        .take_while(|l| !l.contains("------ catalog"))
        .collect();
    assert!(section.contains(&"[policy]") && section.contains(&"# [policy.mcp]"));
    for l in &section {
        assert!(!l.to_lowercase().contains("refuse"), "{l}");
    }
    assert!(
        !Config::EXAMPLE_TOML.to_lowercase().contains("deny"),
        "the template never speaks of a deny"
    );
}

fn policy_only(policy: &str) -> Result<Config> {
    Ok(parse_policy(policy)?.0)
}

fn parse_policy(policy: &str) -> Result<(Config, Vec<String>)> {
    Config::parse(&format!(
        "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[policy]\n{policy}\n"
    ))
}

#[test]
fn a_policy_with_only_enforcement_loads_and_every_tool_inherits_it() {
    let (cfg, warnings) = parse_policy("enforcement = \"notify\"").unwrap();
    assert_eq!(cfg.policy.enforcement, Posture::Notify);
    assert!(cfg.policy.tools.is_empty() && cfg.policy.mcp.is_empty());
    assert!(warnings.is_empty());
    assert_eq!(policy_only("").unwrap().policy.enforcement, Posture::Open);
}

/// `[policy] external_text` (theseus-9bp, theseus-nsqqo): unset, it follows
/// `enforcement` (`approve` asks; `notify` and `open` notify); set, it wins;
/// anything but `ask` or `notify` fails, and the error names the two.
#[test]
fn an_unset_external_text_follows_enforcement() {
    use crate::external::Mode;
    for (enforcement, mode) in [
        ("approve", Mode::Ask),
        ("notify", Mode::Notify),
        ("open", Mode::Notify),
    ] {
        let p = policy_only(&format!("enforcement = \"{enforcement}\""))
            .unwrap()
            .policy;
        assert_eq!((p.external_mode(), p.external_key()), (mode, "enforcement"));
        let set = policy_only(&format!(
            "enforcement = \"{enforcement}\"\nexternal_text = \"ask\""
        ))
        .unwrap()
        .policy;
        assert_eq!(
            (set.external_mode(), set.external_key()),
            (Mode::Ask, "external_text")
        );
    }
    let loose = policy_only("enforcement = \"open\"\nexternal_text = \"notify\"").unwrap();
    assert_eq!(loose.policy.external_mode(), Mode::Notify);
    let e = format!(
        "{:#}",
        policy_only("external_text = \"approve\"").unwrap_err()
    );
    assert!(e.contains("ask") && e.contains("notify"), "{e}");
    let (t, _) = Config::parse(Config::EXAMPLE_TOML).unwrap();
    assert_eq!(
        t.policy.external_mode(),
        Mode::Ask,
        "the template shows ask"
    );
}

/// The spellings the loader once accepted with a warning, which no
/// deployment still uses (the owner's note checked by key name, 2026-09-29),
/// now fail to load like any unknown key (theseus-0g4): the renamed lists,
/// the policy class keys, the hook spans, and the old output-cap name.
#[test]
fn the_retired_spellings_fail_to_load() {
    for (section, key) in [
        ("tools", "deny_paths = [\"~/.ssh\"]"),
        ("policy", "deny_argv = [[\"sudo\"]]"),
        ("policy", "read = \"allow\""),
        ("policy", "run = \"confirm\""),
        ("telemetry", "hook_spans = false"),
        ("model", "max_tokens = 1000"),
    ] {
        let doc = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[{section}]\n{key}\n");
        let e = format!("{:#}", Config::parse(&doc).unwrap_err());
        assert!(e.contains("unknown field"), "{section}.{key}: {e}");
    }
}

/// `effort`, `thinking_display`, and a provider's `kind` are enums: a
/// value outside them fails to load and the error lists the ones there are.
#[test]
fn enum_keys_name_their_values_when_one_is_wrong() {
    for (doc, values) in [
        (
            "[model]\neffort = \"huge\"",
            "`low`, `medium`, `high`, `xhigh`, `max`",
        ),
        (
            "[profiles.p]\nmodel = \"m\"\nthinking_display = \"full\"",
            "`summarized`, `omitted`, `updates`",
        ),
        (
            "[providers.p]\napi_base = \"https://x\"\napi_key_secret = \"anthropic_api_key\"\n\
                 kind = \"openai\"",
            "`anthropic_messages`",
        ),
    ] {
        let text = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{doc}\n");
        let e = format!("{:#}", Config::parse(&text).unwrap_err());
        assert!(e.contains("unknown variant") && e.contains(values), "{e}");
    }
    let (cfg, _) = Config::parse(
        "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[model]\neffort = \"xhigh\"\n\
             thinking_display = \"omitted\"\n",
    )
    .unwrap();
    let d = &cfg.all_profiles()["default"];
    assert_eq!(
        (d.effort, d.thinking_display),
        (Some(Effort::Xhigh), ThinkingDisplay::Omitted)
    );
}

/// `[telemetry]` keeps its five keys (theseus-hee), and the exporter is
/// in every build, so an endpoint loads with no warning (until then a
/// build without the `otel` feature warned once). A blank endpoint counts
/// as unset.
#[test]
fn an_otlp_endpoint_loads_quietly_now_that_every_build_exports() {
    let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\notlp_headers = \"op://v/h/f\"\n\n[telemetry]\n";
    let (cfg, warnings) = Config::parse(&format!(
        "{base}otlp_endpoint = \"http://127.0.0.1:4318\"\nheaders_secret = \"otlp_headers\"\n\
             service_name = \"theseus-x\"\nmetrics_interval_secs = 5\nexport_timeout_secs = 3\n"
    ))
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let t = &cfg.telemetry;
    assert_eq!(t.endpoint(), Some("http://127.0.0.1:4318"));
    assert_eq!(t.headers_secret.as_deref(), Some("otlp_headers"));
    assert_eq!(
        (
            t.service_name.as_str(),
            t.metrics_interval_secs,
            t.export_timeout_secs
        ),
        ("theseus-x", 5, 3)
    );
    for quiet in ["", "otlp_endpoint = \"  \"\n"] {
        let (c, w) = Config::parse(&format!("{base}service_name = \"theseus\"\n{quiet}")).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.telemetry.endpoint(), None);
    }
}

/// The owner's vault config (its [kernel] keys checked 2026-09-29, by name
/// only) loads with no warning; the session limit is the $100 default; and
/// `theseusd config` shows `spend_limit_usd`.
#[test]
fn the_vault_config_loads_with_the_default_spend_limit() {
    let text = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [kernel]\nadmission_ceiling = 8\nheartbeat_secs = 60\n\
                    default_deadline_secs = 600\nconfirm_ttl_secs = 900\n";
    let (cfg, warnings) = Config::parse(text).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(cfg.kernel.spend_limit_usd, 100.0);
    assert_eq!(
        cfg.kernel.to_kernel_config().spend_limit_micros,
        100_000_000
    );
    let shown = toml::to_string(&cfg.kernel).unwrap();
    assert!(shown.contains("spend_limit_usd = 100.0"), "{shown}");
    let (t, w) = Config::parse(Config::EXAMPLE_TOML).unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(t.kernel.spend_limit_usd, 100.0);
    for bad in ["0.0", "-5.0", "nan"] {
        let doc = format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[kernel]\nspend_limit_usd = {bad}\n"
        );
        assert!(Config::parse(&doc).is_err(), "{bad}");
    }
}

/// The owner's vault config (its [discord] keys checked 2026-09-29, by name
/// only) has no `notice_embeds` (theseus-w4f). It loads unchanged, with no
/// warning and the embeds off; the key parses either way, and the
/// template leaves it off.
#[test]
fn the_vault_discord_section_loads_with_notice_embeds_off() {
    let text = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [discord]\nenabled = true\ntoken_secret = \"discord_bot_token\"\n\
                    bindings_file = \"bindings.toml\"\nedit_interval_ms = 1200\n";
    let (cfg, warnings) = Config::parse(text).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!cfg.discord.notice_embeds);
    assert_eq!(cfg.discord.edit_interval_ms, 1200);
    assert!(!DiscordConfig::default().notice_embeds);
    for on in [true, false] {
        let doc = text.replace("1200\n", &format!("1200\nnotice_embeds = {on}\n"));
        assert_eq!(Config::parse(&doc).unwrap().0.discord.notice_embeds, on);
    }
    assert!(!Config::example().discord.notice_embeds);
}

/// Context files in two levels (theseus-c48): `[context] files`, which
/// every session gets, then the files of the persona in play,
/// `[personas.<name>] files`, chosen by `[context] default_persona`. Every
/// path must be absolute or start with `~/`, and the key is named when one
/// is not. Loading checks spelling only: nothing is read.
/// The context paths a config's own `[personas]` would carry, as (path, persona).
fn paths_of(cfg: &Config, persona: Option<&str>) -> Vec<(String, Option<String>)> {
    let book = crate::personas::Book::of_config(cfg);
    cfg.context_paths(&book, persona, book.engaging())
        .into_iter()
        .map(|p| (p.path, p.persona))
        .collect()
}

#[test]
fn context_files_come_in_two_levels_and_must_be_absolute_or_home_paths() {
    let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [context]\nfiles = [\"~/w/USER.md\", \"/nowhere/RULES.md\"]\n\
                    default_persona = \"theseus\"\n\n\
                    [personas.theseus]\nfiles = [\"~/w/persona.md\"]\n\n\
                    [personas.quiet]\n";
    let (cfg, warnings) = Config::parse(base).unwrap();
    // The tables are read once, by the migration: the only thing the config
    // says of them now is that they do nothing.
    assert_eq!(warnings, cfg.persona_warnings());
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(
        crate::personas::Book::of_config(&cfg).default_key(),
        Some("theseus")
    );
    let paths = |persona: Option<&str>| paths_of(&cfg, persona);
    let system = |p: &str| (p.to_string(), None);
    // With two or more personas every persona's files are in the request
    // whoever is in play (theseus-c1uz), the system level first.
    let all = [
        system("~/w/USER.md"),
        system("/nowhere/RULES.md"),
        ("~/w/persona.md".to_string(), Some("theseus".to_string())),
    ];
    assert_eq!(
        paths(Some("theseus")),
        all,
        "the system level first, then every persona's"
    );
    assert_eq!(
        paths(Some("quiet")),
        all,
        "a persona that names no files adds none of its own"
    );
    assert_eq!(paths(None), paths(Some("quiet")));
    // With one persona, the persona in play's files only, as before.
    let (one, _) = Config::parse(
        "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
         [context]\nfiles = [\"~/w/USER.md\"]\ndefault_persona = \"theseus\"\n\n\
         [personas.theseus]\nfiles = [\"~/w/persona.md\"]\n",
    )
    .unwrap();
    let one_paths = |persona: Option<&str>| paths_of(&one, persona);
    assert_eq!(
        one_paths(Some("theseus")),
        [system("~/w/USER.md"), all[2].clone()]
    );
    assert_eq!(
        one_paths(None),
        [system("~/w/USER.md")],
        "no persona in play, none of its files"
    );
    for (bad, key) in [
        ("[context]\nfiles = [\"USER.md\"]", "context.files"),
        ("[personas.p]\nfiles = [\"~other/x\"]", "personas.p.files"),
        ("[context]\nfiles = [\"\"]", "context.files"),
    ] {
        let doc = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{bad}\n");
        let e = format!("{:#}", Config::parse(&doc).unwrap_err());
        assert!(e.contains(key) && e.contains("absolute path"), "{e}");
    }
    // A profile no longer names context files, and neither does [model].
    for old in [
        "[model]\ncontext_files = []",
        "[profiles.p]\nmodel = \"m\"\ncontext_files = []",
    ] {
        let doc = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{old}\n");
        let e = format!("{:#}", Config::parse(&doc).unwrap_err());
        assert!(e.contains("unknown field `context_files`"), "{e}");
    }
    // A config that never names context files has none, says nothing new,
    // and `theseusd config` shows neither table.
    let (plain, w) = Config::parse("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert!(plain
        .context_paths(
            &crate::personas::Book::of_config(&plain),
            crate::personas::Book::of_config(&plain).default_key(),
            false
        )
        .is_empty());
    let shown = toml::to_string_pretty(&plain).unwrap();
    assert!(
        !shown.contains("[context]") && !shown.contains("personas"),
        "{shown}"
    );
    let shown = toml::to_string_pretty(&cfg).unwrap();
    assert!(
        shown.contains("[context]") && shown.contains("[personas.theseus]"),
        "{shown}"
    );
    // The template: an empty system level, and the persona commented.
    let t = Config::example();
    assert!(t.context.is_empty() && t.personas.is_empty() && t.tools.roots.is_empty());
    let has = |line: &str| Config::EXAMPLE_TOML.lines().any(|l| l.starts_with(line));
    assert!(has("[context]"));
    assert!(has("files = []"));
    assert!(has("# default_persona = \"theseus\""));
    assert!(has("# [personas.theseus]"));
    assert!(has("# roots = [\"/home/zeroaltitude/reports\"]"));
    assert!(!Config::EXAMPLE_TOML.contains("context_files"));
}

/// `[context] default_persona` must name a persona the config defines,
/// and the loader says which ones it does. Personas with no default are
/// never in play before Jev, and loading says so.
#[test]
fn an_unknown_default_persona_is_refused_naming_the_known_ones() {
    let doc = |rest: &str| format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{rest}\n");
    let e = format!(
        "{:#}",
        Config::parse(&doc(
            "[context]\ndefault_persona = \"thesues\"\n\n[personas.theseus]\n\n[personas.quiet]"
        ))
        .unwrap_err()
    );
    assert!(
            e.contains("context.default_persona = \"thesues\" is not a persona: the personas are quiet, theseus"),
            "{e}"
        );
    let e = format!(
        "{:#}",
        Config::parse(&doc("[context]\ndefault_persona = \"theseus\"")).unwrap_err()
    );
    assert!(e.contains("no [personas.<name>] table is defined"), "{e}");
    let (cfg, w) = Config::parse(&doc("[personas.theseus]\nfiles = [\"~/p.md\"]")).unwrap();
    assert_eq!(crate::personas::Book::of_config(&cfg).default_key(), None);
    assert!(
        w.len() == 1 && w[0].starts_with("[personas] and [context] default_persona are read once"),
        "{w:?}"
    );
}

/// `narrative` is a top-level key, off unless the config says true
/// (theseus-5fy). The vault config (checked 2026-09-28) has no such key and
/// loads unchanged; the template turns it on; written under a table it is
/// that table's key and fails to load.
#[test]
fn narrative_is_off_unless_the_config_says_true() {
    let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
    let (cfg, warnings) = Config::parse(base).unwrap();
    assert!(!cfg.narrative && warnings.is_empty(), "{warnings:?}");
    for (value, want) in [("true", true), ("false", false)] {
        let (cfg, _) = Config::parse(&format!("narrative = {value}\n\n{base}")).unwrap();
        assert_eq!(cfg.narrative, want, "narrative = {value}");
    }
    let (cfg, warnings) = Config::parse(Config::EXAMPLE_TOML).unwrap();
    assert!(cfg.narrative && warnings.is_empty(), "{warnings:?}");
    let e = format!(
        "{:#}",
        Config::parse(&format!("{base}\n[web]\nnarrative = true\n")).unwrap_err()
    );
    assert!(e.contains("unknown field `narrative`"), "{e}");
    assert!(Config::parse(&format!("narrative = \"yes\"\n{base}")).is_err());
    // `theseusd config` prints it first, where it still means the top level.
    let shown = toml::to_string_pretty(&cfg).unwrap();
    assert!(shown.starts_with("narrative = true\n"), "{shown}");
}

/// There is no deny posture: `deny`, wherever a posture goes, fails to
/// load, and the error names open, notify, and approve (theseus-8az).
#[test]
fn deny_and_the_old_words_fail_to_load_naming_the_three_postures() {
    let three = |e: &str| e.contains("open") && e.contains("notify") && e.contains("approve");
    for doc in [
        "enforcement = \"deny\"",
        "[policy.tools]\n\"proc.run\" = \"deny\"",
        "[policy.mcp]\n\"x\" = \"deny\"",
    ] {
        let e = format!("{:#}", policy_only(doc).unwrap_err());
        assert!(
            e.contains("unknown variant `deny`") && three(&e),
            "{doc}: {e}"
        );
    }
    for old in ["strict", "ask", "allow", "confirm"] {
        let e = format!(
            "{:#}",
            policy_only(&format!("enforcement = \"{old}\"")).unwrap_err()
        );
        assert!(three(&e), "{old}: {e}");
    }
    assert!(
        policy_only("[policy.overrides]\n\"fs.edit\" = \"allow\"").is_err(),
        "the old overrides table is [policy.tools] now"
    );
}

#[test]
fn an_unknown_tool_name_still_fails_to_load() {
    let e = format!(
        "{:#}",
        policy_only("[policy.tools]\n\"proc.rum\" = \"approve\"").unwrap_err()
    );
    assert!(e.contains("\"proc.rum\" is not a tool"), "{e}");
    policy_only("[policy.tools]\n\"mcp:x/y\" = \"approve\"").unwrap();
    assert!(policy_only("[policy.tools]\n\"mcp:x\" = \"approve\"").is_err());
    assert!(policy_only("[policy.mcp]\n\"a/b/c\" = \"approve\"").is_err());
}

/// `op` and `theseusd` are not on the default approve list: the floor
/// asks for them whatever the lists say.
#[test]
fn the_default_approve_argv_leaves_op_and_theseusd_to_the_floor() {
    let d = default_approve_argv();
    assert!(
        !d.iter().any(|a| a[0] == "op" || a[0] == "theseusd"),
        "{d:?}"
    );
    assert!(d.contains(&vec!["sudo".to_string()]), "{d:?}");
    assert_eq!(
        crate::policy::floor_argv(),
        argvs(&[&["theseusd"], &["op"]])
    );
}

/// The template holds no copy of the prices (theseus-vwar): its one
/// [catalog] table is the commented new model, so a config from it runs
/// every model at the code's figures, which `theseus catalog` lists, and
/// the code's next fix of a price reaches it.
#[test]
fn the_template_holds_no_price_table() {
    use crate::catalog::Catalog;
    let cfg = Config::example();
    assert!(cfg.catalog.is_empty(), "{:?}", cfg.catalog.keys());
    let (c, code) = (Catalog::with_overrides(&cfg.catalog), Catalog::builtin());
    assert_eq!((c.version, c.entries), (code.version, code.entries));
}

/// A [catalog] table over a built-in model may name one price; a model
/// the built-in table lacks must name every figure; a price is dollars.
#[test]
fn a_catalog_table_names_what_it_changes_and_a_new_model_names_everything() {
    let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n";
    let (cfg, _) = Config::parse(&format!(
        "{base}[catalog.\"claude-sonnet-5-5\"]\ncache_read_per_mtok = 0.1\n"
    ))
    .unwrap();
    let c = crate::catalog::Catalog::with_overrides(&cfg.catalog);
    let s = c.get("claude-sonnet-5-5").unwrap();
    assert_eq!((s.cache_read_per_mtok, s.output_per_mtok), (0.1, 10.0));
    let e = format!(
        "{:#}",
        Config::parse(&format!(
            "{base}[catalog.\"mystery\"]\ninput_per_mtok = 1.0\n"
        ))
        .unwrap_err()
    );
    assert!(
        e.contains("catalog.\"mystery\" is not a built-in model")
            && e.contains("provider, context_window, max_output_tokens, output_per_mtok"),
        "{e}"
    );
    let e = format!(
        "{:#}",
        Config::parse(&format!(
            "{base}[catalog.\"glm-5.3\"]\noutput_per_mtok = -1.0\n"
        ))
        .unwrap_err()
    );
    assert!(e.contains("catalog.\"glm-5.3\".output_per_mtok"), "{e}");
    assert!(Config::parse(&format!("{base}[catalog.\"glm-5.3\"]\nprice = 1.0\n")).is_err());
}

/// `[approval]` was retired (theseus-zmgb) and its key is gone: a config that
/// still has the section fails to load, and the error names it. That is
/// deliberate, and so it is for the other keys retired with it (`[labels]`,
/// `kernel.default_budget`, `kernel.control_reserve`, `sandbox.memory_mb`).
/// The owner's vault config without it loads with no warning, and the template
/// has none.
#[test]
fn a_retired_approval_section_fails_to_load() {
    let vault = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                     [kernel]\nadmission_ceiling = 8\nheartbeat_secs = 60\n\n\
                     [discord]\nenabled = true\ntoken_secret = \"discord_bot_token\"\n\
                     bindings_file = \"bindings.toml\"\nedit_interval_ms = 1200\n\n\
                     [web]\nenabled = true\nbind = \"127.0.0.1\"\nport = 7433\n";
    let (_, warnings) = Config::parse(vault).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        !Config::EXAMPLE_TOML.contains("[approval]"),
        "the template has no section"
    );
    let e = format!(
        "{:#}",
        Config::parse(&format!("{vault}\n[approval]\nchannels = [\"cli\"]\n")).unwrap_err()
    );
    assert!(e.contains("unknown field `approval`"), "{e}");
}

/// `[web] bind` is a loopback address or the config fails to load, since
/// the web UI has no auth yet (theseus-2fo). The owner's `127.0.0.1` and the
/// template's load unchanged.
#[test]
fn a_web_bind_off_loopback_fails_to_load() {
    let doc = |bind: &str| {
        format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[web]\nenabled = false\n\
                 bind = \"{bind}\"\nport = 7433\n"
        )
    };
    for ok in ["127.0.0.1", "127.0.0.2", "::1"] {
        Config::parse(&doc(ok)).unwrap_or_else(|e| panic!("{ok}: {e:#}"));
    }
    for bad in [
        "0.0.0.0",
        "192.168.1.20",
        "::",
        "localhost",
        "example.com",
        "",
    ] {
        let e = format!("{:#}", Config::parse(&doc(bad)).unwrap_err());
        assert!(
            e.contains(&format!("web.bind = {bad:?} is not a loopback address"))
                && e.contains("the web UI has no auth yet"),
            "{bad}: {e}"
        );
    }
    assert_eq!(Config::example().web.bind, "127.0.0.1");
}

/// `[web] dev_origin` (theseus-zab) is off unless set, and when set it is
/// a dev page on this machine or the config fails to load. A config
/// without it, as the owner's is, loads unchanged.
#[test]
fn a_dev_origin_is_off_by_default_and_a_page_on_this_machine() {
    let doc = |line: &str| {
        format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[web]\nenabled = true\n\
                 bind = \"127.0.0.1\"\nport = 7433\n{line}\n"
        )
    };
    assert_eq!(Config::parse(&doc("")).unwrap().0.web.dev_origin, None);
    assert_eq!(Config::example().web.dev_origin, None);
    assert_eq!(WebConfig::default().dev_origin, None);
    for ok in [
        "http://localhost:5173",
        "http://LocalHost:5173",
        "http://127.0.0.1:5173",
        "http://127.0.0.2:4173",
        "http://[::1]:5173",
    ] {
        let (cfg, _) = Config::parse(&doc(&format!("dev_origin = \"{ok}\"")))
            .unwrap_or_else(|e| panic!("{ok}: {e:#}"));
        assert_eq!(cfg.web.dev_origin.as_deref(), Some(ok));
    }
    for bad in [
        "https://localhost:5173",
        "http://localhost",
        "http://localhost:",
        "http://localhost:0",
        "http://localhost:70000",
        "http://localhost:5173/",
        "http://localhost:5173/ws",
        "http://user@localhost:5173",
        "http://evil.example:5173",
        "http://localhost.evil.example:5173",
        "http://0.0.0.0:5173",
        "http://192.168.1.20:5173",
        "http://::1:5173",
        "http://[::]:5173",
        "localhost:5173",
        "null",
        "",
    ] {
        let e = format!(
            "{:#}",
            Config::parse(&doc(&format!("dev_origin = \"{bad}\""))).unwrap_err()
        );
        assert!(
            e.contains(&format!(
                "web.dev_origin = {bad:?} is not a dev page's origin"
            )),
            "{bad}: {e}"
        );
    }
}

/// The fields that could carry a credential are the URLs: a user, a
/// password, or a query in one names its field, and such a note is never
/// kept as a copy (theseus-2fo). The template has none.
#[test]
fn a_url_that_could_carry_a_credential_is_named() {
    assert_eq!(Config::example().credential_in_url(), None);
    let with = |f: &dyn Fn(&mut Config)| {
        let mut c = Config::example();
        f(&mut c);
        c.credential_in_url()
    };
    assert_eq!(
        with(&|c| c.model.api_base = "https://user:pw@api.example.com".into()),
        Some("model.api_base".into())
    );
    assert_eq!(
        with(&|c| {
            c.providers.get_mut("zai").unwrap().api_base =
                "https://api.z.ai/api/anthropic?key=x".into()
        }),
        Some("providers.zai.api_base".into())
    );
    assert_eq!(
        with(&|c| c.telemetry.otlp_endpoint = Some("http://tok@127.0.0.1:4318".into())),
        Some("telemetry.otlp_endpoint".into())
    );
    assert_eq!(
        with(&|c| c.model.api_base = "https://api.anthropic.com/v1".into()),
        None
    );
}

/// `cache_ttl` (theseus-ev1) is "5m" or "1h", on a profile and on
/// `[model]`, whose implicit profile carries it. A profile that says
/// none inherits `[cache]`'s (theseus-ezeg): the raw profile keeps
/// none, and serializes none; any other value is refused.
#[test]
fn cache_ttl_is_five_minutes_or_an_hour_per_profile() {
    let on_sonnet =
        Config::EXAMPLE_TOML.replace("\n[profiles.glm]", "\ncache_ttl = \"1h\"\n\n[profiles.glm]");
    let (cfg, _) = Config::parse(&on_sonnet).unwrap();
    assert_eq!(cfg.profile("sonnet").unwrap().ttl(), CacheTtl::OneHour);
    assert_eq!(cfg.profile("glm").unwrap().ttl(), CacheTtl::FiveMinutes);
    assert_eq!(cfg.profile("default").unwrap().ttl(), CacheTtl::FiveMinutes);
    let text = toml::to_string(&cfg.profiles["glm"]).unwrap();
    assert!(!text.contains("cache_ttl"), "{text}");
    let text = toml::to_string(&cfg.profiles["sonnet"]).unwrap();
    assert!(text.contains("cache_ttl = \"1h\""), "{text}");

    // A 1-hour `[cache] ttl`: the default profile and every profile that
    // says none, sonnet's and glm's, cache for an hour.
    let on_model = Config::EXAMPLE_TOML.replace("\n[cache]\n", "\n[cache]\nttl = \"1h\"\n");
    let (cfg, _) = Config::parse(&on_model).unwrap();
    assert_eq!(cfg.profile("default").unwrap().ttl(), CacheTtl::OneHour);
    assert_eq!(cfg.profile("sonnet").unwrap().ttl(), CacheTtl::OneHour);
    assert_eq!(cfg.ttl_of("glm"), Some((CacheTtl::OneHour, "cache")));
    let wrong = Config::EXAMPLE_TOML.replace(
        "\n[profiles.glm]",
        "\ncache_ttl = \"10m\"\n\n[profiles.glm]",
    );
    let e = format!("{:#}", Config::parse(&wrong).unwrap_err());
    assert!(e.contains("cache_ttl") || e.contains("10m"), "{e}");
    assert_eq!(
        (CacheTtl::FiveMinutes.marker(), CacheTtl::OneHour.marker()),
        (
            serde_json::json!({"type": "ephemeral"}),
            serde_json::json!({"type": "ephemeral", "ttl": "1h"})
        )
    );
    assert!(CacheTtl::FiveMinutes < CacheTtl::OneHour);
}

/// `[aws.accounts.<id>]` (row 29, C1): an account is its 12 digits, its
/// key's two `[secrets]` entries, a region, and the regions a call may
/// name; a stand-in endpoint only on this machine. Anything else fails to
/// load, and the error says which. The template binds none.
#[test]
fn an_aws_account_names_its_id_its_key_and_its_regions() {
    assert!(
        Config::example().aws.is_empty(),
        "the template binds no account"
    );
    let with = |aws: &str| {
        Config::parse(&format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\
                 aws_access_key_id = \"op://v/k/notesPlain#AWS_ACCESS_KEY_ID\"\n\
                 aws_secret_access_key = \"op://v/k/notesPlain#AWS_SECRET_ACCESS_KEY\"\n\
                 other_key = \"op://v/o/f\"\n\n{aws}\n"
        ))
        .map(|(c, _)| c)
    };
    let ok = with("[aws.accounts.111122223333]\nregion = \"us-west-2\"").unwrap();
    let a = &ok.aws.accounts["111122223333"];
    assert_eq!(a.credentials, AwsCredentialNames::default());
    assert_eq!(a.allowed_regions(), ["us-west-2"]);
    assert_eq!(a.endpoint, None);
    let named = with(
        "[aws.accounts.444455556666]\nregion = \"eu-central-1\"\n\
             regions = [\"eu-central-1\", \"us-east-1\"]\n\
             credentials = { access_key_id = \"other_key\", secret_access_key = \"other_key\" }\n\
             endpoint = \"http://127.0.0.1:4566\"",
    )
    .unwrap();
    let a = &named.aws.accounts["444455556666"];
    assert_eq!(a.credentials.access_key_id, "other_key");
    assert_eq!(a.allowed_regions(), ["eu-central-1", "us-east-1"]);
    for (bad, says) in [
        ("[aws.accounts.home]\nregion = \"us-west-2\"", "12 digits"),
        (
            "[aws.accounts.11112222333]\nregion = \"us-west-2\"",
            "12 digits",
        ),
        (
            "[aws.accounts.111122223333]\nregion = \"us-west-2\"\n\
                 credentials = { access_key_id = \"nope\" }",
            "credentials.access_key_id = \"nope\" has no matching entry under [secrets]",
        ),
        (
            "[aws.accounts.111122223333]\nregion = \"US-West-2\"",
            "not a region's name",
        ),
        (
            "[aws.accounts.111122223333]\nregion = \"west\"",
            "not a region's name",
        ),
        (
            "[aws.accounts.111122223333]\nregion = \"us-west-2\"\nregions = [\"us-east-1\"]",
            "is not one of its regions: us-east-1",
        ),
        (
            "[aws.accounts.111122223333]\nregion = \"us-west-2\"\n\
                 endpoint = \"https://sts.us-west-2.amazonaws.com\"",
            "is not a stand-in on this machine",
        ),
        (
            "[aws.accounts.111122223333]\nregion = \"us-west-2\"\nprofile = \"default\"",
            "unknown field `profile`",
        ),
        ("[aws.accounts.111122223333]", "missing field `region`"),
        ("[aws]\nregion = \"us-west-2\"", "unknown field `region`"),
    ] {
        let e = format!("{:#}", with(bad).unwrap_err());
        assert!(e.contains(says), "{bad}: {e}");
    }
}

/// `[policy.aws]` (AWS design §3.9): a class (`read`, `write`, `run`), a
/// service, or a service and an operation, as aws.describe names them.
#[test]
fn policy_aws_takes_read_a_service_or_an_operation() {
    let cfg = policy_only(
        "[policy.aws]\nread = \"open\"\nwrite = \"notify\"\nrun = \"approve\"\nec2 = \"notify\"\n\
             \"s3:ListBuckets\" = \"approve\"\nresource-groups = \"open\"",
    )
    .unwrap();
    assert_eq!(cfg.policy.aws["read"], Posture::Open);
    assert_eq!(cfg.policy.aws["write"], Posture::Notify);
    assert_eq!(cfg.policy.aws["run"], Posture::Approve);
    assert_eq!(cfg.policy.aws["ec2"], Posture::Notify);
    assert_eq!(cfg.policy.aws["s3:ListBuckets"], Posture::Approve);
    assert!(policy_only("").unwrap().policy.aws.is_empty());
    for (bad, says) in [
        (
            "\"EC2\" = \"open\"",
            "is not a class (`read`, `write`, `run`), a service",
        ),
        ("\"s3:listBuckets\" = \"open\"", "is not a class"),
        ("\"s3:\" = \"open\"", "is not a class"),
        ("read = \"never\"", "open"),
    ] {
        let e = format!(
            "{:#}",
            policy_only(&format!("[policy.aws]\n{bad}")).unwrap_err()
        );
        assert!(e.contains(says), "{bad}: {e}");
    }
}

/// A `[policy.tools]` line loads for every tool: the toollets', and the
/// core's own, the web's, a task's, a wake's, and AWS's. Until row 29 a
/// config with the template's `task.create` or `wake.at` line uncommented
/// failed to load.
#[test]
fn a_policy_tools_line_loads_for_every_tool() {
    let names: Vec<String> = theseus_tools::default_registry()
        .all()
        .map(|t| t.name().to_string())
        .chain(crate::web::NAMES.map(String::from))
        .chain(crate::task::NAMES.map(String::from))
        .chain(crate::task_graph::tools::NAMES.map(String::from))
        .chain(crate::wake::NAMES.map(String::from))
        .chain(crate::routine_tool::NAMES.map(String::from))
        .chain(crate::project_tool::NAMES.map(String::from))
        .chain(crate::glide::NAMES.map(String::from))
        .chain(crate::extend::NAMES.map(String::from))
        .chain(crate::aws::NAMES.map(String::from))
        .chain(crate::google::NAMES.map(String::from))
        .collect();
    for n in &names {
        policy_only(&format!("[policy.tools]\n\"{n}\" = \"approve\""))
            .unwrap_or_else(|e| panic!("{n}: {e:#}"));
    }
    let e = format!(
        "{:#}",
        policy_only("[policy.tools]\n\"aws.nope\" = \"open\"").unwrap_err()
    );
    assert!(
        e.contains("is not a tool") && e.contains("aws.s3.list"),
        "{e}"
    );
}

/// Every key the code can read appears in the template (set or commented),
/// so a new field cannot be added without documenting it.
#[test]
fn example_template_mentions_every_key() {
    let built = toml::Value::try_from(Config::example()).unwrap();
    let mut missing = Vec::new();
    walk(&built, "", "", &mut |path, key| {
        // A map key such as `"fs.read"` appears quoted.
        if !Config::EXAMPLE_TOML.contains(&format!("{key} ="))
            && !Config::EXAMPLE_TOML.contains(&format!("\"{key}\""))
        {
            missing.push(path.to_string());
        }
    });
    // Optional fields that serialize as absent still need a commented line.
    for must in [
        "system =",
        "otlp_endpoint =",
        "headers_secret =",
        "[policy.mcp]",
        "default_persona =",
        "[personas.",
        "[broker.programs.",
        "[broker.secrets.",
        "posture =",
        "cache_ttl =",
        "caches =",
        "[aws.accounts.",
        "credentials =",
        "access_key_id =",
        "secret_access_key =",
        "region =",
        "regions =",
        "endpoint =",
        "[policy.aws]",
    ] {
        assert!(
            Config::EXAMPLE_TOML.contains(must),
            "template must document `{must}`"
        );
    }
    assert!(missing.is_empty(), "undocumented config keys: {missing:?}");
}

fn walk(v: &toml::Value, path: &str, key: &str, f: &mut dyn FnMut(&str, &str)) {
    match v {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let p = format!("{path}.{k}");
                walk(v, p.trim_start_matches('.'), k, f);
            }
        }
        _ => f(path, key),
    }
}

/// A config table cannot loosen a built-in China-origin row's origin
/// (theseus-a1wvq.1), whatever its other fields; tightening is accepted, and
/// the same word `cn` again is.
#[test]
fn a_config_table_cannot_loosen_a_builtin_cn_origin() {
    use crate::catalog::{CatalogRow, ModelOrigin};
    let with = |id: &str, origin: ModelOrigin| {
        let mut cfg = Config::example();
        cfg.catalog.insert(
            id.into(),
            CatalogRow {
                origin: Some(origin),
                ..Default::default()
            },
        );
        cfg.validate()
    };
    for id in [
        "glm-5.3",
        "glm-5.3-flash",
        "deepseek-flash",
        "deepseek-v4-pro",
    ] {
        for loose in [ModelOrigin::Us, ModelOrigin::Eu, ModelOrigin::Unknown] {
            let e = format!("{:#}", with(id, loose).unwrap_err());
            assert!(
                e.contains(&format!("catalog.\"{id}\".origin"))
                    && e.contains("a China-origin model's origin is not configurable"),
                "{id} {loose:?}: {e}"
            );
        }
        with(id, ModelOrigin::Cn).unwrap();
    }
    // Tightening a Claude row is allowed: one no class names loads, and one a
    // class lists is refused only by routing's own check (theseus-a1wvq.4).
    with("claude-opus-5", ModelOrigin::Cn).unwrap();
    with("claude-opus-5", ModelOrigin::Unknown).unwrap();
    for tight in [ModelOrigin::Cn, ModelOrigin::Unknown] {
        let e = format!("{:#}", with("claude-sonnet-5-5", tight).unwrap_err());
        assert!(e.contains("is of restricted origin"), "{tight:?}: {e}");
    }
    // A word that is none of the four fails the load.
    let toml = format!(
        "{}\n[catalog.\"glm-5.3\"]\norigin = \"china\"\n",
        Config::EXAMPLE_TOML
    );
    assert!(Config::from_toml(&toml).is_err());
}

/// A provider's own word never makes a China-origin host anything but `cn`
/// (theseus-a1wvq.1), and `profile_origin` reads the host, the family and
/// the row together.
#[test]
fn a_provider_cannot_vouch_for_a_china_endpoint() {
    use crate::catalog::ModelOrigin;
    let mut cfg = Config::example();
    cfg.providers.insert(
        "dsproxy".into(),
        ProviderConfig {
            api_base: "https://api.deepseek.com/anthropic".into(),
            api_key_secret: "zai_api_key".into(),
            kind: ProviderKind::AnthropicMessages,
            timeouts: None,
            origin: Some(ModelOrigin::Us),
        },
    );
    let e = format!("{:#}", cfg.validate().unwrap_err());
    assert!(
        e.contains("providers.dsproxy.origin") && e.contains("China-origin endpoint"),
        "{e}"
    );
    cfg.providers.get_mut("dsproxy").unwrap().origin = Some(ModelOrigin::Cn);
    cfg.validate().unwrap();
    // An unknown host takes the operator's word.
    cfg.providers.insert(
        "acme".into(),
        ProviderConfig {
            api_base: "https://llm.acme.example/anthropic".into(),
            api_key_secret: "zai_api_key".into(),
            kind: ProviderKind::AnthropicMessages,
            timeouts: None,
            origin: Some(ModelOrigin::Us),
        },
    );
    cfg.validate().unwrap();
}

#[test]
fn a_profiles_origin_reads_its_provider_its_model_and_the_catalog() {
    use crate::catalog::{Catalog, ModelOrigin};
    let toml = format!(
        "{}\n[profiles.disguised]\nprovider = \"zai\"\nmodel = \"claude-sonnet-5-5\"\n\
         [profiles.proxied]\nprovider = \"acme\"\nmodel = \"claude-sonnet-5-5\"\n\
         [profiles.vouched]\nprovider = \"acmeus\"\nmodel = \"claude-sonnet-5-5\"\n\
         [providers.acme]\napi_base = \"https://llm.acme.example/anthropic\"\n\
         api_key_secret = \"zai_api_key\"\n\
         [providers.acmeus]\napi_base = \"https://llm.acme.example/anthropic\"\n\
         api_key_secret = \"zai_api_key\"\norigin = \"us\"\n",
        Config::EXAMPLE_TOML
    );
    let cfg = Config::from_toml(&toml).unwrap();
    let catalog = Catalog::builtin();
    let o = |p: &str| cfg.profile_origin(p, &catalog);
    assert_eq!(o("sonnet"), ModelOrigin::Us);
    assert_eq!(o("glm"), ModelOrigin::Cn);
    assert_eq!(o("disguised"), ModelOrigin::Cn);
    assert_eq!(o("proxied"), ModelOrigin::Unknown);
    assert_eq!(o("vouched"), ModelOrigin::Us);
    assert_eq!(o("nonesuch"), ModelOrigin::Unknown);
}

/// `max_wakes` is 20 in the default, the template and the kernel's limits; an
/// explicit value wins, and 0 is floored at 1 (theseus-o1h3f.8).
#[test]
fn max_wakes_defaults_to_twenty_everywhere() {
    let (t, w) = Config::parse(Config::EXAMPLE_TOML).unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(t.kernel.max_wakes, 20);
    assert_eq!(t.kernel.to_kernel_config().wakes.max_wakes, 20);
    assert!(
        Config::EXAMPLE_TOML.contains("\nmax_wakes = 20 "),
        "the template names the default"
    );
    let with = |n: u32| {
        let doc =
            format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[kernel]\nmax_wakes = {n}\n");
        Config::parse(&doc)
            .unwrap()
            .0
            .kernel
            .to_kernel_config()
            .wakes
            .max_wakes
    };
    assert_eq!(with(3), 3);
    assert_eq!(with(0), 1);
}

/// R6 (theseus-gyjyo.8): `[policy] external_text` and `[policy] external_programs` are gone, and
/// a config that names either fails to load, with a message that says the `injection` label
/// replaced them. A document with neither loads, and so does the template.
#[test]
fn external_text_and_external_programs_fail_to_load_naming_the_injection_label() {
    let doc = |line: &str| format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[policy]\n{line}\n");
    for line in ["external_text = \"notify\"", "external_text = \"ask\"", "external_programs = [\"gh\"]"] {
        let e = format!("{:#}", Config::parse(&doc(line)).unwrap_err());
        let key = line.split(' ').next().unwrap();
        assert!(e.contains(key) && e.contains("`injection` label"), "{line}: {e}");
    }
    Config::parse(&doc("enforcement = \"notify\"")).unwrap();
    assert!(!Config::EXAMPLE_TOML.contains("external_text"));
    assert!(!Config::EXAMPLE_TOML.contains("external_programs"));
}
