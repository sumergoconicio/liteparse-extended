//! Wave-5 cumulative workbook regressions for the `litx-inspect` crate.
//! File: `spreadsheet_wave5.rs`.
//! Responsibility: prove bounded sheet, cell, and value-byte work across
//! XLSX/XLSM/ODS while preserving ordinary output and relationship validation.
use litx_inspect::{InspectOptions, inspect};
use std::{io::Write, path::PathBuf};

const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const ODF_PREFIX: &str = r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">"#;

/// Creates a unique fixture directory under the ignored Wave-5 verification tree.
/// Accepts a human-readable label; returns the created path; fails if the
/// directory cannot be created or the system clock predates the Unix epoch.
fn directory(label: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/verification/wave5-remediation/spreadsheet/fixtures")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// Writes string entries into a stored ZIP fixture.
/// Accepts a label, extension, and entry name/body pairs; returns the fixture
/// path; fails on filesystem or ZIP encoding errors.
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

/// Wraps worksheet rows in the SpreadsheetML namespace.
/// Accepts row XML; returns a worksheet document; has no failure states.
fn sheet(rows: &str) -> String {
    format!(r#"<worksheet xmlns="{S}"><sheetData>{rows}</sheetData></worksheet>"#)
}

/// Builds an OOXML workbook from raw workbook, relationship, and part XML.
/// Accepts a label, extension, macro-enabled content-type flag, sheet
/// declarations, relationship fragments, and extra parts; returns the fixture
/// path; fails on filesystem or ZIP encoding errors.
fn ooxml(
    label: &str,
    extension: &str,
    macro_enabled: bool,
    declarations: &str,
    relationships: &str,
    extra_parts: &[(&str, &str)],
) -> PathBuf {
    let content_type = if macro_enabled {
        "application/vnd.ms-excel.sheet.macroEnabled.main+xml"
    } else {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
    };
    let mut entries: Vec<(String, String)> = vec![
        (
            "[Content_Types].xml".into(),
            format!(
                r#"<Types xmlns="{CT}"><Override PartName="/xl/workbook.xml" ContentType="{content_type}"/></Types>"#
            ),
        ),
        (
            "xl/workbook.xml".into(),
            format!(
                r#"<workbook xmlns="{S}" xmlns:r="{R}"><sheets>{declarations}</sheets></workbook>"#
            ),
        ),
        (
            "xl/_rels/workbook.xml.rels".into(),
            format!(r#"<Relationships xmlns="{PKG}">{relationships}</Relationships>"#),
        ),
    ];
    entries.extend(
        extra_parts
            .iter()
            .map(|(name, body)| ((*name).to_string(), (*body).to_string())),
    );
    let entries = entries
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect::<Vec<_>>();
    zip_fixture(label, extension, &entries)
}

/// Builds a conventional workbook with one worksheet part per supplied body.
/// Accepts a label, worksheet bodies, additional relationships, and extra
/// parts; returns an XLSX path; fails on fixture creation errors.
fn workbook(
    label: &str,
    sheet_bodies: &[&str],
    extra_rels: &[String],
    extra_parts: &[(&str, &str)],
) -> PathBuf {
    let declarations = (1..=sheet_bodies.len())
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
    let part_names = (1..=sheet_bodies.len())
        .map(|i| format!("xl/worksheets/sheet{i}.xml"))
        .collect::<Vec<_>>();
    let mut parts = part_names
        .iter()
        .map(String::as_str)
        .zip(sheet_bodies.iter().copied())
        .collect::<Vec<_>>();
    parts.extend_from_slice(extra_parts);
    ooxml(
        label,
        "xlsx",
        false,
        &declarations,
        &format!("{worksheet_rels}{}", extra_rels.join("")),
        &parts,
    )
}

/// Builds an ODS fixture containing raw table XML.
/// Accepts a label and table fragments; returns the ODS path; fails on
/// filesystem or ZIP encoding errors.
fn ods(label: &str, tables: &str) -> PathBuf {
    let content = format!(
        "{ODF_PREFIX}<office:body><office:spreadsheet>{tables}</office:spreadsheet></office:body></office:document-content>"
    );
    zip_fixture(
        label,
        "ods",
        &[
            ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
            ("content.xml", &content),
        ],
    )
}

/// Returns options representing full extraction without a page limit.
/// Accepts no inputs; returns full-extraction options; has no failure states.
fn extract_options() -> InspectOptions {
    InspectOptions {
        max_pages: None,
        ..Default::default()
    }
}

/// Asserts the common honest-partial workbook budget outcome.
/// Accepts a report, declared sheet count, retained sheet count, and diagnostic
/// fragment; returns unit; panics with report details when an invariant fails.
fn assert_partial(
    report: &litx_inspect::InspectReport,
    declared: usize,
    retained: usize,
    diagnostic: &str,
) {
    assert_eq!(report.counts.sheets, Some(declared));
    assert_eq!(report.sheets.len(), retained, "{:?}", report.errors);
    assert!(
        report.errors.iter().any(|e| e.message.contains(diagnostic)),
        "missing {diagnostic:?} in {:?}",
        report.errors
    );
    assert!(report.text.truncated, "text must be honestly partial");
    assert!(
        report.markdown.truncated,
        "Markdown must be honestly partial"
    );
}

#[test]
/// Clamps unlimited and huge XLSX extraction to the hard sheet ceiling.
/// Accepts no inputs; returns unit; panics if either option mode processes
/// beyond 4096 sheets, loses the declared count, or omits partial diagnostics.
fn xlsx_hard_sheet_cap_binds_extract_and_huge_page_limit() {
    let declarations = (1..=4097)
        .map(|i| format!(r#"<sheet name="S{i}" sheetId="{i}" r:id="r1"/>"#))
        .collect::<Vec<_>>()
        .join("");
    let path = ooxml(
        "xlsx-hard-sheet-cap",
        "xlsx",
        false,
        &declarations,
        &format!(r#"<Relationship Id="r1" Type="{R}/worksheet" Target="worksheets/sheet1.xml"/>"#),
        &[("xl/worksheets/sheet1.xml", &sheet(""))],
    );
    for options in [
        extract_options(),
        InspectOptions {
            max_pages: Some(200_000),
            ..Default::default()
        },
    ] {
        let report = inspect(&path, &options);
        assert_partial(&report, 4097, 4096, "sheet cap");
    }
}

#[test]
/// Clamps unlimited and huge ODS extraction to the hard sheet ceiling.
/// Accepts no inputs; returns unit; panics if either option mode processes
/// beyond 4096 tables or reports the workbook as complete.
fn ods_hard_sheet_cap_binds_extract_and_huge_page_limit() {
    let tables = (1..=4097)
        .map(|i| format!(r#"<table:table table:name="T{i}"/>"#))
        .collect::<Vec<_>>()
        .join("");
    let path = ods("ods-hard-sheet-cap", &tables);
    for options in [
        extract_options(),
        InspectOptions {
            max_pages: Some(200_000),
            ..Default::default()
        },
    ] {
        let report = inspect(&path, &options);
        assert_partial(&report, 4097, 4096, "sheet cap");
    }
}

#[test]
/// Preserves smaller user sheet limits for both OOXML and ODS.
/// Accepts no inputs; returns unit; panics if either format ignores a two-sheet
/// limit or fails to preserve declared counts and truncation diagnostics.
fn smaller_sheet_limits_apply_to_xlsx_and_ods() {
    let body = sheet(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row>"#);
    let xlsx = workbook("xlsx-small-limit", &[&body, &body, &body], &[], &[]);
    let tables = (1..=3)
        .map(|i| format!(r#"<table:table table:name="T{i}"/>"#))
        .collect::<Vec<_>>()
        .join("");
    let ods = ods("ods-small-limit", &tables);
    let options = InspectOptions {
        max_pages: Some(2),
        ..Default::default()
    };
    for path in [xlsx, ods] {
        let report = inspect(&path, &options);
        assert_partial(&report, 3, 2, "sheet cap");
    }
}

#[test]
/// Keeps small ordinary XLSX, XLSM, and ODS workbooks complete.
/// Accepts no inputs; returns unit; panics on unexpected diagnostics, missing
/// sheets, or truncation under full extraction.
fn ordinary_xlsx_xlsm_and_ods_remain_complete() {
    let body = sheet(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row>"#);
    let declarations =
        r#"<sheet name="S1" sheetId="1" r:id="r1"/><sheet name="S2" sheetId="2" r:id="r2"/>"#;
    let relationships = format!(
        r#"<Relationship Id="r1" Type="{R}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="r2" Type="{R}/worksheet" Target="worksheets/sheet2.xml"/>"#
    );
    for (extension, macro_enabled) in [("xlsx", false), ("xlsm", true)] {
        let path = ooxml(
            "ordinary-ooxml",
            extension,
            macro_enabled,
            declarations,
            &relationships,
            &[
                ("xl/worksheets/sheet1.xml", &body),
                ("xl/worksheets/sheet2.xml", &body),
            ],
        );
        let report = inspect(&path, &extract_options());
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.sheets.len(), 2);
        assert!(!report.text.truncated);
        assert!(!report.markdown.truncated);
    }
    let path = ods(
        "ordinary-ods",
        r#"<table:table table:name="one"/><table:table table:name="two"/>"#,
    );
    let report = inspect(&path, &extract_options());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.sheets.len(), 2);
    assert!(!report.text.truncated);
    assert!(!report.markdown.truncated);
}

#[test]
/// Stops repeated shared-string scans at the cumulative workbook byte budget.
/// Accepts no inputs; returns unit; panics if the first useful sheet is lost,
/// later sheets are retried, or 64 MiB of logical values is not enforced.
fn xlsx_shared_string_byte_budget_is_cumulative_across_sheets() {
    let shared = format!(
        r#"<sst xmlns="{S}"><si><t>{}</t></si></sst>"#,
        "S".repeat(1 << 20)
    );
    let repeated = (1..=40)
        .map(|row| format!(r#"<row r="{row}"><c r="A{row}" t="s"><v>0</v></c></row>"#))
        .collect::<Vec<_>>()
        .join("");
    let first = sheet(&repeated);
    let second = sheet(&repeated);
    let third =
        sheet(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>unreachable</t></is></c></row>"#);
    let path = workbook(
        "xlsx-shared-byte-budget",
        &[&first, &second, &third],
        &[format!(
            r#"<Relationship Id="strings" Type="{R}/sharedStrings" Target="sharedStrings.xml"/>"#
        )],
        &[("xl/sharedStrings.xml", &shared)],
    );
    let report = inspect(&path, &extract_options());
    assert_partial(&report, 3, 1, "value-byte budget");
    assert_eq!(report.sheets[0].name, "S1");
    assert!(report.text.content.contains('S'));
    assert!(!report.text.content.contains("unreachable"));
}

#[test]
/// Retains value-byte charges when an earlier worksheet fails.
/// Accepts no inputs; returns unit; panics if a malformed first sheet resets
/// the budget or the third sheet is processed after second-sheet exhaustion.
fn xlsx_failed_sheet_does_not_reset_value_byte_budget() {
    let shared = format!(
        r#"<sst xmlns="{S}"><si><t>{}</t></si></sst>"#,
        "F".repeat(1 << 20)
    );
    let mut rows = (1..=40)
        .map(|row| format!(r#"<row r="{row}"><c r="A{row}" t="s"><v>0</v></c></row>"#))
        .collect::<Vec<_>>();
    rows.push(r#"<row r="41"><c r="A41" t="s"><v>9</v></c></row>"#.into());
    let failed = sheet(&rows.join(""));
    let repeated = (1..=40)
        .map(|row| format!(r#"<row r="{row}"><c r="A{row}" t="s"><v>0</v></c></row>"#))
        .collect::<Vec<_>>()
        .join("");
    let second = sheet(&repeated);
    let third =
        sheet(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>unreachable</t></is></c></row>"#);
    let path = workbook(
        "xlsx-failed-sheet-byte-budget",
        &[&failed, &second, &third],
        &[format!(
            r#"<Relationship Id="strings" Type="{R}/sharedStrings" Target="sharedStrings.xml"/>"#
        )],
        &[("xl/sharedStrings.xml", &shared)],
    );
    let report = inspect(&path, &extract_options());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.message.contains("sheet 1") && e.message.contains("out of bounds"))
    );
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.message.contains("sheet 2") && e.message.contains("value-byte budget")),
        "{:?}",
        report.errors
    );
    assert!(
        report.sheets.is_empty(),
        "failed and exhausted sheets are dropped"
    );
    assert!(!report.text.content.contains("unreachable"));
    assert!(report.text.truncated);
}

#[test]
/// Charges ODF row/column repeats as logical cell visits before expansion.
/// Accepts no inputs; returns unit; panics if the first sheet is lost, the
/// repeated second sheet exceeds 262144 cumulative visits, or sheet three runs.
fn ods_repeated_cells_exhaust_cumulative_cell_budget() {
    let path = ods(
        "ods-repeat-cell-budget",
        r#"<table:table table:name="first"><table:table-row><table:table-cell><text:p>ok</text:p></table:table-cell></table:table-row></table:table><table:table table:name="second"><table:table-row table:number-rows-repeated="262144"><table:table-cell><text:p>x</text:p></table:table-cell></table:table-row></table:table><table:table table:name="unreachable"><table:table-row><table:table-cell><text:p>no</text:p></table:table-cell></table:table-row></table:table>"#,
    );
    let report = inspect(&path, &extract_options());
    assert_partial(&report, 3, 1, "cell budget");
    assert_eq!(report.sheets[0].name, "first");
    assert!(!report.text.content.contains("unreachable"));
}

#[test]
/// Preserves the blank-tail fast path without letting an empty sheet reset work.
/// Accepts no inputs; returns unit; panics if the ordinary empty sheet is
/// dropped, earlier charges are reset, or processing continues after exhaustion.
fn ods_empty_sheet_preserves_blank_tail_and_cumulative_cell_budget() {
    let path = ods(
        "ods-empty-sheet-cell-budget",
        r#"<table:table table:name="first"><table:table-row table:number-rows-repeated="200000"><table:table-cell><text:p>x</text:p></table:table-cell></table:table-row></table:table><table:table table:name="blank"><table:table-row table:number-rows-repeated="1048572"><table:table-cell table:number-columns-repeated="16384"/></table:table-row></table:table><table:table table:name="third"><table:table-row table:number-rows-repeated="100000"><table:table-cell><text:p>y</text:p></table:table-cell></table:table-row></table:table><table:table table:name="unreachable"><table:table-row><table:table-cell><text:p>no</text:p></table:table-cell></table:table-row></table:table>"#,
    );
    let report = inspect(&path, &extract_options());
    assert_partial(&report, 4, 2, "cell budget");
    assert_eq!(report.sheets[0].name, "first");
    assert_eq!(report.sheets[1].name, "blank");
    assert!(!report.text.content.contains("unreachable"));
}

#[test]
/// Charges expanded ODF value bytes for row and column repeats before profiling.
/// Accepts no inputs; returns unit; panics if a legal 80 MiB logical expansion
/// bypasses the 64 MiB workbook value-byte budget or later tables are processed.
fn ods_repeated_values_exhaust_cumulative_byte_budget() {
    let value = "V".repeat(1024);
    let tables = format!(
        r#"<table:table table:name="first"><table:table-row table:number-rows-repeated="5"><table:table-cell table:number-columns-repeated="16384" office:value-type="string" office:string-value="{value}"/></table:table-row></table:table><table:table table:name="unreachable"/>"#
    );
    let path = ods("ods-repeat-byte-budget", &tables);
    let report = inspect(&path, &extract_options());
    assert_partial(&report, 2, 0, "value-byte budget");
}

#[test]
/// Bounds a 5000-sheet by 5000-relationship workbook without pairwise retries.
/// Accepts no inputs; returns unit; panics if the declared count changes, more
/// than 4096 sheets are retained, or cap diagnostics are absent.
fn xlsx_5000_sheets_and_relationships_are_bounded() {
    let declarations = (1..=5000)
        .map(|i| format!(r#"<sheet name="S{i}" sheetId="{i}" r:id="r{i}"/>"#))
        .collect::<Vec<_>>()
        .join("");
    let relationships = (1..=5000)
        .map(|i| {
            format!(
                r#"<Relationship Id="r{i}" Type="{R}/worksheet" Target="worksheets/shared.xml"/>"#
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let body = sheet(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row>"#);
    let path = ooxml(
        "xlsx-5000-sheets-rels",
        "xlsx",
        false,
        &declarations,
        &relationships,
        &[("xl/worksheets/shared.xml", &body)],
    );
    let report = inspect(&path, &extract_options());
    assert_partial(&report, 5000, 4096, "sheet cap");
}

#[test]
/// Preserves duplicate, external, mismatched, and root-escape relationship rejection.
/// Accepts no inputs; returns unit; panics if indexed lookup weakens any
/// existing validation or reports malformed worksheet output as complete.
fn xlsx_malformed_relationships_still_fail_closed() {
    let body = sheet("");
    let cases = [
        (
            "duplicate",
            format!(
                r#"<Relationship Id="r1" Type="{R}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="r1" Type="{R}/worksheet" Target="worksheets/sheet1.xml"/>"#
            ),
            "missing or duplicate relationship id",
        ),
        (
            "external",
            format!(
                r#"<Relationship Id="r1" Type="{R}/worksheet" Target="worksheets/sheet1.xml" TargetMode="External"/>"#
            ),
            "external or mismatched package relationship",
        ),
        (
            "mismatched",
            format!(r#"<Relationship Id="r1" Type="{R}/styles" Target="worksheets/sheet1.xml"/>"#),
            "external or mismatched package relationship",
        ),
        (
            "root-escape",
            format!(r#"<Relationship Id="r1" Type="{R}/worksheet" Target="../../../escape.xml"/>"#),
            "relationship escapes package root",
        ),
    ];
    for (label, relationship, expected) in cases {
        let path = ooxml(
            &format!("xlsx-malformed-{label}"),
            "xlsx",
            false,
            r#"<sheet name="S1" sheetId="1" r:id="r1"/>"#,
            &relationship,
            &[("xl/worksheets/sheet1.xml", &body)],
        );
        let report = inspect(&path, &extract_options());
        assert!(report.sheets.is_empty(), "{label}: unexpected sheet");
        assert!(
            report.errors.iter().any(|e| e.message.contains(expected)),
            "{label}: {:?}",
            report.errors
        );
        assert!(report.text.truncated, "{label}: partial text not flagged");
    }
}
