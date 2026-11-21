//! Bounded, offline Quick Look. Claims retain their source; they are not verified identities.
use serde::Serialize;
use std::path::{Path, PathBuf};
mod archive;
mod binary;
mod comics;
mod detect;
mod djvu;
mod epub;
mod images;
mod iwork;
mod json_profile;
mod markdown;
mod mobi;
mod odf;
mod office;
mod parquet;
mod pdf;
mod profile;
mod rar;
mod spreadsheet;
mod tables;
mod tabular;
mod text;
mod thumbnail;
mod xml;
mod xml_profile;
pub use djvu::{DjvuAnalysis, DjvuPageSignal};
pub use images::{ImageAnalysis, OcrProbe};
pub use iwork::DerivedPreview;
pub use json_profile::JsonProfile;
pub use markdown::Markdown;
pub use parquet::ParquetProfile;
pub use pdf::{PdfAnalysis, PdfGeometry};
pub use tables::{DocumentTable, TableCell};
pub use tabular::DatasetPreview;
pub use xml_profile::{XmlName, XmlPath, XmlProfile};

/// Explicitly linked libav bindings, never part of the default binary.
#[cfg(feature = "ffmpeg-link")]
pub use ffmpeg_next as linked_ffmpeg;

#[derive(Debug, Clone)]
pub struct InspectOptions {
    pub identify: bool,
    pub max_tokens: Option<usize>,
    pub max_pages: Option<usize>,
    pub max_chars: usize,
    pub max_units: usize,
    pub thumbnail: Option<PathBuf>,
    pub thumbnail_max_edge: u32,
    pub packet: bool,
    pub probe_ocr: bool,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            identify: false,
            max_tokens: None,
            max_pages: Some(5),
            max_chars: 16_000,
            max_units: 5,
            thumbnail: None,
            thumbnail_max_edge: 320,
            packet: false,
            probe_ocr: false,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Format {
    pub extension: String,
    pub detected_mime: Option<String>,
    pub detected_name: Option<String>,
    pub bucket: String,
    pub basis: String,
    pub disagreement: bool,
    pub detector: String,
}

#[derive(Debug, Serialize)]
pub struct Claim {
    pub key: String,
    pub value: String,
    pub source: &'static str,
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spine_items: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tables: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheets: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slides: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paragraphs: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<usize>,
    pub chars: usize,
    pub bytes: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct Text {
    pub tokens: usize,
    pub token_basis: &'static str,
    pub content: String,
    pub truncated: bool,
    pub chars: usize,
    pub origin: String,
}

#[derive(Debug, Serialize)]
pub struct StageError {
    pub stage: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct Thumbnail {
    pub written_to: PathBuf,
    pub width: u32,
    pub height: u32,
    pub basis: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ArchiveEntry {
    pub name: String,
    pub size: u64,
    pub compressed_size: u64,
    pub timestamp: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub index: usize,
    pub source_part: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub dpi: Option<u16>,
    pub version: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Preflight {
    pub page_number: usize,
    pub unit_kind: &'static str,
    pub needs_ocr: bool,
    pub reasons: Vec<String>,
    pub confidence: &'static str,
    pub coverage_available: bool,
}

#[derive(Debug, Serialize)]
pub struct Column {
    pub index: usize,
    pub header: Option<String>,
    pub inferred_type: String,
    pub non_empty_count: usize,
    pub distinct_count: usize,
    pub total_bytes: usize,
    pub max_bytes: usize,
    pub mean_bytes: f64,
    pub avg_prose_score: f64,
    pub is_prose_candidate: bool,
    pub sample_values: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Sheet {
    pub preview_rows: Vec<Vec<String>>,
    pub preview_truncated: bool,
    pub headers_preview: Vec<Option<String>>,
    pub name: String,
    pub index: usize,
    pub used_range: Option<String>,
    pub rows: usize,
    pub cols: usize,
    pub cell_count: usize,
    pub non_empty_cells: usize,
    pub formula_count: usize,
    pub merged_ranges: Vec<String>,
    pub is_longest: bool,
    pub columns: Vec<Column>,
}

#[derive(Debug, Serialize)]
pub struct InspectReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub djvu: Option<DjvuAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived_preview: Option<DerivedPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset: Option<DatasetPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parquet: Option<ParquetProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<JsonProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xml: Option<XmlProfile>,
    pub source_path: PathBuf,
    pub artifacts: Vec<Artifact>,
    pub markdown: Markdown,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf: Option<PdfAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageAnalysis>,
    pub pages: Vec<Page>,
    pub format: Format,
    pub claims: Vec<Claim>,
    pub counts: Counts,
    pub entries: Vec<ArchiveEntry>,
    pub sheets: Vec<Sheet>,
    pub tables: Vec<DocumentTable>,
    pub preflight: Vec<Preflight>,
    pub text: Text,
    pub thumbnail: Option<Thumbnail>,
    pub errors: Vec<StageError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct Artifact {
    pub kind: &'static str,
    pub path: PathBuf,
}

impl InspectReport {
    pub(crate) fn append_text(&mut self, value: &str, cap: usize) {
        let chars = value.chars().count();
        self.append_counted(value, chars, 1, cap);
    }

    pub(crate) fn append_counted(
        &mut self,
        prefix: &str,
        chars: usize,
        repeats: usize,
        cap: usize,
    ) {
        self.counts.chars = self
            .counts
            .chars
            .saturating_add(chars.saturating_mul(repeats));
        if let Some(copies) = cap.saturating_sub(self.text.chars).checked_div(chars) {
            let copies = repeats.min(copies.saturating_add(1));
            for _ in 0..copies {
                let remaining = cap.saturating_sub(self.text.chars);
                let taken = chars.min(remaining);
                self.text.content.extend(prefix.chars().take(taken));
                self.text.chars += taken;
            }
        }
        self.text.truncated = self.counts.chars > self.text.chars;
    }
    pub(crate) fn error(&mut self, stage: &'static str, message: impl ToString) {
        self.errors.push(StageError {
            stage,
            message: message.to_string(),
        });
    }

    pub fn quick_look(&self) -> String {
        let mut out = format!(
            "{} ({})\n{} bytes, {} chars\n",
            self.format.bucket,
            self.format
                .detected_name
                .as_deref()
                .unwrap_or("extension hint"),
            self.counts.bytes,
            self.counts.chars
        );
        if let Some(pages) = self.counts.pages {
            out.push_str(&format!("{pages} pages\n"));
        }
        for claim in &self.claims {
            out.push_str(&format!(
                "{}: {} [{}]\n",
                claim.key, claim.value, claim.source
            ));
        }
        if let Some(image) = &self.image {
            out.push_str(&format!(
                "image: {}x{}, {} [{}]\n",
                image.width, image.height, image.classification, image.basis
            ));
            if let Some(probe) = &image.probe {
                out.push_str(&format!(
                    "OCR: {} words, mean confidence {:.2} [{}]\n",
                    probe.word_count, probe.mean_confidence, probe.basis
                ));
            }
        }
        out.push_str(&self.text.content);
        for error in &self.errors {
            out.push_str(&format!("\n[{}] {}", error.stage, error.message));
        }
        out
    }
}

pub fn inspect(path: &Path, options: &InspectOptions) -> InspectReport {
    Inspector::default().inspect(path, options)
}

/// Sequential inspection session. OCR models load lazily and stay local to this session.
#[derive(Default)]
pub struct Inspector {
    ocr: images::OcrSession,
}

impl Inspector {
    pub fn inspect(&mut self, path: &Path, options: &InspectOptions) -> InspectReport {
        inspect_with_session(path, options, &mut self.ocr)
    }
}

/// Inspect one file while reusing the caller's lazy OCR session when supported.
///
/// Accepts a source path, inspection limits and output options, and a mutable
/// session. Returns a report with bounded identification and any supported
/// extraction. Filesystem, parser, renderer, and OCR failures are recorded in
/// `report.errors`; unsupported formats retain their static support boundary.
fn inspect_with_session(
    path: &Path,
    options: &InspectOptions,
    ocr: &mut images::OcrSession,
) -> InspectReport {
    let extension = detect::extension(path);
    let mut report = InspectReport {
        djvu: None,
        derived_preview: None,
        dataset: None,
        parquet: None,
        json: None,
        xml: None,
        source_path: std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        artifacts: vec![],
        markdown: Markdown::default(),
        pdf: None,
        image: None,
        pages: vec![],
        format: Format {
            extension: extension.clone(),
            detected_mime: None,
            detected_name: None,
            bucket: if extension == "txt" { "Text" } else { "Other" }.into(),
            basis: "extension".into(),
            disagreement: false,
            detector: "extension".into(),
        },
        claims: vec![],
        counts: Counts::default(),
        entries: vec![],
        sheets: vec![],
        tables: vec![],
        preflight: vec![],
        text: Text {
            origin: "native".into(),
            token_basis: "whitespace_words",
            ..Text::default()
        },
        thumbnail: None,
        errors: vec![],
        packet: None,
    };
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => report.counts.bytes = meta.len(),
        Ok(_) => report.error("input", "input is not a regular file"),
        Err(error) => report.error("input", error),
    }
    if report.errors.is_empty() {
        if let Err(error) = detect::detect(path, &mut report) {
            report.error("detection", error);
        } else if report.format.detected_mime.as_deref().is_some_and(|mime| {
            mime == "application/zip"
                || mime == "application/epub+zip"
                || mime.starts_with("application/vnd.openxmlformats-officedocument.")
                || mime.starts_with("application/vnd.oasis.opendocument.")
                || mime == "application/vnd.ms-excel.sheet.macroEnabled.12"
                || mime.starts_with("application/vnd.apple.")
        }) || (report.format.basis == "extension"
            && matches!(
                report.format.extension.as_str(),
                "zip"
                    | "cbz"
                    | "epub"
                    | "docx"
                    | "xlsx"
                    | "xlsm"
                    | "pptx"
                    | "odt"
                    | "ods"
                    | "odp"
                    | "pages"
                    | "numbers"
                    | "key"
            ))
        {
            match archive::Archive::open(path, &mut report) {
                Ok(mut archive) => {
                    office::inspect(&mut archive, &mut report, options);
                    epub::inspect(&mut archive, &mut report, options);
                    comics::cbz(&mut archive, &mut report, options);
                    iwork::inspect(&mut archive, &mut report, options, ocr);
                    archive::summarize(&mut report, options);
                    // Only allowlisted embedded preview entries may be read.
                    // LITX-AUD-012: fall back only when no thumbnail exists.
                    if options.thumbnail.is_some() && report.thumbnail.is_none() {
                        for name in [
                            "docProps/thumbnail.jpeg",
                            "docProps/thumbnail.png",
                            "Thumbnails/thumbnail.png",
                        ] {
                            match archive.read(name) {
                                Ok(Some(bytes)) => {
                                    thumbnail::write(
                                        std::io::Cursor::new(bytes),
                                        "embedded",
                                        options,
                                        &mut report,
                                    );
                                    break;
                                }
                                Ok(None) => {}
                                Err(error) => report.error("thumbnail", error),
                            }
                        }
                    }
                }
                Err(error) => report.error("archive", error),
            }
        } else if report
            .format
            .detected_mime
            .as_deref()
            .is_some_and(|m| m.contains("mobipocket") || m == "application/vnd.amazon.ebook")
            || (report.format.basis == "extension"
                && matches!(extension.as_str(), "mobi" | "azw" | "azw3" | "prc"))
        {
            mobi::inspect(path, &mut report, options);
        } else if report
            .format
            .detected_mime
            .as_deref()
            .is_some_and(|m| m.contains("djvu"))
            || (report.format.basis == "extension" && matches!(extension.as_str(), "djvu" | "djv"))
        {
            djvu::inspect(path, &mut report, options);
        } else if report
            .format
            .detected_mime
            .as_deref()
            .is_some_and(|m| m == "application/vnd.rar" || m == "application/x-rar-compressed")
            || (report.format.basis == "extension" && matches!(extension.as_str(), "cbr" | "rar"))
        {
            rar::inspect(path, &mut report);
        } else if report.format.detected_mime.as_deref() == Some("application/pdf")
            || (report.format.basis == "extension" && extension == "pdf")
        {
            pdf::inspect(path, &mut report, options, ocr);
        } else if report.format.bucket == "Images" {
            images::inspect(path, &mut report, options, ocr);
        } else if report.format.detected_mime.as_deref() == Some("application/vnd.apache.parquet")
            || extension == "parquet"
        {
            parquet::inspect(path, &mut report, options);
        } else if report.format.basis == "extension" && matches!(extension.as_str(), "csv" | "tsv")
            || matches!(
                report.format.detected_mime.as_deref(),
                Some("text/csv" | "text/tab-separated-values")
            )
        {
            tabular::csv(path, &mut report, options);
        } else if report.format.detected_mime.as_deref() == Some("application/json")
            || (report.format.basis == "extension" && extension == "json")
        {
            json_profile::inspect(path, &mut report, options);
        } else if matches!(
            report.format.detected_mime.as_deref(),
            Some("application/xml" | "text/xml")
        ) || (report.format.basis == "extension" && extension == "xml")
        {
            xml_profile::inspect(path, &mut report, options);
        } else if report.format.basis == "extension"
            && matches!(report.format.bucket.as_str(), "Text" | "Source Code")
        {
            text::inspect(path, &mut report, options);
        }
    }
    if options.probe_ocr && report.format.bucket != "Images" && report.image.is_none() {
        report.error(
            "probe_ocr",
            "OCR probing is supported for raster images only",
        );
    }
    if report.sheets.iter().any(|s| s.preview_truncated) {
        report.text.truncated = true;
    }
    // LITX-AUD-015: dataset preview limits are partial output too.
    if let Some(dataset) = &report.dataset {
        report.text.truncated |= dataset.has_more || dataset.values_truncated;
    }
    if let Some(budget) = options.token_budget()
        && truncate_words(&mut report.text.content, budget)
    {
        report.text.chars = report.text.content.chars().count();
        report.text.truncated = true;
    }
    report.text.tokens = report.text.content.split_whitespace().count();
    markdown::finish(&mut report, options);
    if options.packet {
        report.packet = Some(serde_json::json!({
            "Format": report.format.bucket, "Language": office::language(&report),
            "counts": report.counts, "excerpt": report.text.content,
        }));
    }
    report
}

impl InspectOptions {
    /// Pages where pagination exists, otherwise slides, sections, or spine items.
    pub(crate) fn text_unit_limit(&self) -> usize {
        self.max_pages.unwrap_or(usize::MAX)
    }

    fn token_budget(&self) -> Option<usize> {
        // Only an explicit word-token budget truncates; page limits never
        // imply one (LITX-AUD-008).
        self.max_tokens
    }
}

fn truncate_words(value: &mut String, budget: usize) -> bool {
    let mut words = value.split_whitespace();
    let mut end = 0;
    for word in words.by_ref().take(budget) {
        end = word.as_ptr() as usize - value.as_ptr() as usize + word.len();
    }
    if words.next().is_some() {
        value.truncate(end);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    /// Explicit page limits must never imply a word-token budget; only an
    /// explicit `max_tokens` caps words (LITX-AUD-008).
    #[test]
    fn explicit_pages_without_max_tokens_means_no_token_budget() {
        let options = InspectOptions {
            identify: true,
            max_tokens: None,
            max_pages: Some(2),
            ..Default::default()
        };
        assert_eq!(options.token_budget(), None);
        let explicit = InspectOptions {
            max_tokens: Some(500),
            ..Default::default()
        };
        assert_eq!(explicit.token_budget(), Some(500));
    }

    #[test]
    #[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
    fn one_inspector_reuses_its_ocr_engine_between_files() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/image.png");
        let options = InspectOptions {
            probe_ocr: true,
            ..Default::default()
        };
        let mut inspector = Inspector::default();
        let first = inspector.inspect(&path, &options);
        assert!(first.errors.is_empty(), "{:?}", first.errors);
        let handle = inspector.ocr.api.as_ref().unwrap().handle.clone();
        let second = inspector.inspect(&path, &options);
        assert!(second.errors.is_empty(), "{:?}", second.errors);
        assert!(std::sync::Arc::ptr_eq(
            &handle,
            &inspector.ocr.api.as_ref().unwrap().handle
        ));
        assert_eq!(second.image.unwrap().probe.unwrap().word_count, 0);
    }
}
