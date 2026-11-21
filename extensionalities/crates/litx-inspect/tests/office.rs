use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn pptx_follows_relationship_order_and_limits_only_preflight() {
    let options = InspectOptions {
        max_units: 1,
        max_chars: 9,
        ..Default::default()
    };
    let r = inspect(&fixture("deck.pptx"), &options);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.slides, Some(2));
    assert_eq!(r.text.content, "This is t");
    assert!(r.text.truncated);
    assert_eq!(
        r.counts.chars,
        "This is the first slide in presentation order.\nScan\n"
            .chars()
            .count()
    );
    assert_eq!(r.preflight.len(), 1);
    assert!(!r.preflight[0].needs_ocr);
    assert_eq!(r.preflight[0].unit_kind, "slide");
}

#[test]
fn docx_native_text_metadata_sections_and_provenance() {
    let options = InspectOptions {
        packet: true,
        ..Default::default()
    };
    let r = inspect(&fixture("notes.docx"), &options);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.format.bucket, "Office Docs");
    assert_eq!(r.format.basis, "bytes");
    assert!(!r.format.disagreement);
    assert!(r.claims.iter().any(|c| c.key == "author"
        && c.value == "Ada Example"
        && c.source == "container_metadata"));
    assert!(
        r.claims
            .iter()
            .any(|c| c.key == "language" && c.value == "en-GB")
    );
    assert_eq!(r.counts.paragraphs, Some(3));
    assert_eq!(r.counts.pages, None); // app.xml's unverified Pages=999 is only a claim.
    assert_eq!(
        r.text.content,
        "This is a native text layer with an ampersand & evidence.\nSection one ends here.\nScan\n"
    );
    assert_eq!(r.preflight.len(), 2);
    assert!(!r.preflight[0].needs_ocr);
    assert!(r.preflight[1].needs_ocr);
    assert_eq!(r.preflight[1].unit_kind, "section");
    assert_eq!(r.preflight[1].confidence, "deterministic");
    assert_eq!(r.packet.unwrap()["Language"], "English");
}
