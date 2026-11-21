//! Integration tests for the audit remediation container bounds: ZIP EOCD/ZIP64
//! preflight parity, plus the retained hostile Parquet schema/chunk fixtures.
//! Parquet extraction is deferred by user decision (2026-11-18): every Parquet
//! case now pins the static identification-only deferral instead of decoder
//! validation. Synthetic fixtures are preserved under the remediation
//! evidence directory for CLI and batch survival probes.

use litx_inspect::{InspectOptions, inspect};
use parquet_format_safe::{
    ConvertedType, FieldRepetitionType, FileMetaData, ListType, LogicalType, MapType,
    SchemaElement, Type,
    thrift::protocol::{TCompactInputProtocol, TCompactOutputProtocol},
};
use std::path::{Path, PathBuf};

/// Deterministic evidence directory; crafted malformed files are kept here so the
/// final CLI and batch survival probes can reuse them.
fn fixtures_dir() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/verification/audit-remediation-20261118/containers/fixtures");
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// Writes `data` (PAR1 header plus any chunk bytes) followed by the thrift footer
/// for `metadata` and returns the crafted file path.
fn write_parquet(directory: &Path, name: &str, data: &[u8], metadata: &FileMetaData) -> PathBuf {
    let mut bytes = data.to_vec();
    let mut footer = Vec::new();
    metadata
        .write_to_out_protocol(&mut TCompactOutputProtocol::new(&mut footer))
        .unwrap();
    bytes.extend_from_slice(&footer);
    bytes.extend_from_slice(&(footer.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"PAR1");
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// A group (non-leaf) schema element.
fn group(
    name: &str,
    repetition: Option<FieldRepetitionType>,
    children: i32,
    converted: Option<ConvertedType>,
) -> SchemaElement {
    SchemaElement::new(
        None,
        None,
        repetition,
        name.to_string(),
        Some(children),
        converted,
        None,
        None,
        None,
        None,
    )
}

/// A primitive INT64 leaf schema element.
fn leaf(name: &str, repetition: FieldRepetitionType) -> SchemaElement {
    SchemaElement::new(
        Some(Type::INT64),
        None,
        Some(repetition),
        name.to_string(),
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

/// A primitive BYTE_ARRAY leaf schema element.
fn binary_leaf(name: &str, repetition: FieldRepetitionType) -> SchemaElement {
    SchemaElement::new(
        Some(Type::BYTE_ARRAY),
        None,
        Some(repetition),
        name.to_string(),
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

/// A single-row, single-column valid Parquet file from the real writer, returned as
/// (bytes before the footer, parsed footer metadata) so tests can mutate the schema.
fn seed() -> (Vec<u8>, FileMetaData) {
    use parquet_reader::{
        data_type::Int64Type,
        file::{properties::WriterProperties, writer::SerializedFileWriter},
        schema::parser::parse_message_type,
    };
    use std::sync::Arc;
    let schema = Arc::new(parse_message_type("message rows { REQUIRED INT64 id; }").unwrap());
    let mut writer =
        SerializedFileWriter::new(Vec::new(), schema, Arc::new(WriterProperties::default()))
            .unwrap();
    let mut row_group = writer.next_row_group().unwrap();
    let mut column = row_group.next_column().unwrap().unwrap();
    column
        .typed::<Int64Type>()
        .write_batch(&[1], None, None)
        .unwrap();
    column.close().unwrap();
    row_group.close().unwrap();
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

/// Replaces the seed schema and column-chunk count while keeping one physical row.
fn mutate_schema(
    seed: &(Vec<u8>, FileMetaData),
    schema: Vec<SchemaElement>,
    chunks: usize,
) -> FileMetaData {
    let mut metadata = seed.1.clone();
    let chunk = seed.1.row_groups[0].columns[0].clone();
    // Column orders are per leaf column; the mutated schema no longer matches them.
    metadata.column_orders = None;
    metadata.schema = schema;
    metadata.row_groups[0].columns = (0..chunks).map(|_| chunk.clone()).collect();
    metadata
}

/// LITX-AUD-001: malformed LIST/MAP/MAP_KEY_VALUE schema shapes previously
/// reproduced `parquet` crate assertion panics. Under the user-approved deferral
/// they must receive the static identification-only diagnostic without any
/// schema/footer decoding, so no dependency assertion can run and the process
/// (and any persistent batch worker) survives.
#[test]
fn malformed_list_and_map_schemas_defer_without_decoding() {
    const OPT: FieldRepetitionType = FieldRepetitionType::OPTIONAL;
    const REP: FieldRepetitionType = FieldRepetitionType::REPEATED;
    const REQ: FieldRepetitionType = FieldRepetitionType::REQUIRED;
    let directory = fixtures_dir();
    let seed = seed();
    let root = |children: i32| group("schema", None, children, None);
    let cases: Vec<(&str, Vec<SchemaElement>, usize)> = vec![
        // LIST group with zero children (live audit panic repro).
        (
            "list-zero-children",
            vec![root(1), group("l", Some(OPT), 0, Some(ConvertedType::LIST))],
            0,
        ),
        // LIST group with two children.
        (
            "list-two-children",
            vec![
                root(1),
                group("l", Some(OPT), 2, Some(ConvertedType::LIST)),
                leaf("a", OPT),
                leaf("b", OPT),
            ],
            2,
        ),
        // LIST group whose only child is not REPEATED.
        (
            "list-child-not-repeated",
            vec![
                root(1),
                group("l", Some(OPT), 1, Some(ConvertedType::LIST)),
                leaf("e", OPT),
            ],
            1,
        ),
        // LIST 3-level wrapper without the element field.
        (
            "list-wrapper-without-element",
            vec![
                root(1),
                group("l", Some(OPT), 1, Some(ConvertedType::LIST)),
                group("list", Some(REP), 0, None),
            ],
            0,
        ),
        // MAP group with zero children.
        (
            "map-zero-children",
            vec![root(1), group("m", Some(OPT), 0, Some(ConvertedType::MAP))],
            0,
        ),
        // MAP group with a primitive key-value child.
        (
            "map-primitive-child",
            vec![
                root(1),
                group("m", Some(OPT), 1, Some(ConvertedType::MAP)),
                leaf("kv", REP),
            ],
            1,
        ),
        // MAP key-value child that is not REPEATED.
        (
            "map-key-value-not-repeated",
            vec![
                root(1),
                group("m", Some(OPT), 1, Some(ConvertedType::MAP)),
                group("key_value", Some(OPT), 2, None),
                leaf("key", REQ),
                leaf("value", OPT),
            ],
            2,
        ),
        // MAP key-value child with three fields.
        (
            "map-key-value-three-fields",
            vec![
                root(1),
                group("m", Some(OPT), 1, Some(ConvertedType::MAP)),
                group("key_value", Some(REP), 3, None),
                leaf("key", REQ),
                leaf("value", OPT),
                leaf("extra", OPT),
            ],
            3,
        ),
        // MAP key-value pair whose key is a group.
        (
            "map-group-key",
            vec![
                root(1),
                group("m", Some(OPT), 1, Some(ConvertedType::MAP)),
                group("key_value", Some(REP), 1, None),
                group("key", Some(OPT), 1, None),
                leaf("inner", OPT),
            ],
            1,
        ),
        // MAP_KEY_VALUE annotation with zero children.
        (
            "map-key-value-zero-children",
            vec![
                root(1),
                group("m", Some(OPT), 0, Some(ConvertedType::MAP_KEY_VALUE)),
            ],
            0,
        ),
        // Non-root group without a repetition hits a dependency assertion.
        (
            "group-missing-repetition",
            vec![root(1), group("g", None, 1, None), leaf("x", OPT)],
            1,
        ),
    ];
    for (case, schema, chunks) in cases {
        let metadata = mutate_schema(&seed, schema, chunks);
        let path = write_parquet(&directory, &format!("{case}.parquet"), &seed.0, &metadata);
        let report = inspect(&path, &InspectOptions::default());
        assert!(
            report.errors.iter().any(|e| e.stage == "parquet"
                && e.message == "Parquet extraction is deferred; identification only"),
            "{case}: static deferral expected, got {:?}",
            report.errors
        );
        assert!(report.dataset.is_none(), "{case}");
        assert!(report.parquet.is_none(), "{case}: no schema profile");
    }
}

/// LITX-AUD-001 deferral scope: valid standard and legacy LIST/MAP schema
/// shapes receive the same static identification-only deferral; no footer/schema
/// profile is produced for any Parquet input while extraction is deferred.
#[test]
fn valid_list_and_map_schemas_also_defer_extraction() {
    const OPT: FieldRepetitionType = FieldRepetitionType::OPTIONAL;
    const REP: FieldRepetitionType = FieldRepetitionType::REPEATED;
    const REQ: FieldRepetitionType = FieldRepetitionType::REQUIRED;
    let directory = fixtures_dir();
    let root = |children: i32| group("schema", None, children, None);
    let cases: Vec<(&str, Vec<SchemaElement>)> = vec![
        // Standard 3-level LIST.
        (
            "valid-list-3level",
            vec![
                root(1),
                group("l", Some(OPT), 1, Some(ConvertedType::LIST)),
                group("list", Some(REP), 1, None),
                leaf("element", OPT),
            ],
        ),
        // Legacy 2-level LIST with a repeated primitive.
        (
            "valid-list-2level-primitive",
            vec![
                root(1),
                group("l", Some(OPT), 1, Some(ConvertedType::LIST)),
                leaf("element", REP),
            ],
        ),
        // Legacy 2-level LIST with a multi-field repeated group.
        (
            "valid-list-legacy-array",
            vec![
                root(1),
                group("l", Some(OPT), 1, Some(ConvertedType::LIST)),
                group("array", Some(REP), 2, None),
                binary_leaf("str", REQ),
                leaf("num", OPT),
            ],
        ),
        // Legacy parquet-thrift `_tuple` list wrapper.
        (
            "valid-list-legacy-tuple",
            vec![
                root(1),
                group("l", Some(OPT), 1, Some(ConvertedType::LIST)),
                group("l_tuple", Some(REP), 1, None),
                binary_leaf("str", REQ),
            ],
        ),
        // Standard MAP whose key-value group carries the legacy MAP_KEY_VALUE mark.
        (
            "valid-map-legacy-key-value",
            vec![
                root(1),
                group("m", Some(OPT), 1, Some(ConvertedType::MAP)),
                group(
                    "key_value",
                    Some(REP),
                    2,
                    Some(ConvertedType::MAP_KEY_VALUE),
                ),
                binary_leaf("key", REQ),
                leaf("value", OPT),
            ],
        ),
        // MAP annotated only through the logical type.
        (
            "valid-map-logical-only",
            vec![
                root(1),
                {
                    let mut m = group("m", Some(OPT), 1, None);
                    m.logical_type = Some(LogicalType::MAP(MapType::new()));
                    m
                },
                group("key_value", Some(REP), 2, None),
                binary_leaf("key", REQ),
                leaf("value", OPT),
            ],
        ),
        // LIST annotated only through the logical type.
        (
            "valid-list-logical-only",
            vec![
                root(1),
                {
                    let mut l = group("l", Some(OPT), 1, None);
                    l.logical_type = Some(LogicalType::LIST(ListType::new()));
                    l
                },
                group("list", Some(REP), 1, None),
                leaf("element", OPT),
            ],
        ),
        // MAP without a value field is decoded as a list of keys (#1642).
        (
            "valid-map-keys-only",
            vec![
                root(1),
                group("m", Some(OPT), 1, Some(ConvertedType::MAP)),
                group("key_value", Some(REP), 1, None),
                binary_leaf("key", REQ),
            ],
        ),
    ];
    for (case, schema) in cases {
        let metadata = FileMetaData::new(1, schema, 0, Vec::new(), None, None, None, None, None);
        let path = write_parquet(&directory, &format!("{case}.parquet"), b"PAR1", &metadata);
        let report = inspect(&path, &InspectOptions::default());
        assert!(
            report.errors.iter().any(|e| e.stage == "parquet"
                && e.message == "Parquet extraction is deferred; identification only"),
            "{case}: static deferral expected, got {:?}",
            report.errors
        );
        assert!(report.parquet.is_none(), "{case}: no schema profile");
        assert!(report.dataset.is_none(), "{case}");
        assert_eq!(
            report.format.detected_mime.as_deref(),
            Some("application/vnd.apache.parquet"),
            "{case}: format identification is retained"
        );
    }
}

/// LITX-AUD-001 deferral scope at the decoder level: a real file holding nested
/// LIST and MAP columns is never handed to the row reader; the retained fixture
/// proves the deferral applies even to fully valid, decodable content.
#[cfg(feature = "parquet-preview")]
#[test]
fn valid_nested_list_and_map_columns_are_not_decoded_under_deferral() {
    use parquet_reader::{
        data_type::{ByteArray, ByteArrayType, Int64Type},
        file::{properties::WriterProperties, writer::SerializedFileWriter},
        schema::parser::parse_message_type,
    };
    use std::sync::Arc;
    let directory = fixtures_dir();
    let path = directory.join("valid-nested-list-map.parquet");
    let schema = Arc::new(
        parse_message_type(
            "message rows {
                OPTIONAL group l (LIST) {
                    REPEATED group list { OPTIONAL INT64 element; }
                }
                OPTIONAL group m (MAP) {
                    REPEATED group key_value {
                        REQUIRED BINARY key (UTF8);
                        OPTIONAL INT64 value;
                    }
                }
            }",
        )
        .unwrap(),
    );
    let mut writer = SerializedFileWriter::new(
        std::fs::File::create(&path).unwrap(),
        schema,
        Arc::new(WriterProperties::default()),
    )
    .unwrap();
    let mut row_group = writer.next_row_group().unwrap();
    // Row 1 is l=[10,20], m={a:1,b:2}; row 2 is l=[30], m={c:3}.
    let mut column = row_group.next_column().unwrap().unwrap();
    column
        .typed::<Int64Type>()
        .write_batch(&[10, 20, 30], Some(&[3, 3, 3]), Some(&[0, 1, 0]))
        .unwrap();
    column.close().unwrap();
    let keys: Vec<ByteArray> = ["a", "b", "c"].into_iter().map(ByteArray::from).collect();
    let mut column = row_group.next_column().unwrap().unwrap();
    column
        .typed::<ByteArrayType>()
        .write_batch(&keys, Some(&[2, 2, 2]), Some(&[0, 1, 0]))
        .unwrap();
    column.close().unwrap();
    let mut column = row_group.next_column().unwrap().unwrap();
    column
        .typed::<Int64Type>()
        .write_batch(&[1, 2, 3], Some(&[3, 3, 3]), Some(&[0, 1, 0]))
        .unwrap();
    column.close().unwrap();
    row_group.close().unwrap();
    writer.close().unwrap();

    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report.errors.iter().any(|e| e.stage == "parquet"
            && e.message == "Parquet extraction is deferred; identification only"),
        "{:?}",
        report.errors
    );
    assert!(report.dataset.is_none(), "no row preview may be produced");
    assert!(
        report.parquet.is_none(),
        "no footer profile may be produced"
    );
    assert!(
        report.text.content.is_empty() && report.text.truncated,
        "omitted rows must be reported as incomplete empty text"
    );
}

/// Builds a valid one-entry stored ZIP containing no entry payload.
/// Accepts the archive comment bytes and returns complete ZIP bytes; the
/// fixed one-byte name and zero payload cannot overflow the test format.
fn stored_zip(comment: &[u8]) -> Vec<u8> {
    stored_zip_with_data(&[], comment)
}

/// Builds a valid one-entry stored ZIP with caller-supplied payload and comment.
/// Accepts byte slices for the stored payload and archive comment, returning
/// complete ZIP bytes. Test inputs must fit the ZIP32 `u32` size fields.
fn stored_zip_with_data(data: &[u8], comment: &[u8]) -> Vec<u8> {
    let name = b"a";
    let crc = crc32fast::hash(data);
    let size = data.len() as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PK\x03\x04");
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&crc.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(data);
    let cd_offset = bytes.len() as u32;
    bytes.extend_from_slice(b"PK\x01\x02");
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&crc.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(name);
    let cd_size = bytes.len() as u32 - cd_offset;
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&cd_size.to_le_bytes());
    bytes.extend_from_slice(&cd_offset.to_le_bytes());
    bytes.extend_from_slice(&(comment.len() as u16).to_le_bytes());
    bytes.extend_from_slice(comment);
    bytes
}

/// A 22-byte zero-field EOCD record used to shadow a real directory.
fn dummy_eocd() -> Vec<u8> {
    let mut bytes = b"PK\x05\x06".to_vec();
    bytes.extend_from_slice(&[0; 18]);
    bytes
}

/// Writes ZIP bytes into the preserved fixtures directory.
fn write_zip(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// LITX-AUD-002: an appended dummy EOCD must not shadow the real directory and let
/// the `zip` crate fall back past the pre-allocation guards; multiple plausible
/// EOCD candidates are rejected as ambiguous.
#[test]
fn zip_with_appended_dummy_eocd_is_rejected_as_ambiguous() {
    let directory = fixtures_dir();
    let mut bytes = stored_zip(b"");
    bytes.extend_from_slice(&dummy_eocd());
    let path = write_zip(&directory, "ambiguous-eocd.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.stage == "archive" && e.message.contains("ambiguous")),
        "{:?}",
        report.errors
    );
    assert!(report.entries.is_empty());
}

/// LITX-AUD-002: a ZIP64 locator anywhere in the trailing window must keep the
/// bounded-ZIP64 rejection even when a later dummy EOCD shadows the real record.
#[test]
fn zip64_locator_in_tail_window_is_rejected_despite_dummy_eocd() {
    let directory = fixtures_dir();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/zip64-directory.zip");
    let mut bytes = std::fs::read(fixture).unwrap();
    bytes.extend_from_slice(&dummy_eocd());
    let path = write_zip(&directory, "zip64-eocd-shadow.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.stage == "archive" && e.message.contains("ZIP64")),
        "{:?}",
        report.errors
    );
}

/// LITX-AUD-002: the declared central directory range must end at or before the
/// EOCD record, so preflight and the decoder cannot disagree on the directory.
#[test]
fn zip_central_directory_beyond_eocd_is_rejected() {
    let directory = fixtures_dir();
    let mut bytes = stored_zip(b"");
    let eocd = bytes.len() - 22;
    bytes[eocd + 16..eocd + 20].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
    let path = write_zip(&directory, "cd-beyond-eocd.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.stage == "archive" && e.message.contains("central directory")),
        "{:?}",
        report.errors
    );
}

/// LITX-AUD-002 preservation: ordinary ZIPs, including ones with an archive
/// comment, must keep opening and listing their entries.
#[test]
fn valid_zip_with_comment_still_opens() {
    let directory = fixtures_dir();
    let bytes = stored_zip(b"release notes");
    let path = write_zip(&directory, "valid-comment.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].name, "a");
}

/// W3-SYS-ARCHIVE-TAIL: locator-like bytes in an ordinary archive comment are
/// comment data, not a ZIP64 directory, and must not reject the valid ZIP.
/// Accepts no arguments and returns unit; assertions panic on a false rejection
/// or incorrect entry listing.
#[test]
fn zip64_signature_in_comment_is_not_a_locator() {
    let directory = fixtures_dir();
    let bytes = stored_zip(b"release notes contain PK\x06\x07 as data");
    let path = write_zip(&directory, "zip64-signature-in-comment.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.entries.len(), 1);
}

/// W3-SYS-ARCHIVE-TAIL: locator-like bytes in a stored entry payload are file
/// data, not a ZIP64 directory, and must not reject the valid ZIP. Accepts no
/// arguments and returns unit; assertions panic on rejection or wrong entry metadata.
#[test]
fn zip64_signature_in_entry_data_is_not_a_locator() {
    let directory = fixtures_dir();
    let bytes = stored_zip_with_data(b"payload PK\x06\x07 payload", b"");
    let path = write_zip(&directory, "zip64-signature-in-entry.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].size, 20);
}

/// Rewrites a Parquet file's footer after applying `mutate` to the parsed metadata.
fn mutate_metadata(path: &Path, mutate: impl FnOnce(&mut FileMetaData)) {
    let bytes = std::fs::read(path).unwrap();
    let size =
        u32::from_le_bytes(bytes[bytes.len() - 8..bytes.len() - 4].try_into().unwrap()) as usize;
    let footer_start = bytes.len() - 8 - size;
    let mut metadata = FileMetaData::read_from_in_protocol(&mut TCompactInputProtocol::new(
        &mut std::io::Cursor::new(&bytes[footer_start..bytes.len() - 8]),
        1 << 20,
    ))
    .unwrap();
    mutate(&mut metadata);
    let mut footer = Vec::new();
    metadata
        .write_to_out_protocol(&mut TCompactOutputProtocol::new(&mut footer))
        .unwrap();
    let mut file = bytes[..footer_start].to_vec();
    file.extend_from_slice(&footer);
    file.extend_from_slice(&(footer.len() as u32).to_le_bytes());
    file.extend_from_slice(b"PAR1");
    std::fs::write(path, file).unwrap();
}

/// Writes a fresh copy of the valid seed Parquet file under `name`.
fn write_seed(directory: &Path, name: &str) -> (PathBuf, FileMetaData) {
    let (data, metadata) = seed();
    (write_parquet(directory, name, &data, &metadata), metadata)
}

/// LITX-AUD-003 deferral scope: an inverted chunk layout (dictionary page offset
/// above the data page offset) is never inspected; the file receives the static
/// identification-only deferral before any chunk range logic can run.
#[test]
fn parquet_inverted_chunk_offsets_defer_without_chunk_reads() {
    let directory = fixtures_dir();
    let (path, metadata) = write_seed(&directory, "chunk-offset-inversion.parquet");
    let data_offset = metadata.row_groups[0].columns[0]
        .meta_data
        .as_ref()
        .unwrap()
        .data_page_offset;
    mutate_metadata(&path, |metadata| {
        let meta = metadata.row_groups[0].columns[0]
            .meta_data
            .as_mut()
            .unwrap();
        meta.dictionary_page_offset = Some(data_offset + 100);
    });
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report.errors.iter().any(|e| e.stage == "parquet"
            && e.message == "Parquet extraction is deferred; identification only"),
        "{:?}",
        report.errors
    );
    assert!(report.dataset.is_none(), "no chunk data may be read");
}

/// LITX-AUD-003 deferral scope: overlapping chunk ranges are never compared;
/// the file receives the static identification-only deferral.
#[test]
fn parquet_overlapping_chunk_ranges_defer_without_chunk_reads() {
    let directory = fixtures_dir();
    let (path, _) = write_seed(&directory, "chunk-overlap.parquet");
    mutate_metadata(&path, |metadata| {
        let duplicate = metadata.row_groups[0].clone();
        metadata.row_groups.push(duplicate);
        metadata.num_rows = 2;
    });
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report.errors.iter().any(|e| e.stage == "parquet"
            && e.message == "Parquet extraction is deferred; identification only"),
        "{:?}",
        report.errors
    );
    assert!(report.dataset.is_none(), "no chunk data may be read");
}

/// LITX-AUD-003 deferral scope: a negative chunk file offset is never read;
/// the file receives the static identification-only deferral.
#[test]
fn parquet_negative_chunk_file_offset_defers_without_chunk_reads() {
    let directory = fixtures_dir();
    let (path, _) = write_seed(&directory, "chunk-negative-file-offset.parquet");
    mutate_metadata(&path, |metadata| {
        metadata.row_groups[0].columns[0].file_offset = -1;
    });
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report.errors.iter().any(|e| e.stage == "parquet"
            && e.message == "Parquet extraction is deferred; identification only"),
        "{:?}",
        report.errors
    );
    assert!(report.dataset.is_none(), "no chunk data may be read");
}

/// LITX-AUD-017 deferral scope: MAP values are never previewed, so no key or
/// entry needs bounding; the oversized key written below must not appear in any
/// output while extraction is deferred.
#[cfg(feature = "parquet-preview")]
#[test]
fn parquet_map_values_are_not_previewed_under_deferral() {
    use parquet_reader::{
        data_type::{ByteArray, ByteArrayType, Int64Type},
        file::{properties::WriterProperties, writer::SerializedFileWriter},
        schema::parser::parse_message_type,
    };
    use std::sync::Arc;
    let directory = fixtures_dir();
    let path = directory.join("map-preview-bounds.parquet");
    let schema = Arc::new(
        parse_message_type(
            "message rows {
                OPTIONAL group m (MAP) {
                    REPEATED group key_value {
                        REQUIRED BINARY key (UTF8);
                        OPTIONAL INT64 value;
                    }
                }
            }",
        )
        .unwrap(),
    );
    let mut writer = SerializedFileWriter::new(
        std::fs::File::create(&path).unwrap(),
        schema,
        Arc::new(WriterProperties::default()),
    )
    .unwrap();
    let mut row_group = writer.next_row_group().unwrap();
    let long_key = "k".repeat(600);
    let keys: Vec<ByteArray> = ["a", "b", "c", "d", "e", long_key.as_str(), "f"]
        .into_iter()
        .map(ByteArray::from)
        .collect();
    let mut column = row_group.next_column().unwrap().unwrap();
    column
        .typed::<ByteArrayType>()
        .write_batch(&keys, Some(&[2; 7]), Some(&[0, 1, 1, 1, 1, 1, 1]))
        .unwrap();
    column.close().unwrap();
    let mut column = row_group.next_column().unwrap().unwrap();
    column
        .typed::<Int64Type>()
        .write_batch(
            &[1, 2, 3, 4, 5, 6, 7],
            Some(&[3; 7]),
            Some(&[0, 1, 1, 1, 1, 1, 1]),
        )
        .unwrap();
    column.close().unwrap();
    row_group.close().unwrap();
    writer.close().unwrap();

    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report.errors.iter().any(|e| e.stage == "parquet"
            && e.message == "Parquet extraction is deferred; identification only"),
        "{:?}",
        report.errors
    );
    assert!(report.dataset.is_none(), "no map preview may be produced");
    let json = serde_json::to_string(&report).unwrap();
    assert!(
        !json.contains(&long_key),
        "attacker-controlled keys must not appear in output"
    );
}

/// LITX-AUD-001 wave-2 variant 1 deferral scope: a MAP-annotated root hiding a
/// zero-child LIST previously reached the record reader's root assertion. No
/// reader-tree validation runs under deferral; the fixture pins the static
/// identification-only diagnostic.
#[cfg(feature = "parquet-preview")]
#[test]
fn annotated_root_defers_without_reader_tree_validation() {
    let directory = fixtures_dir();
    let (path, _) = write_seed(&directory, "w2-root-map-hidden-list.parquet");
    mutate_metadata(&path, |metadata| {
        metadata.schema = vec![
            group("schema", None, 1, Some(ConvertedType::MAP)),
            group(
                "l",
                Some(FieldRepetitionType::OPTIONAL),
                0,
                Some(ConvertedType::LIST),
            ),
        ];
        metadata.row_groups[0].columns = vec![];
        metadata.num_rows = 1;
        metadata.column_orders = None;
    });
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report.errors.iter().any(|e| e.stage == "parquet"
            && e.message == "Parquet extraction is deferred; identification only"),
        "{:?}",
        report.errors
    );
    assert!(report.dataset.is_none());
    assert!(report.parquet.is_none());
}

/// LITX-AUD-001 wave-2 variant 2 deferral scope: LIST wrapping a REPEATED MAP
/// wrapping a REPEATED LIST previously required reader-mirrored traversal. No
/// traversal runs under deferral; the fixture pins the static diagnostic.
#[cfg(feature = "parquet-preview")]
#[test]
fn list_wrapper_map_defers_without_reader_tree_validation() {
    let directory = fixtures_dir();
    let (path, _) = write_seed(&directory, "w2-list-map-list-wrapper.parquet");
    mutate_metadata(&path, |metadata| {
        metadata.schema = vec![
            group("schema", None, 1, None),
            group(
                "l",
                Some(FieldRepetitionType::OPTIONAL),
                1,
                Some(ConvertedType::LIST),
            ),
            group(
                "m",
                Some(FieldRepetitionType::REPEATED),
                1,
                Some(ConvertedType::MAP),
            ),
            group(
                "l2",
                Some(FieldRepetitionType::REPEATED),
                1,
                Some(ConvertedType::LIST),
            ),
            leaf("e", FieldRepetitionType::OPTIONAL),
        ];
        metadata.num_rows = 1;
    });
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report.errors.iter().any(|e| e.stage == "parquet"
            && e.message == "Parquet extraction is deferred; identification only"),
        "{:?}",
        report.errors
    );
    assert!(report.dataset.is_none());
    assert!(report.parquet.is_none());
}

/// LITX-AUD-001 wave-2 variant 3 deferral scope: an empty non-root group
/// previously panicked the `GroupReader`. No reader is built under deferral; the
/// fixture pins the static identification-only diagnostic.
#[cfg(feature = "parquet-preview")]
#[test]
fn empty_non_root_group_defers_without_reader_construction() {
    let directory = fixtures_dir();
    let (path, _) = write_seed(&directory, "w2-empty-non-root-group.parquet");
    mutate_metadata(&path, |metadata| {
        metadata.schema = vec![
            group("schema", None, 1, None),
            group("g", Some(FieldRepetitionType::OPTIONAL), 0, None),
        ];
        metadata.row_groups[0].columns = vec![];
        metadata.num_rows = 1;
        metadata.column_orders = None;
    });
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report.errors.iter().any(|e| e.stage == "parquet"
            && e.message == "Parquet extraction is deferred; identification only"),
        "{:?}",
        report.errors
    );
    assert!(report.dataset.is_none());
    assert!(report.parquet.is_none());
}

