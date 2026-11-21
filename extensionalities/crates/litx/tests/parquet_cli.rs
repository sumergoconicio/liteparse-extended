//! V-PARQUET-DEFER-CLI: CLI transport parity for the user-approved Parquet
//! support reduction (2026-11-18). Inspect and extract both retain format
//! identification, emit the static deferral diagnostic with incomplete empty
//! content, and a persistent batch recovers across valid Parquet, malformed
//! Parquet, and ordinary text inputs.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const DEFERRAL: &str = "Parquet extraction is deferred; identification only";

/// Compact-Thrift FileMetaData: version 1, required schema root and two primitive
/// leaves, zero rows/row groups. A valid footer envelope around an empty data
/// section; no column data pages exist.
fn valid_parquet_bytes() -> Vec<u8> {
    let footer: &[u8] = b"\x15\x02\x19\x3c\x48\x06schema\x15\x04\x00\x15\x04\x25\x00\x18\x02id\x00\x15\x0c\x25\x02\x18\x04name\x00\x16\x00\x19\x0c\x00";
    let mut bytes = b"PAR1".to_vec();
    bytes.extend_from_slice(footer);
    bytes.extend_from_slice(&(footer.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"PAR1");
    bytes
}

fn workdir(tag: &str) -> PathBuf {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "parquet-cli-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn write(directory: &std::path::Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn litx(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_glassbox"));
    command.args(args).current_dir(env!("CARGO_MANIFEST_DIR"));
    command
}

/// Asserts the identification-only contract on one CLI JSON report.
fn assert_deferred(report: &serde_json::Value) {
    let errors = report["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "one static diagnostic: {errors:?}");
    assert_eq!(errors[0]["stage"], "parquet");
    assert_eq!(errors[0]["message"], DEFERRAL, "the diagnostic is static");
    assert!(report["parquet"].is_null(), "no footer/schema profile");
    assert!(report["dataset"].is_null(), "no row preview");
    assert_eq!(report["text"]["content"], "", "no extracted text");
    assert_eq!(
        report["text"]["truncated"], true,
        "omitted text must be marked incomplete"
    );
    assert_eq!(
        report["markdown"]["truncated"], true,
        "markdown must report partial output"
    );
    assert_eq!(
        report["format"]["detected_mime"], "application/vnd.apache.parquet",
        "format identification is retained"
    );
}

#[test]
fn parquet_inspect_and_extract_both_defer_extraction() {
    let directory = workdir("parity");
    let path = write(&directory, "empty.parquet", &valid_parquet_bytes());
    let path = path.to_str().unwrap();
    let inspect = litx(&["inspect", path, "--no-ocr"]).output().unwrap();
    assert!(
        inspect.status.success(),
        "document-level stage errors keep the aggregate exit code 0: {}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    let extract = litx(&["extract", path, "--no-ocr"]).output().unwrap();
    assert!(
        extract.status.success(),
        "extract keeps the aggregate exit code 0: {}",
        String::from_utf8_lossy(&extract.stderr)
    );
    let fresh: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    let full: serde_json::Value = serde_json::from_slice(&extract.stdout).unwrap();
    assert_deferred(&fresh);
    assert_deferred(&full);
    assert_eq!(
        fresh["errors"], full["errors"],
        "inspect and extract publish the identical static deferral"
    );
}

#[test]
fn malformed_parquet_defers_with_exit_zero_and_static_message() {
    let directory = workdir("malformed");
    // PAR1 magic with a hostile garbage tail; no footer parsing may occur.
    let mut bytes = b"PAR1".to_vec();
    bytes.extend_from_slice(&[0xff; 200]);
    let path = write(&directory, "broken.parquet", &bytes);
    let output = litx(&["inspect", path.to_str().unwrap(), "--no-ocr"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "document-level stage errors keep the aggregate exit code 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_deferred(&report);
}

#[test]
fn parquet_batch_recovers_across_valid_malformed_and_text_inputs() {
    let directory = workdir("batch");
    let valid = write(&directory, "valid.parquet", &valid_parquet_bytes());
    let mut malformed = b"PAR1".to_vec();
    malformed.extend_from_slice(&[0xff; 200]);
    let broken = write(&directory, "broken.parquet", &malformed);
    let text = write(&directory, "notes.txt", b"ordinary text survives the batch");
    let mut child = litx(&["inspect", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{}", serde_json::json!({"id": "p1", "path": valid})).unwrap();
    writeln!(input, "{}", serde_json::json!({"id": 2, "path": broken})).unwrap();
    writeln!(input, "{}", serde_json::json!({"id": "t3", "path": text})).unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stage errors must not fail the batch: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 3, "one response per request, in order");
    assert_eq!(responses[0]["id"], "p1");
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[2]["id"], "t3");
    assert_deferred(&responses[0]["report"]);
    assert_deferred(&responses[1]["report"]);
    assert_eq!(
        responses[2]["report"]["errors"],
        serde_json::json!([]),
        "the worker recovers cleanly after deferred Parquet inputs"
    );
    assert!(
        responses[2]["report"]["text"]["content"]
            .as_str()
            .unwrap()
            .contains("ordinary text survives"),
        "ordinary text still extracts after Parquet deferrals"
    );
}
