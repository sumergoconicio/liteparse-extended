//! Native PDF inspection with bounded text, OCR, and Markdown recovery.
use crate::{Claim, InspectOptions, InspectReport, Preflight, binary::Source, xml};
use liteparse::{stages, types::PdfInput};
use serde::Serialize;
use std::path::Path;

/// LITX-AUD-010(b): the native page hard cap is always enforced; tests inject
/// a smaller seam so the cap path runs without a 4097-page document.
#[cfg(not(test))]
const MAX_PAGES: usize = 4096;
#[cfg(test)]
const MAX_PAGES: usize = 8;
const MAX_PREFLIGHT: usize = 64;
/// LITX-AUD-014: cumulative cap on decoded native + OCR text bytes. The test
/// seam exercises the cap path without allocating a 64 MiB document.
#[cfg(not(test))]
const MAX_TEXT_BYTES: usize = 64 << 20;
#[cfg(test)]
const MAX_TEXT_BYTES: usize = 32;
/// LITX-AUD-014 follow-up: aggregate budget for OCR rasters retained while
/// the PDFium lock is held so probes can run unlocked. Peak retained memory
/// is this budget plus the single transient render being charged (each
/// render is already bounded individually by the 2048px DPI normalization).
#[cfg(not(test))]
const MAX_OCR_STAGING_BYTES: usize = 128 << 20;
#[cfg(test)]
const MAX_OCR_STAGING_BYTES: usize = 1 << 20;

/// Charge one retained raster against the shared staging budget with checked
/// arithmetic, billing the retained allocation (`Vec::capacity`), never the
/// payload length: the queue keeps the whole buffer alive. The rejected
/// raster is dropped by the caller.
fn charge_ocr_staging(remaining: &mut usize, raster: &Vec<u8>) -> Result<(), String> {
    match remaining.checked_sub(raster.capacity()) {
        Some(next) => {
            *remaining = next;
            Ok(())
        }
        None => Err(format!(
            "PDF OCR raster staging budget of {MAX_OCR_STAGING_BYTES} bytes exhausted"
        )),
    }
}

#[derive(Debug, Serialize)]
pub struct PdfAnalysis {
    pub digital_signal: &'static str,
    pub identification_pages: Vec<PdfPageSignal>,
    pub preview_complete: bool,
    pub basis: &'static str,
    pub metadata: liteparse::DocumentMetadata,
    pub geometry: Vec<PdfGeometry>,
    pub complexity: Vec<stages::PageComplexityStats>,
    pub text_complete: bool,
}

#[derive(Debug, Serialize)]
pub struct PdfPageSignal {
    pub page_number: usize,
    pub text_layer: &'static str,
    pub native_chars: usize,
    pub needs_ocr: bool,
    pub ocr_status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct PdfGeometry {
    pub page_number: usize,
    pub width_points: f32,
    pub height_points: f32,
}

pub(crate) fn inspect(
    path: &Path,
    report: &mut InspectReport,
    options: &InspectOptions,
    ocr: &mut crate::images::OcrSession,
) {
    let bytes = (|| -> xml::Result<Vec<u8>> {
        let mut source = Source::open(path)?;
        source.range(0, source.len as usize)
    })();
    match bytes {
        Ok(bytes) => inspect_bytes(bytes, report, options, ocr),
        Err(error) => report.error("pdf", error),
    }
}

pub(crate) fn inspect_bytes(
    bytes: Vec<u8>,
    report: &mut InspectReport,
    options: &InspectOptions,
    ocr: &mut crate::images::OcrSession,
) {
    // Keep the synchronous API usable from callers already inside a Tokio runtime.
    let result = std::thread::scope(|scope| {
        scope
            .spawn(|| inner(bytes, report, options, ocr).map_err(|e| e.to_string()))
            .join()
    });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => report.error("pdf", error),
        Err(_) => report.error("pdf", "upstream PDF inspection panicked"),
    }
}

/// Inspects PDF bytes with the production page-complexity implementation.
/// Accepts owned PDF bytes, the destination report, inspection limits, and the
/// reusable OCR session. Returns `Ok(())` after updating the report, or a
/// document-open/setup error; page extraction and complexity failures are
/// recorded as partial-output diagnostics in the report.
fn inner(
    bytes: Vec<u8>,
    report: &mut InspectReport,
    options: &InspectOptions,
    ocr: &mut crate::images::OcrSession,
) -> xml::Result<()> {
    inner_with_complexity(bytes, report, options, ocr, |document, page| {
        stages::page_complexity(document, page).map_err(Into::into)
    })
}

