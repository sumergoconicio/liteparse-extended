#[cfg(feature = "magika")]
#[test]
fn missing_onnx_runtime_degrades_the_report_without_downloading() {
    if std::env::var_os("ORT_DYLIB_PATH").is_some() {
        return;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hello.txt");
    let report = litx_inspect::inspect(&path, &Default::default());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.stage == "magika" && e.message.contains("ORT_DYLIB_PATH"))
    );
    assert_eq!(report.format.detector, "file-format");
}
