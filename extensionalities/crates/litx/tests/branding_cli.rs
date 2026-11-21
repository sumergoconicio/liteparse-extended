use std::io::Write;
use std::process::{Command, Stdio};

const BINARIES: [&str; 2] = [env!("CARGO_BIN_EXE_glassbox"), env!("CARGO_BIN_EXE_litx")];

#[test]
fn both_commands_expose_glassbox_branding_and_the_same_version() {
    let mut versions = Vec::new();
    for binary in BINARIES {
        let help = Command::new(binary).arg("--help").output().unwrap();
        assert!(help.status.success());
        let help = String::from_utf8(help.stdout).unwrap();
        assert!(help.contains("What's in the box?!"));
        for command in ["inspect", "extract", "defaults"] {
            assert!(help.contains(command));
        }
        let version = Command::new(binary).arg("--version").output().unwrap();
        assert!(version.status.success());
        let version = String::from_utf8(version.stdout).unwrap();
        assert!(version.starts_with("glassbox "));
        versions.push(version);
    }
    assert_eq!(versions[0], versions[1]);
}

#[test]
fn compatibility_command_preserves_inspection_extraction_and_defaults() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hello.txt");
    for arguments in [
        vec!["inspect".as_ref(), fixture.as_os_str()],
        vec!["extract".as_ref(), fixture.as_os_str()],
        vec!["defaults".as_ref()],
    ] {
        let reports: Vec<serde_json::Value> = BINARIES
            .iter()
            .map(|binary| {
                let output = Command::new(binary).args(&arguments).output().unwrap();
                assert!(output.status.success());
                serde_json::from_slice(&output.stdout).unwrap()
            })
            .collect();
        assert_eq!(reports[0], reports[1]);
    }
}

#[test]
fn compatibility_command_preserves_batch_responses_and_request_errors() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hello.txt");
    let mut responses = Vec::new();
    for binary in BINARIES {
        let mut child = Command::new(binary)
            .args(["inspect", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        writeln!(input, "{{not-json").unwrap();
        for id in ["first", "repeat"] {
            writeln!(input, "{}", serde_json::json!({"id": id, "path": fixture})).unwrap();
        }
        drop(input);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let lines: Vec<serde_json::Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0]["error"].is_object());
        assert_eq!(lines[1]["id"], "first");
        assert_eq!(lines[2]["id"], "repeat");
        assert_eq!(lines[1]["report"], lines[2]["report"]);
        responses.push(lines);
    }
    assert_eq!(responses[0], responses[1]);
}