/// Inspects PDF bytes using `complexity` for per-page OCR and geometry signals.
/// Accepts the same inputs as [`inner`] plus a callback receiving an open
/// PDFium document and extracted page. Returns `Ok(())` after report assembly
/// or a document-open/setup error; page callback failures become partial-output
/// diagnostics. Tests inject a page-local failure without changing public options.
fn inner_with_complexity<F>(
    bytes: Vec<u8>,
    report: &mut InspectReport,
    options: &InspectOptions,
    ocr: &mut crate::images::OcrSession,
    complexity: F,
) -> xml::Result<()>
where
    F: Fn(
        &stages::Document<'_>,
        &liteparse::types::Page,
    ) -> xml::Result<stages::PageComplexityStats>,
{
    // Byte input prevents extension-driven conversion or subprocesses upstream.
    let input = PdfInput::Bytes(bytes);
    let scan_limit = options.max_pages.unwrap_or(MAX_PAGES).min(MAX_PAGES);
    let locked = {
        let library = stages::Library::init();
        let document = stages::open(&library, &input, None, &[])?;
        let count = document.page_count().max(0) as usize;
        report.counts.pages = Some(count);
        // LITX-AUD-014: the native default-path preflight keeps the 64-page
        // window of the upstream probe it replaces.
        if !options.identify && options.max_units > MAX_PREFLIGHT && count > MAX_PREFLIGHT {
            report.error("pdf_preflight", "PDF preflight capped at 64 pages");
        }
        for (tag, key) in [
            ("Title", "title"),
            ("Author", "author"),
            ("Creator", "creator"),
            ("Producer", "producer"),
            ("Subject", "subject"),
            ("Keywords", "keywords"),
        ] {
            if let Some(value) = document.meta_text(tag).filter(|v| !v.is_empty()) {
                report.claims.push(Claim {
                    key: key.into(),
                    value,
                    source: "container_metadata",
                });
            }
        }
        report.pdf = Some(PdfAnalysis {
            digital_signal: "not_sampled",
            identification_pages: vec![],
            preview_complete: true,
            basis: "upstream",
            metadata: stages::document_metadata(&input, &document),
            geometry: vec![],
            complexity: vec![],
            text_complete: count <= scan_limit,
        });
        // LITX-AUD-010(b): the native hard cap is reported during previews
        // and full extraction alike.
        if count > MAX_PAGES {
            report.error(
                "pdf_text",
                "PDF page scan cap exceeded; character counts are partial",
            );
        }
        let mut text_bytes = 0usize;
        let mut text_cap_exhausted = false;
        let mut extract_failed = false;
        let mut markdown_pages = Vec::new();
        // Native text per page, kept for the unlocked phase where OCR may
        // replace it before anything is appended to the report.
        let mut page_texts: Vec<(usize, String)> = Vec::new();
        // Rasters buffered while the PDFium lock is held; the OCR probes
        // themselves run after the lock is released (LITX-AUD-014). Retained
        // capacity is bounded by a shared aggregate budget; once exhausted,
        // no further rasters are rendered at all.
        let mut ocr_jobs: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut staging_budget = MAX_OCR_STAGING_BYTES;
        let mut staging_exhausted = false;
        for number in 1..=count.min(scan_limit) {
            let extracted = stages::extract(
                &document,
                &stages::ExtractRequest {
                    target_pages: Some(&[number as u32]),
                    max_pages: 1,
                    ..Default::default()
                },
            );
            let pages = match extracted {
                Ok(extracted) => extracted.pages,
                Err(error) => {
                    report.pdf.as_mut().unwrap().text_complete = false;
                    report.pdf.as_mut().unwrap().preview_complete = false;
                    extract_failed = true;
                    report.error("pdf_text", format!("page {number}: {error}"));
                    continue;
                }
            };
            // LITX-AUD-014: the default (!identify, max_units > 0) preflight
            // computes complexity from the document this inspection already
            // parsed — no second parse, no conversion fallback.
            let preflight_window = !options.identify
                && options.max_units > 0
                && number <= options.max_units.min(MAX_PREFLIGHT);
            let stats = if options.identify || preflight_window {
                match pages
                    .first()
                    .map(|page| complexity(&document, page))
                    .transpose()
                {
                    Ok(stats) => stats,
                    Err(error) => {
                        let pdf = report.pdf.as_mut().unwrap();
                        pdf.text_complete = false;
                        pdf.preview_complete = false;
                        extract_failed = true;
                        report.error("pdf_text", format!("page {number}: {error}"));
                        None
                    }
                }
            } else {
                None
            };
            // Markdown projection has the same plain text as grid projection.
            // Project once, then charge each page before cloning or retaining it.
            let projected_pages = stages::project_for_markdown(pages);
            let mut stop = false;
            for page in projected_pages {
                report.pdf.as_mut().unwrap().geometry.push(PdfGeometry {
                    page_number: number,
                    width_points: page.page_width,
                    height_points: page.page_height,
                });
                text_bytes = text_bytes.saturating_add(page.text.len());
                if text_bytes > MAX_TEXT_BYTES {
                    report.pdf.as_mut().unwrap().text_complete = false;
                    report.error(
                        "pdf_text",
                        "PDF cumulative text cap exceeded; character counts are partial",
                    );
                    text_cap_exhausted = true;
                    stop = true;
                    break;
                }
                let text = page.text.clone();
                if let Some(stats) = stats.as_ref() {
                    let native_chars = text.chars().filter(|c| !c.is_whitespace()).count();
                    let text_layer = match (native_chars > 0, stats.full_page_image) {
                        (true, false) => "digital",
                        (true, true) => "scan_with_text_layer",
                        (false, true) => "scanned",
                        (false, false) => "no_text_layer",
                    };
                    let mut ocr_status = "not_needed";
                    if options.identify {
                        if stats.needs_ocr && staging_exhausted {
                            // The shared staging budget is spent: do not render
                            // further rasters that would only be rejected.
                            ocr_status = "failed";
                        } else if stats.needs_ocr {
                            let raster = (|| -> xml::Result<Vec<u8>> {
                                let source_page = document.page((number - 1) as i32)?;
                                let edge = source_page.width().max(source_page.height())
                                    * source_page.user_unit();
                                if !edge.is_finite() || edge <= 0.0 {
                                    return Err("invalid PDF OCR geometry".into());
                                }
                                let rendered = stages::screenshots(
                                    &document,
                                    Some(&[number as u32]),
                                    &stages::ScreenshotOptions {
                                        dpi: (72.0 * 2048.0 / edge).min(150.0),
                                        detect_rects: false,
                                        render_form_fields: false,
                                        continue_on_page_error: false,
                                    },
                                )?;
                                Ok(rendered
                                    .into_iter()
                                    .next()
                                    .ok_or("missing OCR raster")?
                                    .image_bytes)
                            })();
                            match raster {
                                Ok(png) => match charge_ocr_staging(&mut staging_budget, &png) {
                                    Ok(()) => {
                                        ocr_status = "pending";
                                        ocr_jobs.push((number, png));
                                    }
                                    Err(message) => {
                                        staging_exhausted = true;
                                        ocr_status = "failed";
                                        let pdf = report.pdf.as_mut().unwrap();
                                        pdf.preview_complete = false;
                                        // Resource-limit rejection leaves the
                                        // extraction incomplete; the rejected
                                        // raster is dropped here.
                                        pdf.text_complete = false;
                                        report
                                            .error("pdf_ocr", format!("page {number}: {message}"));
                                    }
                                },
                                Err(error) => {
                                    ocr_status = "failed";
                                    let pdf = report.pdf.as_mut().unwrap();
                                    pdf.preview_complete = false;
                                    // LITX-AUD-010: a failed OCR attempt leaves the
                                    // extraction incomplete.
                                    pdf.text_complete = false;
                                    report.error("pdf_ocr", format!("page {number}: {error}"));
                                }
                            }
                        }
                        report
                            .pdf
                            .as_mut()
                            .unwrap()
                            .identification_pages
                            .push(PdfPageSignal {
                                page_number: number,
                                text_layer,
                                native_chars,
                                needs_ocr: stats.needs_ocr,
                                ocr_status,
                            });
                    }
                    report.preflight.push(Preflight {
                        page_number: number,
                        unit_kind: "page",
                        needs_ocr: stats.needs_ocr,
                        reasons: stats.reasons.iter().map(|r| r.as_str().into()).collect(),
                        confidence: "deterministic",
                        coverage_available: true,
                    });
                    report.pdf.as_mut().unwrap().complexity.push(stats.clone());
                }
                page_texts.push((number, text));
                markdown_pages.push(page);
            }
            if stop || text_bytes > MAX_TEXT_BYTES {
                break;
            }
        }
        let thumbnail_png = if options.thumbnail.is_some() && count > 0 {
            Some((|| -> xml::Result<Vec<u8>> {
                if !(1..=4096).contains(&options.thumbnail_max_edge) {
                    return Err("thumbnail edge must be between 1 and 4096".into());
                }
                let page = document.page(0)?;
                let long_edge = page.width().max(page.height()) * page.user_unit();
                if !long_edge.is_finite() || long_edge <= 0.0 {
                    return Err("invalid PDF page geometry for thumbnail".into());
                }
                let dpi = 72.0 * (options.thumbnail_max_edge as f32 / long_edge).min(1.0);
                let rendered = stages::screenshots(
                    &document,
                    Some(&[1]),
                    &stages::ScreenshotOptions {
                        dpi,
                        detect_rects: false,
                        render_form_fields: false,
                        continue_on_page_error: false,
                    },
                )?;
                Ok(rendered
                    .into_iter()
                    .next()
                    .ok_or("missing thumbnail raster")?
                    .image_bytes)
            })())
        } else {
            None
        };
        // Release the PDFium lock before OCR probes and thumbnail disk writes
        // (LITX-AUD-014); the whole inspection shares this single parse.
        (
            page_texts,
            ocr_jobs,
            markdown_pages,
            text_bytes,
            text_cap_exhausted,
            extract_failed,
            thumbnail_png,
        )
    };
    // Unlocked phase: OCR probes, text assembly, and thumbnail disk writes run
    // without holding the process-global PDFium lock.
    let (
        mut page_texts,
        ocr_jobs,
        markdown_pages,
        mut text_bytes,
        mut text_cap_exhausted,
        extract_failed,
        thumbnail_png,
    ) = locked;
    let mut ocr_pages = std::collections::HashMap::new();
    let saw_native_text = page_texts.iter().any(|(_, text)| !text.trim().is_empty());
    let mut saw_ocr_text = false;
    for (number, png) in ocr_jobs {
        if text_cap_exhausted {
            // LITX-AUD-014: once the cumulative text cap is spent, stop the
            // queued probes immediately: no OCR work whose output cannot be
            // kept, no duplicate cap errors, and no page left pending. The
            // single cap error was already reported by the phase that
            // exhausted the budget.
            if let Some(entry) = report
                .pdf
                .as_mut()
                .unwrap()
                .identification_pages
                .iter_mut()
                .find(|p| p.page_number == number)
            {
                entry.ocr_status = "failed";
            }
            continue;
        }
        let recognized = (|| -> xml::Result<(crate::OcrProbe, String)> {
            let image = image::load_from_memory(&png)?;
            crate::images::probe(&image, ocr)
        })();
        match recognized {
            Ok((probe, recognized)) => {
                if let Some(entry) = report
                    .pdf
                    .as_mut()
                    .unwrap()
                    .identification_pages
                    .iter_mut()
                    .find(|p| p.page_number == number)
                {
                    entry.ocr_status = if probe.word_count > 0 {
                        "text_detected"
                    } else {
                        "no_text_detected"
                    };
                }
                if probe.word_count > 0 {
                    // LITX-AUD-014: OCR output counts towards the cumulative
                    // text cap just like native text.
                    text_bytes = text_bytes.saturating_add(recognized.len());
                    if text_bytes > MAX_TEXT_BYTES {
                        report.pdf.as_mut().unwrap().text_complete = false;
                        report.error(
                            "pdf_text",
                            "PDF cumulative text cap exceeded; character counts are partial",
                        );
                        text_cap_exhausted = true;
                    }
                    if let Some((_, text)) = page_texts.iter_mut().find(|(page, _)| *page == number)
                    {
                        *text = recognized.clone();
                    }
                    ocr_pages.insert(number, recognized);
                    saw_ocr_text = true;
                }
            }
            Err(error) => {
                if let Some(entry) = report
                    .pdf
                    .as_mut()
                    .unwrap()
                    .identification_pages
                    .iter_mut()
                    .find(|p| p.page_number == number)
                {
                    entry.ocr_status = "failed";
                }
                let pdf = report.pdf.as_mut().unwrap();
                pdf.preview_complete = false;
                // LITX-AUD-010: a failed OCR attempt leaves the extraction
                // incomplete even when every page fits the scan limit.
                pdf.text_complete = false;
                report.error("pdf_ocr", format!("page {number}: {error}"));
            }
        }
    }
    // LITX-AUD-019: a pure OCR recovery is "ocr"; only a document with native
    // text alongside OCR text is "native_and_ocr".
    if saw_ocr_text {
        report.text.origin = if saw_native_text {
            "native_and_ocr"
        } else {
            "ocr"
        }
        .into();
    }
    for (_, text) in &page_texts {
        report.append_text(text, options.max_chars);
    }
    // LITX-AUD-010/014: a failed OCR attempt, a failed page extraction, or
    // cumulative text-cap exhaustion also marks the text payload partial.
    // Applied after the appends, which recompute text.truncated from the
    // character budget on every append.
    if text_cap_exhausted
        || extract_failed
        || report
            .pdf
            .as_ref()
            .unwrap()
            .identification_pages
            .iter()
            .any(|p| p.ocr_status == "failed")
    {
        report.text.truncated = true;
    }
    let signals = stages::document_signals(&markdown_pages, true);
    for page in &markdown_pages {
        let (body, basis) = if let Some(text) = ocr_pages.get(&page.page_number) {
            (crate::markdown::escape(text), "ocr_text_fallback")
        } else {
            let blocks = stages::extract_blocks(
                page,
                &signals,
                &stages::BlockOptions {
                    outline: &[],
                    image_mode: liteparse::config::ImageMode::Off,
                    keep_headers_footers: true,
                },
            );
            (
                stages::render_page_markdown(page, blocks.as_deref()),
                "inferred_layout",
            )
        };
        crate::markdown::append(
            report,
            &format!("## Page {}\n\n{body}\n\n", page.page_number),
            options,
            basis,
        );
    }
    if let Some(result) = thumbnail_png {
        match result {
            Ok(png) => {
                crate::thumbnail::write(std::io::Cursor::new(png), "rendered", options, report)
            }
            Err(error) => report.error("thumbnail", error),
        }
    }
    if options.identify {
        let pdf = report.pdf.as_mut().unwrap();
        let digital = pdf.identification_pages.iter().any(|p| p.native_chars > 0);
        let scanned = pdf
            .identification_pages
            .iter()
            .any(|p| matches!(p.text_layer, "scanned" | "scan_with_text_layer"));
        pdf.digital_signal = match (digital, scanned) {
            (true, true) => "mixed",
            (true, false) => "digital",
            (false, true) => "scanned",
            _ => "unknown",
        };
    }
    if report.counts.pages.is_some_and(|n| n > scan_limit) {
        report.text.truncated = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum PageSpec<'a> {
        Empty,
        Text(&'a str),
        Scan {
            jpeg: &'a [u8],
            width: u32,
            height: u32,
        },
    }

    fn synthetic_pdf(pages: &[PageSpec]) -> Vec<u8> {
        let mut objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            Vec::new(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ];
        let mut kids = Vec::new();
        for page in pages {
            let (content, resources): (Vec<u8>, String) = match page {
                PageSpec::Empty => (Vec::new(), "<< >>".to_string()),
                PageSpec::Text(text) => (
                    format!("BT /F1 20 Tf 50 700 Td ({text}) Tj ET").into_bytes(),
                    "<< /Font << /F1 3 0 R >> >>".to_string(),
                ),
                PageSpec::Scan {
                    jpeg,
                    width,
                    height,
                } => {
                    let mut image = format!(
                        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
                        jpeg.len()
                    )
                    .into_bytes();
                    image.extend_from_slice(jpeg);
                    image.extend_from_slice(b"\nendstream");
                    objects.push(image);
                    let image_object = objects.len();
                    (
                        b"q 600 0 0 800 0 0 cm /Scan Do Q".to_vec(),
                        format!("<< /XObject << /Scan {image_object} 0 R >> >>"),
                    )
                }
            };
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(&content);
            stream.extend_from_slice(b"\nendstream");
            objects.push(stream);
            let content_object = objects.len();
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources {resources} /Contents {content_object} 0 R >>").into_bytes());
            kids.push(objects.len());
        }
        objects[1] = format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.iter()
                .map(|n| format!("{n} 0 R"))
                .collect::<Vec<_>>()
                .join(" "),
            pages.len()
        )
        .into_bytes();
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let start = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn write_temp_pdf(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "litx-pdf-caps-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn noisy_jpeg(width: u32, height: u32) -> (Vec<u8>, u32, u32) {
        let mut seed = 0x9e3779b97f4a7c15u64;
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..width.saturating_mul(height).saturating_mul(3) {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            pixels.push((seed >> 33) as u8);
        }
        let image = image::RgbImage::from_raw(width, height, pixels).unwrap();
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        (jpeg.into_inner(), width, height)
    }

    #[test]
    fn page_count_hard_cap_emits_a_pdf_text_error_during_full_extraction() {
        // LITX-AUD-010(b): the native page cap must be reported even when no
        // explicit max_pages limit is set. The test seam lowers the cap to 8.
        let path = write_temp_pdf(
            "many.pdf",
            &synthetic_pdf(&vec![PageSpec::Empty; MAX_PAGES + 1]),
        );
        let options = InspectOptions {
            max_pages: None,
            max_units: 0,
            ..Default::default()
        };
        let report = crate::inspect(&path, &options);
        assert_eq!(report.counts.pages, Some(MAX_PAGES + 1));
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.stage == "pdf_text" && e.message.contains("cap")),
            "{:?}",
            report.errors
        );
        assert!(!report.pdf.as_ref().unwrap().text_complete);
        assert!(report.text.truncated);
    }

    #[test]
    fn native_text_under_the_cumulative_cap_keeps_the_report_complete() {
        // Under the 32-byte test seam nothing is marked partial.
        let path = write_temp_pdf(
            "under-cap.pdf",
            &synthetic_pdf(&[PageSpec::Text("Tiny text")]),
        );
        let report = crate::inspect(
            &path,
            &InspectOptions {
                max_pages: None,
                max_units: 0,
                ..Default::default()
            },
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.pdf.as_ref().unwrap().text_complete);
        assert!(!report.text.truncated);
        assert!(report.text.content.contains("Tiny text"));
    }

    #[test]
    fn native_text_tripping_the_cumulative_cap_marks_the_report_partial() {
        // LITX-AUD-014 follow-up: cumulative text-cap exhaustion must also
        // mark the text payload partial, not only pdf.text_complete. The test
        // seam caps cumulative decoded text at 32 bytes.
        let path = write_temp_pdf(
            "over-cap.pdf",
            &synthetic_pdf(&[PageSpec::Text(
                "Native text beyond the tiny test cap of thirty-two bytes.",
            )]),
        );
        let report = crate::inspect(
            &path,
            &InspectOptions {
                max_pages: None,
                max_units: 0,
                ..Default::default()
            },
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.stage == "pdf_text" && e.message.contains("cap")),
            "{:?}",
            report.errors
        );
        assert!(!report.pdf.as_ref().unwrap().text_complete);
        assert!(report.text.truncated);
    }

    #[test]
    fn staging_budget_charges_with_checked_arithmetic() {
        // Exact fit succeeds and exhausts the budget; the next byte is
        // refused without wrapping.
        let mut budget = MAX_OCR_STAGING_BYTES;
        charge_ocr_staging(&mut budget, &vec![0u8; MAX_OCR_STAGING_BYTES]).unwrap();
        assert_eq!(budget, 0);
        charge_ocr_staging(&mut budget, &vec![0u8]).unwrap_err();
        assert_eq!(budget, 0);
        let mut full = MAX_OCR_STAGING_BYTES;
        charge_ocr_staging(&mut full, &vec![0u8; MAX_OCR_STAGING_BYTES + 1]).unwrap_err();
        assert_eq!(full, MAX_OCR_STAGING_BYTES);
    }

    #[test]
    fn staging_budget_charges_retained_capacity_not_payload_length() {
        // The queue charges the retained allocation, not the payload: a
        // small-payload raster whose Vec capacity exceeds the remaining
        // budget must be refused with the budget unchanged, while a raster
        // whose capacity exactly fits is accepted.
        let mut budget = MAX_OCR_STAGING_BYTES;
        let mut spare = Vec::with_capacity(MAX_OCR_STAGING_BYTES + 1);
        spare.extend_from_slice(&[0u8; 8]);
        assert!(spare.len() <= budget);
        assert!(spare.capacity() > budget);
        charge_ocr_staging(&mut budget, &spare).unwrap_err();
        assert_eq!(budget, MAX_OCR_STAGING_BYTES);
        let exact: Vec<u8> = Vec::with_capacity(MAX_OCR_STAGING_BYTES);
        assert_eq!(exact.capacity(), MAX_OCR_STAGING_BYTES);
        charge_ocr_staging(&mut budget, &exact).unwrap();
        assert_eq!(budget, 0);
    }

    #[test]
    fn ocr_staging_budget_rejects_unbounded_raster_retention() {
        // LITX-AUD-014 follow-up: the aggregate retained-raster budget (test
        // seam: 1 MiB) rejects the first oversized noisy render, skips
        // further renders, reports the exhaustion once, and leaves no
        // pending OCR statuses. Runs before any OCR probe, so no tessdata
        // is required.
        let jpegs: Vec<(Vec<u8>, u32, u32)> = (0..3).map(|_| noisy_jpeg(900, 1200)).collect();
        let specs: Vec<PageSpec> = jpegs
            .iter()
            .map(|(jpeg, width, height)| PageSpec::Scan {
                jpeg,
                width: *width,
                height: *height,
            })
            .collect();
        let path = write_temp_pdf("staging.pdf", &synthetic_pdf(&specs));
        let report = crate::inspect(
            &path,
            &InspectOptions {
                identify: true,
                max_pages: None,
                max_units: 0,
                ..Default::default()
            },
        );
        let ocr_errors = report
            .errors
            .iter()
            .filter(|e| e.stage == "pdf_ocr")
            .count();
        assert_eq!(ocr_errors, 1, "{:?}", report.errors);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.stage == "pdf_ocr" && e.message.contains("staging")),
            "{:?}",
            report.errors
        );
        let pdf = report.pdf.as_ref().unwrap();
        assert!(!pdf.text_complete);
        assert!(report.text.truncated);
        assert_eq!(report.counts.pages, Some(3));
        assert_eq!(pdf.geometry.len(), 3);
        assert_eq!(pdf.identification_pages.len(), 3);
        assert!(pdf.identification_pages.iter().all(|p| p.needs_ocr));
        assert!(
            pdf.identification_pages
                .iter()
                .all(|p| p.ocr_status == "failed"),
            "{:?}",
            pdf.identification_pages
        );
    }

    fn solid_jpeg(width: u32, height: u32, shade: u8) -> (Vec<u8>, u32, u32) {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([shade, shade, shade]));
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        (jpeg.into_inner(), width, height)
    }

    #[test]
    fn ocr_probes_stop_after_native_text_cap_without_extra_errors() {
        // LITX-AUD-014: once the native phase exhausts the cumulative text
        // cap, the unlocked phase must not run queued OCR probes at all:
        // no pdf_ocr errors, no duplicate cap errors, and no pending status.
        let (jpeg, width, height) = solid_jpeg(500, 650, 200);
        let path = write_temp_pdf(
            "native-cap-stop.pdf",
            &synthetic_pdf(&[
                PageSpec::Scan {
                    jpeg: &jpeg,
                    width,
                    height,
                },
                PageSpec::Text("This native page trips the tiny cumulative text cap."),
            ]),
        );
        let report = crate::inspect(
            &path,
            &InspectOptions {
                identify: true,
                max_pages: None,
                max_units: 0,
                ..Default::default()
            },
        );
        let cap_errors = report
            .errors
            .iter()
            .filter(|e| e.stage == "pdf_text" && e.message.contains("cap"))
            .count();
        assert_eq!(cap_errors, 1, "{:?}", report.errors);
        assert!(
            !report.errors.iter().any(|e| e.stage == "pdf_ocr"),
            "{:?}",
            report.errors
        );
        let pdf = report.pdf.as_ref().unwrap();
        assert!(!pdf.text_complete);
        assert!(report.text.truncated);
        assert_eq!(pdf.identification_pages.len(), 1);
        assert_eq!(pdf.identification_pages[0].ocr_status, "failed");
    }

    #[test]
    #[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
    fn ocr_probes_stop_immediately_when_ocr_text_trips_the_cap() {
        // LITX-AUD-014: when OCR output trips the cumulative cap (test seam:
        // 32 bytes), the first tripping probe emits the single cap error and
        // every still-queued probe is abandoned, never left pending.
        let fixture =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/native.pdf");
        let directory = std::env::temp_dir().join(format!(
            "litx-pdf-ocr-stop-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let thumbnail = directory.join("ocr-source.png");
        let rendered = crate::inspect(
            &fixture,
            &InspectOptions {
                thumbnail: Some(thumbnail.clone()),
                thumbnail_max_edge: 1200,
                max_units: 0,
                ..Default::default()
            },
        );
        assert!(thumbnail.exists(), "{:?}", rendered.errors);
        let image = image::open(&thumbnail).unwrap().to_rgb8();
        let (width, height) = (image.width(), image.height());
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let specs: Vec<PageSpec> = (0..3)
            .map(|_| PageSpec::Scan {
                jpeg: jpeg.get_ref(),
                width,
                height,
            })
            .collect();
        let path = directory.join("scans.pdf");
        std::fs::write(&path, synthetic_pdf(&specs)).unwrap();
        let report = crate::inspect(
            &path,
            &InspectOptions {
                identify: true,
                max_pages: None,
                max_units: 0,
                ..Default::default()
            },
        );
        let cap_errors = report
            .errors
            .iter()
            .filter(|e| e.stage == "pdf_text" && e.message.contains("cap"))
            .count();
        assert_eq!(cap_errors, 1, "{:?}", report.errors);
        let pdf = report.pdf.as_ref().unwrap();
        assert!(!pdf.text_complete);
        assert!(report.text.truncated);
        assert_eq!(pdf.identification_pages.len(), 3);
        assert_eq!(pdf.identification_pages[0].ocr_status, "text_detected");
        assert_eq!(pdf.identification_pages[1].ocr_status, "failed");
        assert_eq!(pdf.identification_pages[2].ocr_status, "failed");
    }

    #[test]
    #[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
    fn ocr_text_tripping_the_cumulative_cap_marks_the_report_partial() {
        // OCR output charges the same cumulative cap: a small native page
        // plus an OCR'd scan exceeds the 32-byte test seam.
        let fixture =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/native.pdf");
        let directory = std::env::temp_dir().join(format!(
            "litx-pdf-ocr-cap-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let thumbnail = directory.join("ocr-source.png");
        let rendered = crate::inspect(
            &fixture,
            &InspectOptions {
                thumbnail: Some(thumbnail.clone()),
                thumbnail_max_edge: 1200,
                max_units: 0,
                ..Default::default()
            },
        );
        assert!(thumbnail.exists(), "{:?}", rendered.errors);
        let image = image::open(&thumbnail).unwrap().to_rgb8();
        let (width, height) = (image.width(), image.height());
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let path = directory.join("mixed.pdf");
        std::fs::write(
            &path,
            synthetic_pdf(&[
                PageSpec::Text("Native text"),
                PageSpec::Scan {
                    jpeg: jpeg.get_ref(),
                    width,
                    height,
                },
            ]),
        )
        .unwrap();
        let report = crate::inspect(
            &path,
            &InspectOptions {
                identify: true,
                max_pages: None,
                max_units: 0,
                ..Default::default()
            },
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.stage == "pdf_text" && e.message.contains("cap")),
            "{:?}",
            report.errors
        );
        assert!(!report.pdf.as_ref().unwrap().text_complete);
        assert!(report.text.truncated);
    }

    /// W3-SYS-PDF-001: a page-local complexity failure must retain every
    /// recoverable native and Markdown page while marking the report partial.
    /// Accepts no arguments and returns unit; assertion failures identify lost
    /// text/layout, a missing diagnostic, or incorrect completeness flags.
    #[test]
    fn complexity_failure_preserves_recovered_pages() {
        let bytes = synthetic_pdf(&[
            PageSpec::Text("first page"),
            PageSpec::Text("second page"),
            PageSpec::Text("third page"),
        ]);
        let path = write_temp_pdf("complexity-failure.pdf", &bytes);
        let mut report = crate::inspect(
            &path,
            &InspectOptions {
                max_pages: Some(0),
                max_units: 0,
                ..Default::default()
            },
        );
        report.text = crate::Text::default();
        report.markdown = crate::Markdown::default();
        report.errors.clear();
        let options = InspectOptions {
            identify: true,
            max_pages: None,
            max_units: 0,
            ..Default::default()
        };
        let result = inner_with_complexity(
            bytes,
            &mut report,
            &options,
            &mut crate::images::OcrSession::default(),
            |document, page| {
                if page.page_number == 2 {
                    Err("injected page complexity failure".into())
                } else {
                    stages::page_complexity(document, page).map_err(Into::into)
                }
            },
        );
        if let Err(error) = result {
            report.error("pdf", error);
        }
        for expected in ["first page", "second page", "third page"] {
            assert!(
                report.text.content.contains(expected),
                "missing recovered text {expected:?}: {:?}",
                report.text
            );
            assert!(
                report.markdown.content.contains(expected),
                "missing recovered Markdown {expected:?}: {:?}",
                report.markdown.content
            );
        }
        assert!(report.errors.iter().any(|error| {
            error.stage == "pdf_text" && error.message.contains("page 2: injected")
        }));
        assert!(!report.pdf.as_ref().unwrap().text_complete);
        assert!(!report.pdf.as_ref().unwrap().preview_complete);
        assert!(report.text.truncated);
    }

    /// W3-SYS-PDF-001: an over-budget native page must not leak into Markdown
    /// after the cumulative text budget rejects it. Accepts no arguments and
    /// returns unit; assertion failures identify budget or truncation regressions.
    #[test]
    fn cumulative_native_cap_also_bounds_markdown_pages() {
        let overflowing = "This page exceeds the tiny cumulative native-text budget.";
        let path = write_temp_pdf(
            "markdown-cumulative-cap.pdf",
            &synthetic_pdf(&[PageSpec::Text("Kept"), PageSpec::Text(overflowing)]),
        );
        let report = crate::inspect(
            &path,
            &InspectOptions {
                max_pages: None,
                max_units: 0,
                ..Default::default()
            },
        );
        assert!(report.text.content.contains("Kept"));
        assert!(!report.text.content.contains(overflowing));
        assert!(report.markdown.content.contains("Kept"));
        assert!(
            !report.markdown.content.contains(overflowing),
            "over-budget page was retained in Markdown: {}",
            report.markdown.content
        );
        assert!(report.text.truncated);
        assert!(report.markdown.truncated);
    }
}
