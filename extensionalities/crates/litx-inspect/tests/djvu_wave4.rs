//! Security regressions for DjVu malformed-input and legacy-fixture boundaries.
use litx_inspect::{InspectOptions, InspectReport, inspect};
use std::path::{Path, PathBuf};

const DIAGNOSTIC: &str = "DjVu extraction is unsupported; identification only";

/// Return the ignored fixture directory for malformed-boundary live checks.
///
/// Accepts no inputs. Returns the absolute path. Directory creation errors
/// panic so no test can silently write outside the authorized target tree.
fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/verification/djvu-identification-only/fixtures/security")
}

/// Write one deterministic malformed fixture under the ignored target tree.
///
/// Accepts a filename and complete bytes. Returns the written path. Panics on
/// filesystem failures; only test-owned deterministic fixtures are replaced.
fn write_fixture(name: &str, bytes: &[u8]) -> PathBuf {
    let directory = fixture_directory();
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Assert one malformed or legacy input produces only the support boundary.
///
/// Accepts a path and options. Returns the inspection report. Panics on page,
/// text, analysis, thumbnail, OCR, or additional DjVu diagnostics.
fn assert_boundary(path: &Path, options: &InspectOptions) -> InspectReport {
    let report = inspect(path, options);
    assert_eq!(
        report.format.bucket, "Ebooks",
        "{path:?}: {:?}",
        report.format
    );
    let errors: Vec<_> = report
        .errors
        .iter()
        .filter(|error| error.stage == "djvu")
        .collect();
    assert_eq!(errors.len(), 1, "{path:?}: {:?}", report.errors);
    assert_eq!(errors[0].message, DIAGNOSTIC, "{path:?}");
    assert!(report.djvu.is_none(), "{path:?}");
    assert!(report.pages.is_empty(), "{path:?}");
    assert_eq!(report.counts.pages, None, "{path:?}");
    assert!(report.preflight.is_empty(), "{path:?}");
    assert!(report.text.content.is_empty(), "{path:?}");
    assert_eq!(report.counts.chars, 0, "{path:?}");
    assert!(report.text.truncated, "{path:?}");
    assert!(report.markdown.truncated, "{path:?}");
    assert!(report.thumbnail.is_none(), "{path:?}");
    assert!(
        report.errors.iter().all(|error| error.stage == "djvu"),
        "{path:?}: {:?}",
        report.errors
    );
    report
}

/// Prove short magic and malicious chunk lengths cannot enter raw recovery.
///
/// Accepts no inputs and returns unit. Panics if malformed standalone data is
/// decoded, produces parser errors, or yields anything beyond identification.
#[test]
fn malformed_magic_and_chunk_lengths_share_the_static_boundary() {
    let mut malformed_chunk = b"AT&TFORM".to_vec();
    malformed_chunk.extend_from_slice(&u32::MAX.to_be_bytes());
    malformed_chunk.extend_from_slice(b"DJVUINFO");
    malformed_chunk.extend_from_slice(&u32::MAX.to_be_bytes());
    malformed_chunk.extend_from_slice(&[0xff; 10]);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty.djvu", Vec::new()),
        ("magic-only.djvu", b"AT&T".to_vec()),
        ("form-only.djvu", b"AT&TFORM".to_vec()),
        (
            "short-info.djv",
            b"AT&TFORM\0\0\0\x0bDJVUINFO\0\0\0\x03\x02\x58\x03".to_vec(),
        ),
        ("hostile-chunk-length.djvu", malformed_chunk),
        (
            "not-djvu-payload.djvu",
            b"not a document, only an extension hint".to_vec(),
        ),
    ];
    for (name, bytes) in cases {
        let path = write_fixture(name, &bytes);
        assert_boundary(&path, &Default::default());
    }
}

/// Prove real legacy standalone, bundle, indirect, and truncated fixtures stop.
///
/// Accepts no inputs and returns unit. Panics on any former geometry, native
/// text, BZZ, OCR, or raw-recovery behavior across default and full options.
#[test]
fn legacy_valid_and_invalid_fixtures_have_the_same_boundary() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    for name in [
        "single.djvu",
        "bundled.djvu",
        "indirect.djvu",
        "truncated.djvu",
    ] {
        let path = fixtures.join(name);
        for options in [
            InspectOptions::default(),
            InspectOptions {
                identify: true,
                max_pages: None,
                ..Default::default()
            },
        ] {
            assert_boundary(&path, &options);
        }
    }
}

/// Prove valid DjVu magic controls routing even under an unrelated extension.
///
/// Accepts no inputs and returns unit. Panics if magic detection falls through
/// to another handler or if any internal payload is read beyond identification.
#[test]
fn djvu_magic_under_another_extension_is_identified_without_extraction() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/single.djvu");
    let bytes = std::fs::read(source).unwrap();
    let path = write_fixture("djvu-magic.bin", &bytes);
    let report = assert_boundary(&path, &Default::default());
    assert_eq!(report.format.extension, "bin");
    assert_eq!(
        report.format.detected_mime.as_deref(),
        Some("image/vnd.djvu")
    );
    assert!(report.format.disagreement);
}
