//! iWork previews are derived representations, never native IWA extraction.
use crate::{InspectOptions, InspectReport, archive::Archive, images::OcrSession};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DerivedPreview {
    pub source_part: String,
    pub content_type: &'static str,
    pub partial: bool,
}

pub(crate) fn inspect(
    archive: &mut Archive,
    report: &mut InspectReport,
    options: &InspectOptions,
    ocr: &mut OcrSession,
) {
    if !matches!(
        report.format.extension.as_str(),
        "pages" | "numbers" | "key"
    ) {
        return;
    }
    for name in [
        "QuickLook/Preview.pdf",
        "QuickLook/Preview.txt",
        "QuickLook/Preview.png",
        "QuickLook/Preview.jpg",
        "preview.png",
        "preview.jpg",
        "QuickLook/Thumbnail.jpg",
    ] {
        match archive.read(name) {
            Ok(Some(bytes)) => {
                let pdf = name.ends_with(".pdf");
                let text = name.ends_with(".txt");
                report.derived_preview = Some(DerivedPreview {
                    source_part: name.into(),
                    content_type: if pdf {
                        "application/pdf"
                    } else if text {
                        "text/plain"
                    } else {
                        "image"
                    },
                    partial: true,
                });
                let mut preview_options = options.clone();
                preview_options.max_pages = Some(options.max_pages.unwrap_or(1).min(1));
                preview_options.max_units = options.max_units.min(1);
                preview_options.max_chars = options.max_chars.min(4000);
                // LITX-AUD-019: the notice is provenance-neutral; the actual
                // preview outcome (native, OCR, or mixed) sets the basis.
                crate::markdown::append(
                    report,
                    &format!(
                        "> Partial iWork preview from {name}. Original document structure was not decoded.\n\n> Bounded embedded preview: first page/image, at most 4000 characters.\n\n"
                    ),
                    &preview_options,
                    "",
                );
                if pdf {
                    crate::pdf::inspect_bytes(bytes, report, &preview_options, ocr);
                } else if text {
                    match std::str::from_utf8(&bytes) {
                        Ok(text) => {
                            report.append_text(text, preview_options.max_chars);
                            crate::markdown::append(
                                report,
                                &crate::markdown::escape(&report.text.content),
                                &preview_options,
                                "text_fallback",
                            );
                        }
                        Err(error) => report.error("iwork_preview", error),
                    }
                } else {
                    preview_options.thumbnail = None;
                    // OCR'd image text appends its own ocr_text_fallback
                    // Markdown (LITX-AUD-009); nothing more to add here.
                    crate::images::inspect_bytes(bytes.clone(), report, &preview_options, ocr);
                    crate::thumbnail::write(
                        std::io::Cursor::new(bytes),
                        "embedded",
                        options,
                        report,
                    );
                }
                if report.markdown.basis.is_empty() {
                    report.markdown.basis = "text_fallback";
                }
                report.text.origin = "embedded_preview".into();
                report.text.truncated = true;
                return;
            }
            Ok(None) => {}
            Err(error) => {
                report.error("iwork_preview", error);
                return;
            }
        }
    }
    report.error(
        "iwork_preview",
        "no supported embedded QuickLook preview; native IWA extraction is unavailable",
    );
}
