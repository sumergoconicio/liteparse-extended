#![cfg(all(feature = "parquet-preview", not(target_arch = "wasm32")))]

//! Hostile Parquet chunk fixtures retained from the audit remediation. Under the
//! user-approved 2026-11-18 support reduction, Parquet extraction is deferred:
//! these external-path, unsupported-codec, and oversized-page cases must receive
//! the static identification-only deferral without any chunk metadata or page
//! bytes being read. They no longer pin decoder-specific rejection messages.

use litx_inspect::{InspectOptions, inspect};
use parquet_format_safe::{
    CompressionCodec, FileMetaData,
    thrift::protocol::{TCompactInputProtocol, TCompactOutputProtocol},
};
use parquet_reader::{
    data_type::Int64Type,
    file::{properties::WriterProperties, writer::SerializedFileWriter},
    schema::parser::parse_message_type,
};
use std::{path::PathBuf, sync::Arc};

fn seed() -> (Vec<u8>, FileMetaData) {
    let schema = Arc::new(parse_message_type("message rows { REQUIRED INT64 id; }").unwrap());
    let mut writer =
        SerializedFileWriter::new(Vec::new(), schema, Arc::new(WriterProperties::default()))
            .unwrap();
    let mut group = writer.next_row_group().unwrap();
    let mut column = group.next_column().unwrap().unwrap();
    column
        .typed::<Int64Type>()
        .write_batch(&[1], None, None)
        .unwrap();
    column.close().unwrap();
    group.close().unwrap();
    let bytes = writer.into_inner().unwrap();
    let footer_size =
        u32::from_le_bytes(bytes[bytes.len() - 8..bytes.len() - 4].try_into().unwrap()) as usize;
    let footer_start = bytes.len() - 8 - footer_size;
    let metadata = FileMetaData::read_from_in_protocol(&mut TCompactInputProtocol::new(
        &bytes[footer_start..bytes.len() - 8],
        1 << 20,
    ))
    .unwrap();
    (bytes[..footer_start].to_vec(), metadata)
}

#[test]
fn hostile_parquet_chunks_defer_without_reading_chunk_metadata_or_pages() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tests")
        .join(format!(
            "parquet-bounds-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).unwrap();
    for case in ["external", "codec", "page"] {
        let (mut data, mut metadata) = seed();
        let chunk = &mut metadata.row_groups[0].columns[0];
        match case {
            "external" => chunk.file_path = Some("/not-opened/sensitive.parquet".into()),
            "codec" => chunk.meta_data.as_mut().unwrap().codec = CompressionCodec::GZIP,
            _ => {
                // DATA_PAGE, i32::MAX expanded bytes, zero compressed bytes.
                let header = b"\x15\x00\x15\xfe\xff\xff\xff\x0f\x15\x00\x00";
                data = b"PAR1".to_vec();
                data.extend_from_slice(header);
                let meta = chunk.meta_data.as_mut().unwrap();
                meta.data_page_offset = 4;
                meta.dictionary_page_offset = None;
                meta.total_compressed_size = header.len() as i64;
            }
        }
        let mut footer = Vec::new();
        metadata
            .write_to_out_protocol(&mut TCompactOutputProtocol::new(&mut footer))
            .unwrap();
        data.extend_from_slice(&footer);
        data.extend_from_slice(&(footer.len() as u32).to_le_bytes());
        data.extend_from_slice(b"PAR1");
        let path = directory.join(format!("{case}.parquet"));
        std::fs::write(&path, data).unwrap();
        let report = inspect(&path, &InspectOptions::default());
        assert!(
            report.errors.iter().any(|e| e.stage == "parquet"
                && e.message == "Parquet extraction is deferred; identification only"),
            "{case}: static deferral expected, got {:?}",
            report.errors
        );
        assert!(report.parquet.is_none(), "{case}: no footer profile");
        assert!(report.dataset.is_none(), "{case}: no row preview");
        assert!(
            report.text.content.is_empty() && report.text.truncated,
            "{case}: incomplete empty text"
        );
        let json = serde_json::to_string(&report).unwrap();
        assert!(
            !json.contains("/not-opened"),
            "{case}: attacker-controlled chunk paths must not be echoed: {json}"
        );
    }
}
