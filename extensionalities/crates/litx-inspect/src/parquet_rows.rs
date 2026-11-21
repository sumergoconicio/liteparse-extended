//! Deferred, unregistered Parquet row-reader reference retained for a future approved task.
//! This module is absent from `lib.rs` and has no runtime reachability. Its snapshot/page
//! validation sketches bounded decoding without enabling Parquet extraction in this build.
use crate::{DatasetPreview, xml};
use bytes::{Buf, Bytes};
use parquet_format_safe::{FileMetaData, PageHeader, thrift::protocol::TCompactInputProtocol};
use parquet_reader::{
    errors::{ParquetError, Result},
    file::{
        reader::{ChunkReader, FileReader, Length},
        serialized_reader::SerializedFileReader,
    },
};
use std::{
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
};

const PAGE_CAP: usize = 8 << 20;
const CHUNK_CAP: usize = 16 << 20;
const READ_CAP: usize = 64 << 20;

struct Snapshot {
    length: u64,
    segments: Vec<(u64, Bytes)>,
}

impl Length for Snapshot {
    fn len(&self) -> u64 {
        self.length
    }
}

impl Snapshot {
    fn range(&self, start: u64) -> Result<Bytes> {
        for (offset, bytes) in &self.segments {
            if start >= *offset && start - offset < bytes.len() as u64 {
                return Ok(bytes.slice((start - offset) as usize..));
            }
        }
        Err(ParquetError::General(
            "Parquet read outside preview snapshot".into(),
        ))
    }
}

impl ChunkReader for Snapshot {
    type T = bytes::buf::Reader<Bytes>;
    fn get_read(&self, start: u64) -> Result<Self::T> {
        Ok(self.range(start)?.reader())
    }
    fn get_bytes(&self, start: u64, length: usize) -> Result<Bytes> {
        let bytes = self.range(start)?;
        if length > bytes.len() {
            return Err(ParquetError::General(
                "Parquet read exceeds preview segment".into(),
            ));
        }
        Ok(bytes.slice(..length))
    }
}

fn validate_pages(bytes: &[u8], expanded: &mut usize) -> xml::Result<()> {
    let mut cursor = Cursor::new(bytes);
    let mut pages = 0;
    while cursor.position() < bytes.len() as u64 {
        pages += 1;
        if pages > 16_384 {
            return Err("Parquet page count cap exceeded".into());
        }
        let header = PageHeader::read_from_in_protocol(&mut TCompactInputProtocol::new(
            &mut cursor,
            64 << 10,
        ))?;
        let compressed = usize::try_from(header.compressed_page_size)?;
        let uncompressed = usize::try_from(header.uncompressed_page_size)?;
        if compressed > PAGE_CAP || uncompressed > PAGE_CAP {
            return Err("Parquet page exceeds 8 MiB cap".into());
        }
        for count in [
            header.data_page_header.as_ref().map(|h| h.num_values),
            header.data_page_header_v2.as_ref().map(|h| h.num_values),
            header.dictionary_page_header.as_ref().map(|h| h.num_values),
        ]
        .into_iter()
        .flatten()
        {
            if !(0..=1_000_000).contains(&count) {
                return Err("Parquet page value cap exceeded".into());
            }
        }
        *expanded = expanded
            .checked_add(uncompressed)
            .ok_or("Parquet expanded size overflow")?;
        if *expanded > 128 << 20 {
            return Err("Parquet preview exceeds 128 MiB expanded page cap".into());
        }
        let end = cursor
            .position()
            .checked_add(compressed as u64)
            .ok_or("Parquet page offset overflow")?;
        if end > bytes.len() as u64 {
            return Err("truncated Parquet page".into());
        }
        cursor.set_position(end);
    }
    Ok(())
}

fn bound_value(value: &mut serde_json::Value, truncated: &mut bool) {
    use serde_json::Value;
    match value {
        Value::String(s) => {
            if let Some((index, _)) = s.char_indices().nth(512) {
                s.truncate(index);
                *truncated = true;
            }
        }
        Value::Array(values) => {
            if values.len() > 5 {
                values.truncate(5);
                *truncated = true;
            }
            for v in values {
                bound_value(v, truncated);
            }
        }
        Value::Object(values) => {
            // MAP values get the same bounds as every other preview value: at most
            // five entries and keys capped at the string limit. Truncated keys that
            // collide keep the first entry and stay flagged via `truncated`.
            if values.len() > 5 {
                *truncated = true;
            }
            let entries: Vec<(String, Value)> =
                std::mem::take(values).into_iter().take(5).collect();
            for (key, mut value) in entries {
                let key = match key.char_indices().nth(512) {
                    Some((index, _)) => {
                        *truncated = true;
                        key[..index].to_string()
                    }
                    None => key,
                };
                bound_value(&mut value, truncated);
                values.entry(key).or_insert(value);
            }
        }
        _ => {}
    }
}

