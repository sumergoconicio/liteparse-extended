//! DjVu support boundary: bounded identification only. No document chunks,
//! native text, image layers, geometry, or containers are decoded here.
use crate::{InspectOptions, InspectReport};
use serde::Serialize;
use std::path::Path;

const DIAGNOSTIC: &str = "DjVu extraction is unsupported; identification only";

#[derive(Debug, Serialize)]
pub struct DjvuAnalysis {
    pub decoder: &'static str,
    pub text_complete: bool,
    pub ocr_page_limit: usize,
    pub pages: Vec<DjvuPageSignal>,
}

#[derive(Debug, Serialize)]
pub struct DjvuPageSignal {
    pub page_number: usize,
    pub native_chars: Option<usize>,
    pub text_origin: &'static str,
    pub ocr_status: &'static str,
    pub probe: Option<crate::OcrProbe>,
}

/// Record the static DjVu support boundary without reading document contents.
///
/// Accepts the already-identified source path, a mutable report, and inspection
/// options; the path and options are intentionally unused. Returns `()` after
/// clearing optional analysis and marking omitted text incomplete. No decoding
/// or OCR failure is possible because neither subsystem is invoked; the caller
/// receives one static `djvu` diagnostic in `report.errors`.
pub(crate) fn inspect(_path: &Path, report: &mut InspectReport, _options: &InspectOptions) {
    report.djvu = None;
    report.text.truncated = true;
    report.error("djvu", DIAGNOSTIC);
}
