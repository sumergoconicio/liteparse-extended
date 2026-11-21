//! Wave-4 spreadsheet safety regressions (W3-SYS-SPREADSHEET-WASM32,
//! W4-SYS-SPREADSHEET-ROWOVERFLOW, W3-SYS-SPREADSHEET-SHEETS). All payloads
//! are small synthetic XLSX fixtures; every run stays bounded by the
//! inspection caps and writes only under the shared verification target.
use litx_inspect::{InspectOptions, inspect};
use std::{io::Write, path::PathBuf};

const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Creates a unique writable scratch directory for generated test fixtures.
fn directory(label: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/verification/wave4-remediation/spreadsheet/fixtures")
        .join(format!(
            "{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// Writes the entries as a stored ZIP fixture with the given extension.
fn zip_fixture(label: &str, extension: &str, entries: &[(&str, &str)]) -> PathBuf {
    let path = directory(label).join(format!("payload.{extension}"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, body) in entries {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    path
}

/// Builds a workbook: `sheet_bodies` become worksheets/sheetN.xml with
/// matching r:id relationships, plus any `extra_rels` XML fragments and
/// `extra_parts` ZIP entries (shared strings, duplicate parts, ...).
fn workbook(
    label: &str,
    sheet_bodies: &[&str],
    extra_rels: &[String],
    extra_parts: &[(&str, &str)],
) -> PathBuf {
    let sheets = (1..=sheet_bodies.len())
        .map(|i| format!(r#"<sheet name="S{i}" sheetId="{i}" r:id="r{i}"/>"#))
        .collect::<Vec<_>>()
        .join("");
    let worksheet_rels = (1..=sheet_bodies.len())
        .map(|i| {
            format!(
                r#"<Relationship Id="r{i}" Type="{R}/worksheet" Target="worksheets/sheet{i}.xml"/>"#
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let mut entries: Vec<(String, String)> = vec![
        (
            "[Content_Types].xml".into(),
            format!(
                r#"<Types xmlns="{CT}"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#
            ),
        ),
        (
            "xl/workbook.xml".into(),
            format!(r#"<workbook xmlns="{S}" xmlns:r="{R}"><sheets>{sheets}</sheets></workbook>"#),
        ),
        (
            "xl/_rels/workbook.xml.rels".into(),
            format!(
                r#"<Relationships xmlns="{PKG}">{worksheet_rels}{}</Relationships>"#,
                extra_rels.join("")
            ),
        ),
    ];
    for (index, body) in sheet_bodies.iter().enumerate() {
        entries.push((
            format!("xl/worksheets/sheet{}.xml", index + 1),
            (*body).to_string(),
        ));
    }
    entries.extend(
        extra_parts
            .iter()
            .map(|(name, body)| ((*name).to_string(), (*body).to_string())),
    );
    let entries = entries
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect::<Vec<_>>();
    zip_fixture(label, "xlsx", &entries)
}

/// Wraps `<row>` fragments in a valid worksheet root.
fn sheet(rows: &str) -> String {
    format!(r#"<worksheet xmlns="{S}"><sheetData>{rows}</sheetData></worksheet>"#)
}

#[test]
fn explicit_max_row_then_implicit_row_errors_without_panic() {
    // W4-SYS-SPREADSHEET-ROWOVERFLOW: `r="usize::MAX"` followed by an
    // implicit `<row/>` used to overflow `previous_row + 1` (panic under
    // overflow checks, wrap to 0 in release). The explicit row must fail
    // closed on the 1..=1048576 range and the implicit row must never run.
    let path = workbook(
        "row-max-then-implicit",
        &[&sheet(r#"<row r="18446744073709551615"/><row/>"#)],
        &[],
        &[],
    );
    let r = inspect(&path, &InspectOptions::default());
    assert!(
        r.errors
            .iter()
            .any(|e| e.stage == "sheet" && e.message.contains("out of range")),
        "the out-of-range row must fail closed, got: {:?}",
        r.errors
    );
    assert!(
        r.text.truncated,
        "a dropped sheet leaves partial text, which must be flagged"
    );
    assert!(
        r.markdown.truncated,
        "a dropped sheet leaves partial markdown, which must be flagged"
    );
}

#[test]
fn row_zero_and_above_max_fail_closed_as_range_errors() {
    // W4-SYS-SPREADSHEET-ROWOVERFLOW: rows outside 1..=1048576 must be
    // rejected immediately by the row guard, not by later cell coordinates
    // (a bare above-max row used to pass silently).
    for body in [sheet(r#"<row r="0"/>"#), sheet(r#"<row r="1048577"/>"#)] {
        let path = workbook("row-range", &[&body], &[], &[]);
        let r = inspect(&path, &InspectOptions::default());
        assert!(
            r.errors.iter().any(|e| e.message.contains("out of range")),
            "row outside 1..=1048576 must report a range error, got: {:?}",
            r.errors
        );
    }
}

#[test]
fn row_boundary_and_implicit_rows_stay_supported() {
    // Positive regression: the exact boundary row 1048576 and implicit row
    // numbering (no `r` attribute) keep working through the new guard.
    let boundary = sheet(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>H</t></is></c></row><row r="1048576"><c r="A1048576" t="inlineStr"><is><t>v</t></is></c></row>"#,
    );
    let path = workbook("row-boundary", &[&boundary], &[], &[]);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.sheets[0].rows, 1_048_576);
    assert!(!r.text.truncated);

    let implicit = sheet(
        r#"<row><c r="A1" t="inlineStr"><is><t>a</t></is></c></row><row><c r="A2" t="inlineStr"><is><t>b</t></is></c></row>"#,
    );
    let path = workbook("row-implicit", &[&implicit], &[], &[]);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.sheets[0].rows, 2);
    assert_eq!(r.sheets[0].non_empty_cells, 2);
}

#[test]
fn excessive_sheet_count_is_capped_with_honest_partial_report() {
    // W3-SYS-SPREADSHEET-SHEETS: eight declared sheets exceed the default
    // unit limit (5); processing must stay bounded, the count must stay
    // honest, and the report must flag the capped workbook as partial.
    let body = sheet(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row>"#);
    let bodies: Vec<&str> = vec![&body; 8];
    let path = workbook("sheets-cap", &bodies, &[], &[]);
    let r = inspect(&path, &InspectOptions::default());
    assert_eq!(r.counts.sheets, Some(8), "the declared count stays honest");
    assert_eq!(
        r.sheets.len(),
        5,
        "processing stops at the unit limit, got {}",
        r.sheets.len()
    );
    assert!(
        r.errors
            .iter()
            .any(|e| e.stage == "sheet" && e.message.contains("cap")),
        "capping must be reported, got: {:?}",
        r.errors
    );
    assert!(r.text.truncated, "capped workbooks are partial output");
    assert!(r.markdown.truncated);
}

#[test]
fn normal_multi_sheet_workbook_keeps_full_results() {
    // Positive regression: a three-sheet workbook well under the cap keeps
    // every sheet and reports no truncation.
    let body = sheet(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row>"#);
    let bodies: Vec<&str> = vec![&body; 3];
    let path = workbook("sheets-normal", &bodies, &[], &[]);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.sheets, Some(3));
    assert_eq!(r.sheets.len(), 3);
    assert!(!r.text.truncated);
    assert!(!r.markdown.truncated);
}

#[test]
fn repeated_shared_strings_relationship_fails_closed() {
    // W3-SYS-SPREADSHEET-SHEETS amplification: two distinct sharedStrings
    // relationships used to be parsed one after the other with the last one
    // silently overwriting the first; the duplicate must fail closed.
    let sst = format!(r#"<sst xmlns="{S}"><si><t>one</t></si></sst>"#);
    let path = workbook(
        "shared-strings-duplicate",
        &[&sheet(r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#)],
        &[
            format!(
                r#"<Relationship Id="ss1" Type="{R}/sharedStrings" Target="sharedStrings.xml"/>"#
            ),
            format!(
                r#"<Relationship Id="ss2" Type="{R}/sharedStrings" Target="sharedStrings2.xml"/>"#
            ),
        ],
        &[
            ("xl/sharedStrings.xml", sst.as_str()),
            ("xl/sharedStrings2.xml", sst.as_str()),
        ],
    );
    let r = inspect(&path, &InspectOptions::default());
    assert!(
        r.errors
            .iter()
            .any(|e| e.message.contains("duplicate shared strings")),
        "the duplicated shared-strings part must be rejected, got: {:?}",
        r.errors
    );
    assert!(
        r.sheets.is_empty(),
        "ambiguous strings must not profile sheets"
    );
}

#[test]
fn malformed_shared_string_reference_keeps_honest_partial_text() {
    // Frank's outcome: a shared-string index past the end of the table must
    // drop only the broken sheet, keep the supported partial text, and
    // mark both text and markdown as partial instead of claiming completeness.
    let sst = format!(r#"<sst xmlns="{S}"><si><t>one</t></si></sst>"#);
    let body = sheet(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Header</t></is></c></row><row r="2"><c r="A2" t="s"><v>7</v></c></row>"#,
    );
    let path = workbook(
        "shared-strings-oob",
        &[&body],
        &[format!(
            r#"<Relationship Id="ss1" Type="{R}/sharedStrings" Target="sharedStrings.xml"/>"#
        )],
        &[("xl/sharedStrings.xml", sst.as_str())],
    );
    let r = inspect(&path, &InspectOptions::default());
    assert!(
        r.errors
            .iter()
            .any(|e| e.stage == "sheet" && e.message.contains("shared string index out of bounds")),
        "the broken sheet must report its error, got: {:?}",
        r.errors
    );
    assert!(r.sheets.is_empty(), "the broken sheet is dropped");
    assert!(
        r.text.content.contains("Header"),
        "supported partial text stays: {}",
        r.text.content
    );
    assert!(r.text.truncated, "partial text must be flagged truncated");
    assert!(r.markdown.truncated, "partial markdown must be flagged");
}
