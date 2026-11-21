use litx_inspect::{InspectOptions, inspect};
use std::{io::Write, path::PathBuf};

fn archive(extension: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "archive-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("sample.{extension}"));
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, bytes) in entries {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
    path
}

#[test]
fn word_preview_stops_after_five_sections_and_full_extract_reads_all() {
    let paragraphs: String = (1..=6)
        .map(|i| {
            let section = if i < 6 {
                "<w:pPr><w:sectPr/></w:pPr>"
            } else {
                ""
            };
            format!("<w:p>{section}<w:r><w:t>Section{i}</w:t></w:r></w:p>")
        })
        .collect();
    let xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{paragraphs}<w:sectPr/></w:body></w:document>"#
    );
    let path = archive("docx", &[
        ("[Content_Types].xml", br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
        ("word/document.xml", xml.as_bytes()),
    ]);
    let preview = inspect(&path, &InspectOptions::default());
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    assert_eq!(preview.counts.paragraphs, Some(6));
    assert!(preview.text.content.contains("Section5"));
    assert!(!preview.text.content.contains("Section6"));
    assert!(!preview.markdown.content.contains("Section6"));
    assert!(preview.text.truncated);
    let full = inspect(
        &path,
        &InspectOptions {
            max_pages: None,
            ..Default::default()
        },
    );
    assert!(full.errors.is_empty(), "{:?}", full.errors);
    assert!(full.text.content.contains("Section6"));
    assert!(full.markdown.content.contains("Section6"));
    assert!(!full.text.truncated);
}

#[test]
fn odf_document_previews_stop_at_five_units_without_limiting_full_text() {
    for (ext, mime, body, unit) in [
        ("odp", "presentation", "presentation", "draw:page"),
        ("odt", "text", "text", "text:section"),
    ] {
        let units: String = (1..=6)
            .map(|i| format!("<{unit}><text:p>Unit{i}</text:p></{unit}>"))
            .collect();
        let content = format!(
            r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"><office:body><office:{body}>{units}</office:{body}></office:body></office:document-content>"#
        );
        let mime = format!("application/vnd.oasis.opendocument.{mime}");
        let path = archive(
            ext,
            &[
                ("mimetype", mime.as_bytes()),
                ("content.xml", content.as_bytes()),
            ],
        );
        let preview = inspect(&path, &InspectOptions::default());
        assert!(preview.errors.is_empty(), "{ext}: {:?}", preview.errors);
        assert!(preview.text.content.contains("Unit5"));
        assert!(!preview.text.content.contains("Unit6"), "{ext}");
        assert!(!preview.markdown.content.contains("Unit6"), "{ext}");
        assert!(preview.text.truncated);
        let full = inspect(
            &path,
            &InspectOptions {
                max_pages: None,
                ..Default::default()
            },
        );
        assert!(full.errors.is_empty(), "{ext}: {:?}", full.errors);
        assert!(full.text.content.contains("Unit6"), "{ext}");
        assert!(!full.text.truncated);
    }
}

