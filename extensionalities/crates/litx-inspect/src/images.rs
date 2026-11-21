use crate::{InspectOptions, InspectReport, Preflight, binary::Source, xml};
use image::{ImageReader, Limits};
use serde::Serialize;
use std::{io::Cursor, path::Path};

#[derive(Debug, Serialize)]
pub struct ImageAnalysis {
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: f64,
    pub dpi: Option<[f64; 2]>,
    pub basis: &'static str,
    pub sampled_pixels: usize,
    pub white_fraction: f64,
    pub histogram: [usize; 16],
    pub classification: &'static str,
    pub probe: Option<OcrProbe>,
    pub ocr_status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct OcrProbe {
    pub basis: &'static str,
    pub engine: &'static str,
    pub language: &'static str,
    pub word_count: usize,
    pub mean_confidence: f64,
}

#[derive(Default)]
pub(crate) struct OcrSession {
    pub(crate) api: Option<tesseract_rs::TesseractAPI>,
}

pub(crate) fn inspect(
    path: &Path,
    report: &mut InspectReport,
    options: &InspectOptions,
    ocr: &mut OcrSession,
) {
    let result = (|| -> xml::Result<()> {
        let mut source = Source::open(path)?;
        let bytes = source.range(0, source.len as usize)?;
        inner(bytes, report, options, ocr)
    })();
    if let Err(error) = result {
        report.error("image", error);
    }
}

pub(crate) fn inspect_bytes(
    bytes: Vec<u8>,
    report: &mut InspectReport,
    options: &InspectOptions,
    ocr: &mut OcrSession,
) {
    if let Err(error) = inner(bytes, report, options, ocr) {
        report.error("image", error);
    }
}

fn inner(
    bytes: Vec<u8>,
    report: &mut InspectReport,
    options: &InspectOptions,
    ocr: &mut OcrSession,
) -> xml::Result<()> {
    let mut reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(128 << 20);
    reader.limits(limits);
    let decoded = reader.decode()?;
    let (width, height) = (decoded.width(), decoded.height());
    let sample = if width.max(height) > 1024 {
        decoded.thumbnail(1024, 1024).to_rgba8()
    } else {
        decoded.to_rgba8()
    };
    let sampled_pixels = sample.width() as usize * sample.height() as usize;
    let mut histogram = [0; 16];
    let mut white = 0;
    for pixel in sample.pixels() {
        let alpha = pixel[3] as usize;
        let rgb = [pixel[0], pixel[1], pixel[2]]
            .map(|v| (v as usize * alpha + 255 * (255 - alpha)) / 255);
        let luma = (299 * rgb[0] + 587 * rgb[1] + 114 * rgb[2]) / 1000;
        histogram[luma / 16] += 1;
        white += usize::from(rgb.iter().all(|&v| v >= 245));
    }
    let white_fraction = white as f64 / sampled_pixels.max(1) as f64;
    let aspect_ratio = width as f64 / height.max(1) as f64;
    let dark_fraction = histogram[0] as f64 / sampled_pixels.max(1) as f64;
    let classification = if white_fraction > 0.995 {
        "blank_or_near_blank"
    } else if white_fraction > 0.55 && dark_fraction > 0.005 && (0.45..=0.9).contains(&aspect_ratio)
    {
        "document_scan_like"
    } else {
        "photo_or_graphic"
    };
    report.counts.pages = Some(1);
    if options.max_units > 0 {
        report.preflight.push(Preflight {
            page_number: 1,
            unit_kind: "image",
            needs_ocr: classification == "document_scan_like",
            reasons: vec![classification.into()],
            confidence: "heuristic",
            coverage_available: false,
        });
    }
    report.image = Some(ImageAnalysis {
        width,
        height,
        aspect_ratio,
        dpi: png_dpi(&bytes).or_else(|| jpeg_dpi(&bytes)),
        basis: "heuristic",
        sampled_pixels,
        white_fraction,
        histogram,
        classification,
        probe: None,
        ocr_status: "not_probed",
    });
    if options.thumbnail.is_some() {
        crate::thumbnail::write(Cursor::new(bytes), "downscaled", options, report);
    }
    if (options.probe_ocr || options.identify) && options.max_units > 0 {
        match probe(&decoded, ocr) {
            Ok((probe, text)) => {
                let word_count = probe.word_count;
                let image = report.image.as_mut().unwrap();
                image.ocr_status = if word_count > 0 {
                    "text_detected"
                } else {
                    "no_text_detected"
                };
                image.probe = Some(probe);
                report.text.origin = "ocr".into();
                report.append_text(&text, options.max_chars);
                // LITX-AUD-009: OCR-recovered image text joins the Markdown
                // output with OCR provenance, not a plain text fallback.
                if word_count > 0 {
                    crate::markdown::append(
                        report,
                        &crate::markdown::escape(&text),
                        options,
                        "ocr_text_fallback",
                    );
                }
            }
            Err(error) => {
                report.image.as_mut().unwrap().ocr_status = "failed";
                report.error("probe_ocr", error);
            }
        }
    }
    Ok(())
}

pub(crate) fn probe(
    image: &image::DynamicImage,
    session: &mut OcrSession,
) -> xml::Result<(OcrProbe, String)> {
    use tesseract_rs::{TessPageSegMode, TesseractAPI};
    if session.api.is_none() {
        let directory = if let Some(path) = std::env::var_os("TESSDATA_PREFIX") {
            std::path::PathBuf::from(path)
        } else {
            let mut candidates = Vec::new();
            if let Some(home) = std::env::var_os("HOME") {
                candidates.push(std::path::PathBuf::from(&home).join(".tesseract-rs/tessdata"));
                candidates.push(
                    std::path::PathBuf::from(home)
                        .join("Library/Application Support/tesseract-rs/tessdata"),
                );
            }
            candidates.extend(
                [
                    "/usr/share/tesseract-ocr/5/tessdata",
                    "/usr/share/tessdata",
                    "/opt/homebrew/share/tessdata",
                ]
                .map(std::path::PathBuf::from),
            );
            candidates
                .into_iter()
                .find(|p| p.join("eng.traineddata").is_file())
                .ok_or("local eng.traineddata unavailable; set TESSDATA_PREFIX (no downloads)")?
        };
        let metadata = std::fs::metadata(directory.join("eng.traineddata"))
            .map_err(|_| "local eng.traineddata unavailable; set TESSDATA_PREFIX (no downloads)")?;
        if metadata.len() == 0 || metadata.len() > 128 << 20 {
            return Err("local OCR model byte cap exceeded or empty model".into());
        }
        directory
            .to_str()
            .ok_or("local tessdata directory must be UTF-8")?;
        let api = TesseractAPI::new();
        api.init(&directory, "eng")?;
        api.set_page_seg_mode(TessPageSegMode::PSM_AUTO)?;
        session.api = Some(api);
    }
    let grayscale = if image.width().max(image.height()) > 2048 {
        image.thumbnail(2048, 2048).to_luma8()
    } else {
        image.to_luma8()
    };
    let api = session.api.as_ref().unwrap();
    api.clear()?;
    api.clear_adaptive_classifier()?;
    api.set_image(
        grayscale.as_raw(),
        grayscale.width() as i32,
        grayscale.height() as i32,
        1,
        grayscale.width() as i32,
    )?;
    api.set_source_resolution(150)?;
    api.recognize()?;
    let text = api.get_utf8_text()?;
    let result = OcrProbe {
        basis: "probed",
        engine: "tesseract",
        language: "eng",
        word_count: text.split_whitespace().count(),
        mean_confidence: (api.mean_text_conf()? as f64 / 100.0).clamp(0.0, 1.0),
    };
    api.clear()?;
    Ok((result, text))
}

fn png_dpi(bytes: &[u8]) -> Option<[f64; 2]> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut offset: usize = 8;
    while offset.checked_add(12)? <= bytes.len() {
        let size = crate::binary::be32(bytes, offset).ok()? as usize;
        let end = offset.checked_add(12)?.checked_add(size)?;
        let chunk = bytes.get(offset..end)?;
        if &chunk[4..8] == b"pHYs" && size == 9 && chunk[16] == 1 {
            let x = crate::binary::be32(chunk, 8).ok()?;
            let y = crate::binary::be32(chunk, 12).ok()?;
            if x > 0 && y > 0 {
                return Some([x as f64 * 0.0254, y as f64 * 0.0254]);
            }
        }
        if &chunk[4..8] == b"IDAT" {
            break;
        }
        offset = end;
    }
    None
}

fn jpeg_dpi(bytes: &[u8]) -> Option<[f64; 2]> {
    if !bytes.starts_with(b"\xff\xd8") {
        return None;
    }
    let mut offset = 2;
    while bytes.get(offset) == Some(&0xff) {
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        if matches!(marker, 0xda | 0xd9) {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }
        let size = crate::binary::be16(bytes, offset).ok()? as usize;
        if size < 2 {
            return None;
        }
        let body = bytes.get(offset + 2..offset + size)?;
        if marker == 0xe0 && body.len() >= 14 && body.starts_with(b"JFIF\0") {
            let scale = match body[7] {
                1 => 1.0,
                2 => 2.54,
                _ => return None,
            };
            let x = crate::binary::be16(body, 8).ok()?;
            let y = crate::binary::be16(body, 10).ok()?;
            return (x > 0 && y > 0).then_some([x as f64 * scale, y as f64 * scale]);
        }
        offset += size;
    }
    None
}
