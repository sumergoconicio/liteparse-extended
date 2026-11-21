use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn opendocument_tables_preserve_headers_spans_and_nested_cell_text() {
    for (name, unit, number) in [("tables.odt", "section", 1), ("tables.odp", "slide", 2)] {
        let report = inspect(&fixture(name), &InspectOptions::default());
        assert!(report.errors.is_empty(), "{name}: {:?}", report.errors);
        let r = serde_json::to_value(report).unwrap();
        assert_eq!(r["counts"]["tables"], 2);
        let table = &r["tables"][0];
        assert_eq!(table["name"], "Evidence");
        assert_eq!(table["rows"], 3);
        assert_eq!(table["cols"], 3);
        assert_eq!(table["unit_kind"], unit);
        assert_eq!(table["unit_number"], number);
        assert_eq!(table["source_part"], "content.xml");
        assert_eq!(table["header_rows"], serde_json::json!([1]));
        let cells = table["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 7);
        assert_eq!(cells[3]["row_span"], 2);
        assert_eq!(cells[4]["column_span"], 2);
        assert_eq!(cells[5]["text"], "Before nested.\nAfter nested.");
        assert_eq!(cells[5]["nested_tables"], serde_json::json!([2]));
        assert_eq!(r["tables"][1]["cells"][0]["text"], "Nested evidence.");
    }
}

#[test]
fn drawingml_tables_preserve_merge_anchors_not_style_inferred_headers() {
    let report = inspect(&fixture("tables.pptx"), &InspectOptions::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let r = serde_json::to_value(report).unwrap();
    assert_eq!(r["counts"]["tables"], 1);
    let table = &r["tables"][0];
    assert_eq!(table["unit_kind"], "slide");
    assert_eq!(table["unit_number"], 1);
    assert_eq!(table["source_part"], "ppt/slides/slide2.xml");
    assert_eq!(table["rows"], 3);
    assert_eq!(table["cols"], 3);
    assert_eq!(table["header_rows"], serde_json::json!([]));
    let cells = table["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 6);
    assert_eq!(cells[0]["column_span"], 2);
    assert_eq!(cells[2]["row_span"], 2);
    assert_eq!(cells[5]["text"], "Bottom");
    assert_eq!(cells[5]["column_span"], 2);
}

#[test]
fn word_tables_preserve_cells_merges_headers_and_nesting() {
    let report = inspect(&fixture("tables.docx"), &InspectOptions::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let r = serde_json::to_value(report).unwrap();
    assert_eq!(r["counts"]["tables"], 2);
    let outer = &r["tables"][0];
    assert_eq!(outer["rows"], 3);
    assert_eq!(outer["cols"], 3);
    assert_eq!(outer["source_part"], "word/document.xml");
    assert_eq!(outer["source"], "archive_entry");
    assert_eq!(outer["unit_kind"], "section");
    assert_eq!(outer["unit_number"], 1);
    assert_eq!(outer["header_rows"], serde_json::json!([1]));
    assert_eq!(outer["cells"].as_array().unwrap().len(), 7);
    let cells = outer["cells"].as_array().unwrap();
    let group = cells
        .iter()
        .find(|c| c["row"] == 2 && c["column"] == 1)
        .unwrap();
    assert_eq!(group["row_span"], 2);
    assert_eq!(group["column_span"], 1);
    assert_eq!(group["text"], "Group");
    let merged = cells
        .iter()
        .find(|c| c["row"] == 2 && c["column"] == 2)
        .unwrap();
    assert_eq!(merged["column_span"], 2);
    let parent_cell = cells
        .iter()
        .find(|c| c["row"] == 3 && c["column"] == 2)
        .unwrap();
    assert_eq!(parent_cell["nested_tables"], serde_json::json!([2]));
    assert_eq!(parent_cell["text"], "Before nested.\nAfter nested.");
    assert_eq!(r["tables"][1]["parent_table"], 1);
    assert_eq!(r["tables"][1]["cells"][0]["text"], "Nested evidence.");
    assert_eq!(outer["complete"], true);
}
