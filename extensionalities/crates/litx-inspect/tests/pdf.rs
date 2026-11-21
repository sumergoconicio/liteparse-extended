use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
#[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
fn identify_pdf_samples_five_pages_and_distinguishes_text_layers_from_scans() {
    let r = inspect(
        &fixture("multipage.pdf"),
        &InspectOptions {
            identify: true,
            max_tokens: Some(10_000),
            max_chars: 100_000,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.pages, Some(6));
    assert_eq!(r.preflight.len(), 5);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["pdf"]["digital_signal"], "mixed");
    assert_eq!(v["pdf"]["preview_complete"], true);
    assert_eq!(v["pdf"]["text_complete"], false);
    let pages = v["pdf"]["identification_pages"].as_array().unwrap();
    assert_eq!(pages.len(), 5);
    assert_eq!(pages[0]["text_layer"], "digital");
    assert_eq!(pages[1]["text_layer"], "scanned");
    assert_eq!(pages[1]["ocr_status"], "no_text_detected");
    assert_eq!(pages[2]["text_layer"], "digital");
    assert_eq!(pages[2]["ocr_status"], "not_needed");
    assert_eq!(pages[3]["text_layer"], "no_text_layer");
    assert!(
        !r.text
            .content
            .contains("Final page confirms full-document extraction.")
    );
    assert_eq!(r.pdf.unwrap().geometry.len(), 5);
}

#[test]
fn native_pdf_keeps_full_page_counts_and_normalizes_upstream_preflight() {
    let r = inspect(
        &fixture("native.pdf"),
        &InspectOptions {
            max_chars: 12,
            max_units: 2,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.pages, Some(2));
    assert!(r.quick_look().contains("2 pages"));
    assert_eq!(r.text.chars, 12);
    assert!(r.text.content.starts_with("Native PDF"));
    assert!(r.text.truncated);
    assert!(r.counts.chars > 12);
    assert_eq!(r.preflight.len(), 2);
    assert!(r.preflight[0].needs_ocr);
    assert_eq!(r.preflight[0].reasons, ["sparse-text"]);
    assert!(r.preflight[1].needs_ocr);
    assert!(r.preflight[1].coverage_available);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["pdf"]["complexity"][1]["full_page_image"], true);
    assert_eq!(v["pdf"]["text_complete"], true);
    assert!(
        r.claims.iter().any(|c| c.key == "title"
            && c.value == "Native PDF"
            && c.source == "container_metadata")
    );
}

