use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn epub_reads_metadata_spine_order_and_bounded_body_text() {
    let r = inspect(
        &fixture("native.epub"),
        &InspectOptions {
            packet: true,
            max_chars: 12,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(r.format.bucket, "Ebooks");
    assert_eq!(r.format.basis, "bytes");
    assert_eq!(v["counts"]["spine_items"], 2);
    assert_eq!(r.text.content, "First chapte");
    assert!(r.text.truncated);
    assert_eq!(
        r.counts.chars,
        "First chapter & evidence.\nSecond chapter.\n"
            .chars()
            .count()
    );
    assert_eq!(v["packet"]["Language"], "Spanish");
    assert!(
        r.claims
            .iter()
            .any(|c| c.key == "author" && c.value == "Example Writer")
    );
    let unsafe_report = inspect(&fixture("escaping.epub"), &Default::default());
    assert!(
        unsafe_report
            .errors
            .iter()
            .any(|e| e.stage == "epub" && e.message.contains("escapes"))
    );
}

#[test]
fn mobi_metadata_survives_unsupported_or_encrypted_text() {
    for name in [
        "native.mobi",
        "native.azw3",
        "encrypted.azw",
        "huff.mobi",
        "broken-lz77.mobi",
    ] {
        let r = inspect(
            &fixture(name),
            &InspectOptions {
                packet: true,
                ..Default::default()
            },
        );
        assert!(
            r.claims
                .iter()
                .any(|c| c.key == "title" && c.value == "Native Pocket Book"),
            "{name}: {:?}",
            r.errors
        );
        assert!(
            r.claims
                .iter()
                .any(|c| c.key == "publisher" && c.value == "Example Press")
        );
        assert_eq!(r.packet.as_ref().unwrap()["Language"], "German");
        if name.starts_with("native.") {
            assert_eq!(r.text.content, "Native MOBI abcabc & evidence.\n");
            if name.ends_with("azw3") {
                assert!(
                    r.claims
                        .iter()
                        .any(|c| c.key == "text_status" && c.value.contains("best_effort"))
                );
            }
        } else {
            assert!(r.text.content.is_empty());
            assert!(
                r.errors.iter().any(|e| e.stage == "mobi_text"),
                "{name}: {:?}",
                r.errors
            );
        }
    }
}

#[test]
fn epub_xhtml_doctype_does_not_load_external_resources() {
    let r = inspect(&fixture("doctype.epub"), &Default::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.text.content, "First chapter.\nSecond chapter.\n");
    let unsafe_report = inspect(&fixture("xxe.epub"), &Default::default());
    assert!(
        unsafe_report
            .errors
            .iter()
            .any(|e| e.stage == "epub_text" && e.message.contains("entity"))
    );
    assert!(!unsafe_report.text.content.contains("root:"));
    assert!(unsafe_report.text.content.contains("Second chapter."));
}