pub(crate) fn preview(
    file: &mut File,
    length: u64,
    footer: Vec<u8>,
    metadata: &FileMetaData,
) -> xml::Result<DatasetPreview> {
    let footer_start = length - footer.len() as u64;
    let mut snapshot = Snapshot {
        length,
        segments: vec![
            (0, Bytes::from_static(b"PAR1")),
            (footer_start, Bytes::from(footer)),
        ],
    };
    let mut bytes_read = 0usize;
    let mut expanded = 0usize;
    let mut rows_covered = 0;
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    for group in &metadata.row_groups {
        if rows_covered >= 5 {
            break;
        }
        rows_covered += group.num_rows;
        if group.num_rows == 0 {
            continue;
        }
        for column in &group.columns {
            if column.file_path.is_some() {
                return Err("external Parquet column chunks are not opened".into());
            }
            let meta = column
                .meta_data
                .as_ref()
                .ok_or("missing Parquet column metadata")?;
            if ![0, 1, 7].contains(&meta.codec.0) {
                return Err("bounded Parquet row preview supports UNCOMPRESSED, SNAPPY and LZ4_RAW codecs; schema remains available".into());
            }
            if column.file_offset < 0 {
                return Err(
                    format!("invalid Parquet chunk file offset {}", column.file_offset).into(),
                );
            }
            if let Some(dictionary) = meta.dictionary_page_offset
                && dictionary > meta.data_page_offset
            {
                return Err(format!(
                    "invalid Parquet chunk offsets: dictionary_page_offset {dictionary} exceeds data_page_offset {}",
                    meta.data_page_offset
                )
                .into());
            }
            let start = u64::try_from(
                meta.dictionary_page_offset
                    .unwrap_or(meta.data_page_offset)
                    .min(meta.data_page_offset),
            )?;
            let size = usize::try_from(meta.total_compressed_size)?;
            bytes_read = bytes_read
                .checked_add(size)
                .ok_or("Parquet read size overflow")?;
            if size > CHUNK_CAP || bytes_read > READ_CAP {
                return Err("Parquet preview exceeds 16 MiB chunk / 64 MiB read cap".into());
            }
            let end = start
                .checked_add(size as u64)
                .ok_or("Parquet chunk offset overflow")?;
            if start < 4 || end > footer_start {
                return Err("Parquet chunk range outside file data".into());
            }
            // Chunks are validated page-by-page as read; an overlap would let one
            // validated range stand in for a different declared chunk when the
            // decoder resolves snapshot segments, so ranges must stay disjoint.
            if let Some(&(other_start, other_end)) = ranges
                .iter()
                .find(|&&(other_start, other_end)| start < other_end && other_start < end)
            {
                return Err(format!(
                    "overlapping Parquet chunk ranges: [{start}, {end}) and [{other_start}, {other_end})"
                )
                .into());
            }
            ranges.push((start, end));
            file.seek(SeekFrom::Start(start))?;
            let mut bytes = vec![0; size];
            file.read_exact(&mut bytes)?;
            validate_pages(&bytes, &mut expanded)?;
            snapshot.segments.push((start, Bytes::from(bytes)));
        }
    }
    let mut preview = DatasetPreview {
        headers: vec![],
        rows: vec![],
        row_limit: 5,
        has_more: metadata.num_rows > 5,
        values_truncated: false,
    };
    if metadata.num_rows == 0 {
        // Retain names even for empty tables, without asking a decoder for nonexistent pages.
        for node in metadata.schema.iter().skip(1).filter(|s| s.type_.is_some()) {
            preview.headers.push(node.name.clone());
        }
        return Ok(preview);
    }
    let reader = SerializedFileReader::new(snapshot)?;
    for row in reader.get_row_iter(None)?.take(5) {
        let row = row?;
        if preview.headers.is_empty() {
            preview.headers = row
                .get_column_iter()
                .map(|(name, _)| name.clone())
                .collect();
        }
        let mut values = row
            .get_column_iter()
            .map(|(_, field)| field.to_json_value())
            .collect::<Vec<_>>();
        for value in &mut values {
            bound_value(value, &mut preview.values_truncated);
        }
        preview.rows.push(values);
    }
    if preview.rows.len() as i64 != metadata.num_rows.min(5) {
        return Err("Parquet preview row count disagrees with footer".into());
    }
    Ok(preview)
}
