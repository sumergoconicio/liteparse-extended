//! Header listing only. No RAR payload is decompressed or written to disk.
use crate::{
    ArchiveEntry, InspectReport, Page,
    binary::{Source, slice},
    comics, office, xml,
};
use std::path::Path;

fn le16(b: &[u8], i: usize) -> xml::Result<u16> {
    Ok(u16::from_le_bytes(slice(b, i, 2)?.try_into()?))
}
fn le32(b: &[u8], i: usize) -> xml::Result<u32> {
    Ok(u32::from_le_bytes(slice(b, i, 4)?.try_into()?))
}

pub(crate) fn inspect(path: &Path, report: &mut InspectReport) {
    if let Err(error) = inner(path, report) {
        report.error("rar", error);
    }
}

fn inner(path: &Path, report: &mut InspectReport) -> xml::Result<()> {
    let mut source = Source::open(path)?;
    let signature = source.range(0, source.len.min(8) as usize)?;
    let version = if signature.starts_with(b"Rar!\x1a\x07\x00") {
        4
    } else if signature == b"Rar!\x1a\x07\x01\x00" {
        5
    } else {
        return Err("invalid RAR signature".into());
    };
    let comic = report.format.extension == "cbr";
    office::identify(
        report,
        if comic { "cbr" } else { "rar" },
        if comic {
            "application/vnd.comicbook-rar"
        } else {
            "application/vnd.rar"
        },
        if comic {
            "RAR comic book"
        } else {
            "RAR archive"
        },
    );
    let mut cursor = if version == 4 { 7 } else { 8 };
    let mut blocks = 0usize;
    let mut main_seen = false;
    while cursor < source.len {
        blocks += 1;
        if blocks > 16_384 {
            return Err("RAR header-count cap exceeded".into());
        }
        let (next, finished) = if version == 4 {
            rar4(&mut source, cursor, report, &mut main_seen)?
        } else {
            rar5(&mut source, cursor, report, &mut main_seen)?
        };
        if next <= cursor || next > source.len {
            return Err("RAR data block exceeds file".into());
        }
        cursor = next;
        if finished {
            break;
        }
    }
    if !main_seen {
        return Err("missing RAR main header".into());
    }
    report.counts.entries = Some(report.entries.len());
    if comic {
        report.counts.pages = Some(report.pages.len());
        report.error("rar_preview", "header-only RAR: image dimensions and cover are unavailable (no decompression); page count uses image entry names");
    }
    Ok(())
}

fn entry(
    report: &mut InspectReport,
    name: String,
    size: u64,
    packed: u64,
    directory: bool,
) -> xml::Result<()> {
    if report.entries.len() >= 4096 || name.len() > 4096 {
        return Err("RAR entry-count/name cap exceeded".into());
    }
    let total_names: usize = report.entries.iter().map(|e| e.name.len()).sum();
    if total_names + name.len() > 1 << 20 {
        return Err("RAR cumulative name cap exceeded".into());
    }
    if name.starts_with(['/', '\\'])
        || name.contains(['\\', ':'])
        || name.split('/').any(|c| c == "..")
    {
        report.error(
            "rar",
            format!("unsafe entry name retained as data: {name:?}"),
        );
    }
    if !directory && comics::image_name(&name) && report.format.extension == "cbr" {
        report.pages.push(Page {
            index: report.pages.len() + 1,
            source_part: name.clone(),
            width: None,
            height: None,
            dpi: None,
            version: None,
        });
    }
    report.entries.push(ArchiveEntry {
        name,
        size,
        compressed_size: packed,
        timestamp: None,
    });
    Ok(())
}

