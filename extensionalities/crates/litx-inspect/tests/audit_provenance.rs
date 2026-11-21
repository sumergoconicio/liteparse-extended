//! Red/green regression tests for the native/OCR provenance audit findings
//! (LITX-AUD-008/009/010/012/013/014/015/019). Synthetic inputs only.
use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;

/// Creates a unique writable scratch directory under the shared test target.
fn scratch(tag: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "provenance-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn identify_with_explicit_pages_does_not_imply_a_token_budget() {
    let path = scratch("tokens").join("words.txt");
    std::fs::write(&path, "word ".repeat(600)).unwrap();
    let r = inspect(
        &path,
        &InspectOptions {
            identify: true,
            max_tokens: None,
            max_pages: Some(2),
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.text.tokens, 600);
    assert!(!r.text.truncated);
    assert!(!r.markdown.truncated);
}

#[test]
fn dataset_has_more_marks_text_and_markdown_truncated() {
    // LITX-AUD-015: a truncated dataset preview must surface in the
    // text/Markdown partial-status flags.
    let path = scratch("csv-more").join("rows.csv");
    let mut content = String::from("name,value\n");
    for index in 0..7 {
        content.push_str(&format!("row{index},{index}\n"));
    }
    std::fs::write(&path, content).unwrap();
    let r = inspect(&path, &Default::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let dataset = r.dataset.as_ref().unwrap();
    assert!(dataset.has_more);
    assert_eq!(dataset.rows.len(), 5);
    assert!(r.text.truncated);
    assert!(r.markdown.truncated);
}

#[test]
fn dataset_values_truncated_marks_text_and_markdown_truncated() {
    // LITX-AUD-015: per-value truncation must also surface as partial output.
    let path = scratch("csv-values").join("long.csv");
    let content = format!("name,value\nshort,{}\n", "x".repeat(600));
    std::fs::write(&path, content).unwrap();
    let r = inspect(&path, &Default::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let dataset = r.dataset.as_ref().unwrap();
    assert!(dataset.values_truncated);
    assert!(!dataset.has_more);
    assert!(r.text.truncated);
    assert!(r.markdown.truncated);
}
