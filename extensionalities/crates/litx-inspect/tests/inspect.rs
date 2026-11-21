use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn bytes_win_with_explicit_extension_disagreement() {
    let r = inspect(&fixture("disguised.pdf"), &InspectOptions::default());
    assert_eq!(r.format.bucket, "Images");
    assert_eq!(r.format.detected_mime.as_deref(), Some("image/png"));
    assert!(r.format.disagreement);
    assert_eq!(r.format.basis, "bytes");
}

#[test]
fn archive_caps_and_truncation_are_partial_report_errors() {
    for (name, message) in [
        ("bomb.zip", "total-uncompressed"),
        ("entry-cap.zip", "entry-count"),
        ("truncated.zip", ""),
    ] {
        let r = inspect(&fixture(name), &InspectOptions::default());
        assert!(
            r.errors
                .iter()
                .any(|e| e.stage == "archive" && e.message.contains(message)),
            "{name}: {:?}",
            r.errors
        );
        assert!(r.counts.bytes > 0);
    }
}

#[test]
fn zip_lists_untrusted_names_without_extracting() {
    let r = inspect(&fixture("listing.zip"), &InspectOptions::default());
    assert_eq!(r.counts.entries, Some(2));
    assert_eq!(r.entries[0].name, "note.txt");
    assert_eq!(r.entries[0].size, 14);
    assert_eq!(
        r.entries[0].timestamp.as_deref(),
        Some("2026-01-02T03:04:06")
    );
    assert!(r.entries.iter().any(|e| e.name == "../escape.txt"));
    assert!(r.errors.iter().any(|e| e.message.contains("unsafe")));
    assert_eq!(r.format.basis, "bytes");
    assert_eq!(r.format.bucket, "Archives");
}

#[test]
fn thumbnail_downscales_verified_image_and_never_overwrites() {
    let dir = fixture("../target/tests");
    std::fs::create_dir_all(&dir).unwrap();
    let output = dir.join(format!(
        "thumb-{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let opts = InspectOptions {
        thumbnail: Some(output.clone()),
        ..Default::default()
    };
    let r = inspect(&fixture("image.png"), &opts);
    let thumbnail = r.thumbnail.unwrap();
    assert_eq!((thumbnail.width, thumbnail.height), (320, 200));
    assert_eq!(thumbnail.basis, "downscaled");
    let before = std::fs::read(&output).unwrap();
    let r = inspect(&fixture("image.png"), &opts);
    assert!(r.errors.iter().any(|e| e.stage == "thumbnail"));
    assert_eq!(std::fs::read(output).unwrap(), before);
}