fn rar4(
    source: &mut Source,
    cursor: u64,
    report: &mut InspectReport,
    main_seen: &mut bool,
) -> xml::Result<(u64, bool)> {
    let prefix = source.range(cursor, 7)?;
    let kind = prefix[2];
    let flags = le16(&prefix, 3)?;
    let size = le16(&prefix, 5)? as usize;
    if size < 7 {
        return Err("invalid RAR4 header size".into());
    }
    let h = source.range(cursor, size)?;
    if crc32fast::hash(&h[2..]) as u16 != le16(&h, 0)? {
        return Err("RAR4 header CRC mismatch".into());
    }
    if !*main_seen && kind != 0x73 {
        return Err("RAR4 main header must follow signature".into());
    }
    let mut packed = if flags & 0x8000 != 0 {
        u64::from(le32(&h, 7)?)
    } else {
        0
    };
    if kind == 0x73 {
        slice(&h, 0, 13)?;
        *main_seen = true;
        if flags & 0x80 != 0 {
            return Err("encrypted RAR4 headers are unavailable".into());
        }
        if flags & 1 != 0 {
            report.error("rar", "multi-volume RAR: listing covers this volume only");
        }
    } else if kind == 0x74 {
        let mut unpacked = u64::from(le32(&h, 11)?);
        let name_size = le16(&h, 26)? as usize;
        let name_at = if flags & 0x100 != 0 {
            packed |= u64::from(le32(&h, 32)?) << 32;
            unpacked |= u64::from(le32(&h, 36)?) << 32;
            40
        } else {
            32
        };
        let raw_name = slice(&h, name_at, name_size)?;
        let raw_name = if let Some(zero) = raw_name.iter().position(|&b| b == 0) {
            report.error(
                "rar",
                "RAR4 encoded Unicode filename is shown using its legacy name only",
            );
            &raw_name[..zero]
        } else {
            raw_name
        };
        let name = String::from_utf8_lossy(raw_name).into_owned();
        entry(report, name, unpacked, packed, flags & 0xe0 == 0xe0)?;
    }
    let next = cursor
        .checked_add(size as u64)
        .and_then(|n| n.checked_add(packed))
        .ok_or("RAR4 block size overflow")?;
    Ok((next, kind == 0x7b))
}

fn vint(bytes: &[u8], at: &mut usize) -> xml::Result<u64> {
    let mut result = 0u64;
    for shift in (0..=63).step_by(7) {
        let b = *bytes.get(*at).ok_or("truncated RAR variable integer")?;
        *at += 1;
        if shift == 63 && b > 1 {
            return Err("RAR variable integer overflow".into());
        }
        result |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err("RAR variable integer overflow".into())
}

fn rar5(
    source: &mut Source,
    cursor: u64,
    report: &mut InspectReport,
    main_seen: &mut bool,
) -> xml::Result<(u64, bool)> {
    let prefix = source.range(cursor, source.len.saturating_sub(cursor).min(7) as usize)?;
    let mut used = 4;
    let size = vint(&prefix, &mut used)?;
    if size > 2 << 20 {
        return Err("RAR5 header byte cap exceeded".into());
    }
    let h = source.range(cursor, used + size as usize)?;
    if crc32fast::hash(&h[4..]) != le32(&h, 0)? {
        return Err("RAR5 header CRC mismatch".into());
    }
    let mut at = used;
    let kind = vint(&h, &mut at)?;
    let flags = vint(&h, &mut at)?;
    let extra = if flags & 1 != 0 {
        vint(&h, &mut at)?
    } else {
        0
    };
    let packed = if flags & 2 != 0 {
        vint(&h, &mut at)?
    } else {
        0
    };
    if extra > size {
        return Err("RAR5 extra area exceeds header".into());
    }
    let body = &h[..h.len() - extra as usize];
    if !*main_seen && kind != 1 && kind != 4 {
        return Err("RAR5 main header must follow signature".into());
    }
    match kind {
        1 => {
            let archive_flags = vint(body, &mut at)?;
            *main_seen = true;
            if archive_flags & 1 != 0 {
                report.error("rar", "multi-volume RAR: listing covers this volume only");
            }
        }
        2 => {
            let file_flags = vint(body, &mut at)?;
            let unpacked = vint(body, &mut at)?;
            if file_flags & 8 != 0 {
                return Err("RAR5 unknown unpacked sizes are unsupported".into());
            }
            let _attributes = vint(body, &mut at)?;
            if file_flags & 2 != 0 {
                slice(body, at, 4)?;
                at += 4;
            }
            if file_flags & 4 != 0 {
                slice(body, at, 4)?;
                at += 4;
            }
            let _compression = vint(body, &mut at)?;
            let _host = vint(body, &mut at)?;
            let name_size = vint(body, &mut at)?;
            if name_size > 4096 {
                return Err("RAR5 entry-name cap exceeded".into());
            }
            let name = std::str::from_utf8(slice(body, at, name_size as usize)?)?.to_string();
            entry(report, name, unpacked, packed, file_flags & 1 != 0)?;
        }
        4 => return Err("encrypted RAR5 headers are unavailable".into()),
        _ => {}
    }
    let next = cursor
        .checked_add(h.len() as u64)
        .and_then(|n| n.checked_add(packed))
        .ok_or("RAR5 block size overflow")?;
    Ok((next, kind == 5))
}
