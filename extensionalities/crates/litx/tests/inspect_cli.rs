//! CLI integration regressions for native inspection and extraction reports.
use std::process::Command;

#[test]
fn inspect_defaults_to_500_words_while_extract_preserves_all_text() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "word-default-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("words.txt");
    std::fs::write(&path, "word ".repeat(600)).unwrap();
    for (command, words) in [("inspect", 500), ("extract", 600)] {
        let output = Command::new(env!("CARGO_BIN_EXE_glassbox"))
            .arg(command)
            .arg(&path)
            .arg("--no-ocr")
            .output()
            .unwrap();
        assert!(output.status.success());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["errors"], serde_json::json!([]));
        assert_eq!(report["text"]["tokens"], words);
        assert_eq!(report["text"]["truncated"], command == "inspect");
    }
}

#[test]
fn batch_extract_persists_markdown_and_reports_for_mixed_documents() {
    use std::io::Write;
    use std::process::Stdio;
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "batch-output-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_glassbox"))
        .args(["extract", "--batch", "--no-ocr", "--output-dir"])
        .arg(&directory)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for name in ["notes.docx", "native.pdf", "grid.xlsx"] {
        writeln!(
            input,
            "{}",
            serde_json::json!({"path":format!("../../fixtures/{name}")})
        )
        .unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(reports.len(), 3);
    for response in &reports {
        let report = &response["report"];
        assert_eq!(report["errors"], serde_json::json!([]));
        assert_eq!(report["artifacts"].as_array().unwrap().len(), 2);
        for artifact in report["artifacts"].as_array().unwrap() {
            let bytes = std::fs::read(artifact["path"].as_str().unwrap()).unwrap();
            if artifact["kind"] == "markdown" {
                assert_eq!(
                    String::from_utf8(bytes).unwrap(),
                    report["markdown"]["content"]
                );
            } else {
                let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(saved["source_path"], report["source_path"]);
            }
        }
    }
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 6);
}

