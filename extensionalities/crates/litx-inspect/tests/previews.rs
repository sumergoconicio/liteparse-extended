use litx_inspect::{InspectOptions, inspect};
use std::{io::Read, path::PathBuf};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[test]
fn embedded_preview_under_edge_cap_is_byte_identical() {
    let output = fixture("../target/tests").join(format!(
        "embedded-{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    let r = inspect(
        &fixture("notes.odt"),
        &InspectOptions {
            thumbnail: Some(output.clone()),
            ..Default::default()
        },
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let thumb = r.thumbnail.unwrap();
    assert_eq!(thumb.basis, "embedded");
    assert_eq!((thumb.width, thumb.height), (64, 40));
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(fixture("notes.odt")).unwrap()).unwrap();
    let mut expected = vec![];
    archive
        .by_name("Thumbnails/thumbnail.png")
        .unwrap()
        .read_to_end(&mut expected)
        .unwrap();
    assert_eq!(std::fs::read(output).unwrap(), expected);
}

#[test]
fn preview_limit_counts_unicode_scalars_and_does_not_limit_structure() {
    let r = inspect(
        &fixture("notes.odt"),
        &InspectOptions {
            max_chars: 4,
            max_units: 0,
            ..Default::default()
        },
    );
    assert_eq!(r.text.content, "Café");
    assert_eq!(r.text.chars, 4);
    assert!(r.text.truncated);
    assert!(r.counts.chars > 4);
    assert_eq!(r.counts.paragraphs, Some(2));
    assert!(r.preflight.is_empty());
}

#[test]
fn ebook_and_comic_covers_preserve_bytes_and_never_overwrite() {
    for name in ["native.epub", "native.mobi", "native.azw3", "pages.cbz"] {
        let output = fixture("../target/tests").join(format!(
            "cover-{name}-{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(output.parent().unwrap()).unwrap();
        let options = InspectOptions {
            thumbnail: Some(output.clone()),
            ..Default::default()
        };
        let r = inspect(&fixture(name), &options);
        assert!(r.errors.is_empty(), "{name}: {:?}", r.errors);
        let thumbnail = r.thumbnail.unwrap();
        assert_eq!((thumbnail.width, thumbnail.height), (64, 40));
        let first = std::fs::read(&output).unwrap();
        let expected = if name.ends_with("mobi") || name.ends_with("azw3") {
            let bytes = std::fs::read(fixture(name)).unwrap();
            let offset = u32::from_be_bytes(bytes[94..98].try_into().unwrap()) as usize;
            bytes[offset..].to_vec()
        } else {
            let mut zip =
                zip::ZipArchive::new(std::fs::File::open(fixture(name)).unwrap()).unwrap();
            let mut expected = vec![];
            zip.by_name(if name.ends_with("epub") {
                "Book/images/cover.png"
            } else {
                "02.png"
            })
            .unwrap()
            .read_to_end(&mut expected)
            .unwrap();
            expected
        };
        assert_eq!(first, expected);
        let repeated = inspect(&fixture(name), &options);
        assert!(repeated.errors.iter().any(|e| e.stage == "thumbnail"));
        assert_eq!(std::fs::read(output).unwrap(), first);
    }
}
