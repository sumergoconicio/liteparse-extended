//! Structured-format amplification bounds from audit LITX-AUD-004/005/006/007/016/018.
//! All payloads are small synthetic fixtures; budgets are injected or inherently bounded.
use litx_inspect::{InspectOptions, inspect};
use std::{io::Write, path::PathBuf};

fn directory(label: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/verification/audit-remediation-20261118/structured/fixtures")
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

fn ods(label: &str, content: &str) -> PathBuf {
    zip_fixture(
        label,
        "ods",
        &[
            ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
            ("content.xml", content),
        ],
    )
}

fn odt(label: &str, content: &str) -> PathBuf {
    zip_fixture(
        label,
        "odt",
        &[
            ("mimetype", "application/vnd.oasis.opendocument.text"),
            ("content.xml", content),
        ],
    )
}

const ODF_PREFIX: &str = r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">"#;

fn xlsx(label: &str, sheet: &str) -> PathBuf {
    zip_fixture(
        label,
        "xlsx",
        &[
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Huge" sheetId="1" r:id="s1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="s1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            ("xl/worksheets/sheet1.xml", sheet),
        ],
    )
}

#[test]
fn xlsx_grid_cell_count_multiplication_is_exact_on_wide_targets() {
    // Cells at A1 and XFD1048576 produce the audit's 1,048,576 x 16,384 grid.
    let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="1048576"><c r="XFD1048576"><v>2</v></c></row></sheetData></worksheet>"#;
    let path = xlsx("grid-max", sheet);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let sheet = &r.sheets[0];
    assert_eq!((sheet.rows, sheet.cols), (1_048_576, 16_384));
    assert_eq!(sheet.cell_count, 1_048_576usize * 16_384);
}

#[test]
fn json_loop_fields_share_a_total_entry_budget_across_depths() {
    // Three nested repeated arrays over a 400-key leaf object would emit 1,200
    // field suffixes; the shared 1,024-entry budget must truncate the tail loop.
    let leaf = |v: i32| {
        let pairs = (1..=400)
            .map(|i| format!("\"k{i}\":{v}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{{{pairs}}}")
    };
    let c = format!("[{},{}]", leaf(1), leaf(2));
    let b = format!("[{{\"c\":{c}}},{{\"c\":{c}}}]");
    let json = format!("{{\"a\":[{{\"b\":{b}}},{{\"b\":{b}}}]}}");
    let path = directory("json-loop-entries").join("nested.json");
    std::fs::write(&path, json).unwrap();
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let loops = v["json"]["loops"].as_array().unwrap();
    assert_eq!(loops.len(), 3);
    let fields: Vec<usize> = loops
        .iter()
        .map(|l| l["fields"].as_array().unwrap().len())
        .collect();
    assert_eq!(fields, [400, 400, 224]);
    assert_eq!(loops[0]["fields_truncated"], false);
    assert_eq!(loops[1]["fields_truncated"], false);
    assert_eq!(loops[2]["fields_truncated"], true);
}

#[test]
fn json_loop_fields_share_a_total_byte_budget_across_depths() {
    // 60-byte keys: 400 x 68 + 400 x 64 = 52,800 bytes for the first two loops;
    // the shared 64 KiB budget leaves room for exactly 212 tail-loop fields.
    let key = |i: usize| format!("k{i:059}");
    let leaf = |v: i32| {
        let pairs = (1..=400)
            .map(|i| format!("\"{}\":{v}", key(i)))
            .collect::<Vec<_>>()
            .join(",");
        format!("{{{pairs}}}")
    };
    let c = format!("[{},{}]", leaf(1), leaf(2));
    let b = format!("[{{\"c\":{c}}},{{\"c\":{c}}}]");
    let json = format!("{{\"a\":[{{\"b\":{b}}},{{\"b\":{b}}}]}}");
    let path = directory("json-loop-bytes").join("nested.json");
    std::fs::write(&path, json).unwrap();
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let loops = v["json"]["loops"].as_array().unwrap();
    assert_eq!(loops.len(), 3);
    let fields: Vec<usize> = loops
        .iter()
        .map(|l| l["fields"].as_array().unwrap().len())
        .collect();
    assert_eq!(fields, [400, 400, 212]);
    assert_eq!(loops[2]["fields_truncated"], true);
    let total_bytes: usize = loops
        .iter()
        .flat_map(|l| l["fields"].as_array().unwrap().iter())
        .map(|f| f.as_str().unwrap().len())
        .sum();
    assert!(total_bytes <= 64 << 10, "{total_bytes}");
}

#[test]
fn xlsx_preview_marks_more_rows_when_rows_pass_the_preview_cutoff() {
    // Ten rows: the first five data rows fill the preview, and the five
    // unbuffered rows past the cutoff must still mark the sheet truncated.
    let mut rows = vec![String::from(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>H</t></is></c></row>"#,
    )];
    for i in 2..=10 {
        rows.push(format!(
            r#"<row r="{i}"><c r="A{i}" t="inlineStr"><is><t>v{i}</t></is></c></row>"#
        ));
    }
    let sheet = format!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{}</sheetData></worksheet>"#,
        rows.join("")
    );
    let path = xlsx("xlsx-ten-rows", &sheet);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let sheet = &r.sheets[0];
    assert_eq!(sheet.rows, 10);
    assert_eq!(sheet.preview_rows.len(), 5, "{:?}", sheet.preview_rows);
    assert!(
        sheet.preview_truncated,
        "rows past the preview cutoff must mark the preview as truncated"
    );
    assert!(
        r.markdown
            .content
            .contains("Dataset preview: first five data rows"),
        "markdown must carry the dataset-preview notice, got: {}",
        r.markdown.content
    );
}

#[test]
fn odt_prose_shares_document_expansion_budget_across_paragraphs_and_sections() {
    // 18,000 paragraphs each expand a 16 KiB <text:s> run: ~280 MiB of
    // cumulative expansion work from a ~1 MB fixture. Every paragraph is
    // expanded transiently, so peak memory stays bounded while the shared
    // 256 MiB document budget must stop the work with honest errors.
    let section = |paragraphs: usize| {
        let mut out = String::from("<text:section>");
        for _ in 0..paragraphs {
            out.push_str(r#"<text:p><text:s text:c="16384"/>x</text:p>"#);
        }
        out.push_str("</text:section>");
        out
    };
    let content = format!(
        "{ODF_PREFIX}<office:body><office:text>{}{}{}</office:text></office:body></office:document-content>",
        section(6000),
        section(6000),
        section(6000)
    );
    let path = odt("odt-prose-expansion-budget", &content);
    let r = inspect(&path, &InspectOptions::default());
    assert!(
        r.errors
            .iter()
            .any(|e| e.message.contains("document expansion budget")),
        "shared budget exhaustion must be reported, got: {:?}",
        r.errors
    );
    assert!(
        r.markdown.truncated,
        "partial output must be marked after budget exhaustion"
    );
}

#[test]
fn odt_tables_share_document_expansion_budget_across_cells_and_tables() {
    // 17,000 cells each expanding exactly 16 KiB of <text:s> spaces: the
    // shared budget covers exactly 16,384 cells (256 MiB), so tables 1..2048
    // stay complete, table 2049 fails, and failed tables are dropped.
    let cell = r#"<table:table-cell><text:p><text:s text:c="16384"/></text:p></table:table-cell>"#;
    let row = format!("<table:table-row>{}</table:table-row>", cell.repeat(8));
    let mut tables = String::new();
    for i in 0..2125 {
        tables.push_str(&format!(
            r#"<table:table table:name="T{i}">{row}</table:table>"#
        ));
    }
    let content = format!(
        "{ODF_PREFIX}<office:body><office:text>{tables}</office:text></office:body></office:document-content>"
    );
    let path = odt("odt-table-expansion-budget", &content);
    let r = inspect(&path, &InspectOptions::default());
    assert!(
        r.errors
            .iter()
            .any(|e| e.message.contains("document expansion budget")),
        "shared budget exhaustion must be reported, got: {:?}",
        r.errors
    );
    assert_eq!(
        r.tables.len(),
        2048,
        "only the tables that fit the shared budget are retained"
    );
    assert!(
        r.tables.iter().all(|t| t.complete),
        "retained tables stay complete"
    );
    // Whole expanded bytes (spaces) are charged, while retention stays
    // bounded by the separate output budget.
    let first = &r.tables[0].cells[0];
    assert_eq!(first.text_chars, 16_384);
    assert_eq!(first.text.chars().count(), 16_000);
    assert!(first.text_truncated);
    assert!(
        r.markdown.truncated,
        "partial output must be marked after budget exhaustion"
    );
}
#[test]
fn odt_repeated_cells_expand_source_text_once_and_keep_every_copy() {
    // Positive preservation: one source cell repeated over rows and columns
    // yields the same text in every expanded cell.
    let content = format!(
        "{ODF_PREFIX}<office:body><office:text><table:table table:name=\"R\">\
         <table:table-row table:number-rows-repeated=\"2\">\
         <table:table-cell table:number-columns-repeated=\"3\"><text:p>Cell</text:p></table:table-cell>\
         </table:table-row></table:table></office:text></office:body></office:document-content>"
    );
    let path = odt("odt-repeat-positive", &content);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let table = &r.tables[0];
    assert!((table.rows, table.cols) == (2, 3));
    assert!(table.complete);
    assert_eq!(table.cells.len(), 6);
    assert!(table.cells.iter().all(|c| c.text == "Cell"));
}

#[test]
fn odt_failed_tables_do_not_reset_the_aggregate_cell_work_budget() {
    // Each table expands 150k cells, then fails its local 200k expansion cap.
    // Failed work must stay charged: the second table trips the shared
    // document cell/grid cap instead of restarting from zero.
    let table = |name: &str| {
        format!(
            "<table:table table:name=\"{name}\">\
             <table:table-row table:number-rows-repeated=\"150000\">\
             <table:table-cell><text:p>x</text:p></table:table-cell></table:table-row>\
             <table:table-row table:number-rows-repeated=\"60000\">\
             <table:table-cell><text:p>x</text:p></table:table-cell></table:table-row>\
             </table:table>"
        )
    };
    let content = format!(
        "{ODF_PREFIX}<office:body><office:text>{}{}</office:text></office:body></office:document-content>",
        table("A"),
        table("B")
    );
    let path = odt("odt-work-evasion", &content);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.tables.is_empty(), "{:?}", r.tables);
    assert!(
        r.errors
            .iter()
            .any(|e| e.message.contains("document table cell/grid cap exceeded")),
        "{:?}",
        r.errors
    );
}

#[test]
fn ods_typed_value_cells_skip_paragraph_text_expansion() {
    // The float value attribute overrides paragraph text, so a paragraph that
    // would fail expansion (explicit-space cap) must not even be expanded.
    // The second float cell has no value attribute and keeps paragraph text.
    let content = format!(
        "{ODF_PREFIX}<office:body><office:spreadsheet><table:table table:name=\"Typed\">\
         <table:table-row>\
         <table:table-cell office:value-type=\"float\" office:value=\"2.5\">\
         <text:p><text:s text:c=\"99999\"/></text:p></table:table-cell>\
         <table:table-cell office:value-type=\"float\"><text:p>3.5</text:p></table:table-cell>\
         </table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"
    );
    let path = ods("ods-typed-override", &content);
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let sheet = &r.sheets[0];
    assert_eq!(sheet.non_empty_cells, 2);
    assert!(r.text.content.contains("2.5"), "{}", r.text.content);
    assert!(r.text.content.contains("3.5"), "{}", r.text.content);
}

fn xlsx_with_strings(label: &str, sheet: &str, shared_strings: &str) -> PathBuf {
    zip_fixture(
        label,
        "xlsx",
        &[
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Shared" sheetId="1" r:id="s1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="s1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="strings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#,
            ),
            ("xl/worksheets/sheet1.xml", sheet),
            ("xl/sharedStrings.xml", shared_strings),
        ],
    )
}

