//! V-XML-05: standalone CLI transport parity for XML structural profiling —
//! fresh versus persistent modes, valid-invalid-valid recovery, pre-EOF flush
//! and the aggregate exit-code convention for document-level stage errors.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn workdir(tag: &str) -> PathBuf {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "xml-cli-{tag}-{}",
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

const VALID: &[u8] = b"<catalog><book id=\"SENTINEL-V1\">rust</book><book>xml</book></catalog>";
const INVALID: &[u8] = b"<a>SENTINEL-BAD1&undefined;</a>";

fn litx(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_glassbox"));
    command.args(args).current_dir(env!("CARGO_MANIFEST_DIR"));
    command
}

#[test]
fn xml_inspect_and_extract_publish_identical_profiles_without_values() {
    let directory = workdir("parity");
    let path = write(&directory, "catalog.xml", VALID);
    let path = path.to_str().unwrap();
    let inspect = litx(&["inspect", path, "--no-ocr"]).output().unwrap();
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    let extract = litx(&["extract", path, "--no-ocr"]).output().unwrap();
    assert!(
        extract.status.success(),
        "{}",
        String::from_utf8_lossy(&extract.stderr)
    );
    let fresh: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    let full: serde_json::Value = serde_json::from_slice(&extract.stdout).unwrap();
    assert_eq!(fresh["errors"], serde_json::json!([]));
    assert_eq!(full["errors"], serde_json::json!([]));
    assert_eq!(
        fresh["xml"], full["xml"],
        "inspect and extract publish the identical structural profile"
    );
    assert_eq!(fresh["xml"]["total_elements"], 3);
    assert_eq!(fresh["xml"]["paths"].as_array().unwrap().len(), 2);
    for output in [&inspect.stdout, &extract.stdout] {
        let text = String::from_utf8_lossy(output);
        assert!(!text.contains("SENTINEL"), "no attribute values leak");
        assert!(!text.contains("rust"), "no text values leak");
    }
}

#[test]
fn xml_fresh_cli_fails_closed_on_invalid_input_with_exit_zero() {
    let directory = workdir("invalid");
    let path = write(&directory, "bad.xml", INVALID);
    let output = litx(&["inspect", path.to_str().unwrap(), "--no-ocr"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "document-level stage errors keep the aggregate exit code 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["errors"][0]["stage"], "xml");
    assert!(report["xml"].is_null(), "no profile on invalid input");
    // Framework convention: markdown.truncated signals incompleteness from a
    // stage error; text.truncated only means text content was clipped.
    assert_eq!(report["markdown"]["truncated"], true);
    assert_eq!(report["text"]["truncated"], false);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        !text.contains("SENTINEL"),
        "content-free diagnostics: {text}"
    );
    assert!(
        !report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains('<')
    );
}

#[test]
fn xml_batch_recovers_valid_invalid_valid_and_matches_fresh_transport() {
    let directory = workdir("batch");
    let good = write(&directory, "good.xml", VALID);
    let bad = write(&directory, "bad.xml", INVALID);
    let mut child = litx(&["inspect", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{}", serde_json::json!({"id": "v1", "path": good})).unwrap();
    writeln!(input, "{}", serde_json::json!({"id": 2, "path": bad})).unwrap();
    writeln!(input, "{}", serde_json::json!({"id": "v3", "path": good})).unwrap();
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
    assert_eq!(responses[0]["id"], "v1");
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[2]["id"], "v3");
    assert_eq!(responses[0]["report"]["errors"], serde_json::json!([]));
    assert_eq!(responses[1]["report"]["errors"][0]["stage"], "xml");
    assert!(
        responses[1]["report"]["xml"].is_null(),
        "invalid input publishes no profile"
    );
    assert_eq!(responses[2]["report"]["errors"], serde_json::json!([]));
    assert_eq!(
        responses[0]["report"]["xml"], responses[2]["report"]["xml"],
        "the worker recovers cleanly after an invalid document"
    );
    // Persistent transport parity: the batch profile equals a fresh run's.
    let fresh = litx(&["inspect", good.to_str().unwrap(), "--no-ocr"])
        .output()
        .unwrap();
    let fresh: serde_json::Value = serde_json::from_slice(&fresh.stdout).unwrap();
    assert_eq!(responses[0]["report"]["xml"], fresh["xml"]);
    let text: String = responses
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect();
    assert!(!text.contains("SENTINEL"), "no values leak through batch");
}

#[test]
fn xml_batch_flushes_structural_response_before_eof() {
    use std::io::BufRead;
    let directory = workdir("flush");
    let good = write(&directory, "good.xml", VALID);
    let mut child = litx(&["inspect", "--batch"])
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
        serde_json::json!({"id": "xml-flush", "path": good})
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
        serde_json::from_str(&response.expect("XML batch response must arrive before EOF"))
            .unwrap();
    assert_eq!(response["id"], "xml-flush");
    assert_eq!(response["report"]["xml"]["total_elements"], 3);
}

#[test]
fn xml_declaration_errors_stay_content_free_through_cli_transport() {
    let directory = workdir("decl-leak");
    let path = write(
        &directory,
        "decl.xml",
        b"<?xml version=\"1.0\" SECRETATTR=\"x\"?><a/>",
    );
    let output = litx(&["inspect", path.to_str().unwrap(), "--no-ocr"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "document-level stage errors keep exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["errors"][0]["stage"], "xml");
    assert!(report["xml"].is_null());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        !text.contains("SECRETATTR"),
        "no declaration attribute names leak: {text}"
    );
}
