use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn cbz_dimensions_follow_archive_order_not_filename_sorting() {
    let r = inspect(
        &fixture("pages.cbz"),
        &InspectOptions {
            max_units: 1,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(r).unwrap();
    assert_eq!(v["format"]["bucket"], "Comics");
    assert_eq!(v["counts"]["pages"], 2);
    assert_eq!(v["pages"][0]["source_part"], "02.png");
    assert_eq!(v["pages"][0]["width"], 64);
    assert_eq!(v["pages"][1]["height"], 80);
}

#[test]
fn cbr_walks_rar4_and_rar5_without_decompressing_pages() {
    for name in ["pages4.cbr", "pages5.cbr"] {
        let r = inspect(&fixture(name), &Default::default());
        assert_eq!(r.counts.pages, Some(2), "{name}: {:?}", r.errors);
        assert_eq!(r.entries[0].name, "02.png");
        let v = serde_json::to_value(r).unwrap();
        assert_eq!(v["format"]["bucket"], "Comics");
        assert_eq!(v["pages"][0]["width"], serde_json::Value::Null);
        assert!(v["thumbnail"].is_null());
        assert!(
            v["errors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["message"].as_str().unwrap().contains("no decompression"))
        );
    }
    for name in ["truncated.cbr", "encrypted.cbr"] {
        let r = inspect(&fixture(name), &Default::default());
        assert!(
            r.errors.iter().any(|e| e.stage == "rar"),
            "{name}: {:?}",
            r.errors
        );
    }
}
