use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn xlsx_profiles_grid_and_prose_without_losing_sheet_order() {
    let r = inspect(&fixture("grid.xlsx"), &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.sheets, Some(2));
    assert_eq!(r.sheets[0].name, "Summary");
    let sheet = &r.sheets[1];
    assert_eq!(sheet.name, "Notes");
    assert_eq!(sheet.used_range.as_deref(), Some("A1:C4"));
    assert_eq!((sheet.rows, sheet.cols, sheet.cell_count), (4, 3, 12));
    assert_eq!(sheet.non_empty_cells, 11);
    assert_eq!(sheet.formula_count, 1);
    assert_eq!(sheet.merged_ranges, ["B4:C4"]);
    assert!(sheet.is_longest);
    assert!(!r.sheets[0].is_longest);
    let notes = &sheet.columns[1];
    assert_eq!(notes.header.as_deref(), Some("Notes"));
    assert_eq!(notes.inferred_type, "string");
    assert_eq!(notes.non_empty_count, 3);
    assert_eq!(notes.distinct_count, 2);
    let prose = "We can inspect the original source and verify the evidence in this answer.";
    let second = "This is another native note about the source and its evidence.";
    assert_eq!(notes.total_bytes, 2 * prose.len() + second.len());
    assert_eq!(notes.max_bytes, prose.len());
    assert_eq!(
        notes.mean_bytes,
        (2 * prose.len() + second.len()) as f64 / 3.0
    );
    assert!(notes.avg_prose_score >= 0.20);
    assert!(notes.is_prose_candidate);
    assert_eq!(notes.sample_values.len(), 3);
    assert_eq!(sheet.columns[0].inferred_type, "number");
    assert_eq!(sheet.columns[2].inferred_type, "mixed");
    assert_eq!(r.preflight.len(), 2);
    assert_eq!(r.preflight[1].unit_kind, "sheet");
    assert!(!r.preflight[1].needs_ocr);
    assert!(
        r.text
            .content
            .contains("This is another native note about the source and its evidence.")
    );
}