#[test]
fn presentation_preview_reads_five_slides_and_full_extract_reads_all() {
    let mut ids = String::new();
    let mut rels = String::new();
    let mut entries = vec![(
        "[Content_Types].xml".to_string(),
        br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/></Types>"#.to_vec(),
    )];
    for index in 1..=6 {
        ids.push_str(&format!(r#"<p:sldId id="{index}" r:id="rId{index}"/>"#));
        rels.push_str(&format!(r#"<Relationship Id="rId{index}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/s{index}.xml"/>"#));
        entries.push((
            format!("ppt/slides/s{index}.xml"),
            format!(r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:p><a:r><a:t>Slide{index}</a:t></a:r></a:p></p:sld>"#).into_bytes(),
        ));
    }
    entries.push(("ppt/presentation.xml".to_string(), format!(r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>"#).into_bytes()));
    entries.push(("ppt/_rels/presentation.xml.rels".to_string(), format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#).into_bytes()));
    let entries: Vec<_> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let path = archive("pptx", &entries);
    let preview = inspect(&path, &InspectOptions::default());
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    assert_eq!(preview.counts.slides, Some(6));
    assert!(preview.text.content.contains("Slide5"));
    assert!(!preview.text.content.contains("Slide6"));
    assert!(!preview.markdown.content.contains("Slide6"));
    assert!(preview.text.truncated);
    let full = inspect(
        &path,
        &InspectOptions {
            max_pages: None,
            ..Default::default()
        },
    );
    assert!(full.errors.is_empty(), "{:?}", full.errors);
    assert!(full.text.content.contains("Slide6"));
    assert!(!full.text.truncated);
}

#[test]
fn epub_preview_reads_five_spine_items_and_full_extract_reads_all() {
    let mut manifest = String::new();
    let mut spine = String::new();
    let mut entries = vec![
        ("mimetype".to_string(), b"application/epub+zip".to_vec()),
        (
            "META-INF/container.xml".to_string(),
            br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.to_vec(),
        ),
    ];
    for index in 1..=6 {
        manifest.push_str(&format!(
            r#"<item id="c{index}" href="c{index}.xhtml" media-type="application/xhtml+xml"/>"#
        ));
        spine.push_str(&format!(r#"<itemref idref="c{index}"/>"#));
        entries.push((
            format!("c{index}.xhtml"),
            format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Chapter{index}</p></body></html>"#).into_bytes(),
        ));
    }
    entries.push((
        "book.opf".to_string(),
        format!(r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata/><manifest>{manifest}</manifest><spine>{spine}</spine></package>"#).into_bytes(),
    ));
    let entries: Vec<_> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let path = archive("epub", &entries);
    let preview = inspect(&path, &InspectOptions::default());
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    assert_eq!(preview.counts.spine_items, Some(6));
    assert!(preview.text.content.contains("Chapter5"));
    assert!(!preview.text.content.contains("Chapter6"));
    assert!(!preview.markdown.content.contains("Chapter6"));
    assert!(preview.text.truncated);
    let full = inspect(
        &path,
        &InspectOptions {
            max_pages: None,
            ..Default::default()
        },
    );
    assert!(full.errors.is_empty(), "{:?}", full.errors);
    assert!(full.text.content.contains("Chapter6"));
    assert!(full.markdown.content.contains("Chapter6"));
    assert!(!full.text.truncated);
}

#[test]
fn cbz_skips_macos_metadata_without_dropping_real_pages() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(64, 80)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let path = archive(
        "cbz",
        &[
            ("__MACOSX/cover.png", b"macOS metadata"),
            ("nested/__MACOSX/cover.png", b"macOS metadata"),
            ("._cover.png", b"AppleDouble metadata"),
            ("nested/._page.png", b"AppleDouble metadata"),
            (".DS_Store", b"Finder metadata"),
            ("cover.png", png.get_ref()),
            ("pages/02.png", png.get_ref()),
            ("pages/__MACOSXish.png", png.get_ref()),
        ],
    );
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.entries, Some(8));
    assert_eq!(r.counts.pages, Some(3));
    assert_eq!(
        r.pages
            .iter()
            .map(|p| p.source_part.as_str())
            .collect::<Vec<_>>(),
        ["cover.png", "pages/02.png", "pages/__MACOSXish.png"]
    );
    assert_eq!(r.pages[0].width, Some(64));
    assert_eq!(r.pages[2].height, Some(80));
    assert!(
        r.claims
            .iter()
            .any(|c| c.key == "cover_entry" && c.value == "cover.png")
    );
}

#[test]
fn zip_markdown_lists_file_count_and_names_without_reading_contents() {
    let path = archive(
        "zip",
        &[
            ("one.txt", b"PRIVATE_CONTENT"),
            ("folder/two.md", b"ALSO_PRIVATE"),
        ],
    );
    let r = inspect(&path, &InspectOptions::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.entries, Some(2));
    assert!(
        r.markdown.content.contains("Entries: 2"),
        "{}",
        r.markdown.content
    );
    assert!(r.markdown.content.contains("one.txt"));
    assert!(r.markdown.content.contains("folder/two.md"));
    assert!(!r.markdown.content.contains("PRIVATE"));
    assert!(r.text.content.is_empty());
}

#[test]
fn iwork_pdf_is_extracted_as_an_explicitly_partial_derived_preview() {
    let bytes =
        std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/native.pdf"))
            .unwrap();
    for extension in ["pages", "key"] {
        let path = archive(
            extension,
            &[
                ("Index/Document.iwa", b"NOT_DECODED"),
                ("QuickLook/Preview.pdf", &bytes),
            ],
        );
        let r = inspect(&path, &InspectOptions::default());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r.text.content.contains("Native PDF"), "{}", r.text.content);
        let value = serde_json::to_value(&r).unwrap();
        assert_eq!(
            value["derived_preview"]["source_part"],
            "QuickLook/Preview.pdf"
        );
        assert_eq!(value["derived_preview"]["partial"], true);
        assert!(
            r.markdown.content.contains("Partial iWork preview"),
            "{}",
            r.markdown.content
        );
        assert!(r.markdown.truncated);
    }
}

#[test]
fn numbers_pdf_preview_extracts_text_but_stays_within_one_page() {
    let bytes =
        std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/native.pdf"))
            .unwrap();
    let path = archive(
        "numbers",
        &[
            ("Index/Document.iwa", b"NOT_DECODED"),
            ("QuickLook/Preview.pdf", &bytes),
        ],
    );
    let r = inspect(
        &path,
        &InspectOptions {
            max_pages: None,
            max_chars: 64 << 20,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(
        r.text.content.contains("Native PDF"),
        "Numbers should expose bounded preview text: {}",
        r.text.content
    );
    assert!(
        r.markdown.content.contains("Bounded embedded preview"),
        "{}",
        r.markdown.content
    );
    assert_eq!(r.pdf.unwrap().geometry.len(), 1);
    assert!(r.text.chars <= 4000);
    assert!(r.derived_preview.unwrap().partial);
}

#[test]
fn iwork_image_preview_retains_provenance_and_preserves_thumbnail_bytes() {
    let bytes =
        std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/image.png"))
            .unwrap();
    let path = archive(
        "pages",
        &[
            ("Index/Document.iwa", b"NOT_DECODED"),
            ("preview.png", &bytes),
        ],
    );
    let output = path.with_extension("thumbnail.png");
    let r = inspect(
        &path,
        &InspectOptions {
            thumbnail: Some(output.clone()),
            thumbnail_max_edge: 1024,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let value = serde_json::to_value(&r).unwrap();
    assert_eq!(value["derived_preview"]["source_part"], "preview.png");
    assert_eq!(value["derived_preview"]["partial"], true);
    assert_eq!(r.image.unwrap().width, 640);
    assert_eq!(r.thumbnail.unwrap().basis, "embedded");
    assert_eq!(std::fs::read(output).unwrap(), bytes);
    assert!(r.markdown.content.contains("Partial iWork preview"));
}

/// Builds a synthetic full-page-scan PDF whose only content is one JPEG
/// image per page, so native text extraction finds nothing and OCR runs.
fn scanned_pdf(output: &std::path::Path, text_image: &std::path::Path, pages: usize) {
    let decoded = image::open(text_image).unwrap().to_rgb8();
    let (width, height) = (decoded.width(), decoded.height());
    let mut jpeg = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(decoded)
        .write_to(&mut jpeg, image::ImageFormat::Jpeg)
        .unwrap();
    let jpeg = jpeg.into_inner();
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        Vec::new(),
        format!("<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", jpeg.len()).into_bytes(),
    ];
    objects[2].extend_from_slice(&jpeg);
    objects[2].extend_from_slice(b"\nendstream");
    let mut kids = Vec::new();
    for _ in 0..pages {
        let content = b"q 600 0 0 800 0 0 cm /Scan Do Q".to_vec();
        objects.push(format!("<< /Length {} >>\nstream\n", content.len()).into_bytes());
        let content_object = objects.len();
        objects[content_object - 1].extend_from_slice(&content);
        objects[content_object - 1].extend_from_slice(b"\nendstream");
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /XObject << /Scan 3 0 R >> >> /Contents {content_object} 0 R >>").into_bytes());
        kids.push(objects.len());
    }
    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.iter()
            .map(|n| format!("{n} 0 R"))
            .collect::<Vec<_>>()
            .join(" ")
    )
    .into_bytes();
    let mut data = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = vec![0];
    for (n, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend(format!("{} 0 obj\n", n + 1).into_bytes());
        data.extend(object);
        data.extend(b"\nendobj\n");
    }
    let xref = data.len();
    data.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).into_bytes());
    for offset in &offsets[1..] {
        data.extend(format!("{offset:010} 00000 n \n").into_bytes());
    }
    data.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .into_bytes(),
    );
    std::fs::write(output, data).unwrap();
}

#[test]
fn zip_thumbnail_falls_back_only_when_no_thumbnail_was_written() {
    // LITX-AUD-012: archives whose cover already produced a thumbnail must
    // not fail a second write attempt for docProps/thumbnail.jpeg.
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(64, 80)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    for (extension, entries) in [
        (
            "cbz",
            vec![
                ("ComicInfo.xml", &b"<ComicInfo/>"[..]),
                ("cover.png", png.get_ref().as_slice()),
                ("docProps/thumbnail.jpeg", png.get_ref().as_slice()),
            ],
        ),
        (
            "epub",
            vec![
                ("mimetype", &b"application/epub+zip"[..]),
                ("META-INF/container.xml", br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
                ("book.opf", br#"<package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><meta name="cover" content="cover"/></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="cover" href="images/cover.png" media-type="image/png"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
                ("c1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Chapter one.</p></body></html>"#),
                ("images/cover.png", png.get_ref().as_slice()),
                ("docProps/thumbnail.jpeg", png.get_ref().as_slice()),
            ],
        ),
    ] {
        let path = archive(extension, &entries);
        let output = path.with_extension("thumb.png");
        let r = inspect(
            &path,
            &InspectOptions {
                thumbnail: Some(output.clone()),
                ..Default::default()
            },
        );
        assert!(
            !r.errors.iter().any(|e| e.stage == "thumbnail"),
            "{extension}: {:?}",
            r.errors
        );
        let thumbnail = r.thumbnail.as_ref().unwrap_or_else(|| {
            panic!("{extension}: thumbnail missing: {:?}", r.errors)
        });
        assert_eq!(thumbnail.written_to, output);
    }
}

#[test]
fn iwork_image_preview_without_ocr_text_is_not_labeled_ocr() {
    // LITX-AUD-019: a blank embedded preview image yields no OCR text, so the
    // Markdown provenance must not claim an OCR fallback.
    let bytes =
        std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/image.png"))
            .unwrap();
    let path = archive("pages", &[("preview.png", &bytes)]);
    let r = inspect(&path, &Default::default());
    assert!(
        !r.errors.iter().any(|e| e.stage == "iwork_preview"),
        "{:?}",
        r.errors
    );
    assert!(r.markdown.content.contains("Partial iWork preview"));
    assert_ne!(r.markdown.basis, "ocr_text_fallback");
}

#[test]
#[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
fn iwork_preview_provenance_follows_the_actual_ocr_outcome() {
    // LITX-AUD-019: OCR'd embedded image and pure-scan PDF previews are
    // labeled ocr_text_fallback, never mixed.
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "iwork-ocr-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let rendered = directory.join("text.png");
    let pdf = inspect(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/native.pdf"),
        &InspectOptions {
            thumbnail: Some(rendered.clone()),
            thumbnail_max_edge: 1200,
            max_units: 0,
            ..Default::default()
        },
    );
    assert!(pdf.errors.is_empty(), "{:?}", pdf.errors);
    let image_path = archive(
        "pages",
        &[("preview.png", &std::fs::read(&rendered).unwrap())],
    );
    let image = inspect(
        &image_path,
        &InspectOptions {
            identify: true,
            ..Default::default()
        },
    );
    assert!(
        image.text.content.contains("Native PDF"),
        "{:?}",
        image.errors
    );
    assert_eq!(image.markdown.basis, "ocr_text_fallback");
    let scanned = directory.join("scanned.pdf");
    scanned_pdf(&scanned, &rendered, 2);
    let pdf_path = archive(
        "key",
        &[
            ("Index/Document.iwa", b"NOT_DECODED"),
            ("QuickLook/Preview.pdf", &std::fs::read(&scanned).unwrap()),
        ],
    );
    let preview = inspect(
        &pdf_path,
        &InspectOptions {
            identify: true,
            ..Default::default()
        },
    );
    assert!(
        preview.text.content.contains("Native PDF"),
        "{:?}",
        preview.errors
    );
    assert_eq!(preview.markdown.basis, "ocr_text_fallback");
}

#[test]
fn iwork_text_preview_is_bounded_and_labeled_as_derived_text() {
    let bytes = "preview words ".repeat(1000);
    let path = archive("pages", &[("QuickLook/Preview.txt", bytes.as_bytes())]);
    let r = inspect(
        &path,
        &InspectOptions {
            max_chars: 64 << 20,
            max_pages: None,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.text.content.starts_with("preview words"));
    assert_eq!(r.text.chars, 4000);
    assert_eq!(r.derived_preview.unwrap().content_type, "text/plain");
    assert!(r.markdown.truncated);
}
