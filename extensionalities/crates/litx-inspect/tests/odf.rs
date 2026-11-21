use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn ods_repeated_cells_are_profiled_without_expanding_blank_tail() {
    let r = inspect(&fixture("grid.ods"), &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.sheets, Some(1));
    let sheet = &r.sheets[0];
    assert_eq!(sheet.name, "Notes");
    assert_eq!(sheet.used_range.as_deref(), Some("A1:B4"));
    assert_eq!((sheet.rows, sheet.cols), (4, 2));
    assert_eq!(sheet.non_empty_cells, 8);
    assert_eq!(sheet.formula_count, 1);
    assert_eq!(sheet.merged_ranges, ["B4:C4"]);
    assert_eq!(sheet.columns[0].inferred_type, "number");
    assert_eq!(sheet.columns[1].non_empty_count, 3);
    assert_eq!(sheet.columns[1].distinct_count, 2);
    assert!(sheet.columns[1].is_prose_candidate);
    assert!(sheet.is_longest);
    assert_eq!(r.preflight[0].unit_kind, "sheet");
    assert!(!r.preflight[0].needs_ocr);
    assert_eq!(r.text.content.matches("We can inspect").count(), 2);
}

#[test]
fn odp_native_slides_and_images_are_counted() {
    let r = inspect(&fixture("deck.odp"), &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.slides, Some(2));
    assert_eq!(r.preflight.len(), 2);
    assert!(!r.preflight[0].needs_ocr);
    assert!(r.preflight[1].needs_ocr);
    assert_eq!(
        r.text.content,
        "This is the first native ODF presentation slide.\nScan\n"
    );
}

#[test]
fn odf_text_preserves_mixed_content_and_explicit_language() {
    let r = inspect(
        &fixture("notes.odt"),
        &InspectOptions {
            packet: true,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.format.bucket, "Office Docs");
    assert_eq!(r.format.basis, "bytes");
    assert_eq!(
        r.text.content,
        "Café et preuves ici.\tTexte  natif.\nScan\n"
    );
    assert_eq!(r.counts.paragraphs, Some(2));
    assert_eq!(r.preflight.len(), 2);
    assert!(!r.preflight[0].needs_ocr);
    assert!(r.preflight[1].needs_ocr);
    assert_eq!(r.packet.unwrap()["Language"], "French");
}
