//! DjVu identification-only contract regressions for library inspection.
use litx_inspect::{InspectOptions, InspectReport, inspect};
use std::path::{Path, PathBuf};

const DIAGNOSTIC: &str = "DjVu extraction is unsupported; identification only";

/// Return an IFF chunk with a big-endian length and required padding byte.
///
/// Accepts a four-byte chunk identifier and payload bytes. Returns the encoded
/// chunk. The length cast panics only for payloads larger than `u32::MAX`.
fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

/// Return a standalone DjVu document containing caller-supplied chunks.
///
/// Accepts INFO width and height plus encoded page chunks. Returns complete
/// file bytes with valid outer magic. No internal chunk validity is implied.
fn standalone(width: u16, height: u16, chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut info = Vec::new();
    info.extend_from_slice(&width.to_be_bytes());
    info.extend_from_slice(&height.to_be_bytes());
    info.extend_from_slice(&[26, 0, 44, 1, 22, 1]);
    let mut page = b"DJVU".to_vec();
    page.extend(chunk(b"INFO", &info));
    for item in chunks {
        page.extend_from_slice(item);
    }
    let mut bytes = b"AT&T".to_vec();
    bytes.extend(chunk(b"FORM", &page));
    bytes
}

/// Return a raw TXTa payload whose text must never reach inspection output.
///
/// Accepts the marker text to embed. Returns a three-byte-length-prefixed
/// payload with a trailing terminator. Oversized test markers are not used.
fn text_payload(text: &str) -> Vec<u8> {
    let mut bytes = (text.len() as u32).to_be_bytes()[1..].to_vec();
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0);
    bytes
}

/// Return the stable ignored fixture directory for controller live checks.
///
/// Accepts no inputs. Returns the absolute directory path. Directory creation
/// failures panic and stop the test rather than writing outside the workspace.
fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/verification/djvu-identification-only/fixtures/library")
}

