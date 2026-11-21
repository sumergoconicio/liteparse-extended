use litx_inspect::{InspectOptions, inspect};
use std::{io::Write, path::PathBuf};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn every_spreadsheet_sheet_previews_only_five_data_rows() {
    let path = fixture("../target/tests").join(format!(
        "five-rows-{}.ods",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file("mimetype", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"application/vnd.oasis.opendocument.spreadsheet")
        .unwrap();
    zip.start_file("content.xml", zip::write::SimpleFileOptions::default())
        .unwrap();
    let mut xml = String::from(
        r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet>"#,
    );
    for name in ["One", "Two"] {
        xml.push_str(&format!(r#"<table:table table:name="{name}"><table:table-row><table:table-cell><text:p>Value</text:p></table:table-cell></table:table-row>"#));
        for index in 1..=7 {
            xml.push_str(&format!("<table:table-row><table:table-cell><text:p>Row{index}</text:p></table:table-cell></table:table-row>"));
        }
        xml.push_str("</table:table>");
    }
    xml.push_str("</office:spreadsheet></office:body></office:document-content>");
    zip.write_all(xml.as_bytes()).unwrap();
    zip.finish().unwrap();
    for options in [
        InspectOptions::default(),
        InspectOptions {
            max_pages: None,
            ..Default::default()
        },
    ] {
        let r = inspect(&path, &options);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let v = serde_json::to_value(&r).unwrap();
        for sheet in v["sheets"].as_array().unwrap() {
            assert_eq!(sheet["rows"], 8);
            assert_eq!(sheet["preview_rows"].as_array().unwrap().len(), 5);
            assert_eq!(sheet["preview_rows"][4][0], "Row5");
            assert_eq!(sheet["preview_truncated"], true);
        }
        let md = v["markdown"]["content"].as_str().unwrap();
        assert!(
            md.contains("## Sheet: One") && md.contains("## Sheet: Two"),
            "{md}"
        );
        assert!(!md.contains("Row6"), "{md}");
    }
}

#[test]
fn epub_markdown_preserves_emphasis_and_spine_order() {
    let r = inspect(&fixture("native.epub"), &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["markdown"]["basis"], "native_structure");
    let md = v["markdown"]["content"].as_str().unwrap();
    assert!(md.contains("First *chapter*"), "{md}");
    assert!(md.find("First").unwrap() < md.find("Second").unwrap());
    assert!(!md.contains("never execute"));
}

#[test]
fn spreadsheet_markdown_contains_sheet_headings_headers_and_rows() {
    for name in ["grid.xlsx", "grid.ods"] {
        let r = inspect(&fixture(name), &InspectOptions::default());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let v = serde_json::to_value(&r).unwrap();
        let md = v["markdown"]["content"].as_str().unwrap();
        assert_eq!(v["markdown"]["basis"], "native_structure");
        assert!(md.contains("## Sheet: Notes"), "{md}");
        assert!(md.contains("| ID | Notes |"), "{md}");
        assert!(md.contains("| --- | --- |"), "{md}");
        assert!(md.contains("We can inspect the original source"), "{md}");
        assert_eq!(
            v["sheets"].as_array().unwrap().last().unwrap()["headers_preview"][0],
            "ID"
        );
    }
}

#[test]
fn odf_markdown_preserves_native_tables_and_slide_boundaries() {
    for name in ["tables.odt", "tables.odp"] {
        let r = inspect(&fixture(name), &InspectOptions::default());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let v = serde_json::to_value(&r).unwrap();
        let md = v["markdown"]["content"].as_str().unwrap();
        assert_eq!(v["markdown"]["basis"], "native_structure");
        assert!(md.contains("|"), "{md}");
        assert!(md.contains("Nested evidence."), "{md}");
        if name.ends_with("odp") {
            assert!(md.contains("## Slide 2"), "{md}");
        }
    }
}

#[test]
fn presentation_markdown_keeps_slide_order_and_tables() {
    let r = inspect(&fixture("tables.pptx"), &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let md = v["markdown"]["content"].as_str().unwrap();
    assert_eq!(v["markdown"]["basis"], "native_structure");
    assert!(md.contains("## Slide 1"), "{md}");
    assert!(md.contains("| Column 1 | Column 2 | Column 3 |"), "{md}");
    assert!(md.contains("Bottom"), "{md}");
}

#[test]
fn pdf_markdown_uses_layout_projection_and_page_boundaries() {
    let r = inspect(&fixture("native.pdf"), &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["markdown"]["basis"], "inferred_layout");
    let md = v["markdown"]["content"].as_str().unwrap();
    assert!(md.contains("## Page 1"), "{md}");
    assert!(md.contains("## Page 2"), "{md}");
    assert!(md.contains("Native PDF"), "{md}");
}

#[test]
fn word_markdown_preserves_heading_list_table_and_section_order() {
    let path = fixture("../target/tests").join(format!(
        "structured-{}.docx",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, xml) in [
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Evidence</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First finding</w:t></w:r></w:p>
<w:tbl><w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:p><w:r><w:t>Source</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Value</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>A|B</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>42</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
<w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>Section end</w:t></w:r></w:p>
<w:p><w:r><w:t>Next section</w:t></w:r></w:p>
</w:body></w:document>"#,
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let md = v["markdown"]["content"]
        .as_str()
        .expect("first-class Markdown");
    assert!(md.contains("# Evidence\n"), "{md}");
    assert!(md.contains("- First finding"), "{md}");
    assert!(md.contains("| Source | Value |"), "{md}");
    assert!(md.contains("| --- | --- |"), "{md}");
    assert!(md.contains(r"| A\|B | 42 |"), "{md}");
    assert!(md.find("First finding").unwrap() < md.find("| Source").unwrap());
    assert!(md.find("| Source").unwrap() < md.find("Section end").unwrap());
    assert!(md.contains("\n---\n"), "{md}");
    assert_eq!(v["markdown"]["basis"], "native_structure");
}
