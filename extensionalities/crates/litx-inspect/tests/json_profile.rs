use litx_inspect::{InspectOptions, inspect};

#[test]
fn json_counts_scalar_leaves_and_describes_repeated_record_fields() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "json-profile-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("records.json");
    std::fs::write(&path, br#"{"items":[{"id":1,"kind":"same","nested":{"value":true}},{"id":2,"kind":"same","nested":{"value":false}}],"empty":[],"nothing":null}"#).unwrap();
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["json"]["total_leaves"], 7);
    assert_eq!(v["json"]["leaf_stats"]["items[].kind"]["count"], 2);
    assert_eq!(v["json"]["leaf_stats"]["items[].kind"]["distinct_count"], 1);
    assert_eq!(v["json"]["loops"][0]["path"], "items[]");
    assert_eq!(v["json"]["loops"][0]["count"], 2);
    assert_eq!(
        v["json"]["loops"][0]["fields"],
        serde_json::json!(["id", "kind", "nested.value"])
    );
    assert!(
        v["markdown"]["content"]
            .as_str()
            .unwrap()
            .contains("JSON structure")
    );
}

#[test]
fn json_profiles_root_arrays_and_keeps_quoted_paths_unambiguous() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "json-edges-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("edges.json");
    std::fs::write(&path, r#"[{"a.b":"café","a":{"b":null},"":{"[]":true}},{"a.b":"café","a":{"b":42},"":{"[]":false}}]"#).unwrap();
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["json"]["total_leaves"], 6);
    assert_eq!(v["json"]["leaf_stats"]["[][\"a.b\"]"]["distinct_count"], 1);
    assert_eq!(v["json"]["leaf_stats"]["[][\"a.b\"]"]["total_bytes"], 10);
    assert_eq!(v["json"]["leaf_stats"]["[].a.b"]["value_type"], "mixed");
    assert_eq!(v["json"]["leaf_stats"]["[][\"\"][\"[]\"]"]["count"], 2);
    assert_eq!(v["json"]["loops"][0]["count"], 2);
}

#[test]
fn malformed_json_does_not_publish_a_complete_profile() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "json-invalid-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("invalid.json");
    for input in ["{\"x\":1} trailing", "[1,2,", "{\"x\":\"\\uD800\"}"] {
        std::fs::write(&path, input).unwrap();
        let r = inspect(&path, &InspectOptions::default());
        assert!(
            r.errors.iter().any(|e| e.stage == "json"),
            "{input}: {:?}",
            r.errors
        );
        assert!(r.json.is_none());
        assert!(r.markdown.truncated);
    }
}
