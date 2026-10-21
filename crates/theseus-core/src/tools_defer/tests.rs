use super::*;

fn def(name: &str) -> Value {
    json!({"name": name, "description": format!("The {name} tool. More words."), "input_schema": {"type": "object"}})
}

fn catalog() -> Vec<Value> {
    [
        "aws_call",
        "fs_read",
        "memory_lookup",
        "proc_run",
        "term_open",
    ]
    .iter()
    .map(|n| def(n))
    .collect()
}

fn deferral(mechanism: Mechanism, loaded: &[&str]) -> Deferral {
    Deferral {
        mechanism,
        loaded: loaded.iter().map(|s| s.to_string()).collect(),
        resolved: false,
        added: Default::default(),
        raised: None,
    }
}

fn names(v: &[Value]) -> Vec<&str> {
    v.iter().filter_map(|t| t["name"].as_str()).collect()
}

#[test]
fn a_hidden_familys_tools_carry_defer_loading_and_the_search_tool_comes_first() {
    let d = deferral(Mechanism::Search, &["files", "commands"]);
    let out = resolve(&catalog(), &d);
    assert_eq!(out[0]["type"], SEARCH_TYPE);
    assert_eq!(out[0]["name"], SEARCH_TOOL);
    assert!(
        out[0].get("defer_loading").is_none(),
        "the search tool is never deferred"
    );
    for t in &out[1..] {
        let hidden = matches!(
            t["name"].as_str(),
            Some("aws_call" | "memory_lookup" | "term_open")
        );
        assert_eq!(
            t.get("defer_loading") == Some(&Value::Bool(true)),
            hidden,
            "{}",
            t["name"]
        );
    }
    // Every tool is still declared: deferral drops nothing on this path.
    assert_eq!(out.len(), catalog().len() + 1);
}

#[test]
fn the_fallback_leaves_hidden_tools_out_and_offers_tools_find() {
    let d = deferral(Mechanism::Find, &["files", "commands", "memory"]);
    let out = resolve(&catalog(), &d);
    assert_eq!(
        names(&out),
        ["fs_read", "memory_lookup", "proc_run", "tools_find"]
    );
    assert!(out.iter().all(|t| t.get("defer_loading").is_none()));
}

#[test]
fn with_nothing_hidden_there_is_no_search_tool_and_no_find() {
    let all = deferral(
        Mechanism::Search,
        &["files", "commands", "memory", "aws", "terminals"],
    );
    let out = resolve(&catalog(), &all);
    assert_eq!(names(&out), names(&catalog()));
    let out = resolve(
        &catalog(),
        &deferral(
            Mechanism::Find,
            &["files", "commands", "memory", "aws", "terminals"],
        ),
    );
    assert_eq!(names(&out), names(&catalog()));
}

#[test]
fn a_set_that_would_hide_everything_hides_nothing() {
    // The API refuses a request whose tools are all deferred.
    let out = resolve(&catalog(), &deferral(Mechanism::Search, &[]));
    assert_eq!(names(&out), names(&catalog()));
}

#[test]
fn an_unknown_tool_is_never_hidden() {
    let mut tools = catalog();
    tools.push(def("brand_new_tool"));
    let out = resolve(&tools, &deferral(Mechanism::Search, &["files"]));
    let t = out.iter().find(|t| t["name"] == "brand_new_tool").unwrap();
    assert!(t.get("defer_loading").is_none());
}

#[test]
fn resolving_is_deterministic_byte_for_byte() {
    let d = deferral(Mechanism::Search, &["files", "commands"]);
    let a = serde_json::to_string(&resolve(&catalog(), &d)).unwrap();
    let b = serde_json::to_string(&resolve(&catalog(), &d)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn the_hidden_tools_are_the_deferred_and_the_left_out() {
    let cat = catalog();
    let native = resolve(&cat, &deferral(Mechanism::Search, &["files", "commands"]));
    let h = hidden_of(&cat, &native);
    let wires: Vec<&str> = h.iter().map(|h| h.wire.as_str()).collect();
    assert_eq!(wires, ["aws_call", "memory_lookup", "term_open"]);
    assert_eq!(h[0].family, "aws");
    assert_eq!(h[0].line, "The aws_call tool.");
    let left = resolve(&cat, &deferral(Mechanism::Find, &["files", "commands"]));
    assert_eq!(hidden_of(&cat, &left), h);
    // Loaded tools are not hidden.
    assert!(hidden_of(&cat, &cat).is_empty());
}

#[test]
fn tools_find_is_not_in_the_catalog_and_its_definition_is_a_tool() {
    assert!(!listed("tools.find"));
    assert!(listed("fs.read"));
    let d = find_definition();
    assert_eq!(d["name"], FIND);
    assert_eq!(d["input_schema"]["required"][0], "query");
}

#[test]
fn the_families_of_a_catalog_are_listed_once() {
    let f = families_of(&catalog());
    assert_eq!(
        f.into_iter().collect::<Vec<_>>(),
        ["aws", "commands", "files", "memory", "terminals"]
    );
}

#[test]
fn an_append_over_the_recorded_set_takes_the_families_a_find_added() {
    // The compilation recorded files and commands; a find of the turn added
    // terminals (theseus-342qy): the append's tools carry it, so they differ
    // from the compilation's and the compile recompiles.
    let recorded = ["files".to_string(), "commands".to_string()];
    let mut d = deferral(Mechanism::Find, &["files", "commands", "terminals"]);
    d.added = ["terminals".to_string()].into();
    let over = d.over(&recorded);
    assert_eq!(
        names(&resolve(&catalog(), &over)),
        ["fs_read", "proc_run", "term_open", "tools_find"]
    );
    // With nothing added, it is the recorded set alone, whatever the fresh choice says.
    let plain = deferral(Mechanism::Find, &["files", "commands", "terminals"]);
    assert_eq!(
        names(&resolve(&catalog(), &plain.over(&recorded))),
        ["fs_read", "proc_run", "tools_find"]
    );
}
