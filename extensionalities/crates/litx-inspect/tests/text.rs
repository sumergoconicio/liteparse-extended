use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;

#[test]
fn markdown_inputs_preserve_native_headings_lists_and_tables() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "text-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("native.md");
    let source =
        "# Chapter\n\n- first\n- second\n\n| Name | Value |\n| --- | --- |\n| café | 42 |\n";
    std::fs::write(&path, source).unwrap();
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.text.content, source);
    assert_eq!(r.markdown.content, source);
    assert_eq!(r.markdown.basis, "native_structure");
}