#[test]
fn xlsx_preview_buffers_shared_strings_within_the_remaining_budget() {
    // A 200-char shared string referenced by six data rows with max_chars 64:
    // the preview must keep the first data row bounded to the remaining budget
    // and mark the sheet truncated instead of dropping or cloning whole strings.
    let shared = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>{}</t></si></sst>"#,
        "S".repeat(200)
    );
    let mut rows =
        String::from(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>H</t></is></c></row>"#);
    for r in 2..=7 {
        rows.push_str(&format!(
            r#"<row r="{r}"><c r="A{r}" t="s"><v>0</v></c></row>"#
        ));
    }
    let sheet = format!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{rows}</sheetData></worksheet>"#
    );
    let path = xlsx_with_strings("xlsx-shared-budget", &sheet, &shared);
    let options = InspectOptions {
        max_chars: 64,
        ..Default::default()
    };
    let r = inspect(&path, &options);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let sheet = &r.sheets[0];
    assert_eq!(sheet.rows, 7);
    assert_eq!(sheet.preview_rows.len(), 1, "{:?}", sheet.preview_rows);
    let cell = &sheet.preview_rows[0][0];
    assert!(cell.chars().count() <= 64, "{}", cell.len());
    assert!(cell.starts_with("SSSS"), "{cell}");
    assert!(sheet.preview_truncated);
    assert!(r.text.truncated);
}

#[test]
fn sheet_preview_is_marked_truncated_when_markdown_budget_stops_rows() {
    // Three short rows fit the SheetRows budget, but the Markdown cap stops
    // row handling mid-sheet; the sheet must still report preview_truncated.
    let content = format!(
        "{ODF_PREFIX}<office:body><office:spreadsheet><table:table table:name=\"S\">\
         <table:table-row><table:table-cell><text:p>A</text:p></table:table-cell></table:table-row>\
         <table:table-row><table:table-cell><text:p>B</text:p></table:table-cell></table:table-row>\
         <table:table-row><table:table-cell><text:p>C</text:p></table:table-cell></table:table-row>\
         </table:table></office:spreadsheet></office:body></office:document-content>"
    );
    let path = ods("ods-markdown-cap", &content);
    let options = InspectOptions {
        max_chars: 24,
        ..Default::default()
    };
    let r = inspect(&path, &options);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let sheet = &r.sheets[0];
    assert_eq!(sheet.rows, 3);
    assert!(r.markdown.truncated);
    assert!(sheet.preview_truncated);
}