/// Write one handcrafted DjVu fixture under the authorized target directory.
///
/// Accepts a filename and complete bytes. Returns the written path. Filesystem
/// failures panic; tests may replace their own deterministic generated fixture.
fn write_fixture(name: &str, bytes: &[u8]) -> PathBuf {
    let directory = fixture_directory();
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Assert the complete static identification-only report boundary.
///
/// Accepts a fixture path and inspection options. Returns the report for
/// additional format assertions. Any text, page, analysis, thumbnail, OCR, or
/// non-static DjVu diagnostic causes an assertion panic.
fn assert_identification_only(path: &Path, options: &InspectOptions) -> InspectReport {
    let report = inspect(path, options);
    assert_eq!(report.format.bucket, "Ebooks", "{path:?}");
    let djvu_errors: Vec<_> = report
        .errors
        .iter()
        .filter(|error| error.stage == "djvu")
        .collect();
    assert_eq!(djvu_errors.len(), 1, "{path:?}: {:?}", report.errors);
    assert_eq!(djvu_errors[0].message, DIAGNOSTIC, "{path:?}");
    assert!(
        report
            .errors
            .iter()
            .all(|error| matches!(error.stage, "djvu" | "probe_ocr")),
        "{path:?}: extraction path ran: {:?}",
        report.errors
    );
    assert!(report.djvu.is_none(), "{path:?}");
    assert!(report.pages.is_empty(), "{path:?}");
    assert_eq!(report.counts.pages, None, "{path:?}");
    assert!(report.preflight.is_empty(), "{path:?}");
    assert!(report.text.content.is_empty(), "{path:?}");
    assert_eq!(report.text.chars, 0, "{path:?}");
    assert_eq!(report.text.tokens, 0, "{path:?}");
    assert_eq!(report.counts.chars, 0, "{path:?}");
    assert!(report.text.truncated, "{path:?}");
    assert!(report.markdown.truncated, "{path:?}");
    assert!(report.thumbnail.is_none(), "{path:?}");
    assert!(report.image.is_none(), "{path:?}");
    report
}

/// Prove former native, compressed, and raster paths all stop at detection.
///
/// Accepts no inputs and returns unit. Panics if any chunk payload is decoded,
/// any page is published, or the static support diagnostic differs by option.
#[test]
fn native_text_and_all_former_decode_paths_are_identification_only() {
    let cases = [
        (
            "native-txta.djvu",
            chunk(b"TXTa", &text_payload("HIDDEN_NATIVE_TEXT")),
        ),
        (
            "compressed-txtz.djvu",
            chunk(b"TXTz", b"not a compressed stream"),
        ),
        (
            "foreground-fgbz.djvu",
            chunk(b"FGbz", &[0x80, 0, 1, 0, 0, 0]),
        ),
        (
            "mask-sjbz.djvu",
            chunk(b"Sjbz", b"hostile arithmetic payload"),
        ),
        (
            "background-bgjp.djvu",
            chunk(b"BGjp", &[0xff, 0xd8, 0xff, 0xc2]),
        ),
        (
            "mask-smmr.djvu",
            chunk(b"Smmr", b"MMR\xff\xff\xff\xff\xff\xff\xff\xff"),
        ),
    ];
    for (name, layer) in cases {
        let path = write_fixture(name, &standalone(600, 800, &[layer]));
        for options in [
            InspectOptions::default(),
            InspectOptions {
                identify: true,
                max_pages: None,
                ..Default::default()
            },
        ] {
            let report = assert_identification_only(&path, &options);
            assert_eq!(
                report.format.detected_mime.as_deref(),
                Some("image/vnd.djvu")
            );
            assert!(!report.text.content.contains("HIDDEN_NATIVE_TEXT"));
            assert!(!report.markdown.content.contains("HIDDEN_NATIVE_TEXT"));
        }
    }
}

/// Prove large geometry and bundled hostile metadata never initiate decoding.
///
/// Accepts no inputs and returns unit. Panics on geometry extraction, DIRM or
/// NAVM processing, page recovery, or a diagnostic other than the boundary.
#[test]
fn large_geometry_and_bundled_hostile_metadata_are_identification_only() {
    let large = write_fixture(
        "large-fake-geometry.djvu",
        &standalone(u16::MAX, u16::MAX, &[]),
    );
    assert_identification_only(&large, &Default::default());

    let page = standalone(
        600,
        800,
        &[chunk(b"TXTa", &text_payload("HIDDEN_BUNDLE_TEXT"))],
    );
    let page_form = page[4..].to_vec();
    let mut body = b"DJVM".to_vec();
    body.extend(chunk(b"DIRM", &[0x81, 0, 1, 0, 0, 0, 32]));
    body.extend(chunk(b"NAVM", b"hostile bookmark payload"));
    body.extend_from_slice(&page_form);
    let mut bundled = b"AT&T".to_vec();
    bundled.extend(chunk(b"FORM", &body));
    let bundled = write_fixture("bundled-dirm-navm.djvu", &bundled);
    let report = assert_identification_only(
        &bundled,
        &InspectOptions {
            identify: true,
            max_pages: None,
            ..Default::default()
        },
    );
    assert_eq!(
        report.format.detected_mime.as_deref(),
        Some("image/vnd.djvu")
    );
    assert!(!report.text.content.contains("HIDDEN_BUNDLE_TEXT"));
}

/// Prove explicit OCR and thumbnail requests cannot invoke either subsystem.
///
/// Accepts no inputs and returns unit. Panics if a thumbnail is created, OCR
/// analysis appears, native text leaks, or DjVu emits an OCR-stage diagnostic.
#[test]
fn requested_ocr_and_thumbnail_never_create_output() {
    let path = write_fixture(
        "ocr-thumbnail-request.djvu",
        &standalone(600, 800, &[chunk(b"BGjp", b"malformed jpeg")]),
    );
    let thumbnail = fixture_directory().join("forbidden-thumbnail.png");
    for options in [
        InspectOptions {
            identify: true,
            thumbnail: Some(thumbnail.clone()),
            ..Default::default()
        },
        InspectOptions {
            probe_ocr: true,
            thumbnail: Some(thumbnail.clone()),
            ..Default::default()
        },
    ] {
        let report = assert_identification_only(&path, &options);
        assert_eq!(
            report
                .errors
                .iter()
                .filter(|e| e.stage == "probe_ocr")
                .count(),
            usize::from(options.probe_ocr)
        );
    }
    assert!(!thumbnail.exists(), "DjVu thumbnail must not be created");
}
