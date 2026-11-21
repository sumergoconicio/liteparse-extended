//! End-to-end CLI regressions for the DjVu identification-only boundary.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const DIAGNOSTIC: &str = "DjVu extraction is unsupported; identification only";

/// Return an IFF chunk with a big-endian length and required padding byte.
///
/// Accepts a four-byte identifier and payload. Returns encoded bytes. The
/// length conversion is safe for the small deterministic fixtures used here.
fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

/// Build a standalone DjVu fixture containing formerly supported native text.
///
/// Accepts no inputs. Returns complete bytes with valid DjVu magic and TXTa.
/// The embedded marker is deliberately invalid for the identification boundary.
fn native_text_document() -> Vec<u8> {
    let text = b"HIDDEN_CLI_NATIVE_TEXT";
    let mut text_payload = (text.len() as u32).to_be_bytes()[1..].to_vec();
    text_payload.extend_from_slice(text);
    text_payload.push(0);
    let info: &[u8] = &[2, 88, 3, 32, 26, 0, 44, 1, 22, 1];
    let mut page = b"DJVU".to_vec();
    page.extend(chunk(b"INFO", info));
    page.extend(chunk(b"TXTa", &text_payload));
    let mut bytes = b"AT&T".to_vec();
    bytes.extend(chunk(b"FORM", &page));
    bytes
}

/// Return the stable ignored fixture directory for controller CLI checks.
///
/// Accepts no inputs. Returns the absolute path. No filesystem mutation occurs
/// until the caller explicitly creates the directory or a fixture.
fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/verification/djvu-identification-only/fixtures/cli")
}

/// Write the native-text CLI fixture under the authorized target directory.
///
/// Accepts no inputs and returns the fixture path. Panics on filesystem errors;
/// only the deterministic test-owned fixture is replaced on repeated runs.
fn native_text_fixture() -> PathBuf {
    let directory = fixture_directory();
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("native-text.djvu");
    std::fs::write(&path, native_text_document()).unwrap();
    path
}

/// Run the real `litx` binary with caller-supplied arguments and environment.
///
/// Accepts argument strings and an optional TESSDATA_PREFIX override. Returns
/// the captured process output. Panics only if the process cannot be spawned.
fn run_litx(args: &[&str], tessdata: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_glassbox"));
    command
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = tessdata {
        command.env("TESSDATA_PREFIX", path);
    }
    command.output().unwrap()
}

/// Parse a successful JSON CLI response without discarding process diagnostics.
///
/// Accepts captured process output. Returns the parsed report. Panics with
/// stderr on a failed status and with stdout on malformed JSON.
fn successful_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)))
}

/// Assert the serialized report honors every identification-only invariant.
///
/// Accepts a JSON report and whether the generic raster-only probe error is
/// expected. Returns unit. Panics on extraction output, DjVu analysis, OCR or
/// thumbnail stages, hidden native text, or a changed static diagnostic.
fn assert_cli_boundary(report: &Value, allow_probe_error: bool) {
    assert_eq!(report["format"]["bucket"], "Ebooks");
    assert_eq!(report["format"]["detected_mime"], "image/vnd.djvu");
    let errors = report["errors"].as_array().unwrap();
    let djvu_errors: Vec<_> = errors
        .iter()
        .filter(|error| error["stage"] == "djvu")
        .collect();
    assert_eq!(djvu_errors.len(), 1, "{errors:?}");
    assert_eq!(djvu_errors[0]["message"], DIAGNOSTIC);
    assert!(
        errors.iter().all(|error| {
            error["stage"] == "djvu" || (allow_probe_error && error["stage"] == "probe_ocr")
        }),
        "{errors:?}"
    );
    assert!(report["djvu"].is_null());
    assert_eq!(report["pages"], serde_json::json!([]));
    assert!(report["counts"]["pages"].is_null());
    assert_eq!(report["preflight"], serde_json::json!([]));
    assert_eq!(report["text"]["content"], "");
    assert_eq!(report["text"]["chars"], 0);
    assert_eq!(report["text"]["tokens"], 0);
    assert_eq!(report["counts"]["chars"], 0);
    assert_eq!(report["text"]["truncated"], true);
    assert_eq!(report["markdown"]["truncated"], true);
    assert!(
        !report["text"]["content"]
            .as_str()
            .unwrap()
            .contains("HIDDEN_CLI_NATIVE_TEXT")
    );
    assert!(
        !report["markdown"]["content"]
            .as_str()
            .unwrap()
            .contains("HIDDEN_CLI_NATIVE_TEXT")
    );
    assert!(report["thumbnail"].is_null());
}

/// Prove inspect, identify, OCR probe, extract, and thumbnail share one boundary.
///
/// Accepts no inputs and returns unit. Panics if any real CLI mode decodes the
/// native text, creates a thumbnail, invokes OCR, or omits partial-output flags.
#[test]
fn all_single_document_cli_modes_are_identification_only() {
    let path = native_text_fixture();
    let path = path.to_str().unwrap();
    let missing = fixture_directory().join("missing-tessdata");
    assert!(!missing.exists());
    for args in [
        vec!["inspect", path],
        vec!["inspect", "--identify", path],
        vec!["extract", "--no-ocr", path],
        vec!["extract", path],
    ] {
        let report = successful_json(&run_litx(&args, Some(&missing)));
        assert_cli_boundary(&report, false);
    }
    let report = successful_json(&run_litx(&["inspect", "--probe-ocr", path], Some(&missing)));
    assert_cli_boundary(&report, true);

    let thumbnail = fixture_directory().join("forbidden-cli-thumbnail.png");
    let thumbnail_arg = thumbnail.to_str().unwrap();
    let report = successful_json(&run_litx(
        &["inspect", "--identify", "--thumbnail", thumbnail_arg, path],
        Some(&missing),
    ));
    assert_cli_boundary(&report, false);
    assert!(!thumbnail.exists(), "DjVu thumbnail must not be created");
    assert!(!missing.exists(), "DjVu must not initialize an OCR model");
}

/// Prove a persistent batch returns the boundary then processes ordinary text.
///
/// Accepts no inputs and returns unit. Panics on a missing response, DjVu text
/// extraction, a poisoned session, or failure of the following plain-text file.
#[test]
fn batch_preserves_the_boundary_and_continues_with_the_next_file() {
    use std::io::Write;

    let path = native_text_fixture();
    let mut child = Command::new(env!("CARGO_BIN_EXE_glassbox"))
        .args(["extract", "--batch", "--no-ocr"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{}", serde_json::json!({"id":"djvu", "path":path})).unwrap();
    writeln!(
        input,
        "{}",
        serde_json::json!({"id":"next", "path":"../../fixtures/hello.txt"})
    )
    .unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], "djvu");
    assert_cli_boundary(&responses[0]["report"], false);
    assert_eq!(responses[1]["id"], "next");
    assert_eq!(responses[1]["report"]["format"]["bucket"], "Text");
    assert_eq!(responses[1]["report"]["errors"], serde_json::json!([]));
    assert_eq!(responses[1]["report"]["text"]["truncated"], false);
    assert!(
        !responses[1]["report"]["text"]["content"]
            .as_str()
            .unwrap()
            .is_empty()
    );
}
