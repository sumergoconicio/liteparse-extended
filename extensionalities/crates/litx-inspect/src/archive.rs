//! Bounded ZIP directory preflight and cumulative entry extraction.
use crate::{ArchiveEntry, InspectOptions, InspectReport};
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use thiserror::Error;
use zip::ZipArchive;

pub(crate) const MAX_ENTRIES: usize = 4096;
pub(crate) const MAX_ENTRY_BYTES: u64 = 16 << 20;
const MAX_TOTAL_BYTES: u64 = 256 << 20;

#[derive(Debug, Error)]
pub(crate) enum ArchiveError {
    #[error("{0}")]
    Limit(&'static str),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub(crate) struct Archive {
    inner: ZipArchive<File>,
    pub names: Vec<String>,
    read_bytes: u64,
}

pub(crate) fn summarize(report: &mut InspectReport, options: &InspectOptions) {
    if !report.markdown.content.is_empty() {
        return;
    }
    let mut text = format!(
        "# Archive listing\n\n- Entries: {}\n\n",
        report.entries.len()
    );
    for entry in &report.entries {
        text.push_str(&format!(
            "- {} ({} bytes)\n",
            crate::markdown::escape(&entry.name).replace(['\r', '\n'], " "),
            entry.size
        ));
        if text.len() > options.max_chars.saturating_mul(4) {
            break;
        }
    }
    crate::markdown::append(report, &text, options, "native_structure");
}

impl Archive {
    pub fn open(path: &Path, report: &mut InspectReport) -> Result<Self, ArchiveError> {
        let mut file = File::open(path)?;
        if file.metadata()?.len() > MAX_TOTAL_BYTES {
            return Err(ArchiveError::Limit("ZIP physical byte cap exceeded"));
        }
        check_directory(&mut file)?;
        let mut inner = ZipArchive::new(file)?;
        if inner.len() > MAX_ENTRIES {
            return Err(ArchiveError::Limit("ZIP entry-count cap exceeded"));
        }
        // The decoder deduplicates exact raw names before the entry loop below
        // can observe them; a dropped duplicate would never be audited.
        if inner.has_duplicate_names() {
            return Err(ArchiveError::Limit(
                "duplicate ZIP entry names are ambiguous",
            ));
        }
        let mut total = 0u64;
        let mut name_bytes = 0usize;
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        for i in 0..inner.len() {
            let entry = inner.by_index_raw(i)?;
            let name = entry.name()?.into_owned();
            total = total
                .checked_add(entry.size())
                .ok_or(ArchiveError::Limit("ZIP size overflow"))?;
            if total > MAX_TOTAL_BYTES {
                return Err(ArchiveError::Limit(
                    "ZIP total-uncompressed-bytes cap exceeded",
                ));
            }
            name_bytes += name.len();
            if name.len() > 4096 || name_bytes > 1 << 20 {
                return Err(ArchiveError::Limit("ZIP entry-name byte cap exceeded"));
            }
            if !seen.insert(name.clone()) {
                return Err(ArchiveError::Limit(
                    "duplicate ZIP entry names are ambiguous",
                ));
            }
            if entry.enclosed_name().is_none() || name.contains('\\') {
                report.error(
                    "archive",
                    format!("unsafe entry name retained as data: {name:?}"),
                );
            }
            names.push(name.clone());
            let timestamp = entry.last_modified().map(|d| {
                format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                    d.year(),
                    d.month(),
                    d.day(),
                    d.hour(),
                    d.minute(),
                    d.second()
                )
            });
            report.entries.push(ArchiveEntry {
                name,
                size: entry.size(),
                compressed_size: entry.compressed_size(),
                timestamp,
            });
        }
        report.counts.entries = Some(names.len());
        Ok(Self {
            inner,
            names,
            read_bytes: 0,
        })
    }

    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, ArchiveError> {
        if !self.names.iter().any(|n| n == name) {
            return Ok(None);
        }
        let entry = self.inner.by_name(name)?;
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(ArchiveError::Limit("ZIP per-entry read cap exceeded"));
        }
        // Charge the prospective worst case before any decompression so the
        // cumulative budget gates the work instead of being checked after it.
        let prospective = entry.size().min(MAX_ENTRY_BYTES) + 1;
        if self.read_bytes + prospective > MAX_TOTAL_BYTES {
            return Err(ArchiveError::Limit("ZIP cumulative read cap exceeded"));
        }
        let mut bytes = Vec::new();
        if let Err(error) = entry.take(MAX_ENTRY_BYTES + 1).read_to_end(&mut bytes) {
            // A failed read may already have decompressed up to the take cap;
            // charge that work so repeated failures cannot bypass the budget.
            self.read_bytes = (self.read_bytes + MAX_ENTRY_BYTES + 1).min(MAX_TOTAL_BYTES);
            return Err(error.into());
        }
        self.read_bytes = (self.read_bytes + bytes.len() as u64).min(MAX_TOTAL_BYTES);
        if bytes.len() as u64 > MAX_ENTRY_BYTES {
            return Err(ArchiveError::Limit("ZIP expanded entry exceeds read cap"));
        }
        Ok(Some(bytes))
    }
}

