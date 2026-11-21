use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn zip_directory_count_is_capped_before_loading_entries() {
    let r = inspect(&fixture("directory-cap.zip"), &InspectOptions::default());
    assert!(
        r.errors
            .iter()
            .any(|e| e.stage == "archive" && e.message.contains("entry-count")),
        "{:?}",
        r.errors
    );
}

#[test]
fn zip64_directories_cannot_bypass_classic_directory_caps() {
    let r = inspect(&fixture("zip64-directory.zip"), &InspectOptions::default());
    assert!(
        r.errors
            .iter()
            .any(|e| e.stage == "archive" && e.message.contains("ZIP64")),
        "{:?}",
        r.errors
    );
}

#[test]
fn forged_language_namespace_never_fills_packet_language() {
    let r = inspect(
        &fixture("spoofed-language.docx"),
        &InspectOptions {
            packet: true,
            ..Default::default()
        },
    );
    assert_eq!(r.packet.unwrap()["Language"], "N/A");
    assert!(!r.claims.iter().any(|c| c.key == "language"));
}

#[test]
fn positive_image_bytes_do_not_enter_office_route_from_filename() {
    let r = inspect(&fixture("image.docx"), &InspectOptions::default());
    assert_eq!(r.format.bucket, "Images");
    assert!(r.format.disagreement);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}

#[test]
fn unsafe_office_inputs_keep_useful_partial_reports() {
    for (name, expected) in [
        ("entity.docx", "DTD"),
        ("malformed.docx", ""),
        ("read-cap.docx", "per-entry"),
        ("encrypted.docx", ""),
        ("external.xlsx", "relationship"),
    ] {
        let r = inspect(&fixture(name), &InspectOptions::default());
        assert!(
            r.errors.iter().any(|e| e.message.contains(expected)),
            "{name}: {:?}",
            r.errors
        );
        assert!(r.counts.bytes > 0);
        assert!(!r.text.content.contains("root:"));
    }
}