#[test]
fn pdf_thumbnail_is_opt_in_bounded_and_refuses_to_overwrite() {
    let path = fixture("../target/tests").join(format!(
        "pdf-preview-{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let options = InspectOptions {
        thumbnail: Some(path.clone()),
        thumbnail_max_edge: 160,
        max_units: 0,
        ..Default::default()
    };
    let r = inspect(&fixture("native.pdf"), &options);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let thumb = r.thumbnail.unwrap();
    assert_eq!((thumb.width, thumb.height), (120, 160));
    assert_eq!(thumb.basis, "rendered");
    assert_eq!(r.counts.pages, Some(2));
    assert!(r.preflight.is_empty());
    let first = std::fs::read(&path).unwrap();
    let repeated = inspect(&fixture("native.pdf"), &options);
    assert!(repeated.errors.iter().any(|e| e.stage == "thumbnail"));
    assert_eq!(first, std::fs::read(path).unwrap());
}

/// Renders the native fixture's first page to a JPEG text image and returns
/// the encoded bytes with pixel dimensions.
fn text_jpeg(directory: &std::path::Path) -> (Vec<u8>, u32, u32) {
    let rendered = directory.join("text-source.png");
    let r = inspect(
        &fixture("native.pdf"),
        &InspectOptions {
            thumbnail: Some(rendered.clone()),
            thumbnail_max_edge: 1200,
            max_units: 0,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let decoded = image::open(&rendered).unwrap().to_rgb8();
    let (width, height) = (decoded.width(), decoded.height());
    let mut jpeg = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(decoded)
        .write_to(&mut jpeg, image::ImageFormat::Jpeg)
        .unwrap();
    (jpeg.into_inner(), width, height)
}

/// Builds a synthetic PDF from per-page specs: `Some(text)` draws a native
/// Helvetica text line, `None` draws the shared JPEG as a full-page scan.
fn synthetic_pdf(
    output: &std::path::Path,
    pages: &[Option<String>],
    jpeg: Option<&(Vec<u8>, u32, u32)>,
) {
    std::fs::write(output, build_pdf(pages, jpeg, b"", false)).unwrap();
}

/// Assemble the synthetic PDF bytes. `prefix` prepends raw bytes ahead of
/// the header (xref offsets are computed after it, so the file stays valid);
/// `dangling_kid` appends a page-tree kid reference to a missing object so
/// that page's extraction fails.
fn build_pdf(
    pages: &[Option<String>],
    jpeg: Option<&(Vec<u8>, u32, u32)>,
    prefix: &[u8],
    dangling_kid: bool,
) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        Vec::new(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    let mut image_object = 0;
    if let Some((bytes, width, height)) = jpeg {
        objects.push(format!("<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", bytes.len()).into_bytes());
        image_object = objects.len();
        objects[image_object - 1].extend_from_slice(bytes);
        objects[image_object - 1].extend_from_slice(b"\nendstream");
    }
    let mut kids = Vec::new();
    for spec in pages {
        let (content, resources) = match (spec, jpeg) {
            (Some(text), _) => (
                format!("BT /F1 20 Tf 50 700 Td ({text}) Tj ET").into_bytes(),
                "<< /Font << /F1 3 0 R >> >>".to_string(),
            ),
            (None, Some(_)) => (
                b"q 600 0 0 800 0 0 cm /Scan Do Q".to_vec(),
                format!("<< /XObject << /Scan {image_object} 0 R >> >>"),
            ),
            (None, None) => (Vec::new(), "<< >>".to_string()),
        };
        objects.push(format!("<< /Length {} >>\nstream\n", content.len()).into_bytes());
        let content_object = objects.len();
        objects[content_object - 1].extend_from_slice(&content);
        objects[content_object - 1].extend_from_slice(b"\nendstream");
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources {resources} /Contents {content_object} 0 R >>").into_bytes());
        kids.push(objects.len());
    }
    if dangling_kid {
        kids.push(999);
    }
    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.iter()
            .map(|n| format!("{n} 0 R"))
            .collect::<Vec<_>>()
            .join(" "),
        kids.len()
    )
    .into_bytes();
    let mut data = prefix.to_vec();
    data.extend_from_slice(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n");
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
    data
}

#[test]
fn page_extraction_error_marks_the_text_payload_partial() {
    // LITX-AUD-014: a page whose extraction fails must mark the text payload
    // partial (text.truncated), not only pdf.text_complete.
    let directory = scratch("extract-error");
    let path = directory.join("broken.pdf");
    std::fs::write(
        &path,
        build_pdf(&[Some("First page text".to_string())], None, b"", true),
    )
    .unwrap();
    let r = inspect(
        &path,
        &InspectOptions {
            max_units: 0,
            ..Default::default()
        },
    );
    assert_eq!(r.counts.pages, Some(2), "{:?}", r.errors);
    assert!(
        r.errors.iter().any(|e| e.stage == "pdf_text"),
        "{:?}",
        r.errors
    );
    assert!(!r.pdf.as_ref().unwrap().text_complete);
    assert!(r.text.truncated);
}

#[test]
fn space_prefixed_pdf_bytes_inspect_without_conversion_fallback() {
    // LITX-AUD-014: the default (!identify, max_units > 0) path must compute
    // complexity from the byte-verified document it already parsed, never
    // through the upstream conversion fallback. A 16-space prefix defeats the
    // detector, so extension routing (.pdf) delivers the file to the PDF
    // inspector, where the upstream byte sniff would misread the spaced
    // payload as ".bin" and fail conversion (the audit's live repro).
    let directory = scratch("prefixed-pdf");
    let path = directory.join("prefixed.pdf");
    std::fs::write(
        &path,
        build_pdf(
            &[Some("Hello native text".to_string())],
            None,
            b"                ",
            false,
        ),
    )
    .unwrap();
    let r = inspect(
        &path,
        &InspectOptions {
            max_units: 8,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.pages, Some(1));
    assert!(r.text.content.contains("Hello native text"));
    assert_eq!(r.pdf.as_ref().unwrap().complexity.len(), 1);
    assert_eq!(r.preflight.len(), 1);
}

fn scratch(tag: &str) -> PathBuf {
    let directory = fixture("../target/tests").join(format!(
        "pdf-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
#[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
fn pure_scan_pdf_reports_ocr_provenance() {
    // LITX-AUD-019: a PDF whose text comes only from OCR must say so.
    let directory = scratch("pure-scan");
    let path = directory.join("scan.pdf");
    synthetic_pdf(&path, &[None, None], Some(&text_jpeg(&directory)));
    let r = inspect(
        &path,
        &InspectOptions {
            identify: true,
            max_pages: None,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.text.content.contains("Native PDF"));
    assert_eq!(r.text.origin, "ocr");
    assert_eq!(r.markdown.basis, "ocr_text_fallback");
    let pdf = r.pdf.as_ref().unwrap();
    assert!(pdf.text_complete);
    for page in &pdf.identification_pages {
        assert_eq!(page.text_layer, "scanned");
        assert_eq!(page.ocr_status, "text_detected");
    }
}

#[test]
#[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
fn mixed_pdf_reports_native_and_ocr_provenance() {
    // LITX-AUD-019: native text pages plus OCR'd scans are native_and_ocr.
    let directory = scratch("mixed");
    let path = directory.join("mixed.pdf");
    let dense = |page: usize| {
        format!("Native page {page} provides digital evidence for the mixed provenance check. ")
            .repeat(9)
    };
    let pages = [Some(dense(1)), None, Some(dense(3)), None];
    synthetic_pdf(&path, &pages, Some(&text_jpeg(&directory)));
    let r = inspect(
        &path,
        &InspectOptions {
            identify: true,
            max_pages: None,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(
        r.text.content.contains("Native page 1"),
        "{}",
        r.text.content
    );
    assert!(r.text.content.contains("Native PDF"));
    assert_eq!(r.text.origin, "native_and_ocr");
    assert_eq!(r.markdown.basis, "mixed");
    let pdf = r.pdf.as_ref().unwrap();
    assert_eq!(pdf.identification_pages[0].text_layer, "digital");
    assert_eq!(pdf.identification_pages[0].ocr_status, "not_needed");
    assert_eq!(pdf.identification_pages[1].text_layer, "scanned");
    assert_eq!(pdf.identification_pages[1].ocr_status, "text_detected");
}

#[test]
#[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
fn pdf_ocr_continues_beyond_page_five() {
    // LITX-AUD-019: full extraction OCRs every scanned page, not just the
    // first identification preview window.
    let directory = scratch("seven");
    let path = directory.join("seven.pdf");
    synthetic_pdf(&path, &vec![None; 7], Some(&text_jpeg(&directory)));
    let full = inspect(
        &path,
        &InspectOptions {
            identify: true,
            max_pages: None,
            ..Default::default()
        },
    );
    assert!(full.errors.is_empty(), "{:?}", full.errors);
    let pdf = full.pdf.as_ref().unwrap();
    assert!(pdf.text_complete);
    assert_eq!(pdf.identification_pages.len(), 7);
    for page in &pdf.identification_pages {
        assert_eq!(page.ocr_status, "text_detected");
    }
    assert!(full.markdown.content.contains("## Page 7"));
    let preview = inspect(
        &path,
        &InspectOptions {
            identify: true,
            ..Default::default()
        },
    );
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    let pdf = preview.pdf.as_ref().unwrap();
    assert!(!pdf.text_complete);
    assert_eq!(pdf.identification_pages.len(), 5);
    assert!(preview.text.truncated);
}

#[test]
fn byte_verified_pdf_works_inside_an_existing_async_runtime() {
    let path = fixture("../target/tests").join(format!(
        "renamed-pdf-{}.bin",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, std::fs::read(fixture("native.pdf")).unwrap()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let r = runtime.block_on(async {
        inspect(
            &path,
            &InspectOptions {
                max_units: 0,
                ..Default::default()
            },
        )
    });
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.pages, Some(2));
    assert!(r.preflight.is_empty());
    assert!(r.pdf.unwrap().text_complete);
    assert!(r.text.content.contains("Native PDF"));
    let broken = inspect(&fixture("broken.pdf"), &Default::default());
    assert!(broken.errors.iter().any(|e| e.stage == "pdf"));
}

#[test]
fn at_least_five_pages_are_checked_while_full_six_page_counts_survive_preview_limits() {
    let r = inspect(
        &fixture("multipage.pdf"),
        &InspectOptions {
            max_chars: 12,
            max_pages: None,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(r.counts.pages, Some(6));
    assert_eq!(r.preflight.len(), 5);
    for (index, page) in r.preflight.iter().enumerate() {
        assert_eq!(page.page_number, index + 1);
        assert!(page.coverage_available);
    }
    assert_eq!(r.preflight[1].reasons, ["scanned"]);
    assert!(!r.preflight[2].needs_ocr);
    assert_eq!(r.preflight[3].reasons, ["no-text"]);
    assert_eq!(r.text.chars, 12);
    assert!(r.text.truncated);
    let pdf = r.pdf.as_ref().unwrap();
    assert!(pdf.text_complete);
    assert_eq!(pdf.geometry.len(), 6);
    assert_eq!(pdf.complexity.len(), 5);
    assert_eq!(pdf.geometry[5].width_points, 800.0);
    assert_eq!(pdf.geometry[5].height_points, 600.0);
    let full = inspect(
        &fixture("multipage.pdf"),
        &InspectOptions {
            max_chars: 4096,
            max_pages: None,
            max_units: usize::MAX,
            ..Default::default()
        },
    );
    assert!(full.errors.is_empty(), "{:?}", full.errors);
    assert_eq!(full.preflight.len(), 6);
    assert_eq!(r.counts.chars, full.counts.chars);
    assert!(
        full.text
            .content
            .contains("Final page confirms full-document extraction.")
    );
}
