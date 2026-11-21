use litx_inspect::{InspectOptions, inspect};
use std::{io::Write, path::PathBuf};

#[test]
fn image_and_pdf_encoded_byte_caps_fail_without_creating_outputs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../");
    let directory = root.join("target/tests").join(format!(
        "raster-cap-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    for (name, fixture, stage) in [
        ("large.png", "image.png", "image"),
        ("large.pdf", "native.pdf", "pdf"),
    ] {
        let path = directory.join(name);
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.write_all(&std::fs::read(root.join("fixtures").join(fixture)).unwrap()[..32])
            .unwrap();
        file.set_len((16 << 20) + 1).unwrap();
        drop(file);
        let output = directory.join(format!("{name}.preview.png"));
        let r = inspect(
            &path,
            &InspectOptions {
                thumbnail: Some(output.clone()),
                ..Default::default()
            },
        );
        assert!(
            r.errors
                .iter()
                .any(|e| e.stage == stage && e.message.contains("byte cap")),
            "{:?}",
            r.errors
        );
        assert!(!output.exists());
    }
    let path = directory.join("wide.png");
    image::RgbImage::new(16_385, 1).save(&path).unwrap();
    let r = inspect(&path, &Default::default());
    assert!(r.errors.iter().any(|e| e.stage == "image"));
    assert!(r.image.is_none());
}
