use litx_inspect::{InspectOptions, inspect};

#[test]
fn csv_preview_reads_five_data_rows_with_quoted_fields_and_newlines() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "csv-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sample.csv");
    std::fs::write(&path, "id,note\n1,\"comma, and\nnewline\"\n2,second\n3,third\n4,fourth\n5,fifth\n6,DO_NOT_EXPORT\n7,last\n").unwrap();
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["dataset"]["headers"], serde_json::json!(["id", "note"]));
    assert_eq!(v["dataset"]["rows"].as_array().unwrap().len(), 5);
    assert_eq!(v["dataset"]["rows"][0][1], "comma, and\nnewline");
    assert_eq!(v["dataset"]["has_more"], true);
    let md = v["markdown"]["content"].as_str().unwrap();
    assert!(md.contains("| id | note |"), "{md}");
    assert!(md.contains("comma, and<br>newline"), "{md}");
    assert!(!md.contains("DO_NOT_EXPORT"), "{md}");
    assert!(!r.text.content.contains("DO_NOT_EXPORT"));
}

#[test]
fn tsv_preview_preserves_empty_fields_and_stops_after_five_data_rows() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "tsv-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sample.tsv");
    std::fs::write(
        &path,
        "id\tnote\n1\t\n2\tsecond\n3\tthird\n4\tfourth\n5\tfifth\n6\tDO_NOT_EXPORT\n",
    )
    .unwrap();
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let dataset = r.dataset.unwrap();
    assert_eq!(dataset.headers, ["id", "note"]);
    assert_eq!(dataset.rows.len(), 5);
    assert_eq!(dataset.rows[0][1], "");
    assert!(dataset.has_more);
    assert!(!r.markdown.content.contains("DO_NOT_EXPORT"));
}

#[test]
fn csv_rejects_a_record_larger_than_its_preview_read_budget() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "csv-cap-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("oversized.csv");
    std::fs::write(&path, format!("header\n{}\n", "x".repeat(2 << 20))).unwrap();
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.dataset.is_none());
    assert!(
        r.errors
            .iter()
            .any(|e| e.stage == "csv" && e.message.contains("cap")),
        "{:?}",
        r.errors
    );
    assert!(r.markdown.truncated);
}
