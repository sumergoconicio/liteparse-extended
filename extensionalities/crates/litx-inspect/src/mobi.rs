use crate::{
    Claim, InspectOptions, InspectReport,
    binary::{Source, be16, be32, slice},
    office, xml,
};
use quick_xml::{Reader, events::Event};
use std::path::Path;

pub(crate) fn inspect(path: &Path, report: &mut InspectReport, options: &InspectOptions) {
    if let Err(error) = inner(path, report, options) {
        report.error("mobi", error);
    }
}

fn claim(report: &mut InspectReport, key: &str, value: impl ToString, source: &'static str) {
    report.claims.push(Claim {
        key: key.into(),
        value: value.to_string(),
        source,
    });
}

fn decode(bytes: &[u8], encoding: u32) -> xml::Result<String> {
    match encoding {
        65001 => Ok(std::str::from_utf8(bytes)?.into()),
        1252 => Ok(encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned()),
        _ => Err("unsupported MOBI text encoding".into()),
    }
}

fn inner(path: &Path, report: &mut InspectReport, options: &InspectOptions) -> xml::Result<()> {
    let mut source = Source::open(path)?;
    let pdb = source.range(0, 78)?;
    if slice(&pdb, 60, 8)? != b"BOOKMOBI" {
        return Err("invalid MOBI PalmDB signature".into());
    }
    let count = be16(&pdb, 76)? as usize;
    if count == 0 || count > 4096 {
        return Err("MOBI record-count cap exceeded".into());
    }
    let table = source.range(78, count * 8)?;
    let mut offsets = Vec::with_capacity(count + 1);
    for i in 0..count {
        offsets.push(u64::from(be32(&table, i * 8)?));
    }
    offsets.push(source.len);
    if offsets[0] < (78 + count * 8) as u64 || offsets.windows(2).any(|p| p[0] >= p[1]) {
        return Err("invalid MOBI record offsets".into());
    }
    let head = source.range(offsets[0], (offsets[1] - offsets[0]) as usize)?;
    if slice(&head, 16, 4)? != b"MOBI" {
        return Err("missing MOBI record header".into());
    }
    let header_size = be32(&head, 20)? as usize;
    if header_size < 116 {
        return Err("short MOBI header".into());
    }
    slice(&head, 16, header_size)?;
    let encoding = be32(&head, 28)?;
    let version = be32(&head, 36)?;
    office::identify(
        report,
        "mobi",
        "application/x-mobipocket-ebook",
        "MOBI ebook",
    );
    // AZW aliases cannot be distinguished by extension; KF8 has a byte version.
    report.format.disagreement = !report.format.extension.is_empty()
        && !matches!(
            report.format.extension.as_str(),
            "mobi" | "azw" | "azw3" | "prc"
        );
    claim(report, "mobi_version", version, "bytes");
    claim(report, "text_encoding", encoding, "bytes");
    let title_offset = be32(&head, 84)? as usize;
    let title_length = be32(&head, 88)? as usize;
    if title_length > 0 {
        claim(
            report,
            "title",
            decode(slice(&head, title_offset, title_length)?, encoding)?,
            "container_metadata",
        );
    }
    let mut cover = None;
    if be32(&head, 128)? & 0x40 != 0 {
        let exth = slice(&head, 16 + header_size, 12)?;
        if &exth[..4] != b"EXTH" {
            return Err("missing EXTH signature".into());
        }
        let exth = slice(&head, 16 + header_size, be32(exth, 4)? as usize)?;
        let records = be32(exth, 8)? as usize;
        if records > 4096 {
            return Err("EXTH record-count cap exceeded".into());
        }
        let mut cursor = 12;
        for _ in 0..records {
            let kind = be32(exth, cursor)?;
            let len = be32(exth, cursor + 4)? as usize;
            if len < 8 {
                return Err("invalid EXTH record length".into());
            }
            let data = slice(exth, cursor + 8, len - 8)?;
            let key = match kind {
                100 => Some("author"),
                101 => Some("publisher"),
                113 | 504 => Some("asin"),
                503 => Some("title"),
                524 => Some("language"),
                _ => None,
            };
            if let Some(key) = key {
                claim(report, key, decode(data, encoding)?, "container_metadata");
            }
            if kind == 201 {
                cover = Some(be32(data, 0)?);
            }
            cursor += len;
        }
    }
    if !report.claims.iter().any(|c| c.key == "language") {
        let locale = be32(&head, 92)?;
        let language = match locale & 0x3ff {
            9 => Some("en"),
            7 => Some("de"),
            10 => Some("es"),
            12 => Some("fr"),
            16 => Some("it"),
            17 => Some("ja"),
            18 => Some("ko"),
            4 => Some("zh"),
            25 => Some("ru"),
            22 => Some("pt"),
            1 => Some("ar"),
            _ => None,
        };
        if let Some(language) = language {
            claim(report, "language", language, "container_metadata");
        }
        claim(report, "mobi_locale", locale, "container_metadata");
    }
    if options.thumbnail.is_some()
        && let Some(relative) = cover
    {
        let result = (|| -> xml::Result<()> {
            let index = be32(&head, 108)?
                .checked_add(relative)
                .ok_or("MOBI cover offset overflow")? as usize;
            if index >= count {
                return Err("MOBI cover record out of bounds".into());
            }
            let bytes = source.range(
                offsets[index],
                (offsets[index + 1] - offsets[index]) as usize,
            )?;
            crate::thumbnail::write(std::io::Cursor::new(bytes), "embedded", options, report);
            Ok(())
        })();
        if let Err(error) = result {
            report.error("thumbnail", error);
        }
    }
    let text_result = (|| -> xml::Result<()> {
        if be16(&head, 12)? != 0 {
            return Err("encrypted MOBI text is unavailable; metadata retained".into());
        }
        let compression = be16(&head, 0)?;
        if !matches!(compression, 1 | 2) {
            return Err("unsupported MOBI compression (HUFF/CDIC); metadata retained".into());
        }
        let text_records = be16(&head, 8)? as usize;
        if text_records >= count {
            return Err("MOBI text record count exceeds database".into());
        }
        let text_length = be32(&head, 4)? as usize;
        if text_length > 32 << 20 {
            return Err("MOBI expanded text cap exceeded".into());
        }
        let mut text = Vec::new();
        let extras = if header_size >= 228 {
            be32(&head, 240)?
        } else {
            0
        };
        for i in 1..=text_records {
            let bytes = source.range(offsets[i], (offsets[i + 1] - offsets[i]) as usize)?;
            let body = strip_trailing(&bytes, extras)?;
            let decoded = if compression == 1 {
                body.to_vec()
            } else {
                palmdoc(body)?
            };
            if decoded.len() > 65_536 || text.len() + decoded.len() > 32 << 20 {
                return Err("MOBI record expansion cap exceeded".into());
            }
            text.extend(decoded);
        }
        if text.len() < text_length {
            return Err("truncated MOBI text".into());
        }
        text.truncate(text_length);
        let text = markup_text(&decode(&text, encoding)?)?;
        report.append_text(&text, options.max_chars);
        claim(
            report,
            "text_status",
            if version >= 8 {
                "best_effort_kf8_flow_text"
            } else {
                "native_mobi_text"
            },
            "bytes",
        );
        Ok(())
    })();
    if let Err(error) = text_result {
        report.error("mobi_text", error);
    }
    Ok(())
}