#[test]
fn inspect_defaults_to_five_pages_but_extract_uses_the_whole_document() {
    for (command, expected_pages) in [("inspect", 5), ("extract", 6)] {
        let output = Command::new(env!("CARGO_BIN_EXE_glassbox"))
            .args([command, "../../fixtures/multipage.pdf", "--no-ocr"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(r["errors"], serde_json::json!([]));
        assert_eq!(r["counts"]["pages"], 6);
        assert_eq!(
            r["pdf"]["geometry"].as_array().unwrap().len(),
            expected_pages
        );
        assert_eq!(r["pdf"]["text_complete"], command == "extract");
        assert_eq!(
            r["text"]["content"]
                .as_str()
                .unwrap()
                .contains("Final page confirms full-document extraction."),
            command == "extract"
        );
    }
}

#[test]
fn extract_with_explicit_max_pages_keeps_every_word_token() {
    // LITX-AUD-008: an explicit page limit must not imply the 500-word
    // identification preview budget during full extraction.
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "extract-pages-tokens-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("words.txt");
    std::fs::write(&path, "word ".repeat(600)).unwrap();
    let mut tokens = Vec::new();
    for args in [
        vec!["extract", "--no-ocr"],
        vec!["extract", "--no-ocr", "--max-pages", "6"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_glassbox"))
            .args(&args)
            .arg(&path)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(r["errors"], serde_json::json!([]));
        tokens.push(r["text"]["tokens"].as_u64().unwrap());
    }
    assert_eq!(tokens[0], 600);
    assert_eq!(tokens[1], 600, "{:?}", tokens);
}

#[test]
fn identify_office_text_obeys_an_explicit_word_token_budget() {
    for name in ["notes.docx", "deck.pptx", "grid.xlsx"] {
        let output = Command::new(env!("CARGO_BIN_EXE_glassbox"))
            .args([
                "inspect",
                &format!("../../fixtures/{name}"),
                "--identify",
                "--max-tokens",
                "3",
                "--packet",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(r["errors"], serde_json::json!([]));
        assert_eq!(r["text"]["tokens"], 3);
        assert_eq!(r["text"]["token_basis"], "whitespace_words");
        assert_eq!(
            r["text"]["content"]
                .as_str()
                .unwrap()
                .split_whitespace()
                .count(),
            3
        );
        assert_eq!(r["text"]["truncated"], true);
        assert_eq!(r["packet"]["excerpt"], r["text"]["content"]);
        if name == "notes.docx" {
            assert_eq!(r["text"]["content"], "This is a");
        }
    }
}

#[test]
fn batch_rejects_oversized_lines_without_losing_the_next_request() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_glassbox"))
        .args(["inspect", "--batch"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{}", serde_json::json!({"path":"x".repeat(70_000)})).unwrap();
    writeln!(
        input,
        "{}",
        serde_json::json!({"id":9,"path":"../../fixtures/hello.txt"})
    )
    .unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 2);
    assert!(
        responses[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("65536")
    );
    assert_eq!(responses[1]["id"], 9);
}

#[test]
fn batch_flushes_each_response_before_stdin_closes() {
    use std::io::{BufRead, Write};
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_glassbox"))
        .args(["inspect", "--batch"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .unwrap();
        let _ = sender.send(line);
    });
    writeln!(
        input,
        "{}",
        serde_json::json!({"path":"../../fixtures/hello.txt"})
    )
    .unwrap();
    input.flush().unwrap();
    let response = receiver.recv_timeout(std::time::Duration::from_secs(10));
    drop(input);
    if response.is_err() {
        let _ = child.kill();
    }
    child.wait().unwrap();
    reader.join().unwrap();
    let response: serde_json::Value =
        serde_json::from_str(&response.expect("worker must respond before EOF")).unwrap();
    assert_eq!(response["report"]["format"]["bucket"], "Text");
}

#[test]
fn batch_correlates_requests_and_continues_after_bad_input() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_glassbox"))
        .args(["inspect", "--batch"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(
        input,
        "{}",
        serde_json::json!({"id":"first", "path":"../../fixtures/hello.txt"})
    )
    .unwrap();
    writeln!(input, "not json").unwrap();
    writeln!(
        input,
        "{}",
        serde_json::json!({"id":2, "path":"missing-test-file"})
    )
    .unwrap();
    writeln!(
        input,
        "{}",
        serde_json::json!({"id":3, "path":"../../fixtures/hello.txt"})
    )
    .unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0]["id"], "first");
    assert_eq!(responses[0]["report"]["format"]["bucket"], "Text");
    assert_eq!(responses[1]["error"]["stage"], "batch");
    assert_eq!(responses[2]["id"], 2);
    assert_eq!(responses[2]["report"]["errors"][0]["stage"], "input");
    assert_eq!(responses[3]["id"], 3);
    assert_eq!(responses[3]["report"]["errors"], serde_json::json!([]));
}

#[test]
fn inspect_emits_a_report_and_optional_packet() {
    let output = Command::new(env!("CARGO_BIN_EXE_glassbox"))
        .args([
            "inspect",
            "../../fixtures/hello.txt",
            "--packet",
            "--pretty",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["counts"]["bytes"],
        include_bytes!("../../../fixtures/hello.txt").len()
    );
    assert_eq!(report["packet"]["Format"], "Text");
    assert!(report["claims"].is_array());
    assert!(report["errors"].is_array());
    assert!(report["packet"].get("Domains").is_none());
}

/// Verify missing OCR data still fails PDF honestly while DjVu never requests OCR.
///
/// Accepts no inputs and returns unit. Panics if the PDF loses its OCR failure
/// provenance or DjVu initializes OCR, emits analysis, or misses the static
/// identification-only diagnostic.
#[test]
fn missing_ocr_model_marks_pdf_incomplete_and_djvu_stays_identification_only() {
    // LITX-AUD-010: a failed PDF OCR attempt must surface as incomplete text,
    // never as a clean complete report. DjVu has an independent static support
    // boundary. The per-process TESSDATA_PREFIX isolates parallel OCR tests.
    let missing = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "missing-tessdata-prov-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    assert!(!missing.exists());
    // native.pdf stays within the five-page scan limit, so incomplete flags
    // can only come from the failed OCR attempt itself.
    let output = Command::new(env!("CARGO_BIN_EXE_glassbox"))
        .args(["inspect", "--identify", "../../fixtures/native.pdf"])
        .env("TESSDATA_PREFIX", &missing)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        r["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["stage"] == "pdf_ocr"),
        "{:?}",
        r["errors"]
    );
    assert_eq!(r["pdf"]["text_complete"], false);
    assert_eq!(r["text"]["truncated"], true);
    assert!(
        r["pdf"]["identification_pages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["ocr_status"] == "failed")
    );
    // DjVu: the same requested identification mode must not consult tessdata.
    let info: &[u8] = &[2, 88, 3, 32, 26, 0, 44, 1, 22, 1];
    let mut page = b"DJVU".to_vec();
    page.extend_from_slice(b"INFO");
    page.extend_from_slice(&(info.len() as u32).to_be_bytes());
    page.extend_from_slice(info);
    let mut form = b"FORM".to_vec();
    form.extend_from_slice(&(page.len() as u32).to_be_bytes());
    form.extend_from_slice(&page);
    let mut bytes = b"AT&T".to_vec();
    bytes.extend_from_slice(&form);
    let djvu = missing.with_file_name(format!(
        "blank-{}.djvu",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&djvu, bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_glassbox"))
        .args(["inspect", "--identify"])
        .arg(&djvu)
        .env("TESSDATA_PREFIX", &missing)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let errors = r["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["stage"], "djvu");
    assert_eq!(
        errors[0]["message"],
        "DjVu extraction is unsupported; identification only"
    );
    assert!(r["djvu"].is_null());
    assert_eq!(r["pages"], serde_json::json!([]));
    assert_eq!(r["preflight"], serde_json::json!([]));
    assert_eq!(r["text"]["content"], "");
    assert_eq!(r["text"]["truncated"], true);
    assert_eq!(r["markdown"]["truncated"], true);
    assert!(r["thumbnail"].is_null());
    assert!(!missing.exists());
}

#[test]
fn missing_local_ocr_model_keeps_native_metrics_without_downloads() {
    let missing = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "missing-tessdata-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    assert!(!missing.exists());
    let output = Command::new(env!("CARGO_BIN_EXE_glassbox"))
        .args([
            "inspect",
            "../../fixtures/document-image.png",
            "--probe-ocr",
        ])
        .env("TESSDATA_PREFIX", &missing)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["image"]["classification"], "document_scan_like");
    assert!(report["image"]["probe"].is_null());
    assert!(report["errors"].as_array().unwrap().iter().any(|e| {
        e["stage"] == "probe_ocr"
            && e["message"]
                .as_str()
                .unwrap()
                .contains("local eng.traineddata")
    }));
    assert!(!missing.exists());
}