/// Builds a stored ZIP with `entries` zero-byte entries and the given archive comment.
fn multi_entry_zip(entries: usize, comment: &[u8]) -> Vec<u8> {
    let names: Vec<String> = (0..entries).map(|i| format!("e{i}")).collect();
    named_zip(&names, comment)
}

/// Builds a stored ZIP with one zero-byte entry per name and the given comment.
fn named_zip(names: &[String], comment: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut central = Vec::new();
    for name in names {
        let offset = bytes.len() as u32;
        bytes.extend_from_slice(b"PK\x03\x04");
        bytes.extend_from_slice(&20u16.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 20]);
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&[0u8; 20]);
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0u8; 12]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = bytes.len() as u32;
    let cd_size = central.len() as u32;
    bytes.extend_from_slice(&central);
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&[0u8; 4]);
    bytes.extend_from_slice(&(names.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&(names.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&cd_size.to_le_bytes());
    bytes.extend_from_slice(&cd_offset.to_le_bytes());
    bytes.extend_from_slice(&(comment.len() as u16).to_le_bytes());
    bytes.extend_from_slice(comment);
    bytes
}

/// LITX-AUD-002 wave-2: the decoder's EOCD search reaches `file_len - 65_558`, one
/// byte beyond the old 65_557 window. A 4200-entry real directory at that offset
/// with a maximum comment plus one trailing byte, shadowed by an empty decoy EOCD
/// at EOF, must be rejected as ambiguous instead of opening as an empty archive.
#[test]
fn zip_decoy_at_eof_cannot_shadow_classic_directory_at_decoder_window_edge() {
    let directory = fixtures_dir();
    let mut bytes = multi_entry_zip(4200, &[b'x'; 65_535]);
    bytes.push(b'j'); // one trailing garbage byte; the real comment still ends before EOF
    let decoy = bytes.len() - 22;
    bytes[decoy..].copy_from_slice(&dummy_eocd());
    let path = write_zip(&directory, "w2-shadow-classic-65558.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.stage == "archive" && e.message.contains("ambiguous")),
        "{:?}",
        report.errors
    );
}

/// LITX-AUD-002 wave-2: the ZIP64 locator sits 20 bytes before the EOCD, so the
/// decoder window reaches `file_len - 65_578`. A ZIP64 EOCD+locator at that edge,
/// shadowed by an empty decoy at EOF, must keep the bounded-ZIP64 rejection.
#[test]
fn zip_decoy_at_eof_cannot_shadow_zip64_locator_at_decoder_window_edge() {
    let directory = fixtures_dir();
    let mut bytes = multi_entry_zip(1, &[]);
    bytes.truncate(bytes.len() - 22); // drop the classic EOCD from the builder
    bytes.extend_from_slice(b"PK\x06\x07"); // ZIP64 locator (20 bytes)
    bytes.extend_from_slice(&[0u8; 16]);
    bytes.extend_from_slice(b"PK\x05\x06"); // real EOCD with ZIP64 sentinel fields
    bytes.extend_from_slice(&[0u8; 4]); // disk numbers
    bytes.extend_from_slice(&[0xff; 4]); // entry-count fields at 0xFFFF
    bytes.extend_from_slice(&[0xff; 8]); // directory size/offset at 0xFFFFFFFF
    bytes.extend_from_slice(&65_535u16.to_le_bytes());
    bytes.extend_from_slice(&[b'x'; 65_535]);
    bytes.push(b'j');
    let decoy = bytes.len() - 22;
    bytes[decoy..].copy_from_slice(&dummy_eocd());
    let path = write_zip(&directory, "w2-shadow-zip64-65578.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.stage == "archive" && e.message.contains("ZIP64")),
        "{:?}",
        report.errors
    );
}

/// LITX-AUD-002 preservation: a valid ZIP with a maximum-length comment still
/// opens; the wider window must not reject ordinary archives.
#[test]
fn valid_zip_with_max_comment_still_opens() {
    let directory = fixtures_dir();
    let bytes = multi_entry_zip(2, &[b'c'; 65_535]);
    let path = write_zip(&directory, "w2-valid-max-comment.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.entries.len(), 2);
}

/// LITX-AUD-002 preservation: the decoder tolerates trailing garbage after the
/// comment (its comment check is `>`); the preflight window must cover that layout
/// instead of reporting a missing EOCD.
#[test]
fn valid_zip_with_max_comment_and_trailing_garbage_still_opens() {
    let directory = fixtures_dir();
    let mut bytes = multi_entry_zip(2, &[b'c'; 65_535]);
    bytes.push(b'j');
    let path = write_zip(&directory, "w2-valid-max-comment-garbage.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.entries.len(), 2);
}

/// LITX-AUD-002: the validated candidate must actually resolve a central directory
/// for the decoder; a lone EOCD whose declared range contains no CDFH signature is
/// rejected with the same conclusion the decoder would reach.
#[test]
fn zip_eocd_without_resolvable_central_directory_is_rejected() {
    let directory = fixtures_dir();
    let mut bytes = stored_zip(b"");
    let eocd = bytes.len() - 22;
    let cd_start = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap());
    // Point the directory inside the single CDFH record, past its signature, with
    // a shortened size so the range still fits before the EOCD.
    bytes[eocd + 16..eocd + 20].copy_from_slice(&(cd_start + 4).to_le_bytes());
    bytes[eocd + 12..eocd + 16].copy_from_slice(&20u32.to_le_bytes());
    let path = write_zip(&directory, "w2-cdfh-unresolvable.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.stage == "archive" && e.message.contains("central directory signature")),
        "{:?}",
        report.errors
    );
}

/// W2-REM-ZIP-DUP: the locked decoder deduplicates exact raw names in its
/// IndexMap before any entry loop runs, so a second same-name entry would be
/// dropped unaudited. Reject the archive instead of trusting the deduped view.
#[test]
fn zip_duplicate_entry_names_are_rejected_before_entry_iteration() {
    let directory = fixtures_dir();
    let bytes = named_zip(&["dup".to_string(), "dup".to_string()], &[]);
    let path = write_zip(&directory, "w2-duplicate-names.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.stage == "archive" && e.message.contains("duplicate")),
        "{:?}",
        report.errors
    );
}

/// W2-REM-ZIP-DUP preservation: distinct names still open cleanly.
#[test]
fn zip_distinct_entry_names_still_open() {
    let directory = fixtures_dir();
    let bytes = named_zip(&["a.txt".to_string(), "b.txt".to_string()], &[]);
    let path = write_zip(&directory, "w2-distinct-names.zip", &bytes);
    let report = inspect(&path, &InspectOptions::default());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.entries.len(), 2);
}