fn strip_trailing(mut data: &[u8], flags: u32) -> xml::Result<&[u8]> {
    for bit in 1..32 {
        if flags & (1 << bit) == 0 {
            continue;
        }
        let mut length = 0usize;
        let mut done = false;
        for n in 0..4 {
            let b = *data
                .get(
                    data.len()
                        .checked_sub(n + 1)
                        .ok_or("truncated MOBI trailing data")?,
                )
                .ok_or("truncated MOBI trailing data")?;
            length |= ((b & 0x7f) as usize) << (7 * n);
            if b & 0x80 != 0 {
                done = true;
                break;
            }
        }
        if !done || length == 0 || length > data.len() {
            return Err("invalid MOBI trailing entry size".into());
        }
        data = &data[..data.len() - length];
    }
    if flags & 1 != 0 {
        let length = (data.last().ok_or("missing MOBI overlap byte")? & 3) as usize + 1;
        data = data
            .get(
                ..data
                    .len()
                    .checked_sub(length)
                    .ok_or("truncated MOBI overlap")?,
            )
            .ok_or("truncated MOBI overlap")?;
    }
    Ok(data)
}

fn palmdoc(data: &[u8]) -> xml::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        i += 1;
        match b {
            1..=8 => {
                out.extend_from_slice(slice(data, i, b as usize)?);
                i += b as usize;
            }
            0 | 9..=127 => out.push(b),
            128..=191 => {
                let pair = u16::from(b) * 256
                    + u16::from(*data.get(i).ok_or("truncated PalmDOC backreference")?);
                i += 1;
                let distance = ((pair & 0x3fff) >> 3) as usize;
                if distance == 0 || distance > out.len() {
                    return Err("invalid PalmDOC backreference distance".into());
                }
                for _ in 0..(pair & 7) + 3 {
                    out.push(out[out.len() - distance]);
                }
            }
            _ => {
                out.push(b' ');
                out.push(b ^ 0x80);
            }
        }
        if out.len() > 65_536 {
            return Err("PalmDOC expansion cap exceeded".into());
        }
    }
    Ok(out)
}

fn markup_text(value: &str) -> xml::Result<String> {
    let mut reader = Reader::from_str(value);
    reader.config_mut().check_end_names = false;
    let mut out = String::new();
    let mut blocked = None;
    let version = quick_xml::XmlVersion::default();
    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let name = e.name().as_ref().to_ascii_lowercase();
                if blocked.is_none() && matches!(name.as_str(), "head" | "style" | "script") {
                    blocked = Some(name.clone());
                }
                if blocked.is_none() && name == "br" {
                    out.push('\n');
                }
            }
            Event::End(e) => {
                let name = e.name().as_ref().to_ascii_lowercase();
                if blocked.as_ref() == Some(&name) {
                    blocked = None;
                }
                if blocked.is_none()
                    && matches!(name.as_str(), "p" | "div" | "h1" | "h2" | "li")
                    && !out.ends_with('\n')
                {
                    out.push('\n');
                }
            }
            Event::Text(e) if blocked.is_none() => out.push_str(&e.xml_content(version)),
            Event::GeneralRef(e) if blocked.is_none() => {
                if let Some(c) = e.resolve_char_ref()? {
                    out.push(c);
                } else {
                    out.push_str(match e.xml_content(version).as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        "nbsp" => " ",
                        _ => "�",
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}
