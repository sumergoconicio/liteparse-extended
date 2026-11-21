//! Parquet identification-only regression tests. On 2026-11-18 the user approved
//! deferring Parquet extraction ("defer extraction, make a note to return to it
//! later"): inspect and extract keep format identification but never parse a
//! footer, schema, row, column, or page. Every Parquet input receives the static
//! deferral diagnostic with incomplete empty content. These tests pin that
//! contract for valid, malformed, extension-routed, and magic-routed inputs.
use litx_inspect::{InspectOptions, inspect};

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

fn workdir(tag: &str) -> std::path::PathBuf {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "parquet-defer-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// Asserts the user-approved identification-only contract: one static deferral
/// diagnostic, no Parquet profile or dataset, and honest incomplete empty text.
fn assert_deferred(report: &litx_inspect::InspectReport) {
    let deferrals: Vec<_> = report
        .errors
        .iter()
        .filter(|e| e.stage == "parquet" && e.message == DEFERRAL)
        .collect();
    assert_eq!(
        deferrals.len(),
        1,
        "exactly one static deferral diagnostic: {:?}",
        report.errors
    );
    assert!(
        report.parquet.is_none(),
        "no footer/schema profile may be produced"
    );
    assert!(report.dataset.is_none(), "no row preview may be produced");
    assert!(report.text.content.is_empty(), "no extracted text");
    assert!(
        report.text.truncated,
        "omitted text must be marked incomplete"
    );
    assert!(
        report.markdown.truncated,
        "markdown must report partial output"
    );
}

#[test]
fn valid_parquet_is_identified_but_extraction_is_deferred() {
    let directory = workdir("valid");
    let path = directory.join("empty.parquet");
    std::fs::write(&path, valid_parquet_bytes()).unwrap();
    let report = inspect(&path, &InspectOptions::default());
    assert_deferred(&report);
    assert_eq!(
        report.format.detected_mime.as_deref(),
        Some("application/vnd.apache.parquet"),
        "format identification is retained"
    );
}

#[test]
fn malformed_parquet_receives_the_same_static_deferral() {
    let directory = workdir("malformed");
    let path = directory.join("broken.parquet");
    // PAR1 magic with a hostile garbage tail: no footer parsing may occur, so
    // the diagnostic stays static instead of reporting decoder details.
    let mut bytes = b"PAR1".to_vec();
    bytes.extend_from_slice(&[0xff; 200]);
    std::fs::write(&path, bytes).unwrap();
    let report = inspect(&path, &InspectOptions::default());
    assert_deferred(&report);
    assert_eq!(
        report.format.detected_mime.as_deref(),
        Some("application/vnd.apache.parquet"),
        "magic-routed identification is retained"
    );
}

#[test]
fn extension_routed_non_parquet_bytes_also_defer_without_decoding() {
    let directory = workdir("extension");
    let path = directory.join("mislabeled.parquet");
    std::fs::write(&path, b"this is not a parquet file").unwrap();
    let report = inspect(&path, &InspectOptions::default());
    assert_deferred(&report);
    assert_eq!(report.format.extension, "parquet");
}

/// A genuinely valid file with real data rows from the locked writer must also
/// receive the deferral: the row backend stays unregistered even when the
/// `parquet-preview` feature (kept as compatibility reference) is enabled.
#[cfg(feature = "parquet-preview")]
#[test]
fn writer_produced_rows_are_never_decoded_under_deferral() {
    use parquet_reader::{
        data_type::Int64Type,
        file::{properties::WriterProperties, writer::SerializedFileWriter},
        schema::parser::parse_message_type,
    };
    use std::sync::Arc;
    let directory = workdir("rows");
    let path = directory.join("rows.parquet");
    let schema = Arc::new(parse_message_type("message rows { REQUIRED INT64 id; }").unwrap());
    let mut writer = SerializedFileWriter::new(
        std::fs::File::create(&path).unwrap(),
        schema,
        Arc::new(WriterProperties::default()),
    )
    .unwrap();
    let mut group = writer.next_row_group().unwrap();
    let mut column = group.next_column().unwrap().unwrap();
    column
        .typed::<Int64Type>()
        .write_batch(&[1, 2, 3], None, None)
        .unwrap();
    column.close().unwrap();
    group.close().unwrap();
    writer.close().unwrap();
    let report = inspect(&path, &InspectOptions::default());
    assert_deferred(&report);
    let json = serde_json::to_string(&report).unwrap();
    assert!(
        !json.contains("\"rows\":3"),
        "no row counts from the footer may be reported: {json}"
    );
}