/// Validates the bounded EOCD/ZIP64 structure before constructing `ZipArchive`.
/// Accepts a seekable ZIP file positioned anywhere and restores it to offset
/// zero on success. Returns `Ok(())` for a supported bounded directory, or an
/// I/O/decoder/limit error when its structure is invalid, ambiguous, ZIP64,
/// multi-disk, or outside the configured entry and directory caps.
fn check_directory(file: &mut File) -> Result<(), ArchiveError> {
    // zip-9.6.1 accepts an EOCD record starting up to 65,558 bytes before EOF
    // (22-byte record + maximum 65,535-byte comment + one tolerated trailing
    // byte) and looks a further 20 bytes earlier for a ZIP64 locator.
    let tail_len = file.metadata()?.len().min(65_578) as usize;
    file.seek(SeekFrom::End(-(tail_len as i64)))?;
    let mut tail = Vec::with_capacity(tail_len);
    file.read_to_end(&mut tail)?;
    let u16_at = |i: usize| u16::from_le_bytes([tail[i], tail[i + 1]]);
    // Collect every EOCD candidate whose comment terminates at or before end of
    // file, mirroring the `zip` crate's acceptance rule. More than one candidate
    // is ambiguous: a shadow record could bypass the caps below while the
    // decoder falls back to the real directory.
    let mut candidates = Vec::new();
    if tail.len() >= 22 {
        for i in 0..=tail.len() - 22 {
            if &tail[i..i + 4] == b"PK\x05\x06" && i + 22 + u16_at(i + 20) as usize <= tail.len() {
                candidates.push(i);
            }
        }
    }
    // zip-9.6.1's ZIP64 probe position is the 20 bytes immediately before an
    // EOCD. Match that structure for every plausible EOCD before ambiguity
    // handling so a shadow decoy cannot hide a locator, including the retained
    // zero-field fixture whose classic EOCD does not carry ZIP64 sentinels.
    // Signatures elsewhere in comments and entry data remain ordinary bytes.
    let has_zip64_locator = candidates
        .iter()
        .copied()
        .any(|i| i >= 20 && &tail[i - 20..i - 16] == b"PK\x06\x07");
    if has_zip64_locator {
        return Err(ArchiveError::Limit(
            "ZIP64 directories require a bounded ZIP64 adapter",
        ));
    }
    let i = match candidates.as_slice() {
        [] => {
            return Err(ArchiveError::Limit(
                "ZIP missing or truncated end-of-directory",
            ));
        }
        &[i] => i,
        _ => {
            return Err(ArchiveError::Limit(
                "ambiguous ZIP end-of-directory candidates",
            ));
        }
    };
    if u16_at(i + 10) as usize > MAX_ENTRIES {
        return Err(ArchiveError::Limit(
            "ZIP entry-count cap exceeded before directory allocation",
        ));
    }
    let directory_bytes =
        u32::from_le_bytes(tail[i + 12..i + 16].try_into().expect("fixed EOCD slice")) as u64;
    if directory_bytes > 2 << 20 {
        return Err(ArchiveError::Limit("ZIP directory byte cap exceeded"));
    }
    if u16_at(i + 4) != 0 || u16_at(i + 6) != 0 || u16_at(i + 8) != u16_at(i + 10) {
        return Err(ArchiveError::Limit("multi-disk ZIP is unsupported"));
    }
    // The declared central directory must end at or before the EOCD record, so
    // the decoder cannot fall back to a different directory than the one the
    // entry-count and directory-size caps above were checked against.
    let eocd_offset = file.metadata()?.len() - tail.len() as u64 + i as u64;
    let directory_offset =
        u32::from_le_bytes(tail[i + 16..i + 20].try_into().expect("fixed EOCD slice")) as u64;
    if directory_offset + directory_bytes > eocd_offset {
        return Err(ArchiveError::Limit(
            "ZIP central directory range exceeds end-of-directory",
        ));
    }
    // Validate the exact central-directory resolution the locked decoder
    // performs for this candidate: the range abutting the EOCD must begin
    // with a CDFH signature, or a bounded forward scan from the declared
    // offset must find one. Otherwise the decoder rejects this archive.
    if u16_at(i + 10) > 0 {
        let abutting = eocd_offset
            .checked_sub(directory_bytes)
            .filter(|&start| start >= directory_offset);
        let resolved = match abutting {
            Some(start) => read_signature(file, start)? == Some(*b"PK\x01\x02"),
            None => false,
        };
        if !resolved && !scan_central_directory_signature(file, directory_offset, eocd_offset)? {
            return Err(ArchiveError::Limit(
                "ZIP central directory signature not found before end-of-directory",
            ));
        }
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}

/// Reads the four bytes at `offset`, returning `None` when the file is shorter.
fn read_signature(file: &mut File, offset: u64) -> Result<Option<[u8; 4]>, ArchiveError> {
    if offset.saturating_add(4) > file.metadata()?.len() {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut signature = [0; 4];
    file.read_exact(&mut signature)?;
    Ok(Some(signature))
}

/// Scans `[start, end)` for a CDFH signature in bounded chunks. The range is
/// capped by the archive physical limit enforced before this preflight runs.
fn scan_central_directory_signature(
    file: &mut File,
    start: u64,
    end: u64,
) -> Result<bool, ArchiveError> {
    if start >= end {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(start))?;
    let mut window = [0u8; 4];
    let mut filled = 0usize;
    let mut buffer = [0u8; 8192];
    let mut remaining = end - start;
    while remaining > 0 {
        let take = remaining.min(buffer.len() as u64) as usize;
        let mut read_total = 0usize;
        while read_total < take {
            let read = file.read(&mut buffer[read_total..take])?;
            if read == 0 {
                break;
            }
            read_total += read;
        }
        if read_total == 0 {
            break;
        }
        for &byte in &buffer[..read_total] {
            window.rotate_left(1);
            window[3] = byte;
            filled = (filled + 1).min(4);
            if filled == 4 && &window == b"PK\x01\x02" {
                return Ok(true);
            }
        }
        remaining -= read_total as u64;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// LSB-first bit writer for hand-built raw deflate streams.
    struct BitWriter {
        bytes: Vec<u8>,
        current: u8,
        bits: u32,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                current: 0,
                bits: 0,
            }
        }

        fn write(&mut self, value: u32, count: u32) {
            for i in 0..count {
                self.current |= (((value >> i) & 1) as u8) << self.bits;
                self.bits += 1;
                if self.bits == 8 {
                    self.bytes.push(self.current);
                    self.current = 0;
                    self.bits = 0;
                }
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.bits > 0 {
                self.bytes.push(self.current);
            }
            self.bytes
        }
    }

    /// Raw deflate stream (fixed Huffman) inflating to `1 + 258 * pairs` zero
    /// bytes: one literal zero followed by `pairs` length-258 distance-1 copies.
    fn deflate_zeros(pairs: usize) -> Vec<u8> {
        let mut writer = BitWriter::new();
        writer.write(1, 1); // BFINAL
        writer.write(1, 2); // BTYPE = fixed Huffman
        writer.write(0x0c, 8); // literal 0 (fixed code 0x30, bit-reversed)
        for _ in 0..pairs {
            writer.write(0xa3, 8); // length 258 (code 285 = 0xC5, bit-reversed)
            writer.write(0, 5); // distance 1 (code 0)
        }
        writer.write(0, 7); // end of block
        writer.finish()
    }

    /// ZIP container whose entries all share one deflate payload but declare
    /// `declared` uncompressed bytes in both headers.
    fn bomb_zip(entries: usize, payload: &[u8], crc: u32, declared: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut central = Vec::new();
        for i in 0..entries {
            let name = format!("b{i}.bin");
            let offset = bytes.len() as u32;
            bytes.extend_from_slice(b"PK\x03\x04");
            bytes.extend_from_slice(&20u16.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&8u16.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&crc.to_le_bytes());
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&declared.to_le_bytes());
            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(payload);
            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&8u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            central.extend_from_slice(&declared.to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = bytes.len() as u32;
        let cd_size = central.len() as u32;
        bytes.extend_from_slice(&central);
        bytes.extend_from_slice(b"PK\x05\x06");
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(entries as u16).to_le_bytes());
        bytes.extend_from_slice(&(entries as u16).to_le_bytes());
        bytes.extend_from_slice(&cd_size.to_le_bytes());
        bytes.extend_from_slice(&cd_offset.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    fn fixtures_dir() -> PathBuf {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/verification/audit-remediation-20261118/containers/fixtures");
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn scratch_report() -> crate::InspectReport {
        let path = fixtures_dir().join("archive-unit-scratch.txt");
        std::fs::write(&path, b"scratch").unwrap();
        crate::inspect(&path, &crate::InspectOptions::default())
    }

    /// LITX-AUD-011: reads that fail after decompression work must still charge the
    /// cumulative budget, and the budget must gate the next read before any
    /// decompression starts. Here each entry declares 4 KiB but inflates past the
    /// per-entry cap; after enough failures the next read must be refused by the
    /// cumulative cap instead of attempting decompression again.
    #[test]
    fn failed_reads_charge_budget_and_gate_later_reads() {
        let pairs = 65_031; // 1 + 258 * pairs = 16,777,999 > MAX_ENTRY_BYTES
        let payload = deflate_zeros(pairs);
        let zeros = 1 + 258 * pairs;
        let crc = crc32fast::hash(&vec![0u8; zeros]);
        let zip = bomb_zip(17, &payload, crc, 4096);
        let path = fixtures_dir().join("zip-failed-read-budget.zip");
        std::fs::write(&path, &zip).unwrap();
        let mut report = scratch_report();
        let mut archive = Archive::open(&path, &mut report).unwrap();
        for i in 0..16 {
            // The entry over-expands past its declared size, so the read fails; the
            // exact error variant is decoder-owned, the charging invariant is ours.
            let error = archive.read(&format!("b{i}.bin")).unwrap_err();
            assert!(
                !matches!(error, ArchiveError::Limit(message) if message.contains("cumulative read cap")),
                "b{i}.bin must not hit the cumulative cap yet: {error}"
            );
        }
        let error = archive.read("b16.bin").unwrap_err();
        assert!(
            matches!(error, ArchiveError::Limit(message) if message.contains("cumulative read cap")),
            "b16.bin: {error}"
        );
    }

    /// LITX-AUD-011 preservation: honest small entries keep reading and the
    /// cumulative charge tracks their real sizes.
    #[test]
    fn honest_entries_read_and_charge_their_real_size() {
        let payload = deflate_zeros(1); // 259 zeros, declares honestly below
        let zeros = 259usize;
        let crc = crc32fast::hash(&vec![0u8; zeros]);
        let zip = bomb_zip(2, &payload, crc, zeros as u32);
        let path = fixtures_dir().join("zip-honest-read-budget.zip");
        std::fs::write(&path, &zip).unwrap();
        let mut report = scratch_report();
        let mut archive = Archive::open(&path, &mut report).unwrap();
        let first = archive.read("b0.bin").unwrap().unwrap();
        assert_eq!(first.len(), zeros);
        assert!(first.iter().all(|&b| b == 0));
        let second = archive.read("b1.bin").unwrap().unwrap();
        assert_eq!(second.len(), zeros);
        assert_eq!(archive.read_bytes, 2 * zeros as u64);
        assert!(archive.read("missing.bin").unwrap().is_none());
    }
}
