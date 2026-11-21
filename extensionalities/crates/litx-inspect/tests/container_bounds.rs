use litx_inspect::{InspectOptions, inspect};
use std::path::PathBuf;

#[test]
fn truncated_and_mutated_native_containers_never_panic_or_overrun_preview() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../");
    let output = root.join("target/tests").join(format!(
        "container-mutations-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&output).unwrap();
    for name in [
        "native.mobi",
        "native.epub",
        "pages.cbz",
        "pages4.cbr",
        "pages5.cbr",
        "bundled.djvu",
    ] {
        let bytes = std::fs::read(root.join("fixtures").join(name)).unwrap();
        let mut cases: Vec<Vec<u8>> = (0..bytes.len())
            .step_by((bytes.len() / 32).max(1))
            .map(|n| bytes[..n].to_vec())
            .collect();
        for position in (0..bytes.len()).step_by((bytes.len() / 32).max(1)) {
            let mut mutated = bytes.clone();
            mutated[position] ^= 0xff;
            cases.push(mutated);
        }
        for (i, data) in cases.into_iter().enumerate() {
            let path = output.join(format!("{i}-{name}"));
            std::fs::write(&path, &data).unwrap();
            let r = std::panic::catch_unwind(|| {
                inspect(
                    &path,
                    &InspectOptions {
                        max_chars: 3,
                        max_units: 0,
                        ..Default::default()
                    },
                )
            })
            .unwrap_or_else(|_| panic!("{name} mutation {i} panicked"));
            assert_eq!(r.counts.bytes, data.len() as u64);
            assert!(r.text.chars <= 3);
        }
    }
}
