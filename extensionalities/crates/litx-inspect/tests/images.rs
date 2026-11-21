use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
#[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
fn identify_images_extracts_text_and_reports_clean_negative_scans() {
    let directory = fixture("../target/tests").join(format!(
        "identify-image-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("text.png");
    let rendered = inspect(
        &fixture("native.pdf"),
        &InspectOptions {
            thumbnail: Some(path.clone()),
            thumbnail_max_edge: 800,
            max_units: 0,
            ..Default::default()
        },
    );
    assert!(rendered.errors.is_empty(), "{:?}", rendered.errors);
    let options = InspectOptions {
        identify: true,
        ..Default::default()
    };
    let r = inspect(&path, &options);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["image"]["ocr_status"], "text_detected");
    assert_eq!(r.text.origin, "ocr");
    assert!(r.text.content.contains("Native PDF"), "{}", r.text.content);
    let blank = inspect(&fixture("image.png"), &options);
    assert!(blank.errors.is_empty(), "{:?}", blank.errors);
    assert_eq!(
        serde_json::to_value(&blank).unwrap()["image"]["ocr_status"],
        "no_text_detected"
    );
    assert!(blank.text.content.is_empty());
}

#[test]
fn scan_like_histogram_and_dpi_are_heuristics_not_ocr_evidence() {
    let r = inspect(&fixture("document-image.png"), &Default::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    let a = &v["image"];
    assert_eq!(a["classification"], "document_scan_like");
    assert!((a["dpi"][0].as_f64().unwrap() - 300.0).abs() < 0.1);
    assert!(a["white_fraction"].as_f64().unwrap() > 0.8);
    assert_eq!(
        a["histogram"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap())
            .sum::<u64>(),
        a["sampled_pixels"].as_u64().unwrap()
    );
    assert!(r.preflight[0].needs_ocr);
    assert!(!r.preflight[0].coverage_available);
    let zero = inspect(
        &fixture("document-image.png"),
        &InspectOptions {
            max_units: 0,
            ..Default::default()
        },
    );
    assert!(zero.preflight.is_empty());
    assert!(zero.image.is_some());
}

#[test]
fn image_heuristics_run_without_thumbnail_or_ocr() {
    let r = inspect(&fixture("image.png"), &Default::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["image"]["width"], 640);
    assert_eq!(v["image"]["height"], 400);
    assert_eq!(v["image"]["basis"], "heuristic");
    assert_eq!(v["image"]["white_fraction"], 1.0);
    assert_eq!(v["image"]["classification"], "blank_or_near_blank");
    assert!(v["image"]["probe"].is_null());
    assert_eq!(r.counts.pages, Some(1));
    assert_eq!(r.preflight[0].confidence, "heuristic");
    assert!(!r.preflight[0].needs_ocr);
    assert!(r.quick_look().contains("blank_or_near_blank [heuristic]"));
}

#[test]
#[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
fn opt_in_ocr_probe_reports_word_count_and_mean_confidence() {
    let r = inspect(
        &fixture("image.png"),
        &InspectOptions {
            probe_ocr: true,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["image"]["probe"]["basis"], "probed");
    assert_eq!(v["image"]["probe"]["engine"], "tesseract");
    assert_eq!(v["image"]["probe"]["word_count"], 0);
    assert_eq!(v["image"]["probe"]["mean_confidence"], 0.0);
    assert_eq!(v["image"]["basis"], "heuristic");
    assert!(
        r.quick_look()
            .contains("0 words, mean confidence 0.00 [probed]")
    );
}

#[test]
fn jpeg_density_units_distinguish_dpi_from_pixel_aspect_ratio() {
    use image::codecs::jpeg::{JpegEncoder, PixelDensity, PixelDensityUnit};
    let directory = fixture("../target/tests").join(format!(
        "jpeg-density-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    for (index, unit, expected) in [
        (0, PixelDensityUnit::Inches, Some([100.0, 100.0])),
        (1, PixelDensityUnit::Centimeters, Some([254.0, 254.0])),
        (2, PixelDensityUnit::PixelAspectRatio, None),
    ] {
        let mut bytes = Vec::new();
        let mut encoder = JpegEncoder::new(&mut bytes);
        encoder.set_pixel_density(PixelDensity {
            density: (100, 100),
            unit,
        });
        encoder
            .encode(&[255; 8 * 8 * 3], 8, 8, image::ExtendedColorType::Rgb8)
            .unwrap();
        let path = directory.join(format!("{index}.jpg"));
        std::fs::write(&path, bytes).unwrap();
        let r = inspect(&path, &Default::default());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.image.as_ref().unwrap().sampled_pixels, 64);
        assert_eq!(r.image.unwrap().dpi, expected);
    }
}

#[test]
fn transparent_background_is_composited_as_white() {
    let directory = fixture("../target/tests").join(format!(
        "alpha-image-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("transparent.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 0]))
        .save(&path)
        .unwrap();
    let r = inspect(&path, &Default::default());
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let image = r.image.unwrap();
    assert_eq!(image.white_fraction, 1.0);
    assert_eq!(image.sampled_pixels, 64);
}

#[test]
#[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
fn ocr_text_images_emit_ocr_markdown_provenance() {
    // LITX-AUD-009: OCR-recovered image text must land in the Markdown
    // output labeled ocr_text_fallback, not the plain text_fallback.
    let directory = fixture("../target/tests").join(format!(
        "ocr-markdown-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("text.png");
    let pdf = inspect(
        &fixture("native.pdf"),
        &InspectOptions {
            thumbnail: Some(path.clone()),
            thumbnail_max_edge: 800,
            max_units: 0,
            ..Default::default()
        },
    );
    assert!(pdf.errors.is_empty(), "{:?}", pdf.errors);
    let r = inspect(
        &path,
        &InspectOptions {
            identify: true,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.text.content.contains("Native PDF"));
    assert_eq!(r.text.origin, "ocr");
    assert_eq!(r.markdown.basis, "ocr_text_fallback");
    assert!(
        r.markdown.content.contains("Native PDF"),
        "{}",
        r.markdown.content
    );
}

#[test]
#[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
fn ocr_probe_recognizes_native_pdf_rendered_words() {
    let directory = fixture("../target/tests").join(format!(
        "real-ocr-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("text.png");
    let pdf = inspect(
        &fixture("native.pdf"),
        &InspectOptions {
            thumbnail: Some(path.clone()),
            thumbnail_max_edge: 800,
            max_units: 0,
            ..Default::default()
        },
    );
    assert!(pdf.errors.is_empty(), "{:?}", pdf.errors);
    let r = inspect(
        &path,
        &InspectOptions {
            probe_ocr: true,
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let probe = r.image.unwrap().probe.unwrap();
    assert!(probe.word_count >= 5, "{probe:?}");
    assert!(probe.mean_confidence > 0.5, "{probe:?}");
}
