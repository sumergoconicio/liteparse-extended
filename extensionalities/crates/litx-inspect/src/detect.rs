use crate::{Format, InspectReport};
use file_format::FileFormat;
use std::{fs::File, io::Read, path::Path};

pub(crate) fn bucket(ext: &str) -> &'static str {
    for (name, extensions) in [
        ("PDF", "pdf"),
        ("Office Docs", "doc docx odt rtf pages"),
        ("Presentations", "ppt pptx odp key"),
        ("Spreadsheets", "xls xlsx xlsm ods numbers"),
        ("Text", "txt md rst tex"),
        ("Tabular Data", "csv tsv parquet arrow"),
        ("Structured Data", "json jsonl xml yaml yml toml"),
        ("Images", "jpg jpeg png gif webp heic avif tif tiff bmp"),
        ("Vector Graphics", "svg eps"),
        ("Ebooks", "epub mobi azw azw3 prc djvu djv"),
        ("Comics", "cbz cbr"),
        ("Audio", "mp3 m4a aac flac wav ogg opus aiff"),
        ("Video", "mp4 mkv mov avi webm m4v mpeg"),
        (
            "Archives",
            "zip rar 7z tar gz bz2 xz zst tar.gz tar.bz2 tar.xz tar.zst",
        ),
        ("Source Code", "py go js ts jsx tsx java c cpp rs sh sql"),
        ("Web", "html htm css mhtml"),
        ("Databases", "sqlite sqlite3 db mdb accdb"),
        ("Personal Info", "eml msg mbox ics vcf"),
        ("Subtitles", "srt vtt ass"),
        ("Fonts", "ttf otf woff woff2"),
        ("3D CAD", "stl obj gltf glb step stp dwg dxf"),
        ("Applications", "exe msi apk deb rpm appimage"),
    ] {
        if extensions.split_whitespace().any(|e| e == ext) {
            return name;
        }
    }
    "Other"
}

pub(crate) fn extension(path: &Path) -> String {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    for suffix in ["tar.gz", "tar.bz2", "tar.xz", "tar.zst"] {
        if name.ends_with(&format!(".{suffix}")) {
            return suffix.into();
        }
    }
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}

pub(crate) fn detect(path: &Path, report: &mut InspectReport) -> std::io::Result<()> {
    // Do not let detector readers recursively inspect unbounded container payloads.
    let mut prefix = Vec::new();
    File::open(path)?.take(65_536).read_to_end(&mut prefix)?;
    let format = FileFormat::from_bytes(&prefix);
    let ext = report.format.extension.clone();
    let detected_ext = format.extension();
    let detected_bucket = bucket(detected_ext);
    let ambiguous = detected_bucket == "Other" || matches!(format, FileFormat::PlainText);
    let chosen_bucket = if ambiguous {
        bucket(&ext)
    } else {
        detected_bucket
    };
    let alias = |s: &str| {
        match s {
            "jpeg" => "jpg",
            "tiff" => "tif",
            "sqlite3" | "db" => "sqlite",
            "htm" => "html",
            "cbz" => "zip",
            "cbr" => "rar",
            other => other,
        }
        .to_string()
    };
    report.format = Format {
        extension: ext.clone(),
        detected_mime: Some(format.media_type().into()),
        detected_name: Some(format.name().into()),
        bucket: chosen_bucket.into(),
        basis: if ambiguous { "extension" } else { "bytes" }.into(),
        disagreement: !ambiguous
            && !ext.is_empty()
            && alias(&ext) != alias(detected_ext)
            && !ext.starts_with(&format!("tar.{detected_ext}")),
        detector: "file-format".into(),
    };
    #[cfg(feature = "magika")]
    if ambiguous {
        if let Err(error) = magika_fallback(&prefix, report) {
            report.error("magika", error);
        }
    }
    Ok(())
}

#[cfg(feature = "magika")]
fn magika_fallback(prefix: &[u8], report: &mut InspectReport) -> crate::xml::Result<()> {
    let path = std::env::var_os("ORT_DYLIB_PATH")
        .ok_or("optional Magika requires ORT_DYLIB_PATH; no runtime is downloaded")?;
    if !Path::new(&path).is_file() {
        return Err("ORT_DYLIB_PATH is not a regular file".into());
    }
    ort::init_from(path)?.commit();
    let mut session = magika::Builder::default()
        .with_intra_threads(2)
        .with_inter_threads(1)
        .build()?;
    let found = session.identify_content_sync(prefix)?;
    let info = found.info();
    report.claims.push(crate::Claim {
        key: "magika_detection".into(),
        value: format!(
            "{}; score={}; sampled_bytes={}",
            info.label,
            found.score(),
            prefix.len()
        ),
        source: "bytes",
    });
    // A statistical alternative remains a claim, not a byte-verified identity.
    Ok(())
}
