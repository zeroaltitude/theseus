use super::*;
use serde_json::json;

const FILE: &str = r#"# Theseus, on this machine.
narrative = false

# The notes import.
[import.notes]
enabled = true   # turn it off to stop the import
every_mins = 30

[self.budget]
usd = 5.0
"#;

fn set(path: &str, v: Json) -> Change {
    Change::Set(parse_path(path).unwrap(), v)
}

#[test]
fn a_value_is_replaced_in_place_and_the_comments_stay() {
    let out = apply(FILE, &[set("import.notes.enabled", json!(false))]).unwrap();
    assert!(
        out.contains("enabled = false   # turn it off to stop the import"),
        "{out}"
    );
    assert!(
        out.starts_with("# Theseus, on this machine.\nnarrative = false\n"),
        "{out}"
    );
    assert!(out.contains("# The notes import.\n[import.notes]"), "{out}");
    assert_eq!(out.matches("every_mins = 30").count(), 1);
}

#[test]
fn a_new_key_goes_after_the_last_entry_of_its_table() {
    let out = apply(FILE, &[set("import.notes.limit", json!(7))]).unwrap();
    assert!(
        out.contains("every_mins = 30\nlimit = 7\n\n[self.budget]"),
        "{out}"
    );
}

#[test]
fn a_new_root_key_goes_before_the_first_table() {
    let out = apply("a = 1\n\n[t]\nb = 2\n", &[set("c", json!("x"))]).unwrap();
    assert_eq!(out, "a = 1\nc = \"x\"\n\n[t]\nb = 2\n");
}

#[test]
fn a_new_table_is_added_at_the_end() {
    let out = apply(FILE, &[set("memory.enabled", json!(true))]).unwrap();
    assert!(out.starts_with(FILE), "{out}");
    assert!(out.ends_with("\n[memory]\nenabled = true\n"), "{out}");
}

#[test]
fn a_key_is_removed_with_its_line_and_the_rest_stays() {
    let out = apply(
        FILE,
        &[Change::Remove(
            parse_path("import.notes.every_mins").unwrap(),
        )],
    )
    .unwrap();
    assert!(!out.contains("every_mins"), "{out}");
    assert!(out.contains("enabled = true   # turn it off"), "{out}");
}

#[test]
fn a_whole_table_is_removed_with_its_sections() {
    let text = "keep = 1\n\n[mcp.servers.a]\ncommand = [\"x\"]\n\n[mcp.servers.a.env]\nK = \"v\"\n\n[other]\nz = 1\n";
    let out = apply(
        text,
        &[Change::Remove(parse_path("mcp.servers.a").unwrap())],
    )
    .unwrap();
    assert!(
        !out.contains("mcp.servers.a") && out.contains("[other]\nz = 1"),
        "{out}"
    );
}

#[test]
fn a_change_the_layout_cannot_take_is_refused_in_words_and_nothing_is_made() {
    // A dotted key inside a table the file writes as a header.
    let text = "[t]\na.b = 1\n";
    let e = apply(text, &[set("t.a.c", json!(2))]).unwrap_err();
    assert!(e.contains("by hand"), "{e}");
}

#[test]
fn removing_what_is_not_there_is_refused() {
    let e = apply(
        FILE,
        &[Change::Remove(parse_path("import.notes.nope").unwrap())],
    )
    .unwrap_err();
    assert!(e.contains("nothing to remove"), "{e}");
}

#[test]
fn a_path_is_read_with_quoted_keys_and_a_bad_one_is_refused() {
    assert_eq!(
        parse_path("self.budget.usd").unwrap(),
        ["self", "budget", "usd"]
    );
    assert_eq!(
        parse_path("policy.tools.\"proc.run\"").unwrap(),
        ["policy", "tools", "proc.run"]
    );
    assert_eq!(
        dotted(&parse_path("policy.tools.\"proc.run\"").unwrap()),
        "policy.tools.\"proc.run\""
    );
    for bad in ["", ".a", "a..b", "a b", "a.\"b", "a.b."] {
        assert!(parse_path(bad).is_err(), "{bad:?}");
    }
}
