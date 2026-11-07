//! One-list regression guards, introduced before changing the product.
use super::*;
use i_slint_backend_testing::ElementRoot;

#[test]
fn one_list_s1_add_starts_exactly_one_read_only_request() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let capture = root.path().join("requests.jsonl");
    let cli = script(
        root.path(),
        &format!(
            "cat >> {}\nprintf '\\n' >> {}\n{}exit 0\n",
            quote(capture.to_str().unwrap()),
            quote(capture.to_str().unwrap()),
            print(&fixture("planned-create-unix"))
        ),
    );
    let c = controller(root.path(), cli);
    c.stage(vec![root.path().join("source.eml")]).unwrap();
    pump(&c, || !c.is_busy());
    let bytes = fs::read_to_string(capture).unwrap_or_default();
    let requests: Vec<serde_json::Value> = bytes
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        requests.len(),
        1,
        "Adding paths must start exactly one automatic check"
    );
    assert_eq!(
        requests[0]["dry_run"], true,
        "Adding must never create copies"
    );
    assert_eq!(requests[0]["output_directory"], serde_json::Value::Null);
}

#[test]
fn one_list_s11_no_removed_concepts_in_accessible_window() {
    init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let c = controller(root.path(), completed(root.path(), "planned-create-unix"));
    let forbidden = [
        "request",
        "draft",
        "session",
        "queue",
        "check only",
        "check against",
        "review",
        "close result",
        "technical details",
        "engine",
        "phase",
        "prepared",
        "force stop",
        "settings unavailable",
    ];
    for element in c.window().root_element().query_descendants().find_all() {
        if let Some(label) = element.accessible_label() {
            let lower = label.to_lowercase();
            assert!(
                !forbidden.iter().any(|word| lower.contains(word)),
                "Removed concept remains accessible: {label}"
            );
        }
    }
}
