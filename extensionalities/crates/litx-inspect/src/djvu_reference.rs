// Module: litx-inspect historical DjVu support.
// File: djvu_reference.rs.
// Responsibility: retain the complete former implementation and integration tests as inactive comments.
// INACTIVE HISTORICAL REFERENCE: this file is not registered, included, configured, or compiled.
// Active DjVu support is identification only. Do not register this reference to bypass that boundary.
// Recovery: strip "// " from nonempty original lines; a bare "//" represents an original blank line.
// The original blocks then match the retained original snapshots byte-for-byte, including blank lines and final newlines.
// ----- ORIGIN extensionalities/crates/litx-inspect/src/djvu.rs -----
// ----- ORIGINAL SHA256 dac4a004c2126055a34bd7c292e505b95aa52ee1610b6a48b66465104cb5cb6d -----
// ----- BEGIN ORIGINAL extensionalities/crates/litx-inspect/src/djvu.rs -----
// //! DjVu inspection with a strict no-BZZ support boundary (W3-DJVU-NOBZZ,
// //! user decision 2026-11-18). No DjVu BZZ decoder is ever invoked:
// //!
// //! - TXTz compressed text is rejected with a static diagnostic; raw TXTa
// //!   text and bounded page-image OCR remain the supported text paths.
// //! - Bundled `FORM:DJVM` documents never reach the dependency's document
// //!   parser, which would BZZ-decode the DIRM metadata tail (component
// //!   kinds/ids/names) and any NAVM bookmark chunk. The raw directory is
// //!   walked instead: offsets and FORM subtypes are validated without
// //!   decompression, and each `FORM:DJVU` page component is reparsed as a
// //!   standalone single-page document whose parse and render paths never
// //!   touch BZZ.
// //! - FGbz foreground palettes carrying a BZZ-encoded blit index (version
// //!   byte bit 7) are rejected before any render, since decoding the blit
// //!   index would invoke BZZ.
// //! - W3-SYS-DJVU-002: the prospective OCR raster size is computed with
// //!   checked arithmetic and reserved in the cumulative decoded-output
// //!   budget before any raster allocation or OCR runs.
// //! - W3-SYS-DJVU-003: pages declaring zero width or height are rejected
// //!   before a Page record is published.
// //! - W4-SYS-DJVU-RASTER-BYPASS: explicit image-layer allocations are reserved
// //!   before decoding. Arithmetic-coded Sjbz masks are rejected for OCR because
// //!   their dimensions cannot be bounded without invoking that decoder.
// use crate::{
//     Claim, InspectOptions, InspectReport, Page,
//     binary::{Source, be16, be32, slice},
//     office, xml,
// };
// use serde::Serialize;
// use std::{
//     collections::{BTreeMap, BTreeSet},
//     path::Path,
// };
//
// #[derive(Debug, Serialize)]
// pub struct DjvuAnalysis {
//     pub decoder: &'static str,
//     pub text_complete: bool,
//     pub ocr_page_limit: usize,
//     pub pages: Vec<DjvuPageSignal>,
// }
//
// #[derive(Debug, Serialize)]
// pub struct DjvuPageSignal {
//     pub page_number: usize,
//     pub native_chars: Option<usize>,
//     pub text_origin: &'static str,
//     pub ocr_status: &'static str,
//     pub probe: Option<crate::OcrProbe>,
// }
//
// /// Inspect a DjVu document and use raw IFF recovery after structured failure.
// ///
// /// Accepts a source path, the mutable report, inspection limits, and the lazy
// /// OCR session. Returns `()` because failures are recorded in `report.errors`.
// /// Before raw recovery it restores every extraction-owned field, preserving
// /// pre-existing unrelated pages, claims, diagnostics, text, and counts.
// pub(crate) fn inspect(
//     path: &Path,
//     report: &mut InspectReport,
//     options: &InspectOptions,
//     ocr: &mut crate::images::OcrSession,
// ) {
//     let page_len = report.pages.len();
//     let claim_len = report.claims.len();
//     let error_len = report.errors.len();
//     let prior_page_count = report.counts.pages;
//     let prior_chars = report.counts.chars;
//     let prior_text = (
//         report.text.tokens,
//         report.text.token_basis,
//         report.text.content.clone(),
//         report.text.truncated,
//         report.text.chars,
//         report.text.origin.clone(),
//     );
//     let prior_markdown = (
//         report.markdown.content.clone(),
//         report.markdown.basis,
//         report.markdown.truncated,
//     );
//     // A fallback report describes the raw walker, never stale structured state.
//     report.djvu = None;
//     if let Err(error) = extract(path, report, options, ocr) {
//         // W4-SYS-DJVU-FALLBACK-DUP: restore before rebuilding from raw bytes.
//         report.pages.truncate(page_len);
//         report.claims.truncate(claim_len);
//         report.errors.truncate(error_len);
//         report.counts.pages = prior_page_count;
//         report.counts.chars = prior_chars;
//         report.text.tokens = prior_text.0;
//         report.text.token_basis = prior_text.1;
//         report.text.content = prior_text.2;
//         report.text.truncated = prior_text.3;
//         report.text.chars = prior_text.4;
//         report.text.origin = prior_text.5;
//         report.markdown.content = prior_markdown.0;
//         report.markdown.basis = prior_markdown.1;
//         report.markdown.truncated = prior_markdown.2;
//         report.djvu = None;
//         // Keep independently readable metadata when the document decoder rejects a file.
//         let _ = inner(path, report, options);
//         let recovered_pages = report.pages.len().saturating_sub(page_len);
//         if report.counts.pages.is_some() || recovered_pages > 0 {
//             report.counts.pages = Some(
//                 prior_page_count
//                     .unwrap_or(0)
//                     .saturating_add(recovered_pages),
//             );
//         }
//         report.text.truncated = true;
//         report.markdown.truncated = true;
//         report.error("djvu", error);
//     }
// }
//
// fn extract(
//     path: &Path,
//     report: &mut InspectReport,
//     options: &InspectOptions,
//     ocr: &mut crate::images::OcrSession,
// ) -> xml::Result<()> {
//     extract_with_budget(path, report, options, ocr, MAX_DECODED_TEXT_BYTES)
// }
//
// /// Entry point with an explicit cumulative decoded-output budget, so tests
// /// can prove that raster budget reservation happens before any allocation.
// fn extract_with_budget(
//     path: &Path,
//     report: &mut InspectReport,
//     options: &InspectOptions,
//     ocr: &mut crate::images::OcrSession,
//     decoded_budget: usize,
// ) -> xml::Result<()> {
//     let mut source = Source::open(path)?;
//     let bytes = source.range(0, source.len as usize)?;
//     // W3-DJVU-NOBZZ: bundled DJVM containers never reach the dependency's
//     // document parser, which would BZZ-decode the DIRM metadata tail and
//     // any NAVM chunk. Walk the raw directory instead.
//     if bytes.len() >= 16 && &bytes[..8] == b"AT&TFORM" && &bytes[12..16] == b"DJVM" {
//         return extract_bundled(&bytes, report, options, ocr, decoded_budget);
//     }
//     extract_standalone(&bytes, report, options, ocr, decoded_budget)
// }
//
// /// The dependency parse limits shared by standalone documents and the
// /// per-page reparse of bundled components.
// fn parse_options() -> djvu_rs::ParseOptions {
//     djvu_rs::ParseOptions {
//         limits: Some(djvu_rs::ResourceLimits {
//             max_file_bytes: Some(16 << 20),
//             max_pages: Some(4096),
//             max_components: Some(4096),
//             max_page_pixels: Some(64 << 20),
//             max_render_pixels: Some(2048 * 2048),
//             ..Default::default()
//         }),
//     }
// }
//
// /// Extract one standalone DjVu page without invoking BZZ.
// ///
// /// Accepts complete file bytes, a mutable report, inspection limits, an OCR
// /// session, and the document-wide decoded-byte budget. Returns `Ok(())` after
// /// publishing all valid geometry, even beyond the text sampling limit. Parse,
// /// page lookup, text, and allocation failures return an error for raw recovery.
// fn extract_standalone(
//     bytes: &[u8],
//     report: &mut InspectReport,
//     options: &InspectOptions,
//     ocr: &mut crate::images::OcrSession,
//     decoded_budget: usize,
// ) -> xml::Result<()> {
//     let document = djvu_rs::DjVuDocument::parse_with_options(bytes, &parse_options())?;
//     office::identify(report, "djvu", "image/vnd.djvu", "DjVu document");
//     let prior_page_count = report.counts.pages;
//     let page_base = report.pages.len();
//     let limit = options.max_pages.unwrap_or(4096).min(4096);
//     let mut analysis = DjvuAnalysis {
//         decoder: "djvu-rs",
//         text_complete: document.page_count() <= limit,
//         ocr_page_limit: limit,
//         pages: Vec::new(),
//     };
//     // LITX-AUD-013(a): cumulative decoded-output budget shared by embedded
//     // text and OCR rasters across the whole document.
//     let mut budget = decoded_budget;
//     for index in 0..document.page_count() {
//         let page = document.page(index)?;
//         if let PageStep::Stop = process_page(
//             page,
//             index,
//             report,
//             options,
//             ocr,
//             &mut analysis,
//             &mut budget,
//             limit,
//         ) {
//             break;
//         }
//     }
//     // W4-SYS-DJVU-COUNTS: count valid published geometry, not raw INFO records.
//     report.counts.pages = Some(
//         prior_page_count
//             .unwrap_or(0)
//             .saturating_add(report.pages.len() - page_base),
//     );
//     report.text.truncated |= !analysis.text_complete;
//     report.djvu = Some(analysis);
//     Ok(())
// }
//
// /// Process a bundled `FORM:DJVM` document without invoking any BZZ decoder.
// ///
// /// The raw DIRM header (flags, component count, offset table) is validated
// /// without touching the BZZ-compressed metadata tail, which carries only
// /// component kinds, ids, and names. The offset table must agree exactly with
// /// the FORM components on disk; FORM subtypes classify components, so a
// /// lying or undecodable tail can never drop or misroute a page. Each
// /// `FORM:DJVU` component is reparsed as a standalone single-page document,
// /// whose parse and render paths never invoke BZZ. Shared `FORM:DJVI`
// /// dictionaries and `FORM:THUM` thumbnails are skipped, and indirect
// /// documents are rejected: external components are never followed.
// /// Bookmarks live in the BZZ-compressed NAVM chunk, which is never decoded;
// /// bookmarks are outside the inspection schema.
// ///
// /// Accepts complete bundle bytes, a mutable report, inspection limits, an OCR
// /// session, and the cumulative decoded-byte budget. Returns `Ok(())` after all
// /// valid page geometry is published. Malformed directories, invalid component
// /// pages, text failures outside page-local recovery, and allocation failures
// /// return an error that triggers the atomic raw fallback.
// fn extract_bundled(
//     bytes: &[u8],
//     report: &mut InspectReport,
//     options: &InspectOptions,
//     ocr: &mut crate::images::OcrSession,
//     decoded_budget: usize,
// ) -> xml::Result<()> {
//     let size = u64::from(be32(bytes, 8)?);
//     if size < 4 {
//         return Err("truncated DjVu FORM".into());
//     }
//     let end64 = 12u64.checked_add(size).ok_or("DjVu chunk overflow")?;
//     if end64 > bytes.len() as u64 {
//         return Err("truncated DjVu FORM".into());
//     }
//     let end = usize::try_from(end64).map_err(|_| "DjVu FORM size overflows")?;
//     let (id, next, _len) = slice_chunk(bytes, 16, end)?;
//     if &id != b"DIRM" {
//         return Err("DjVu document has no leading DIRM".into());
//     }
//     let dir = slice(bytes, 24, _len)?;
//     let flags = *dir.first().ok_or("truncated DjVu directory")?;
//     if flags & 0x7f != 1 {
//         return Err("unsupported DjVu directory version".into());
//     }
//     let count = be16(dir, 1)? as usize;
//     if count > 4096 {
//         return Err("DjVu directory component cap exceeded".into());
//     }
//     if flags & 0x80 == 0 {
//         // Indirect documents carry no offset table; their per-component
//         // kinds, ids, and names live in the BZZ-compressed tail. BZZ
//         // decoding is disabled and external files are never followed.
//         return Err("indirect DjVu documents are unsupported: BZZ directory decoding is disabled and external components are not followed".into());
//     }
//     slice(dir, 3, count * 4)?;
//     let mut offsets = Vec::with_capacity(count);
//     let mut unique = BTreeSet::new();
//     for index in 0..count {
//         let offset = u64::from(be32(dir, 3 + index * 4)?);
//         unique.insert(offset);
//         offsets.push(offset);
//     }
//     if unique.len() != count
//         || unique
//             .iter()
//             .any(|&offset| offset < next as u64 || offset >= end64)
//     {
//         return Err("invalid DjVu directory offsets".into());
//     }
//     // Walk the FORM components once, recording offset -> (start, end, subtype).
//     let mut components: BTreeMap<u64, (usize, usize, [u8; 4])> = BTreeMap::new();
//     let mut cursor = next;
//     let mut blocks = 1;
//     while cursor < end {
//         blocks += 1;
//         if blocks > 4096 {
//             return Err("DjVu chunk-count cap exceeded".into());
//         }
//         let (id, following, len) = slice_chunk(bytes, cursor, end)?;
//         if &id == b"FORM" {
//             if len < 4 {
//                 return Err("short DjVu component FORM".into());
//             }
//             let subtype = slice(bytes, cursor + 8, 4)?;
//             components.insert(
//                 cursor as u64,
//                 (
//                     cursor,
//                     following,
//                     [subtype[0], subtype[1], subtype[2], subtype[3]],
//                 ),
//             );
//         }
//         cursor = following;
//     }
//     if components.keys().copied().collect::<BTreeSet<_>>() != unique {
//         return Err("DjVu directory disagrees with component FORM offsets".into());
//     }
//     office::identify(report, "djvu", "image/vnd.djvu", "DjVu document");
//     report.claims.push(Claim {
//         key: "directory_components".into(),
//         value: count.to_string(),
//         source: "bytes",
//     });
//     let page_count = offsets
//         .iter()
//         .filter_map(|offset| components.get(offset))
//         .filter(|(_, _, subtype)| subtype == b"DJVU")
//         .count();
//     let prior_page_count = report.counts.pages;
//     let page_base = report.pages.len();
//     let limit = options.max_pages.unwrap_or(4096).min(4096);
//     let mut analysis = DjvuAnalysis {
//         decoder: "djvu-rs",
//         text_complete: page_count <= limit,
//         ocr_page_limit: limit,
//         pages: Vec::new(),
//     };
//     let mut budget = decoded_budget;
//     let mut index = 0usize;
//     for offset in &offsets {
//         let Some(&(start, stop, subtype)) = components.get(offset) else {
//             continue;
//         };
//         if &subtype != b"DJVU" {
//             // Shared dictionaries and thumbnails are never decoded.
//             continue;
//         }
//         // Reparse the page component as a standalone single-page document:
//         // single-page parses and renders never invoke BZZ.
//         let mut standalone = Vec::with_capacity(4 + (stop - start));
//         standalone.extend_from_slice(b"AT&T");
//         standalone.extend_from_slice(&bytes[start..stop]);
//         let document = djvu_rs::DjVuDocument::parse_with_options(&standalone, &parse_options())?;
//         let page = document.page(0)?;
//         if let PageStep::Stop = process_page(
//             page,
//             index,
//             report,
//             options,
//             ocr,
//             &mut analysis,
//             &mut budget,
//             limit,
//         ) {
//             break;
//         }
//         index += 1;
//     }
//     // W4-SYS-DJVU-COUNTS: max_pages limits sampling, not published geometry.
//     report.counts.pages = Some(
//         prior_page_count
//             .unwrap_or(0)
//             .saturating_add(report.pages.len() - page_base),
//     );
//     report.text.truncated |= !analysis.text_complete;
//     report.djvu = Some(analysis);
//     Ok(())
// }
//
// /// Whether the page loop should keep going after a page.
// enum PageStep {
//     Continue,
//     Stop,
// }
//
// /// Process one zero-based page: publish geometry (W3-SYS-DJVU-003 rejects
// /// zero dimensions before a Page record is published), recover the raw TXTa
// /// text layer, and fall back to budget-reserved, BZZ-free page-image OCR.
// #[allow(clippy::too_many_arguments)]
// fn process_page(
//     page: &djvu_rs::DjVuPage,
//     index: usize,
//     report: &mut InspectReport,
//     options: &InspectOptions,
//     ocr: &mut crate::images::OcrSession,
//     analysis: &mut DjvuAnalysis,
//     decoded_budget: &mut usize,
//     limit: usize,
// ) -> PageStep {
//     // W3-SYS-DJVU-003: a page with zero dimensions is never published as a
//     // Page record and never reaches text decode, raster allocation, or OCR.
//     if page.width() == 0 || page.height() == 0 {
//         report.error(
//             "djvu",
//             format!("page {}: DjVu page dimensions are zero", index + 1),
//         );
//         analysis.text_complete = false;
//         analysis.pages.push(DjvuPageSignal {
//             page_number: index + 1,
//             native_chars: Some(0),
//             text_origin: "unavailable",
//             ocr_status: "disabled",
//             probe: None,
//         });
//         return PageStep::Continue;
//     }
//     report.pages.push(Page {
//         index: index + 1,
//         source_part: format!("page:{}", index + 1),
//         width: Some(u32::from(page.width())),
//         height: Some(u32::from(page.height())),
//         dpi: Some(page.dpi()),
//         version: page
//             .raw_chunk(b"INFO")
//             .and_then(|info| Some(format!("{}.{}", info.get(5)?, info.get(4)?))),
//     });
//     if index >= limit {
//         analysis.pages.push(DjvuPageSignal {
//             page_number: index + 1,
//             native_chars: None,
//             text_origin: "not_sampled",
//             ocr_status: "page_limit",
//             probe: None,
//         });
//         return PageStep::Continue;
//     }
//     let mut signal = DjvuPageSignal {
//         page_number: index + 1,
//         native_chars: Some(0),
//         text_origin: "unavailable",
//         ocr_status: "disabled",
//         probe: None,
//     };
//     // LITX-AUD-013(d): a failed native text decode stays incomplete even
//     // when OCR runs afterwards and finds nothing on the page.
//     let mut native_error = false;
//     match embedded_text(page, decoded_budget) {
//         Ok(Some(text)) if !text.trim().is_empty() => {
//             signal.native_chars = Some(text.chars().count());
//             signal.text_origin = "embedded_text";
//             signal.ocr_status = "not_needed";
//             if report.text.origin == "ocr" {
//                 report.text.origin = "native_and_ocr".into();
//             }
//             append_page(report, options, index + 1, &text, "text_fallback");
//         }
//         Ok(_) => {}
//         Err(error) => {
//             native_error = true;
//             report.error("djvu_text", format!("page {}: {error}", index + 1));
//         }
//     }
//     if signal.text_origin == "unavailable" && options.identify {
//         // W3-DJVU-NOBZZ: an FGbz palette carrying a BZZ-encoded blit index
//         // is rejected before any budget reservation, render allocation, or
//         // BZZ decode, with the static unsupported diagnostic.
//         if fgbz_requires_bzz(page) {
//             report.error(
//                 "djvu_ocr",
//                 format!("page {}: {FGBZ_UNSUPPORTED}", index + 1),
//             );
//             signal.ocr_status = "unsupported";
//             analysis.text_complete = false;
//             analysis.pages.push(signal);
//             return PageStep::Continue;
//         }
//         // W3-SYS-DJVU-002: reserve the prospective raster size in the
//         // cumulative budget BEFORE any raster allocation or OCR, so a plan
//         // that does not fit stops the document without a partial render
//         // and never runs OCR against an unaccounted allocation.
//         match reserve_render(page, decoded_budget) {
//             Ok(()) => {}
//             Err(ReserveError::Budget(message)) => {
//                 report.error("djvu_text", format!("page {}: {message}", index + 1));
//                 signal.ocr_status = "budget";
//                 analysis.text_complete = false;
//                 analysis.pages.push(signal);
//                 return PageStep::Stop;
//             }
//             Err(ReserveError::Plan(message)) => {
//                 report.error("djvu", format!("page {}: {message}", index + 1));
//                 signal.ocr_status = "failed";
//                 analysis.text_complete = false;
//                 analysis.pages.push(signal);
//                 return PageStep::Continue;
//             }
//         }
//         match recognize(page, ocr) {
//             Ok((probe, text)) => {
//                 signal.ocr_status = if probe.word_count > 0 {
//                     signal.text_origin = "ocr";
//                     report.text.origin = if report.counts.chars == 0 {
//                         "ocr"
//                     } else if report.text.origin == "native" {
//                         "native_and_ocr"
//                     } else {
//                         &report.text.origin
//                     }
//                     .into();
//                     append_page(report, options, index + 1, &text, "ocr_text_fallback");
//                     "text_detected"
//                 } else {
//                     "no_text_detected"
//                 };
//                 signal.probe = Some(probe);
//             }
//             Err(error) => {
//                 signal.ocr_status = "failed";
//                 report.error("djvu_ocr", format!("page {}: {error}", index + 1));
//             }
//         }
//     }
//     // LITX-AUD-013: unavailable native text (unsupported TXTz, failed
//     // parse, budget) keeps the document partial even when OCR recovers
//     // readable glyphs from the page image.
//     if native_error
//         || (signal.text_origin == "unavailable" && signal.ocr_status != "no_text_detected")
//     {
//         analysis.text_complete = false;
//     }
//     analysis.pages.push(signal);
//     PageStep::Continue
// }
//
// fn append_page(
//     report: &mut InspectReport,
//     options: &InspectOptions,
//     number: usize,
//     text: &str,
//     basis: &'static str,
// ) {
//     report.append_text(text, options.max_chars);
//     report.append_text("\n\n", options.max_chars);
//     crate::markdown::append(
//         report,
//         &format!("## Page {number}\n\n{}\n\n", crate::markdown::escape(text)),
//         options,
//         basis,
//     );
// }
//
// /// LITX-AUD-013 support boundary (user decision 2026-11-18): compressed
// /// TXTz text is rejected without invoking BZZ decode. Raw TXTa text and
// /// bounded page-image OCR remain the supported text paths.
// const TXTZ_UNSUPPORTED: &str = "DjVu compressed text (TXTz) is unsupported: BZZ text decoding is disabled; raw TXTa text and OCR remain available";
//
// /// W3-DJVU-NOBZZ support boundary: an FGbz foreground palette whose version
// /// byte has bit 7 set carries a BZZ-encoded blit index, which the render
// /// path would decode with BZZ. Such pages are rejected before any render.
// const FGBZ_UNSUPPORTED: &str = "DjVu page render is unsupported: FGbz palette carries a BZZ-encoded blit index (BZZ decoding is disabled)";
//
// /// Return the raw TXTa text-layer payload for a page, preferring it over
// /// its compressed TXTz twin. Returns `Ok(None)` when neither chunk carries
// /// text, and the static unsupported diagnostic when only TXTz is present.
// fn raw_text_payload(page: &djvu_rs::DjVuPage) -> xml::Result<Option<&[u8]>> {
//     if let Some(payload) = page.raw_chunk(b"TXTa").filter(|chunk| !chunk.is_empty()) {
//         return Ok(Some(payload));
//     }
//     if page
//         .raw_chunk(b"TXTz")
//         .is_some_and(|chunk| !chunk.is_empty())
//     {
//         return Err(TXTZ_UNSUPPORTED.into());
//     }
//     Ok(None)
// }
//
// fn embedded_text(page: &djvu_rs::DjVuPage, budget: &mut usize) -> xml::Result<Option<String>> {
//     let Some(payload) = raw_text_payload(page)? else {
//         return Ok(None);
//     };
//     // LITX-AUD-013: bill the entire raw chunk, including trailing bytes
//     // beyond the parsed text slice, before any parsing work.
//     charge_decoded_budget(budget, payload.len())?;
//     let header = slice(payload, 0, 3)?;
//     let length = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
//     let text = slice(payload, 3, length)?;
//     // LLM text does not require the optional layout-zone tree. Parse only
//     // the validated text range, preserving text even when zones are malformed.
//     let layer =
//         djvu_rs::text::parse_text_layer(&payload[..3 + text.len()], u32::from(page.height()))?;
//     Ok(Some(
//         layer
//             .text
//             .chars()
//             .map(|c| {
//                 if c.is_control() && !matches!(c, '\n' | '\r' | '\t') {
//                     ' '
//                 } else {
//                     c
//                 }
//             })
//             .collect(),
//     ))
// }
//
// /// LITX-AUD-013(a): cumulative cap on decoded text and raster bytes across
// /// the whole document, guarding against decompression blowup.
// pub(crate) const MAX_DECODED_TEXT_BYTES: usize = 64 * 1024 * 1024;
//
// fn charge_decoded_budget(remaining: &mut usize, bytes: usize) -> Result<(), String> {
//     if bytes > *remaining {
//         return Err(format!(
//             "DjVu decoded output budget of {MAX_DECODED_TEXT_BYTES} bytes exhausted"
//         ));
//     }
//     *remaining -= bytes;
//     Ok(())
// }
//
// /// Why a page render could not be reserved up front (W3-SYS-DJVU-002).
// enum ReserveError {
//     /// The prospective raster geometry is not representable on this host.
//     Plan(String),
//     /// The cumulative decoded-output budget is exhausted.
//     Budget(String),
// }
//
// /// Reserve the complete prospective OCR render peak before decoder entry.
// ///
// /// Accepts a parsed page and the mutable document-wide remaining byte budget.
// /// Returns `Ok(())` after atomically deducting the peak. Malformed layer
// /// headers return `ReserveError::Plan`; insufficient cumulative budget returns
// /// `ReserveError::Budget` without changing the remaining bytes.
// fn reserve_render(page: &djvu_rs::DjVuPage, budget: &mut usize) -> Result<(), ReserveError> {
//     let needed =
//         render_allocation_plan(page).map_err(|error| ReserveError::Plan(error.to_string()))?;
//     charge_decoded_budget(budget, needed).map_err(ReserveError::Budget)
// }
//
// /// Compute the 150 DPI target raster geometry for a page and its RGBA byte
// /// size, rejecting plans that are not representable. Returns the
// /// RenderRequest width and height the OCR path allocates and the exact
// /// byte count its pixmap will occupy.
// fn render_plan(page: &djvu_rs::DjVuPage) -> xml::Result<(u32, u32, usize)> {
//     let width = u32::from(page.width());
//     let height = u32::from(page.height());
//     let scale = (150.0 / f64::from(page.dpi().max(1)))
//         .min(2048.0 / f64::from(width.max(height)))
//         .min(1.0);
//     let target_w = (f64::from(width) * scale).max(1.0) as u32;
//     let target_h = (f64::from(height) * scale).max(1.0) as u32;
//     let needed = (target_w as u64)
//         .checked_mul(target_h as u64)
//         .and_then(|pixels| pixels.checked_mul(4))
//         .and_then(|bytes| usize::try_from(bytes).ok())
//         .ok_or("DjVu render plan overflows addressable memory")?;
//     Ok((target_w, target_h, needed))
// }
//
// /// The retained and transient allocations associated with one render layer.
// #[derive(Default)]
// struct LayerAllocation {
//     /// Bytes still alive when later layers and the output pixmap are built.
//     resident: usize,
//     /// Peak bytes alive while this layer itself is decoded.
//     decode: usize,
// }
//
// /// Add two allocation sizes without allowing host-address overflow.
// ///
// /// Accepts an accumulated byte count and an additional byte count. Returns
// /// their checked sum, or a static render-plan error when `usize` overflows.
// fn add_allocation(total: usize, extra: usize) -> xml::Result<usize> {
//     total
//         .checked_add(extra)
//         .ok_or_else(|| "DjVu render allocation plan overflows addressable memory".into())
// }
//
// /// Multiply pixel dimensions by an allocation width with checked arithmetic.
// ///
// /// Accepts width, height, and bytes per pixel. Returns the addressable byte
// /// count, or an error for zero dimensions or arithmetic overflow. Zero is
// /// accepted only for MMR masks, whose decoder defines an empty-bitmap result.
// fn pixel_allocation(width: u32, height: u32, bytes: usize) -> xml::Result<usize> {
//     (width as u64)
//         .checked_mul(height as u64)
//         .and_then(|pixels| pixels.checked_mul(bytes as u64))
//         .and_then(|bytes| usize::try_from(bytes).ok())
//         .ok_or_else(|| "DjVu layer dimensions overflow addressable memory".into())
// }
//
// /// Compute the maximum simultaneous allocation made by the OCR render.
// ///
// /// Accepts a parsed page. Returns the byte peak across background decode, mask
// /// decode, foreground decode, retained layers, output composition, and INFO
// /// rotation. Malformed explicit JPEG, IW44, or MMR dimensions return an error
// /// before `djvu-rs` can allocate for them.
// fn render_allocation_plan(page: &djvu_rs::DjVuPage) -> xml::Result<usize> {
//     let (_, _, output) = render_plan(page)?;
//     let background = if let Some(data) = page.bg44_chunks().first().copied() {
//         iw44_allocation(data, true)?
//     } else if let Some(data) = page.raw_chunk(b"BGjp") {
//         jpeg_allocation(data)?
//     } else {
//         LayerAllocation::default()
//     };
//     let indexed = fgbz_has_colors(page);
//     let mask = if page.raw_chunk(b"Sjbz").is_some() {
//         // JB2 dimensions are arithmetic-coded. The dependency checks its pixel
//         // cap only after invoking that decoder, so bounded OCR must not enter it.
//         return Err("DjVu Sjbz masks are unsupported by bounded OCR".into());
//     } else if let Some(data) = page.raw_chunk(b"Smmr") {
//         smmr_allocation(data, indexed)?
//     } else {
//         LayerAllocation::default()
//     };
//     let foreground = if let Some(data) = page.fg44_chunks().first().copied() {
//         iw44_allocation(data, false)?
//     } else if let Some(data) = page.raw_chunk(b"FGjp") {
//         jpeg_allocation(data)?
//     } else {
//         LayerAllocation::default()
//     };
//
//     let mask_peak = add_allocation(background.resident, mask.decode)?;
//     let foreground_peak = add_allocation(
//         add_allocation(background.resident, mask.resident)?,
//         foreground.decode,
//     )?;
//     let retained = add_allocation(
//         add_allocation(background.resident, mask.resident)?,
//         foreground.resident,
//     )?;
//     let output_factor = if matches!(page.rotation(), djvu_rs::Rotation::None) {
//         1
//     } else {
//         2
//     };
//     let output_peak = add_allocation(
//         retained,
//         output
//             .checked_mul(output_factor)
//             .ok_or("DjVu output allocation overflows")?,
//     )?;
//     Ok(background
//         .decode
//         .max(mask_peak)
//         .max(foreground_peak)
//         .max(output_peak))
// }
//
// /// Calculate the decoder and RGB-to-RGBA peak for one JPEG layer.
// ///
// /// Accepts the raw BGjp or FGjp payload. Returns four bytes per pixel retained
// /// and the sum of the RGB/RGBA conversion peak, conservative full-frame i16
// /// coefficient buffers, and decoder scanline scratch during decoding. Invalid
// /// SOF metadata, truncated segments, zero dimensions, and arithmetic overflow
// /// return an error before `zune-jpeg` is constructed.
// fn jpeg_allocation(data: &[u8]) -> xml::Result<LayerAllocation> {
//     let frame = jpeg_frame(data)?;
//     let resident = pixel_allocation(frame.width, frame.height, 4)?;
//     let conversion = pixel_allocation(frame.width, frame.height, 7)?;
//     let coefficients = jpeg_coefficient_allocation(&frame)?;
//     let scanlines = jpeg_scanline_allocation(&frame)?;
//     let decode = add_allocation(add_allocation(conversion, coefficients)?, scanlines)?;
//     Ok(LayerAllocation { resident, decode })
// }
//
// /// JPEG frame metadata needed to reproduce decoder allocation geometry.
// struct JpegFrame {
//     /// Frame width in pixels.
//     width: u32,
//     /// Frame height in pixels.
//     height: u32,
//     /// Horizontal and vertical sampling factors for each present component.
//     sampling: [(u8, u8); 4],
//     /// Number of initialized entries in `sampling`.
//     component_count: usize,
// }
//
// /// Read a complete JPEG SOF using bounded marker-segment arithmetic.
// ///
// /// Accepts the complete JPEG payload. Returns dimensions and all component
// /// sampling records from the first decoder-supported start-of-frame marker.
// /// Truncated markers, inconsistent lengths, unsupported precision, zero or
// /// excess components, invalid sampling/quantization, and zero dimensions
// /// return an error before any dependency decoder is entered.
// fn jpeg_frame(data: &[u8]) -> xml::Result<JpegFrame> {
//     if data.len() < 2 || data[..2] != [0xff, 0xd8] {
//         return Err("DjVu JPEG layer has no SOI marker".into());
//     }
//     let mut cursor = 2usize;
//     while cursor < data.len() {
//         if data[cursor] != 0xff {
//             return Err("DjVu JPEG layer has a malformed marker".into());
//         }
//         while cursor < data.len() && data[cursor] == 0xff {
//             cursor += 1;
//         }
//         let marker = *data.get(cursor).ok_or("truncated DjVu JPEG marker")?;
//         cursor += 1;
//         if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
//             continue;
//         }
//         if marker == 0xd9 || marker == 0xda {
//             return Err("DjVu JPEG layer has no frame dimensions".into());
//         }
//         let length_bytes = slice(data, cursor, 2)?;
//         let segment_length = usize::from(u16::from_be_bytes([length_bytes[0], length_bytes[1]]));
//         if segment_length < 2 {
//             return Err("DjVu JPEG layer has an invalid segment length".into());
//         }
//         let segment_end = cursor
//             .checked_add(segment_length)
//             .ok_or("DjVu JPEG segment length overflows")?;
//         if segment_end > data.len() {
//             return Err("truncated DjVu JPEG segment".into());
//         }
//         let is_sof = matches!(marker, 0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc9 | 0xca | 0xcb);
//         if is_sof {
//             if segment_length < 8 {
//                 return Err("truncated DjVu JPEG frame header".into());
//             }
//             if data[cursor + 2] != 8 {
//                 return Err("DjVu JPEG layer precision is not eight bits".into());
//             }
//             let height = u32::from(u16::from_be_bytes([data[cursor + 3], data[cursor + 4]]));
//             let width = u32::from(u16::from_be_bytes([data[cursor + 5], data[cursor + 6]]));
//             if width == 0 || height == 0 {
//                 return Err("DjVu JPEG layer dimensions are zero".into());
//             }
//             let component_count = usize::from(data[cursor + 7]);
//             if !(1..=4).contains(&component_count) {
//                 return Err("DjVu JPEG layer component count is outside 1..=4".into());
//             }
//             let expected_length = 8 + 3 * component_count;
//             if segment_length != expected_length {
//                 return Err("DjVu JPEG frame length disagrees with its components".into());
//             }
//             let mut sampling = [(0, 0); 4];
//             for (index, component) in sampling.iter_mut().take(component_count).enumerate() {
//                 let offset = cursor + 8 + 3 * index;
//                 let horizontal = data[offset + 1] >> 4;
//                 let vertical = data[offset + 1] & 0x0f;
//                 if horizontal == 0 || horizontal > 4 || !horizontal.is_power_of_two() {
//                     return Err("DjVu JPEG horizontal sampling is invalid".into());
//                 }
//                 if vertical == 0 || vertical > 4 {
//                     return Err("DjVu JPEG vertical sampling is invalid".into());
//                 }
//                 if data[offset + 2] >= 4 {
//                     return Err("DjVu JPEG quantization table is outside 0..=3".into());
//                 }
//                 *component = (horizontal, vertical);
//             }
//             return Ok(JpegFrame {
//                 width,
//                 height,
//                 sampling,
//                 component_count,
//             });
//         }
//         cursor = segment_end;
//     }
//     Err("DjVu JPEG layer has no frame dimensions".into())
// }
//
// /// Calculate a conservative ceiling for full-frame i16 coefficient buffers.
// ///
// /// Accepts validated JPEG frame metadata. Returns the larger of the interleaved
// /// component-count bound and the non-interleaved sampling-weighted allocation,
// /// covering progressive and split-scan baseline decoding without entropy-scan
// /// parsing. Host-address overflow returns a render-plan error.
// fn jpeg_coefficient_allocation(frame: &JpegFrame) -> xml::Result<usize> {
//     let width = frame.width as usize;
//     let height = frame.height as usize;
//     let components = &frame.sampling[..frame.component_count];
//     let max_h = components
//         .iter()
//         .map(|(horizontal, _)| usize::from(*horizontal))
//         .max()
//         .ok_or("DjVu JPEG layer has no components")?;
//     let max_v = components
//         .iter()
//         .map(|(_, vertical)| usize::from(*vertical))
//         .max()
//         .ok_or("DjVu JPEG layer has no components")?;
//     let mcu_width = max_h
//         .checked_mul(8)
//         .ok_or("DjVu JPEG MCU width overflows")?;
//     let mcu_height = max_v
//         .checked_mul(8)
//         .ok_or("DjVu JPEG MCU height overflows")?;
//     let interleaved_width = width
//         .div_ceil(mcu_width)
//         .checked_mul(mcu_width)
//         .ok_or("DjVu JPEG padded width overflows")?;
//     let interleaved_height = height
//         .div_ceil(mcu_height)
//         .checked_mul(mcu_height)
//         .ok_or("DjVu JPEG padded height overflows")?;
//     let interleaved = interleaved_width
//         .checked_mul(interleaved_height)
//         .and_then(|pixels| pixels.checked_mul(frame.component_count))
//         .and_then(|samples| samples.checked_mul(2))
//         .ok_or("DjVu JPEG interleaved coefficient allocation overflows")?;
//     let scan_width = width
//         .div_ceil(8)
//         .checked_mul(8)
//         .ok_or("DjVu JPEG scan width overflows")?;
//     let scan_height = height
//         .div_ceil(8)
//         .checked_mul(8)
//         .ok_or("DjVu JPEG scan height overflows")?;
//     let sampling_sum = components
//         .iter()
//         .try_fold(0usize, |total, (horizontal, vertical)| {
//             let samples = usize::from(*horizontal) * usize::from(*vertical);
//             total.checked_add(samples)
//         });
//     let non_interleaved = sampling_sum
//         .and_then(|samples| scan_width.checked_mul(scan_height)?.checked_mul(samples))
//         .and_then(|samples| samples.checked_mul(2))
//         .ok_or("DjVu JPEG non-interleaved coefficient allocation overflows")?;
//     Ok(interleaved.max(non_interleaved))
// }
//
// /// Calculate conservative JPEG scanline and upsampling scratch allocations.
// ///
// /// Accepts validated JPEG frame metadata. Returns byte ceilings for per-component
// /// raw MCU rows, two upsampling reference rows, initial/destination rows, and
// /// shared scratch space. Host-address overflow returns a render-plan error.
// fn jpeg_scanline_allocation(frame: &JpegFrame) -> xml::Result<usize> {
//     let width = frame.width as usize;
//     let components = &frame.sampling[..frame.component_count];
//     let max_h = components
//         .iter()
//         .map(|(horizontal, _)| usize::from(*horizontal))
//         .max()
//         .ok_or("DjVu JPEG layer has no components")?;
//     let max_v = components
//         .iter()
//         .map(|(_, vertical)| usize::from(*vertical))
//         .max()
//         .ok_or("DjVu JPEG layer has no components")?;
//     let mcu_width = max_h
//         .checked_mul(8)
//         .ok_or("DjVu JPEG MCU width overflows")?;
//     let mcu_columns = width.div_ceil(mcu_width);
//     let sample_ratio = max_h
//         .checked_mul(max_v)
//         .ok_or("DjVu JPEG sample ratio overflows")?;
//     let mut total = 0usize;
//     let mut max_stride = 0usize;
//     for (horizontal, vertical) in components {
//         let stride = mcu_columns
//             .checked_mul(8)
//             .and_then(|columns| columns.checked_mul(usize::from(*horizontal)))
//             .ok_or("DjVu JPEG component stride overflows")?;
//         max_stride = max_stride.max(stride);
//         let vertical = usize::from(*vertical);
//         let raw_rows = stride
//             .checked_mul(vertical)
//             .and_then(|samples| samples.checked_mul(8))
//             .and_then(|samples| samples.checked_mul(2))
//             .ok_or("DjVu JPEG raw scanline allocation overflows")?;
//         total = add_allocation(total, raw_rows)?;
//         if sample_ratio > 1 {
//             let reference_rows = stride
//                 .checked_mul(vertical)
//                 .and_then(|samples| samples.checked_mul(4))
//                 .ok_or("DjVu JPEG reference scanline allocation overflows")?;
//             let first_row = stride
//                 .checked_mul(vertical)
//                 .and_then(|samples| samples.checked_mul(sample_ratio))
//                 .and_then(|samples| samples.checked_mul(2))
//                 .ok_or("DjVu JPEG first scanline allocation overflows")?;
//             let destination = stride
//                 .checked_mul(sample_ratio)
//                 .and_then(|samples| samples.checked_mul(32))
//                 .ok_or("DjVu JPEG destination scanline allocation overflows")?;
//             total = add_allocation(total, reference_rows)?;
//             total = add_allocation(total, first_row)?;
//             total = add_allocation(total, destination)?;
//         }
//     }
//     if sample_ratio > 1 {
//         let scratch = max_stride
//             .checked_mul(16)
//             .ok_or("DjVu JPEG upsampling scratch allocation overflows")?;
//         total = add_allocation(total, scratch)?;
//     }
//     Ok(total)
// }
//
// /// Calculate a conservative IW44 coefficient, reconstruction, and RGBA peak.
// ///
// /// Accepts the first BG44 or FG44 chunk and whether decoded coefficients stay
// /// cached. Returns resident and decode peaks. Short or non-initial chunks,
// /// zero dimensions, and allocation overflow return an error before IW44 decode.
// fn iw44_allocation(data: &[u8], retain_coefficients: bool) -> xml::Result<LayerAllocation> {
//     if data.len() < 9 || data[0] != 0 {
//         return Err("DjVu IW44 layer has no complete initial header".into());
//     }
//     let width = u32::from(u16::from_be_bytes([data[4], data[5]]));
//     let height = u32::from(u16::from_be_bytes([data[6], data[7]]));
//     if width == 0 || height == 0 {
//         return Err("DjVu IW44 layer dimensions are zero".into());
//     }
//     let planes = if data[2] & 0x80 == 0 { 3 } else { 1 };
//     let blocks = (width as usize).div_ceil(32) * (height as usize).div_ceil(32);
//     // CoefBlock owns 32 inline coefficient bytes, a Vec, and up to 2016
//     // bytes of high-band coefficients. 2112 bytes per block covers both.
//     let coefficients = blocks
//         .checked_mul(2112)
//         .and_then(|bytes| bytes.checked_mul(planes))
//         .ok_or("DjVu IW44 coefficient allocation overflows")?;
//     let padded_width = (width as usize).div_ceil(32) * 32;
//     let padded_height = (height as usize).div_ceil(32) * 32;
//     let reconstructed = padded_width
//         .checked_mul(padded_height)
//         .and_then(|pixels| pixels.checked_mul(2 * planes))
//         .ok_or("DjVu IW44 reconstruction allocation overflows")?;
//     let rgba = pixel_allocation(width, height, 4)?;
//     let resident = if retain_coefficients {
//         add_allocation(coefficients, rgba)?
//     } else {
//         rgba
//     };
//     let decode = add_allocation(add_allocation(coefficients, reconstructed)?, rgba)?;
//     Ok(LayerAllocation { resident, decode })
// }
//
// /// Return whether a raw FGbz palette selects indexed-mask allocation.
// ///
// /// Accepts a parsed page. Returns true only for a nonempty direct-color table.
// /// Malformed palettes return false here and remain decoder errors; BZZ-indexed
// /// palettes are rejected earlier by the no-BZZ guard.
// fn fgbz_has_colors(page: &djvu_rs::DjVuPage) -> bool {
//     page.raw_chunk(b"FGbz")
//         .is_some_and(|chunk| chunk.len() >= 3 && u16::from_be_bytes([chunk[1], chunk[2]]) > 0)
// }
//
// /// Calculate MMR packed-bitmap and row-decoder allocations from its header.
// ///
// /// Accepts an Smmr payload and whether FGbz requires an i32 blit map. Returns
// /// the retained and decode peaks, including `Vec<bool>` row storage and Vec
// /// overhead. Short headers, unsupported flags, and overflow return an error.
// fn smmr_allocation(data: &[u8], indexed: bool) -> xml::Result<LayerAllocation> {
//     let (width, height) = if data.starts_with(b"MMR") {
//         if data.len() < 8 {
//             return Err("truncated DjVu Smmr header".into());
//         }
//         let flags = data[3];
//         if flags & 0xfc != 0 || flags & 0x02 != 0 {
//             return Err("unsupported DjVu Smmr header flags".into());
//         }
//         (
//             u32::from(u16::from_be_bytes([data[4], data[5]])),
//             u32::from(u16::from_be_bytes([data[6], data[7]])),
//         )
//     } else {
//         if data.len() < 4 {
//             return Err("truncated DjVu Smmr header".into());
//         }
//         (
//             u32::from(u16::from_be_bytes([data[0], data[1]])),
//             u32::from(u16::from_be_bytes([data[2], data[3]])),
//         )
//     };
//     if width == 0 || height == 0 {
//         return Ok(LayerAllocation::default());
//     }
//     let packed = (width as usize)
//         .div_ceil(8)
//         .checked_mul(height as usize)
//         .ok_or("DjVu Smmr bitmap allocation overflows")?;
//     let rows = (height as usize)
//         .checked_mul(std::mem::size_of::<Vec<bool>>())
//         .ok_or("DjVu Smmr row-table allocation overflows")?;
//     // The installed Rust Vec<bool> stores one byte per element. Reserve every
//     // completed row, the white reference row, and the current row or its clone.
//     let row_stride = (width as usize)
//         .checked_mul(std::mem::size_of::<bool>())
//         .ok_or("DjVu Smmr row-stride allocation overflows")?;
//     let row_storage = (height as usize)
//         .checked_add(2)
//         .and_then(|rows| rows.checked_mul(row_stride))
//         .ok_or("DjVu Smmr row-storage allocation overflows")?;
//     let decode = add_allocation(add_allocation(packed, rows)?, row_storage)?;
//     let blit_map = if indexed {
//         pixel_allocation(width, height, 4)?
//     } else {
//         0
//     };
//     let resident = add_allocation(packed, blit_map)?;
//     Ok(LayerAllocation { resident, decode })
// }
//
// /// Whether the page's FGbz chunk (if any) carries a BZZ-encoded blit index:
// /// DjVu sets bit 7 of the FGbz version byte when the palette index table is
// /// present, and decoding it would invoke BZZ.
// fn fgbz_requires_bzz(page: &djvu_rs::DjVuPage) -> bool {
//     page.raw_chunk(b"FGbz")
//         .is_some_and(|chunk| !chunk.is_empty() && chunk[0] & 0x80 != 0)
// }
//
// /// Render a pre-reserved page and probe its pixels with OCR.
// ///
// /// Accepts a parsed page and the lazy OCR session. Returns the OCR probe and
// /// recovered text. Render, malformed-pixel, and OCR initialization failures
// /// return an error; `reserve_render` must already have accepted the full layer
// /// peak because `RenderRequest::max_decoded_bytes` does not bound layer decode.
// fn recognize(
//     page: &djvu_rs::DjVuPage,
//     ocr: &mut crate::images::OcrSession,
// ) -> xml::Result<(crate::OcrProbe, String)> {
//     use djvu_rs::djvu_render::{RenderOptions, RenderRequest};
//     // W3-DJVU-NOBZZ: reject BZZ-indexed FGbz palettes before any render,
//     // since decoding the blit index would invoke the BZZ decoder.
//     if fgbz_requires_bzz(page) {
//         return Err(FGBZ_UNSUPPORTED.into());
//     }
//     let (width, height, _) = render_plan(page)?;
//     let pixmap = RenderRequest::new(RenderOptions {
//         width,
//         height,
//         ..Default::default()
//     })
//     .pixmap(page)?;
//     let image = image::RgbaImage::from_raw(pixmap.width, pixmap.height, pixmap.data)
//         .ok_or("invalid DjVu rendered pixel buffer")?;
//     crate::images::probe(&image::DynamicImage::ImageRgba8(image), ocr)
// }
//
// fn inner(path: &Path, report: &mut InspectReport, options: &InspectOptions) -> xml::Result<()> {
//     // LITX-AUD-013: any incomplete page text processing in the fallback
//     // (unsupported TXTz, budget exhaustion, parse failure) marks the text
//     // payload partial, even when a later structural error aborts the walk.
//     let mut partial = false;
//     let result = inner_walk(path, report, options, &mut partial);
//     // An aborted walk after at least one page also leaves the text partial.
//     if partial || (result.is_err() && !report.pages.is_empty()) {
//         report.text.truncated = true;
//     }
//     result
// }
//
// fn inner_walk(
//     path: &Path,
//     report: &mut InspectReport,
//     options: &InspectOptions,
//     partial: &mut bool,
// ) -> xml::Result<()> {
//     let mut source = Source::open(path)?;
//     let mut budget = MAX_DECODED_TEXT_BYTES;
//     let header = source.range(0, 16)?;
//     if &header[..8] != b"AT&TFORM" {
//         return Err("invalid DjVu IFF signature".into());
//     }
//     let size = u64::from(be32(&header, 8)?);
//     let end = 12 + size;
//     if size < 4 || end > source.len {
//         return Err("truncated DjVu FORM".into());
//     }
//     let kind = &header[12..16];
//     if !matches!(kind, b"DJVM" | b"DJVU") {
//         return Err("unsupported DjVu FORM type".into());
//     }
//     office::identify(report, "djvu", "image/vnd.djvu", "DjVu document");
//     report.format.disagreement = !report.format.extension.is_empty()
//         && !matches!(report.format.extension.as_str(), "djvu" | "djv");
//     let mut cursor = 16;
//     let mut expected = None;
//     let mut components = BTreeSet::new();
//     let mut blocks = 0;
//     if kind == b"DJVU" {
//         page(
//             &mut source,
//             cursor,
//             end,
//             report,
//             options,
//             &mut budget,
//             partial,
//         )?;
//         report.counts.pages = Some(1);
//         return Ok(());
//     }
//     while cursor < end {
//         blocks += 1;
//         if blocks > 4096 {
//             return Err("DjVu chunk-count cap exceeded".into());
//         }
//         let (id, next, len) = chunk(&mut source, cursor, end)?;
//         if blocks == 1 && &id != b"DIRM" {
//             return Err("DjVu document has no leading DIRM".into());
//         }
//         match &id {
//             b"DIRM" => {
//                 if expected.is_some() {
//                     return Err("duplicate DjVu directory".into());
//                 }
//                 let dir = source.range(cursor + 8, len as usize)?;
//                 let flags = *dir.first().ok_or("truncated DjVu directory")?;
//                 if flags & 0x7f != 1 {
//                     return Err("unsupported DjVu directory version".into());
//                 }
//                 let count = be16(&dir, 1)? as usize;
//                 if count > 4096 {
//                     return Err("DjVu directory component cap exceeded".into());
//                 }
//                 report.claims.push(Claim {
//                     key: "directory_components".into(),
//                     value: count.to_string(),
//                     source: "bytes",
//                 });
//                 if flags & 0x80 == 0 {
//                     // Indirect documents are out of scope: the component
//                     // kinds/ids live in the BZZ-compressed tail and external
//                     // files are never followed. The text stays partial.
//                     *partial = true;
//                     report.error("djvu", "indirect DjVu page count unavailable: BZZ component flags are not decoded; external files are not followed");
//                     return Ok(());
//                 }
//                 slice(&dir, 3, count * 4)?;
//                 let mut offsets = BTreeSet::new();
//                 for i in 0..count {
//                     offsets.insert(u64::from(be32(&dir, 3 + i * 4)?));
//                 }
//                 if offsets.len() != count
//                     || offsets.iter().any(|&offset| offset < next || offset >= end)
//                 {
//                     return Err("invalid DjVu directory offsets".into());
//                 }
//                 expected = Some(offsets);
//             }
//             b"FORM" => {
//                 if len < 4 {
//                     return Err("short DjVu component FORM".into());
//                 }
//                 let subtype = source.range(cursor + 8, 4)?;
//                 if matches!(subtype.as_slice(), b"DJVU" | b"DJVI" | b"THUM") {
//                     components.insert(cursor);
//                     if subtype == b"DJVU" {
//                         page(
//                             &mut source,
//                             cursor + 12,
//                             cursor + 8 + len,
//                             report,
//                             options,
//                             &mut budget,
//                             partial,
//                         )?;
//                     }
//                 }
//             }
//             _ => {}
//         }
//         cursor = next;
//     }
//     if expected.as_ref() != Some(&components) {
//         return Err("DjVu directory disagrees with component FORM offsets".into());
//     }
//     report.counts.pages = Some(report.pages.len());
//     Ok(())
// }
//
// fn chunk(source: &mut Source, offset: u64, end: u64) -> xml::Result<([u8; 4], u64, u64)> {
//     if end.saturating_sub(offset) < 8 {
//         return Err("truncated DjVu chunk header".into());
//     }
//     let bytes = source.range(offset, 8)?;
//     let length = u64::from(be32(&bytes, 4)?);
//     let next = offset
//         .checked_add(8 + length + (length & 1))
//         .ok_or("DjVu chunk overflow")?;
//     if next > end {
//         return Err("DjVu chunk exceeds parent FORM".into());
//     }
//     Ok((bytes[..4].try_into()?, next, length))
// }
//
// /// Chunk walker over an in-memory byte slice, mirroring `chunk`.
// /// Returns the chunk id, the offset just past its (padded) payload, and the
// /// declared payload length.
// fn slice_chunk(bytes: &[u8], offset: usize, end: usize) -> xml::Result<([u8; 4], usize, usize)> {
//     if end.saturating_sub(offset) < 8 {
//         return Err("truncated DjVu chunk header".into());
//     }
//     let header = slice(bytes, offset, 8)?;
//     let length = usize::try_from(be32(header, 4)?).map_err(|_| "DjVu chunk length overflows")?;
//     let payload = length
//         .checked_add(length & 1)
//         .and_then(|padded| padded.checked_add(8))
//         .ok_or("DjVu chunk overflow")?;
//     let next = offset.checked_add(payload).ok_or("DjVu chunk overflow")?;
//     if next > end {
//         return Err("DjVu chunk exceeds parent FORM".into());
//     }
//     Ok((header[..4].try_into()?, next, length))
// }
//
// /// Recover one page's geometry and raw TXTa text during fallback walking.
// ///
// /// Accepts a byte source, page bounds, mutable report, inspection limits, the
// /// remaining text budget, and a partial-output flag. Returns `Ok(())` after
// /// publishing one valid page. Invalid INFO geometry or versions, malformed
// /// chunks, budget failures outside page-local handling, and page-cap overflow
// /// return an error without publishing the rejected page.
// #[allow(clippy::too_many_arguments)]
// fn page(
//     source: &mut Source,
//     start: u64,
//     end: u64,
//     report: &mut InspectReport,
//     options: &InspectOptions,
//     budget: &mut usize,
//     partial: &mut bool,
// ) -> xml::Result<()> {
//     if report.pages.len() >= 4096 {
//         return Err("DjVu page-count cap exceeded".into());
//     }
//     let (id, mut cursor, len) = chunk(source, start, end)?;
//     if &id != b"INFO" || !(5..=10).contains(&len) {
//         return Err("DjVu page has no valid leading INFO".into());
//     }
//     let info = source.range(start + 8, len as usize)?;
//     let width = u32::from(be16(&info, 0)?);
//     let height = u32::from(be16(&info, 2)?);
//     if width == 0 || height == 0 {
//         return Err("invalid DjVu page dimensions".into());
//     }
//     let dpi = if info.len() >= 8 {
//         u16::from_le_bytes(info[6..8].try_into()?)
//     } else {
//         300
//     };
//     let major = info.get(5).copied().unwrap_or(0);
//     // Match djvu-rs PageInfo: the combined version must stay below 50.
//     let version = match (info.get(4), info.get(5)) {
//         (Some(&minor), Some(&major)) if major != 0xff => (u16::from(major) << 8) | u16::from(minor),
//         (Some(&minor), _) => u16::from(minor),
//         (None, _) => 0,
//     };
//     if version >= 50 {
//         return Err("unsupported DjVu page version".into());
//     }
//     report.pages.push(Page {
//         index: report.pages.len() + 1,
//         source_part: format!("FORM@{}", start - 12),
//         width: Some(width),
//         height: Some(height),
//         dpi: Some(dpi),
//         version: Some(format!("{major}.{}", info[4])),
//     });
//     let mut blocks = 0;
//     let mut text_chunk: Option<([u8; 4], u64, u64)> = None;
//     while cursor < end {
//         blocks += 1;
//         if blocks > 4096 {
//             return Err("DjVu page chunk-count cap exceeded".into());
//         }
//         let (id, next, len) = chunk(source, cursor, end)?;
//         // LITX-AUD-013: prefer the supported plain TXTa over its compressed
//         // TXTz twin, which is rejected without BZZ decode.
//         let replace = match &text_chunk {
//             Some((existing, _, _)) => &id == b"TXTa" && existing != b"TXTa",
//             None => &id == b"TXTz" || &id == b"TXTa",
//         };
//         if replace {
//             text_chunk = Some((id, cursor + 8, len));
//         }
//         cursor = next;
//     }
//     // LITX-AUD-013(b): the raw fallback also recovers embedded text the full
//     // decoder could not reach, within the same scan limit and decode budget.
//     let number = report.pages.len();
//     let text_limit = options.max_pages.unwrap_or(4096).min(4096);
//     if let Some(text_chunk) = text_chunk {
//         if number > text_limit {
//             // Skipped text is never silently complete.
//             *partial = true;
//         } else {
//             match raw_embedded_text(source, text_chunk, height, budget) {
//                 Ok(Some(text)) if !text.trim().is_empty() => {
//                     append_page(report, options, number, &text, "text_fallback");
//                 }
//                 Ok(_) => {}
//                 Err(error) => {
//                     *partial = true;
//                     report.error("djvu_text", format!("page {number}: {error}"));
//                 }
//             }
//         }
//     }
//     Ok(())
// }
//
// /// Decode a raw TXTz/TXTa chunk payload from the fallback IFF walk, mirroring
// /// the full decoder's embedded-text path.
// fn raw_embedded_text(
//     source: &mut Source,
//     (id, offset, len): ([u8; 4], u64, u64),
//     height: u32,
//     budget: &mut usize,
// ) -> xml::Result<Option<String>> {
//     if len == 0 {
//         return Ok(None);
//     }
//     // LITX-AUD-013 support boundary: the raw fallback never decodes TXTz.
//     if &id == b"TXTz" {
//         return Err(TXTZ_UNSUPPORTED.into());
//     }
//     // LITX-AUD-013: bill the entire raw chunk prospectively, before reading
//     // or parsing any of it.
//     charge_decoded_budget(budget, len as usize)?;
//     let payload = source.range(offset, len as usize)?;
//     let header = slice(&payload, 0, 3)?;
//     let length = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
//     let text = slice(&payload, 3, length)?;
//     let layer = djvu_rs::text::parse_text_layer(&payload[..3 + text.len()], height)?;
//     Ok(Some(
//         layer
//             .text
//             .chars()
//             .map(|c| {
//                 if c.is_control() && !matches!(c, '\n' | '\r' | '\t') {
//                     ' '
//                 } else {
//                     c
//                 }
//             })
//             .collect(),
//     ))
// }
//
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     fn test_document(text: &[u8]) -> Vec<u8> {
//         fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
//             let mut bytes = id.to_vec();
//             bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
//             bytes.extend_from_slice(payload);
//             if payload.len() % 2 == 1 {
//                 bytes.push(0);
//             }
//             bytes
//         }
//         let mut page = b"DJVU".to_vec();
//         page.extend(chunk(b"INFO", &[2, 88, 3, 32, 26, 0, 44, 1, 22, 1]));
//         page.extend(chunk(b"TXTa", text));
//         let mut bytes = b"AT&T".to_vec();
//         bytes.extend(chunk(b"FORM", &page));
//         bytes
//     }
//
//     /// A minimal report for direct extract tests, mirroring lib.rs construction.
//     fn test_report(path: &Path) -> InspectReport {
//         InspectReport {
//             djvu: None,
//             derived_preview: None,
//             dataset: None,
//             parquet: None,
//             json: None,
//             xml: None,
//             source_path: path.to_path_buf(),
//             artifacts: vec![],
//             markdown: crate::Markdown::default(),
//             pdf: None,
//             image: None,
//             pages: vec![],
//             format: crate::Format {
//                 extension: "djvu".into(),
//                 detected_mime: None,
//                 detected_name: None,
//                 bucket: "Other".into(),
//                 basis: "extension".into(),
//                 disagreement: false,
//                 detector: "extension".into(),
//             },
//             claims: vec![],
//             counts: crate::Counts::default(),
//             entries: vec![],
//             sheets: vec![],
//             tables: vec![],
//             preflight: vec![],
//             text: crate::Text {
//                 origin: "native".into(),
//                 token_basis: "whitespace_words",
//                 ..crate::Text::default()
//             },
//             thumbnail: None,
//             errors: vec![],
//             packet: None,
//         }
//     }
//
//     fn test_path(name: &str) -> std::path::PathBuf {
//         Path::new(env!("CARGO_MANIFEST_DIR"))
//             .join("../../target/tests")
//             .join(name)
//     }
//
//     #[test]
//     fn embedded_text_bills_the_full_chunk_payload() {
//         // LITX-AUD-013: the structured path bills the whole raw TXTa chunk,
//         // not just the parsed text slice.
//         let mut payload = vec![0, 0, 22];
//         payload.extend_from_slice(b"Raw text budget marker");
//         payload.push(0);
//         payload.extend(std::iter::repeat_n(b' ', 1000));
//         let bytes = test_document(&payload);
//         let document =
//             djvu_rs::DjVuDocument::parse_with_options(&bytes, &Default::default()).unwrap();
//         let page = document.page(0).unwrap();
//         let mut short = payload.len() - 1;
//         let error = embedded_text(page, &mut short).unwrap_err();
//         assert!(error.to_string().contains("budget"), "{error}");
//         let mut exact = payload.len();
//         let text = embedded_text(page, &mut exact).unwrap().unwrap();
//         assert!(text.contains("Raw text budget marker"));
//         assert_eq!(exact, 0);
//     }
//
//     #[test]
//     fn decoded_budget_charges_cumulatively_and_saturates() {
//         // LITX-AUD-013(a): the budget decrements per decoded unit and refuses
//         // any charge that would overrun the 64 MiB cumulative cap.
//         let mut remaining = MAX_DECODED_TEXT_BYTES;
//         assert_eq!(remaining, 64 * 1024 * 1024);
//         charge_decoded_budget(&mut remaining, 1024).unwrap();
//         assert_eq!(remaining, MAX_DECODED_TEXT_BYTES - 1024);
//         charge_decoded_budget(&mut remaining, MAX_DECODED_TEXT_BYTES).unwrap_err();
//         let rest = remaining;
//         charge_decoded_budget(&mut remaining, rest).unwrap();
//         assert_eq!(remaining, 0);
//         charge_decoded_budget(&mut remaining, 1).unwrap_err();
//     }
//
//     #[test]
//     fn render_plan_reports_the_prospective_raster_size() {
//         // W3-SYS-DJVU-002: the prospective raster geometry and byte size are
//         // computable before any allocation. The 600x800 @ 300dpi fixture
//         // renders at 300x400 RGBA = 480,000 bytes.
//         let bytes = test_document(&[]);
//         let document =
//             djvu_rs::DjVuDocument::parse_with_options(&bytes, &Default::default()).unwrap();
//         let page = document.page(0).unwrap();
//         let (width, height, needed) = render_plan(page).unwrap();
//         assert_eq!((width, height), (300, 400));
//         assert_eq!(needed, 300 * 400 * 4);
//     }
//
//     #[test]
//     fn ocr_budget_is_reserved_before_any_render_or_ocr() {
//         // W3-SYS-DJVU-002: with a budget too small for the prospective
//         // raster, the page stops at the reservation step: no probe exists
//         // (OCR never ran), no djvu_ocr error is recorded, and the static
//         // budget diagnostic names the reservation as the failure. This test
//         // fails when the budget is charged after rendering, because the
//         // render and OCR then run first and a probe is produced.
//         let bytes = test_document(&[]);
//         let path = test_path("djvu-nobzz-budget-order.djvu");
//         std::fs::create_dir_all(path.parent().unwrap()).unwrap();
//         std::fs::write(&path, &bytes).unwrap();
//         let mut report = test_report(&path);
//         let options = InspectOptions {
//             identify: true,
//             ..Default::default()
//         };
//         let mut ocr = crate::images::OcrSession { api: None };
//         extract_with_budget(&path, &mut report, &options, &mut ocr, 1).unwrap();
//         std::fs::remove_file(&path).ok();
//         let analysis = report.djvu.as_ref().unwrap();
//         let signal = &analysis.pages[0];
//         assert_eq!(signal.ocr_status, "budget", "{analysis:?}");
//         assert!(signal.probe.is_none(), "{analysis:?}");
//         assert!(
//             report
//                 .errors
//                 .iter()
//                 .any(|error| error.stage == "djvu_text" && error.message.contains("budget")),
//             "{:?}",
//             report.errors
//         );
//         assert!(
//             !report.errors.iter().any(|error| error.stage == "djvu_ocr"),
//             "{:?}",
//             report.errors
//         );
//         assert!(!analysis.text_complete);
//     }
//
//     /// Encode an IFF chunk for direct page-reservation tests.
//     ///
//     /// Accepts the chunk identifier and payload. Returns the padded chunk and
//     /// panics only when its payload cannot be represented by the IFF length.
//     fn test_chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
//         let mut bytes = id.to_vec();
//         bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
//         bytes.extend_from_slice(payload);
//         if payload.len() % 2 == 1 {
//             bytes.push(0);
//         }
//         bytes
//     }
//
//     /// Parse a standalone page with caller-selected INFO and layer chunks.
//     ///
//     /// Accepts the raw INFO payload and encoded layer chunks. Returns an owned
//     /// parsed document and panics if the test page has invalid container
//     /// structure; individual layer payloads may intentionally be malformed.
//     fn parsed_test_page(info: &[u8], layers: &[Vec<u8>]) -> djvu_rs::DjVuDocument {
//         let mut body = b"DJVU".to_vec();
//         body.extend(test_chunk(b"INFO", info));
//         for layer in layers {
//             body.extend_from_slice(layer);
//         }
//         let mut bytes = b"AT&T".to_vec();
//         bytes.extend(test_chunk(b"FORM", &body));
//         djvu_rs::DjVuDocument::parse_with_options(&bytes, &parse_options()).unwrap()
//     }
//
//     /// Return a one-pixel INFO payload for layer-budget tests.
//     ///
//     /// Accepts no inputs and returns ten bytes of version-26.0 metadata at
//     /// 300 DPI. No failure states apply.
//     fn one_pixel_info() -> [u8; 10] {
//         [0, 1, 0, 1, 26, 0, 44, 1, 22, 1]
//     }
//
//     /// Encode a small valid white JPEG for exact allocation-boundary tests.
//     ///
//     /// Accepts width and height in pixels. Returns a valid JPEG and panics if
//     /// the test image cannot be allocated or encoded.
//     fn white_test_jpeg(width: u32, height: u32) -> Vec<u8> {
//         let image = image::RgbImage::from_pixel(width, height, image::Rgb([255; 3]));
//         let mut encoded = std::io::Cursor::new(Vec::new());
//         image::DynamicImage::ImageRgb8(image)
//             .write_to(&mut encoded, image::ImageFormat::Jpeg)
//             .unwrap();
//         encoded.into_inner()
//     }
//
//     /// Assert that both JPEG layer paths reserve their complete decode peak.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if a one-pixel INFO lets a
//     /// 16x16 BGjp or FGjp pass with only four bytes budgeted, or if a valid
//     /// layer is rejected with its computed allocation budget.
//     #[test]
//     fn wave4_jpeg_layers_are_charged_before_decode() {
//         let jpeg = white_test_jpeg(16, 16);
//         let allocation = jpeg_allocation(&jpeg).unwrap();
//         for layer in [b"BGjp", b"FGjp"] {
//             let document = parsed_test_page(&one_pixel_info(), &[test_chunk(layer, &jpeg)]);
//             let page = document.page(0).unwrap();
//             let mut short = 4;
//             assert!(
//                 matches!(
//                     reserve_render(page, &mut short),
//                     Err(ReserveError::Budget(_))
//                 ),
//                 "layer {} bypassed its allocation budget",
//                 String::from_utf8_lossy(layer)
//             );
//             assert_eq!(short, 4, "failed reservation must be atomic");
//             let mut sufficient = allocation.decode;
//             assert!(reserve_render(page, &mut sufficient).is_ok());
//         }
//     }
//
//     /// Assert that simultaneous JPEG layers use their actual peak, not a sum.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if 16x16 BGjp and FGjp layers
//     /// do not fit their computed resident-plus-second-decode peak or if one
//     /// byte less is accepted.
//     #[test]
//     fn wave4_jpeg_peak_excludes_nonoverlapping_output_and_rgb() {
//         let jpeg = white_test_jpeg(16, 16);
//         let allocation = jpeg_allocation(&jpeg).unwrap();
//         let exact = allocation.resident + allocation.decode;
//         let document = parsed_test_page(
//             &one_pixel_info(),
//             &[test_chunk(b"BGjp", &jpeg), test_chunk(b"FGjp", &jpeg)],
//         );
//         let page = document.page(0).unwrap();
//         let mut remaining = exact;
//         assert!(reserve_render(page, &mut remaining).is_ok());
//         assert_eq!(remaining, 0);
//         let mut short = exact - 1;
//         assert!(matches!(
//             reserve_render(page, &mut short),
//             Err(ReserveError::Budget(_))
//         ));
//         assert_eq!(short, exact - 1);
//     }
//
//     /// Assert that page reservations consume one cumulative document budget.
//     ///
//     /// Accepts no inputs and returns `()`. Panics when a second 16x16 JPEG page
//     /// succeeds after the first page leaves one byte less than its decode peak.
//     #[test]
//     fn wave4_layer_charges_accumulate_across_pages() {
//         let jpeg = white_test_jpeg(16, 16);
//         let per_page = jpeg_allocation(&jpeg).unwrap().decode;
//         let document = parsed_test_page(&one_pixel_info(), &[test_chunk(b"BGjp", &jpeg)]);
//         let page = document.page(0).unwrap();
//         let mut remaining = per_page * 2 - 1;
//         assert!(reserve_render(page, &mut remaining).is_ok());
//         assert_eq!(remaining, per_page - 1);
//         assert!(matches!(
//             reserve_render(page, &mut remaining),
//             Err(ReserveError::Budget(_))
//         ));
//         assert_eq!(remaining, per_page - 1);
//     }
//
//     /// Assert that Smmr dimensions are read and charged before MMR decoding.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if a 16x16 header passes with
//     /// an INFO-only budget, a sufficient budget rejects it, or a short header
//     /// is reported as a budget failure instead of a malformed render plan.
//     #[test]
//     fn wave4_smmr_dimensions_are_charged_and_headers_stay_honest() {
//         let document = parsed_test_page(&one_pixel_info(), &[test_chunk(b"Smmr", &[0, 16, 0, 16])]);
//         let page = document.page(0).unwrap();
//         assert_eq!(render_allocation_plan(page).unwrap(), 704);
//         let mut short = 4;
//         assert!(matches!(
//             reserve_render(page, &mut short),
//             Err(ReserveError::Budget(_))
//         ));
//         assert_eq!(short, 4);
//         let mut sufficient = 4096;
//         assert!(reserve_render(page, &mut sufficient).is_ok());
//
//         let large = parsed_test_page(
//             &one_pixel_info(),
//             &[test_chunk(b"Smmr", &[0x20, 0, 0x20, 0])],
//         );
//         let mut large_budget = 64 * 1024 * 1024;
//         assert!(matches!(
//             reserve_render(large.page(0).unwrap(), &mut large_budget),
//             Err(ReserveError::Budget(_))
//         ));
//
//         let mmr = parsed_test_page(
//             &one_pixel_info(),
//             &[test_chunk(b"Smmr", b"MMR\0\0\x10\0\x10")],
//         );
//         let mut short = 4;
//         assert!(matches!(
//             reserve_render(mmr.page(0).unwrap(), &mut short),
//             Err(ReserveError::Budget(_))
//         ));
//
//         let malformed = parsed_test_page(&one_pixel_info(), &[test_chunk(b"Smmr", &[0, 16, 0])]);
//         let mut budget = 4096;
//         assert!(matches!(
//             reserve_render(malformed.page(0).unwrap(), &mut budget),
//             Err(ReserveError::Plan(_))
//         ));
//         assert_eq!(budget, 4096);
//     }
//
//     /// Derive the exact 33x2 MMR peak from the installed decoder allocations.
//     ///
//     /// Accepts no inputs and returns `()`. Panics unless the plan includes the
//     /// packed bitmap, two Vec headers, two completed rows, the white row, and
//     /// the current row at `size_of::<bool>()` bytes per pixel.
//     #[test]
//     fn wave4_mmr_rows_use_actual_bool_storage() {
//         let width = 33usize;
//         let height = 2usize;
//         let packed = width.div_ceil(8) * height;
//         let row_headers = height * std::mem::size_of::<Vec<bool>>();
//         let row_storage = (height + 2) * width * std::mem::size_of::<bool>();
//         let expected_decode = packed + row_headers + row_storage;
//         let allocation = smmr_allocation(&[0, 33, 0, 2], false).unwrap();
//         assert_eq!(allocation.resident, packed);
//         assert_eq!(allocation.decode, expected_decode);
//     }
//
//     /// Assert that explicit IW44 dimensions are also reserved before decode.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if a small BG44 or FG44
//     /// header passes with only INFO output budget, valid headers fail with a
//     /// sufficient budget, or a short initial header is not a plan error.
//     #[test]
//     fn wave4_iw44_dimensions_are_charged_before_decode() {
//         let header = [0, 0, 1, 1, 0, 16, 0, 16, 0];
//         for layer in [b"BG44", b"FG44"] {
//             let document = parsed_test_page(&one_pixel_info(), &[test_chunk(layer, &header)]);
//             let mut short = 4;
//             assert!(matches!(
//                 reserve_render(document.page(0).unwrap(), &mut short),
//                 Err(ReserveError::Budget(_))
//             ));
//             let mut sufficient = 65536;
//             assert!(reserve_render(document.page(0).unwrap(), &mut sufficient).is_ok());
//         }
//         let malformed = parsed_test_page(&one_pixel_info(), &[test_chunk(b"BG44", &[0, 0])]);
//         let mut budget = 4096;
//         assert!(matches!(
//             reserve_render(malformed.page(0).unwrap(), &mut budget),
//             Err(ReserveError::Plan(_))
//         ));
//     }
//
//     /// Assert that dimension-opaque JB2 masks never enter the render decoder.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if Sjbz is accepted for OCR
//     /// or its rejected render plan consumes the cumulative byte budget.
//     #[test]
//     fn wave4_sjbz_never_enters_unbounded_decoder() {
//         let document = parsed_test_page(&one_pixel_info(), &[test_chunk(b"Sjbz", &[])]);
//         let mut budget = 4096;
//         let result = reserve_render(document.page(0).unwrap(), &mut budget);
//         assert!(matches!(result, Err(ReserveError::Plan(_))));
//         assert_eq!(budget, 4096);
//     }
//
//     /// Assert that malformed JPEG headers fail before any decoder allocation.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if a truncated JPEG is
//     /// accepted by render planning or consumes any of the explicit budget.
//     #[test]
//     fn wave4_malformed_jpeg_header_is_a_plan_error() {
//         let document = parsed_test_page(
//             &one_pixel_info(),
//             &[test_chunk(b"BGjp", &[0xff, 0xd8, 0xff])],
//         );
//         let mut budget = 4096;
//         assert!(matches!(
//             reserve_render(document.page(0).unwrap(), &mut budget),
//             Err(ReserveError::Plan(_))
//         ));
//         assert_eq!(budget, 4096);
//     }
//
//     /// Build a SOF-only JPEG for allocation tests without invoking a decoder.
//     ///
//     /// Accepts the SOF marker, dimensions, and per-component sampling bytes.
//     /// Returns SOI, one complete SOF segment, and EOI. Test inputs with too many
//     /// components panic while calculating the test segment length.
//     fn wave5_synthetic_jpeg(marker: u8, width: u16, height: u16, sampling: &[u8]) -> Vec<u8> {
//         let component_count = u8::try_from(sampling.len()).unwrap();
//         let segment_length = u16::try_from(8 + 3 * sampling.len()).unwrap();
//         let mut jpeg = vec![0xff, 0xd8, 0xff, marker];
//         jpeg.extend_from_slice(&segment_length.to_be_bytes());
//         jpeg.push(8);
//         jpeg.extend_from_slice(&height.to_be_bytes());
//         jpeg.extend_from_slice(&width.to_be_bytes());
//         jpeg.push(component_count);
//         for (index, sampling) in sampling.iter().enumerate() {
//             jpeg.extend_from_slice(&[index as u8 + 1, *sampling, 0]);
//         }
//         jpeg.extend_from_slice(&[0xff, 0xd9]);
//         jpeg
//     }
//
//     /// Calculate the conservative coefficient ceiling expected by Wave 5.
//     ///
//     /// Accepts JPEG dimensions and sampling bytes. Returns the larger of the
//     /// interleaved component-count bound and exact non-interleaved allocation.
//     /// Test dimensions and factors are small enough to avoid arithmetic overflow.
//     fn wave5_coefficient_ceiling(width: u16, height: u16, sampling: &[u8]) -> usize {
//         let width = usize::from(width);
//         let height = usize::from(height);
//         let max_h = sampling
//             .iter()
//             .map(|sampling| usize::from(sampling >> 4))
//             .max()
//             .unwrap();
//         let max_v = sampling
//             .iter()
//             .map(|sampling| usize::from(sampling & 0x0f))
//             .max()
//             .unwrap();
//         let interleaved_width = width.div_ceil(8 * max_h) * 8 * max_h;
//         let interleaved_height = height.div_ceil(8 * max_v) * 8 * max_v;
//         let interleaved = 2 * sampling.len() * interleaved_width * interleaved_height;
//         let scan_width = width.div_ceil(8) * 8;
//         let scan_height = height.div_ceil(8) * 8;
//         let sampling_sum = sampling
//             .iter()
//             .map(|sampling| usize::from(sampling >> 4) * usize::from(sampling & 0x0f))
//             .sum::<usize>();
//         let non_interleaved = 2 * scan_width * scan_height * sampling_sum;
//         interleaved.max(non_interleaved)
//     }
//
//     /// Write a standalone DjVu probe with one synthetic JPEG layer.
//     ///
//     /// Accepts a fixture name, BGjp/FGjp chunk identifier, and JPEG payload.
//     /// Returns the retained probe path and panics if the private evidence
//     /// directory cannot be created or written.
//     fn wave5_probe(name: &str, layer: &[u8; 4], jpeg: &[u8]) -> std::path::PathBuf {
//         let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
//             .join("../../target/verification/wave5-remediation/djvu/probes");
//         std::fs::create_dir_all(&directory).unwrap();
//         let mut body = b"DJVU".to_vec();
//         body.extend(test_chunk(b"INFO", &one_pixel_info()));
//         body.extend(test_chunk(layer, jpeg));
//         let mut bytes = b"AT&T".to_vec();
//         bytes.extend(test_chunk(b"FORM", &body));
//         let path = directory.join(name);
//         std::fs::write(&path, bytes).unwrap();
//         path
//     }
//
//     /// Assert progressive 4:4:4 coefficients reject both JPEG layer paths.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if a 3000x3000 SOF2 layer
//     /// fits the 64 MiB budget, consumes budget on rejection, or has no retained
//     /// CLI probe. The synthetic SOF prevents actual decoder allocation.
//     #[test]
//     fn wave5_progressive_coefficients_exhaust_budget_before_both_jpeg_layers() {
//         let sampling = [0x11, 0x11, 0x11];
//         let jpeg = wave5_synthetic_jpeg(0xc2, 3000, 3000, &sampling);
//         let coefficients = wave5_coefficient_ceiling(3000, 3000, &sampling);
//         assert_eq!(coefficients, 54_000_000);
//         for (name, layer) in [("bgjp", b"BGjp"), ("fgjp", b"FGjp")] {
//             wave5_probe(
//                 &format!("info-1x1-{name}-3000x3000-sof2-synthetic.djvu"),
//                 layer,
//                 &jpeg,
//             );
//             let document = parsed_test_page(&one_pixel_info(), &[test_chunk(layer, &jpeg)]);
//             let mut budget = 64 * 1024 * 1024;
//             assert!(
//                 matches!(
//                     reserve_render(document.page(0).unwrap(), &mut budget),
//                     Err(ReserveError::Budget(_))
//                 ),
//                 "{name}: progressive coefficient buffers bypassed the budget"
//             );
//             assert_eq!(budget, 64 * 1024 * 1024);
//         }
//     }
//
//     /// Assert every four-component record contributes to the allocation plan.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if mixed sampling factors or
//     /// a fourth component are ignored. The SOF-only fixture performs no decode.
//     #[test]
//     fn wave5_four_component_sampling_shapes_are_accounted() {
//         let sampling = [0x44, 0x11, 0x21, 0x14];
//         let jpeg = wave5_synthetic_jpeg(0xc2, 65, 33, &sampling);
//         let allocation = jpeg_allocation(&jpeg).unwrap();
//         let coefficients = wave5_coefficient_ceiling(65, 33, &sampling);
//         assert!(allocation.decode >= 7 * 65 * 33 + coefficients);
//         assert_eq!(allocation.resident, 4 * 65 * 33);
//     }
//
//     /// Assert narrow images include sampling and non-interleaved MCU padding.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if four 4x4 components in a
//     /// 1x16384 SOF2 image omit the 16 MiB non-interleaved coefficient ceiling.
//     #[test]
//     fn wave5_narrow_high_sampling_coefficients_include_mcu_padding() {
//         let sampling = [0x44; 4];
//         let jpeg = wave5_synthetic_jpeg(0xc2, 1, 16384, &sampling);
//         let allocation = jpeg_allocation(&jpeg).unwrap();
//         let coefficients = wave5_coefficient_ceiling(1, 16384, &sampling);
//         assert_eq!(coefficients, 16_777_216);
//         assert!(allocation.decode >= 7 * 16_384 + coefficients);
//         wave5_probe(
//             "info-1x1-bgjp-1x16384-four-4x4-sof2-synthetic.djvu",
//             b"BGjp",
//             &jpeg,
//         );
//     }
//
//     /// Assert SOF0 cannot bypass coefficient reservation through its marker.
//     ///
//     /// Accepts no inputs and returns `()`. Panics if a baseline header receives
//     /// only the old RGB/RGBA allowance; split scans are conservatively covered
//     /// without parsing entropy-coded data.
//     #[test]
//     fn wave5_baseline_multiscan_uses_conservative_coefficient_accounting() {
//         let sampling = [0x11, 0x11, 0x11];
//         let jpeg = wave5_synthetic_jpeg(0xc0, 64, 64, &sampling);
//         let allocation = jpeg_allocation(&jpeg).unwrap();
//         let coefficients = wave5_coefficient_ceiling(64, 64, &sampling);
//         assert_eq!(coefficients, 24_576);
//         assert!(allocation.decode >= 7 * 64 * 64 + coefficients);
//     }
//
//     /// Assert malformed SOF component metadata fails before decoder entry.
//     ///
//     /// Accepts no inputs and returns `()`. Panics when zero/excess components,
//     /// bad precision/quantization, invalid sampling, inconsistent lengths, or
//     /// truncated component records are accepted by allocation planning.
//     #[test]
//     fn wave5_malformed_sof_component_records_are_plan_errors() {
//         let mut malformed = vec![
//             wave5_synthetic_jpeg(0xc0, 16, 16, &[]),
//             wave5_synthetic_jpeg(0xc0, 16, 16, &[0x11; 5]),
//             wave5_synthetic_jpeg(0xc0, 16, 16, &[0x00]),
//             wave5_synthetic_jpeg(0xc0, 16, 16, &[0x10]),
//             wave5_synthetic_jpeg(0xc0, 16, 16, &[0x01]),
//             wave5_synthetic_jpeg(0xc0, 16, 16, &[0x31]),
//             wave5_synthetic_jpeg(0xc0, 16, 16, &[0x51]),
//             wave5_synthetic_jpeg(0xc0, 16, 16, &[0x15]),
//         ];
//         let mut bad_precision = wave5_synthetic_jpeg(0xc0, 16, 16, &[0x11]);
//         bad_precision[6] = 12;
//         malformed.push(bad_precision);
//         let mut bad_quantization = wave5_synthetic_jpeg(0xc0, 16, 16, &[0x11]);
//         bad_quantization[13] = 4;
//         malformed.push(bad_quantization);
//         let mut bad_length = wave5_synthetic_jpeg(0xc0, 16, 16, &[0x11]);
//         bad_length[4..6].copy_from_slice(&12u16.to_be_bytes());
//         malformed.push(bad_length);
//         let mut truncated = wave5_synthetic_jpeg(0xc0, 16, 16, &[0x11, 0x11]);
//         truncated.truncate(14);
//         malformed.push(truncated);
//         for (index, jpeg) in malformed.iter().enumerate() {
//             assert!(
//                 jpeg_allocation(jpeg).is_err(),
//                 "malformed SOF case {index} reached decoder planning"
//             );
//         }
//     }
// }
// ----- END ORIGINAL extensionalities/crates/litx-inspect/src/djvu.rs -----
// ----- ORIGIN extensionalities/crates/litx-inspect/tests/djvu.rs -----
// ----- ORIGINAL SHA256 03e288488504eae7abf4c585c8bdd1d126aa0b9b793d3f06809508457d98f89c -----
// ----- BEGIN ORIGINAL extensionalities/crates/litx-inspect/tests/djvu.rs -----
// use litx_inspect::{InspectOptions, inspect};
// use std::path::PathBuf;
// fn fixture(name: &str) -> PathBuf {
//     PathBuf::from(env!("CARGO_MANIFEST_DIR"))
//         .join("../../fixtures")
//         .join(name)
// }
//
// fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
//     let mut bytes = id.to_vec();
//     bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
//     bytes.extend_from_slice(payload);
//     if payload.len() % 2 == 1 {
//         bytes.push(0);
//     }
//     bytes
// }
//
// fn document(chunks: &[Vec<u8>]) -> PathBuf {
//     let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
//         .join("../../target/tests")
//         .join(format!(
//             "djvu-{}",
//             std::time::SystemTime::now()
//                 .duration_since(std::time::UNIX_EPOCH)
//                 .unwrap()
//                 .as_nanos()
//         ));
//     std::fs::create_dir_all(&directory).unwrap();
//     let mut page = b"DJVU".to_vec();
//     page.extend(chunk(b"INFO", &[2, 88, 3, 32, 26, 0, 44, 1, 22, 1]));
//     for item in chunks {
//         page.extend_from_slice(item);
//     }
//     let mut bytes = b"AT&T".to_vec();
//     bytes.extend(chunk(b"FORM", &page));
//     let path = directory.join("sample.djvu");
//     std::fs::write(&path, bytes).unwrap();
//     path
// }
//
// fn text_payload(text: &str) -> Vec<u8> {
//     let mut bytes = (text.len() as u32).to_be_bytes()[1..].to_vec();
//     bytes.extend_from_slice(text.as_bytes());
//     bytes.push(0);
//     bytes
// }
//
// fn bundle(pages: &[Vec<u8>]) -> PathBuf {
//     bundle_ex(pages, None, &[])
// }
//
// fn dir_tail(sizes: &[usize], flags: &[u8]) -> Vec<u8> {
//     let mut metadata = Vec::new();
//     for size in sizes {
//         metadata.extend_from_slice(&(*size as u32).to_be_bytes()[1..]);
//     }
//     metadata.extend_from_slice(flags);
//     for index in 0..sizes.len() {
//         metadata.extend_from_slice(format!("page-{index}\0").as_bytes());
//     }
//     djvu_rs::bzz_encode::bzz_encode(&metadata)
// }
//
// fn bundle_ex(pages: &[Vec<u8>], tail: Option<&[u8]>, extra: &[Vec<u8>]) -> PathBuf {
//     let compressed = tail.map(<[u8]>::to_vec).unwrap_or_else(|| {
//         dir_tail(
//             &pages.iter().map(Vec::len).collect::<Vec<_>>(),
//             &vec![1; pages.len()],
//         )
//     });
//     let dir_size = 3 + 4 * pages.len() + compressed.len();
//     let extra_len: usize = extra.iter().map(Vec::len).sum();
//     let mut offset = 16 + 8 + dir_size + dir_size % 2 + extra_len;
//     let mut directory = vec![0x81];
//     directory.extend_from_slice(&(pages.len() as u16).to_be_bytes());
//     for page in pages {
//         directory.extend_from_slice(&(offset as u32).to_be_bytes());
//         offset += page.len();
//     }
//     directory.extend(compressed);
//     let mut body = b"DJVM".to_vec();
//     body.extend(chunk(b"DIRM", &directory));
//     for item in extra {
//         body.extend_from_slice(item);
//     }
//     for page in pages {
//         body.extend_from_slice(page);
//     }
//     let mut bytes = b"AT&T".to_vec();
//     bytes.extend(chunk(b"FORM", &body));
//     let path = document(&[]).with_file_name("bundled.djvu");
//     std::fs::write(&path, bytes).unwrap();
//     path
// }
//
// #[test]
// #[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
// fn djvu_full_extract_ocr_continues_beyond_page_five() {
//     let page = std::fs::read(document(&[])).unwrap()[4..].to_vec();
//     let path = bundle(&vec![page; 6]);
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             identify: true,
//             max_pages: None,
//             ..Default::default()
//         },
//     );
//     assert!(r.errors.is_empty(), "{:?}", r.errors);
//     assert_eq!(r.counts.pages, Some(6));
//     let analysis = r.djvu.unwrap();
//     assert_eq!(analysis.pages.len(), 6);
//     assert_eq!(
//         analysis.pages.iter().filter(|p| p.probe.is_some()).count(),
//         6
//     );
//     assert_eq!(analysis.pages[5].ocr_status, "no_text_detected");
//     assert!(analysis.text_complete);
//     assert!(!r.text.truncated);
//     let preview = inspect(
//         &path,
//         &InspectOptions {
//             identify: true,
//             ..Default::default()
//         },
//     );
//     assert!(preview.errors.is_empty(), "{:?}", preview.errors);
//     let analysis = preview.djvu.unwrap();
//     assert_eq!(
//         analysis.pages.iter().filter(|p| p.probe.is_some()).count(),
//         5
//     );
//     assert_eq!(analysis.pages[5].ocr_status, "page_limit");
//     assert!(!analysis.text_complete);
//     assert!(preview.text.truncated);
// }
//
// #[test]
// #[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
// fn djvu_recognizes_rendered_text_with_ocr_markdown_provenance() {
//     let path = document(&[]);
//     let thumbnail = path.with_file_name("ocr-source.png");
//     let pdf = inspect(
//         &fixture("native.pdf"),
//         &InspectOptions {
//             thumbnail: Some(thumbnail.clone()),
//             thumbnail_max_edge: 1200,
//             max_units: 0,
//             ..Default::default()
//         },
//     );
//     assert!(pdf.errors.is_empty(), "{:?}", pdf.errors);
//     let image = image::open(thumbnail).unwrap().to_rgb8();
//     let mut jpeg = std::io::Cursor::new(Vec::new());
//     image::DynamicImage::ImageRgb8(image.clone())
//         .write_to(&mut jpeg, image::ImageFormat::Jpeg)
//         .unwrap();
//     let scan = document(&[chunk(b"BGjp", jpeg.get_ref())]);
//     let mut bytes = std::fs::read(&scan).unwrap();
//     bytes[24..26].copy_from_slice(&(image.width() as u16).to_be_bytes());
//     bytes[26..28].copy_from_slice(&(image.height() as u16).to_be_bytes());
//     bytes[30..32].copy_from_slice(&150u16.to_le_bytes());
//     let scan = scan.with_file_name("ocr.djvu");
//     std::fs::write(&scan, bytes).unwrap();
//     let r = inspect(
//         &scan,
//         &InspectOptions {
//             identify: true,
//             max_pages: None,
//             ..Default::default()
//         },
//     );
//     assert!(r.errors.is_empty(), "{:?}", r.errors);
//     assert!(r.text.tokens >= 5, "{:?}", r.text);
//     assert_eq!(r.text.origin, "ocr");
//     assert!(
//         r.djvu.unwrap().pages[0]
//             .probe
//             .as_ref()
//             .unwrap()
//             .mean_confidence
//             > 0.5
//     );
//     assert_eq!(r.markdown.basis, "ocr_text_fallback");
//     assert!(r.markdown.content.contains("## Page 1"));
// }
//
// #[test]
// fn djvu_normalizes_document_control_separators_for_llm_text() {
//     let path = document(&[chunk(
//         b"TXTa",
//         &text_payload("alpha\u{1f}beta\u{b}gamma\0delta"),
//     )]);
//     let r = inspect(&path, &Default::default());
//     assert!(r.errors.is_empty(), "{:?}", r.errors);
//     assert!(r.text.content.contains("alpha beta gamma delta"));
//     assert!(!r.text.content.contains('\0'));
// }
//
// #[test]
// fn djvu_preview_avoids_later_decode_errors_while_full_extract_keeps_other_pages() {
//     let mut pages = Vec::new();
//     for i in 1..=5 {
//         let path = document(&[chunk(b"TXTa", &text_payload(&format!("Unit{i}")))]);
//         pages.push(std::fs::read(path).unwrap()[4..].to_vec());
//     }
//     let bad = document(&[chunk(b"TXTa", &[0, 0, 64, b'x'])]);
//     pages.push(std::fs::read(bad).unwrap()[4..].to_vec());
//     let last = document(&[chunk(b"TXTa", &text_payload("LastNative"))]);
//     pages.push(std::fs::read(last).unwrap()[4..].to_vec());
//     let path = bundle(&pages);
//     let preview = inspect(&path, &Default::default());
//     assert!(preview.errors.is_empty(), "{:?}", preview.errors);
//     assert!(!preview.text.content.contains("LastNative"));
//     assert!(preview.text.truncated);
//     let full = inspect(
//         &path,
//         &InspectOptions {
//             max_pages: None,
//             ..Default::default()
//         },
//     );
//     assert_eq!(full.counts.pages, Some(7));
//     assert!(full.text.content.contains("Unit1"));
//     assert!(full.text.content.contains("LastNative"));
//     assert_eq!(full.errors.len(), 1);
//     assert_eq!(full.errors[0].stage, "djvu_text");
//     assert!(full.errors[0].message.contains("page 6"));
//     assert!(full.text.truncated);
//     assert!(!full.djvu.unwrap().text_complete);
// }
//
// #[test]
// fn djvu_recovers_text_without_requiring_a_valid_layout_zone_tree() {
//     let text = "Genuine text survives unusable layout metadata.";
//     let mut payload = text_payload(text);
//     payload.push(255); // Invalid zone kind, outside the text bytes.
//     let path = document(&[chunk(b"TXTa", &payload)]);
//     let r = inspect(&path, &Default::default());
//     assert!(r.text.content.contains(text), "{:?}", r.errors);
//     assert_eq!(r.text.origin, "native");
// }
//
// #[test]
// fn djvu_extracts_embedded_plain_text_without_ocr() {
//     let text = "DjVu native text: español বাংলা.";
//     let path = document(&[chunk(b"TXTa", &text_payload(text))]);
//     let r = inspect(&path, &Default::default());
//     assert!(r.errors.is_empty(), "{:?}", r.errors);
//     assert!(r.text.content.contains(text), "{:?}", r.text);
//     assert!(r.markdown.content.contains("DjVu native text"));
//     assert_eq!(r.text.origin, "native");
// }
//
// #[test]
// fn djvu_native_text_does_not_require_a_full_resolution_raster_allocation() {
//     let path = document(&[chunk(b"TXTa", &text_payload("Keep native text cheap."))]);
//     let mut bytes = std::fs::read(&path).unwrap();
//     bytes[24..26].copy_from_slice(&6000u16.to_be_bytes());
//     bytes[26..28].copy_from_slice(&6000u16.to_be_bytes());
//     let path = path.with_file_name("large-native.djvu");
//     std::fs::write(&path, bytes).unwrap();
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             identify: true,
//             ..Default::default()
//         },
//     );
//     assert!(r.errors.is_empty(), "{:?}", r.errors);
//     assert!(r.text.content.contains("Keep native text cheap."));
//     assert!(r.djvu.unwrap().pages[0].probe.is_none());
// }
//
// #[test]
// fn djvu_rejects_compressed_text_without_invoking_bzz_decode() {
//     // LITX-AUD-013 support boundary (user decision 2026-11-18): TXTz is
//     // rejected with a static unsupported diagnostic before any decoder
//     // invocation, even when the payload is a valid BZZ stream that formerly
//     // decoded. Raw TXTa text and OCR remain the supported text paths.
//     let text = "Compressed DjVu text for an LLM: español বাংলা.";
//     let compressed = djvu_rs::bzz_encode::bzz_encode(&text_payload(text));
//     let path = document(&[chunk(b"TXTz", &compressed)]);
//     let r = inspect(&path, &Default::default());
//     let error = r
//         .errors
//         .iter()
//         .find(|e| e.stage == "djvu_text")
//         .expect("static unsupported error");
//     assert!(error.message.contains("unsupported"), "{error:?}");
//     assert!(!r.text.content.contains("Compressed DjVu text"));
//     let analysis = r.djvu.as_ref().unwrap();
//     assert!(!analysis.text_complete);
//     assert!(r.text.truncated);
//     assert_eq!(analysis.pages[0].text_origin, "unavailable");
//     assert_eq!(analysis.pages[0].native_chars, Some(0));
// }
//
// #[test]
// fn djvu_rejects_malformed_compressed_text_with_the_same_static_diagnostic() {
//     // A malformed TXTz must produce the same static unsupported diagnostic,
//     // not a decoder error: proof that the decoder is never invoked.
//     let path = document(&[chunk(b"TXTz", &[0, 0, 64, b'x'])]);
//     let r = inspect(&path, &Default::default());
//     let error = r
//         .errors
//         .iter()
//         .find(|e| e.stage == "djvu_text")
//         .expect("static unsupported error");
//     assert!(error.message.contains("unsupported"), "{error:?}");
//     assert!(!r.djvu.as_ref().unwrap().text_complete);
//     assert!(r.text.truncated);
// }
//
// #[test]
// fn djvu_mixed_compressed_and_raw_text_pages_report_honest_partial() {
//     // Mixed documents stay useful: the TXTz page is rejected with disclosure
//     // while the raw TXTa page is extracted; the report stays partial.
//     let compressed = document(&[chunk(
//         b"TXTz",
//         &djvu_rs::bzz_encode::bzz_encode(&text_payload("Hidden compressed words")),
//     )]);
//     let raw = document(&[chunk(b"TXTa", &text_payload("Raw visible words"))]);
//     let path = bundle(&[
//         std::fs::read(compressed).unwrap()[4..].to_vec(),
//         std::fs::read(raw).unwrap()[4..].to_vec(),
//     ]);
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             max_pages: None,
//             ..Default::default()
//         },
//     );
//     let unsupported = r
//         .errors
//         .iter()
//         .filter(|e| e.stage == "djvu_text" && e.message.contains("unsupported"))
//         .count();
//     assert_eq!(unsupported, 1, "{:?}", r.errors);
//     assert!(
//         r.errors.iter().any(|e| e.message.contains("page 1")),
//         "{:?}",
//         r.errors
//     );
//     assert!(!r.text.content.contains("Hidden compressed words"));
//     assert!(r.text.content.contains("Raw visible words"));
//     let analysis = r.djvu.as_ref().unwrap();
//     assert!(!analysis.text_complete);
//     assert!(r.text.truncated);
//     assert_eq!(analysis.pages[0].text_origin, "unavailable");
//     assert_eq!(analysis.pages[1].text_origin, "embedded_text");
// }
//
// #[test]
// fn djvu_fallback_does_not_duplicate_pages_or_emit_stale_bzz_message() {
//     // LITX-AUD-013: when the full decoder fails on a later page, the raw
//     // geometry fallback must not duplicate earlier page entries, and the
//     // retired "BZZ decoding is out of scope" message must stay gone.
//     let good = document(&[chunk(
//         b"TXTz",
//         &djvu_rs::bzz_encode::bzz_encode(&text_payload("First page native text")),
//     )]);
//     let page0 = std::fs::read(good).unwrap()[4..].to_vec();
//     let mut body = b"DJVU".to_vec();
//     body.extend(chunk(b"INFO", &[2, 88, 3]));
//     let mut page1 = b"FORM".to_vec();
//     page1.extend_from_slice(&(body.len() as u32).to_be_bytes());
//     page1.extend_from_slice(&body);
//     let path = bundle(&[page0, page1]);
//     let r = inspect(&path, &Default::default());
//     assert!(r.errors.iter().any(|e| e.stage == "djvu"), "{:?}", r.errors);
//     assert!(
//         !r.errors.iter().any(|e| e.message.contains("out of scope")),
//         "{:?}",
//         r.errors
//     );
//     assert_eq!(r.pages.len(), 1, "{:?}", r.pages);
//     assert_eq!(r.pages[0].width, Some(600));
//     assert_eq!(r.counts.pages, Some(1));
//     // LITX-AUD-013 support boundary: the raw fallback rejects TXTz with the
//     // same static diagnostic instead of decoding it, and the incomplete
//     // fallback marks the text payload partial.
//     let error = r
//         .errors
//         .iter()
//         .find(|e| e.stage == "djvu_text")
//         .expect("static unsupported error");
//     assert!(error.message.contains("unsupported"), "{error:?}");
//     assert!(!r.text.content.contains("First page native text"));
//     assert!(r.text.truncated);
// }
//
// #[test]
// #[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
// fn djvu_native_decode_error_is_not_erased_by_blank_ocr() {
//     // LITX-AUD-013: a failed native text decode keeps text_complete false
//     // even when OCR runs and finds nothing on the blank page.
//     let path = document(&[chunk(b"TXTz", &[0, 0, 64, b'x'])]);
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             identify: true,
//             ..Default::default()
//         },
//     );
//     let analysis = r.djvu.as_ref().unwrap();
//     let error = r
//         .errors
//         .iter()
//         .find(|e| e.stage == "djvu_text")
//         .expect("static unsupported error");
//     assert!(error.message.contains("unsupported"), "{error:?}");
//     assert_eq!(analysis.pages[0].ocr_status, "no_text_detected");
//     assert!(!analysis.text_complete);
//     assert!(r.text.truncated);
// }
//
// #[test]
// #[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
// fn djvu_mixed_native_and_ocr_pages_report_mixed_provenance() {
//     // LITX-AUD-019: a document with embedded text on one page and OCR'd
//     // raster text on another is native_and_ocr, with mixed Markdown basis.
//     let native = document(&[chunk(
//         b"TXTa",
//         &text_payload("DjVu embedded native page words"),
//     )]);
//     let native_page = std::fs::read(&native).unwrap()[4..].to_vec();
//     let directory = native.parent().unwrap().to_path_buf();
//     let thumbnail = directory.join("mixed-ocr-source.png");
//     let pdf = inspect(
//         &fixture("native.pdf"),
//         &InspectOptions {
//             thumbnail: Some(thumbnail.clone()),
//             thumbnail_max_edge: 1200,
//             max_units: 0,
//             ..Default::default()
//         },
//     );
//     assert!(pdf.errors.is_empty(), "{:?}", pdf.errors);
//     let image = image::open(thumbnail).unwrap().to_rgb8();
//     let mut jpeg = std::io::Cursor::new(Vec::new());
//     image::DynamicImage::ImageRgb8(image.clone())
//         .write_to(&mut jpeg, image::ImageFormat::Jpeg)
//         .unwrap();
//     let scan = document(&[chunk(b"BGjp", jpeg.get_ref())]);
//     let mut bytes = std::fs::read(&scan).unwrap();
//     bytes[24..26].copy_from_slice(&(image.width() as u16).to_be_bytes());
//     bytes[26..28].copy_from_slice(&(image.height() as u16).to_be_bytes());
//     bytes[30..32].copy_from_slice(&150u16.to_le_bytes());
//     let scan_page = bytes[4..].to_vec();
//     let path = bundle(&[native_page, scan_page]);
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             identify: true,
//             max_pages: None,
//             ..Default::default()
//         },
//     );
//     assert!(r.errors.is_empty(), "{:?}", r.errors);
//     assert_eq!(r.text.origin, "native_and_ocr");
//     assert!(r.text.content.contains("DjVu embedded native page"));
//     assert!(r.text.content.contains("Native PDF"));
//     let analysis = r.djvu.as_ref().unwrap();
//     assert_eq!(analysis.pages[0].text_origin, "embedded_text");
//     assert_eq!(analysis.pages[0].ocr_status, "not_needed");
//     assert_eq!(analysis.pages[1].text_origin, "ocr");
//     assert_eq!(analysis.pages[1].ocr_status, "text_detected");
//     assert_eq!(r.markdown.basis, "mixed");
//     assert!(analysis.text_complete);
// }
//
// #[test]
// fn djvu_bundled_directory_tail_is_never_bzz_decoded() {
//     // LITX-AUD-013 / W3-DJVU-NOBZZ: the BZZ-compressed DIRM metadata tail
//     // (component kinds/ids/names) is never decoded. A valid tail that
//     // wrongly declares the second FORM:DJVU component a thumbnail must not
//     // drop the page: the raw offset table and FORM subtypes drive recovery.
//     let page_a = std::fs::read(document(&[chunk(
//         b"TXTa",
//         &text_payload("Tail-visible alpha"),
//     )]))
//     .unwrap()[4..]
//         .to_vec();
//     let page_b = std::fs::read(document(&[chunk(
//         b"TXTa",
//         &text_payload("Tail-visible beta"),
//     )]))
//     .unwrap()[4..]
//         .to_vec();
//     let tail = dir_tail(&[page_a.len(), page_b.len()], &[1, 2]);
//     let path = bundle_ex(&[page_a, page_b], Some(&tail), &[]);
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             max_pages: None,
//             ..Default::default()
//         },
//     );
//     assert!(r.errors.is_empty(), "{:?}", r.errors);
//     assert_eq!(r.counts.pages, Some(2), "{:?}", r.errors);
//     assert_eq!(r.pages.len(), 2, "{:?}", r.pages);
//     assert!(
//         r.text.content.contains("Tail-visible alpha"),
//         "{:?}",
//         r.text
//     );
//     assert!(r.text.content.contains("Tail-visible beta"), "{:?}", r.text);
//     let analysis = r.djvu.as_ref().unwrap();
//     assert!(analysis.text_complete, "{analysis:?}");
//     assert!(!r.text.truncated);
// }
// #[test]
// fn djvu_navm_is_never_bzz_decoded() {
//     // LITX-AUD-013 / W3-DJVU-NOBZZ: a NAVM chunk with a standin compressed
//     // payload must be skipped without invoking the BZZ decoder. Bookmarks
//     // are outside the inspection schema; raw text is still recovered.
//     let good = document(&[chunk(b"TXTa", &text_payload("Navigation-free raw text"))]);
//     let page = std::fs::read(good).unwrap()[4..].to_vec();
//     let path = bundle_ex(&[page], None, &[chunk(b"NAVM", b"standin compressed navm")]);
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             max_pages: None,
//             ..Default::default()
//         },
//     );
//     assert_eq!(r.counts.pages, Some(1), "{:?}", r.errors);
//     assert!(
//         r.text.content.contains("Navigation-free raw text"),
//         "{:?}",
//         r.errors
//     );
//     assert!(
//         !r.errors.iter().any(|e| e.message.contains("BZZ")),
//         "{:?}",
//         r.errors
//     );
// }
//
// #[test]
// fn djvu_fgbz_palette_index_is_rejected_before_any_bzz_decode() {
//     // LITX-AUD-013 / W3-DJVU-NOBZZ: an FGbz chunk whose version byte sets
//     // the index-table bit requires BZZ for rendering. OCR is skipped with a
//     // static unsupported diagnostic before any render allocation, render,
//     // or BZZ decode; the page is honestly incomplete.
//     let mut fgbz = vec![0x80, 0x00, 0x01]; // version bit 7, one palette color
//     fgbz.extend_from_slice(&[0, 0, 0]); // BGR triple
//     fgbz.extend_from_slice(&[0, 0, 1]); // index count = 1
//     fgbz.extend_from_slice(b"standin compressed index table");
//     let path = document(&[chunk(b"FGbz", &fgbz)]);
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             identify: true,
//             ..Default::default()
//         },
//     );
//     let error = r
//         .errors
//         .iter()
//         .find(|e| e.message.contains("unsupported"))
//         .expect("static unsupported error");
//     assert!(error.message.contains("BZZ"), "{error:?}");
//     assert!(
//         !r.errors.iter().any(|e| e.message.contains("BZZ error")),
//         "{:?}",
//         r.errors
//     );
//     let analysis = r.djvu.as_ref().unwrap();
//     assert!(analysis.pages[0].probe.is_none());
//     assert_eq!(analysis.pages[0].ocr_status, "unsupported");
//     assert!(!analysis.text_complete);
//     assert!(r.text.truncated);
// }
//
// #[test]
// fn djvu_bundled_zero_dimension_page_is_not_published() {
//     // W3-SYS-DJVU-003: a zero-dimension page in a bundled document is
//     // rejected before a page record is published; report.pages never
//     // carries 0x0 geometry.
//     let good = document(&[]);
//     let page0 = std::fs::read(good).unwrap()[4..].to_vec();
//     let zero = document(&[]);
//     let mut bytes = std::fs::read(zero).unwrap();
//     bytes[24..26].copy_from_slice(&0u16.to_be_bytes());
//     bytes[26..28].copy_from_slice(&0u16.to_be_bytes());
//     let page1 = bytes[4..].to_vec();
//     let path = bundle(&[page0, page1]);
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             max_pages: None,
//             ..Default::default()
//         },
//     );
//     assert!(
//         r.errors.iter().any(|e| e.message.contains("dimensions")),
//         "{:?}",
//         r.errors
//     );
//     assert!(
//         r.pages
//             .iter()
//             .all(|p| p.width != Some(0) && p.height != Some(0)),
//         "{:?}",
//         r.pages
//     );
//     assert_eq!(r.pages.len(), 1, "{:?}", r.pages);
// }
//
// #[test]
// fn djvu_shared_dictionary_is_not_counted_as_a_page() {
//     let r = inspect(&fixture("bundled.djvu"), &Default::default());
//     // W3-DJVU-NOBZZ: this old geometry fixture has no compressed DIRM type
//     // table. The raw offset-table walk recovers both pages' geometry without
//     // the dependency whole-document parser, whose DJVI-first assembly used
//     // to reject the file with a spurious "missing INFO" error. The readable
//     // geometry is preserved, now with no error at all.
//     assert!(r.errors.is_empty(), "{:?}", r.errors);
//     let v = serde_json::to_value(r).unwrap();
//     assert_eq!(v["counts"]["pages"], 2);
//     assert_eq!(v["pages"][0]["width"], 600);
//     assert_eq!(v["pages"][1]["height"], 1200);
//     assert_eq!(v["pages"][0]["dpi"], 300);
//     assert_eq!(v["pages"][0]["version"], "0.26");
//     let single = inspect(&fixture("single.djvu"), &Default::default());
//     assert_eq!(single.counts.pages, Some(1));
//     let indirect = inspect(&fixture("indirect.djvu"), &Default::default());
//     assert_eq!(indirect.counts.pages, None);
//     assert!(indirect.errors.iter().any(|e| e.message.contains("BZZ")));
//     let truncated = inspect(&fixture("truncated.djvu"), &Default::default());
//     assert!(truncated.errors.iter().any(|e| e.stage == "djvu"));
// }
//
// #[test]
// fn djvu_rejects_zero_dimensions_before_ocr_or_raster() {
//     // LITX-AUD-013: a zero-size page is rejected at the INFO preflight,
//     // before any raster allocation or OCR attempt. The OCR probe must
//     // never run for such a page.
//     let path = document(&[]);
//     let mut bytes = std::fs::read(&path).unwrap();
//     bytes[24..26].copy_from_slice(&0u16.to_be_bytes());
//     bytes[26..28].copy_from_slice(&0u16.to_be_bytes());
//     let path = path.with_file_name("zero-dim.djvu");
//     std::fs::write(&path, bytes).unwrap();
//     let r = inspect(
//         &path,
//         &InspectOptions {
//             identify: true,
//             ..Default::default()
//         },
//     );
//     assert!(
//         r.errors.iter().any(|e| e.message.contains("dimensions")),
//         "{:?}",
//         r.errors
//     );
//     // W3-SYS-DJVU-003: the invalid 0x0 page is never published.
//     assert!(r.pages.is_empty(), "{:?}", r.pages);
//     let analysis = r.djvu.as_ref().unwrap();
//     assert!(analysis.pages[0].probe.is_none());
//     assert_eq!(analysis.pages[0].ocr_status, "disabled");
//     assert!(!analysis.text_complete);
//     assert!(r.text.truncated);
// }
//
// #[test]
// fn djvu_raw_text_budget_covers_the_full_chunk_payload() {
//     // LITX-AUD-013: the entire raw TXTa chunk is billed against the decoded
//     // text budget, including trailing bytes beyond the LLM text slice. This
//     // payload's text slice is 22 bytes while the full chunk is one byte
//     // over the 64 MiB budget; it must fail closed. The oversized file also
//     // exceeds the structured decoder's file cap, so this exercises the raw
//     // fallback's accounting.
//     const BUDGET: usize = 64 * 1024 * 1024;
//     let words = text_payload("Raw text budget marker");
//     let mut payload = Vec::with_capacity(BUDGET + 1);
//     payload.extend_from_slice(&words);
//     payload.extend(std::iter::repeat_n(b' ', BUDGET + 1 - words.len()));
//     assert_eq!(payload.len(), BUDGET + 1);
//     let path = document(&[chunk(b"TXTa", &payload)]);
//     let r = inspect(&path, &Default::default());
//     assert!(
//         r.errors
//             .iter()
//             .any(|e| e.stage == "djvu_text" && e.message.contains("budget")),
//         "{:?}",
//         r.errors
//     );
//     assert!(!r.text.content.contains("Raw text budget marker"));
//     assert!(r.text.truncated);
// }
//
// #[test]
// fn djvu_prefers_raw_text_over_compressed_twin() {
//     // LITX-AUD-013: a page carrying both text chunks keeps the raw TXTa and
//     // never touches the unsupported TXTz.
//     let path = document(&[
//         chunk(
//             b"TXTz",
//             &djvu_rs::bzz_encode::bzz_encode(&text_payload("Compressed twin")),
//         ),
//         chunk(b"TXTa", &text_payload("Raw twin words")),
//     ]);
//     let r = inspect(&path, &Default::default());
//     assert!(r.errors.is_empty(), "{:?}", r.errors);
//     assert!(r.text.content.contains("Raw twin words"));
//     assert!(!r.text.content.contains("Compressed twin"));
// }
//
// #[test]
// fn djvu_fallback_prefers_raw_text_over_compressed_twin() {
//     // LITX-AUD-013: the raw fallback applies the same preference when the
//     // structured decoder rejects a later page.
//     let mut page0 = b"DJVU".to_vec();
//     page0.extend(chunk(b"INFO", &[2, 88, 3, 32, 26, 0, 44, 1, 22, 1]));
//     page0.extend(chunk(
//         b"TXTz",
//         &djvu_rs::bzz_encode::bzz_encode(&text_payload("Compressed twin")),
//     ));
//     page0.extend(chunk(b"TXTa", &text_payload("Raw twin words")));
//     let mut form0 = b"FORM".to_vec();
//     form0.extend_from_slice(&(page0.len() as u32).to_be_bytes());
//     form0.extend_from_slice(&page0);
//     let mut body1 = b"DJVU".to_vec();
//     body1.extend(chunk(b"INFO", &[2, 88, 3]));
//     let mut form1 = b"FORM".to_vec();
//     form1.extend_from_slice(&(body1.len() as u32).to_be_bytes());
//     form1.extend_from_slice(&body1);
//     let path = bundle(&[form0, form1]);
//     let r = inspect(&path, &Default::default());
//     assert!(r.text.content.contains("Raw twin words"), "{:?}", r.errors);
//     assert!(!r.text.content.contains("Compressed twin"));
//     assert!(r.text.truncated);
// }
// ----- END ORIGINAL extensionalities/crates/litx-inspect/tests/djvu.rs -----
// ----- ORIGIN extensionalities/crates/litx-inspect/tests/djvu_wave4.rs -----
// ----- ORIGINAL SHA256 9e9431786ab158b0230be53ffdb6c705e4646e1d4c70a5ad7972abba47e78924 -----
// ----- BEGIN ORIGINAL extensionalities/crates/litx-inspect/tests/djvu_wave4.rs -----
// //! Integration regressions for Wave-4 DjVu fallback state and raster-budget safety.
// use litx_inspect::{InspectOptions, inspect};
// use std::path::{Path, PathBuf};
//
// /// Return an IFF chunk with a big-endian length and required pad byte.
// ///
// /// Accepts the four-byte chunk identifier and its payload. Returns the complete
// /// encoded chunk. This helper panics only if the payload length exceeds `u32`.
// fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
//     let mut bytes = id.to_vec();
//     bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
//     bytes.extend_from_slice(payload);
//     if payload.len() % 2 == 1 {
//         bytes.push(0);
//     }
//     bytes
// }
//
// /// Return a ten-byte DjVu INFO payload at 300 DPI.
// ///
// /// Accepts the page width and height in pixels. Returns a version-26.0 INFO
// /// payload. Width and height are truncated to the format's sixteen-bit fields.
// fn info(width: u16, height: u16) -> Vec<u8> {
//     let mut payload = Vec::new();
//     payload.extend_from_slice(&width.to_be_bytes());
//     payload.extend_from_slice(&height.to_be_bytes());
//     payload.extend_from_slice(&[26, 0, 44, 1, 22, 1]);
//     payload
// }
//
// /// Encode a TXTa payload containing only text and its terminator.
// ///
// /// Accepts the plain text to embed. Returns the three-byte-length-prefixed
// /// payload. Text longer than the twenty-four-bit format limit is truncated by
// /// the cast and is not used by these fixtures.
// fn text_payload(text: &str) -> Vec<u8> {
//     let mut bytes = (text.len() as u32).to_be_bytes()[1..].to_vec();
//     bytes.extend_from_slice(text.as_bytes());
//     bytes.push(0);
//     bytes
// }
//
// /// Build one `FORM:DJVU` page without the outer `AT&T` signature.
// ///
// /// Accepts an optional INFO payload and subsequent encoded chunks. Returns the
// /// complete page FORM. A missing INFO is intentional for malformed fixtures.
// fn page_form(info_payload: Option<&[u8]>, chunks: &[Vec<u8>]) -> Vec<u8> {
//     let mut body = b"DJVU".to_vec();
//     if let Some(payload) = info_payload {
//         body.extend(chunk(b"INFO", payload));
//     }
//     for item in chunks {
//         body.extend_from_slice(item);
//     }
//     chunk(b"FORM", &body)
// }
//
// /// Return the dedicated Wave-4 fixture directory, creating it when necessary.
// ///
// /// Accepts no inputs. Returns the absolute fixture path. Panics if the evidence
// /// directory cannot be created.
// fn fixture_directory() -> PathBuf {
//     Path::new(env!("CARGO_MANIFEST_DIR"))
//         .join("../../target/verification/wave4-remediation/djvu/fixtures")
// }
//
// /// Write a standalone DjVu fixture into the controller's evidence directory.
// ///
// /// Accepts a filename and complete page FORM. Returns the written path. Panics
// /// if the evidence directory or fixture cannot be written.
// fn standalone(name: &str, page: &[u8]) -> PathBuf {
//     let mut bytes = b"AT&T".to_vec();
//     bytes.extend_from_slice(page);
//     write_fixture(name, &bytes)
// }
//
// /// Write an MMR correction fixture without altering earlier evidence.
// ///
// /// Accepts a filename and complete page FORM. Returns the new correction
// /// fixture path. Panics if directory creation or the file write fails.
// fn mmr_standalone(name: &str, page: &[u8]) -> PathBuf {
//     let directory = fixture_directory()
//         .parent()
//         .unwrap()
//         .join("mmr-correction")
//         .join("fixtures");
//     std::fs::create_dir_all(&directory).unwrap();
//     let path = directory.join(name);
//     let mut bytes = b"AT&T".to_vec();
//     bytes.extend_from_slice(page);
//     std::fs::write(&path, bytes).unwrap();
//     path
// }
//
// /// Write a bundled DjVu fixture with a raw, uncompressed-free offset table.
// ///
// /// Accepts a filename and page FORMs in document order. Returns the written
// /// path. Panics for too many pages or filesystem failures; test fixtures stay
// /// far below those limits.
// fn bundle(name: &str, pages: &[Vec<u8>]) -> PathBuf {
//     let directory_size = 3 + 4 * pages.len();
//     let mut offset = 16 + 8 + directory_size + directory_size % 2;
//     let mut directory = vec![0x81];
//     directory.extend_from_slice(&(pages.len() as u16).to_be_bytes());
//     for page in pages {
//         directory.extend_from_slice(&(offset as u32).to_be_bytes());
//         offset += page.len();
//     }
//     let mut body = b"DJVM".to_vec();
//     body.extend(chunk(b"DIRM", &directory));
//     for page in pages {
//         body.extend_from_slice(page);
//     }
//     let mut bytes = b"AT&T".to_vec();
//     bytes.extend(chunk(b"FORM", &body));
//     write_fixture(name, &bytes)
// }
//
// /// Write fixture bytes under the authorized evidence directory.
// ///
// /// Accepts a filename and complete file bytes. Returns the absolute path.
// /// Panics if directory creation or the file write fails.
// fn write_fixture(name: &str, bytes: &[u8]) -> PathBuf {
//     let directory = fixture_directory();
//     std::fs::create_dir_all(&directory).unwrap();
//     let path = directory.join(name);
//     std::fs::write(&path, bytes).unwrap();
//     path
// }
//
// /// Assert that each invalid later INFO leaves one honest copy of page one.
// ///
// /// Accepts no inputs and returns `()`. Panics when fallback duplicates any
// /// extraction-owned output, retains stale analysis, or fails to mark both text
// /// and Markdown partial.
// #[test]
// fn fallback_restores_output_for_every_invalid_info_variant() {
//     let first = page_form(
//         Some(&info(600, 800)),
//         &[chunk(b"TXTa", &text_payload("Hello page one"))],
//     );
//     let variants = [
//         ("missing-info", page_form(None, &[])),
//         ("short-info", page_form(Some(&[2, 88, 3]), &[])),
//         (
//             "version-99-26",
//             page_form(Some(&[2, 88, 3, 32, 26, 99, 44, 1, 22, 1]), &[]),
//         ),
//     ];
//     for (name, invalid) in variants {
//         let path = bundle(&format!("fallback-{name}.djvu"), &[first.clone(), invalid]);
//         let report = inspect(
//             &path,
//             &InspectOptions {
//                 max_pages: None,
//                 ..Default::default()
//             },
//         );
//         assert_eq!(
//             report.text.content, "Hello page one\n\n",
//             "{name}: {:?}",
//             report.errors
//         );
//         assert_eq!(report.counts.chars, 16, "{name}");
//         assert_eq!(report.text.chars, 16, "{name}");
//         assert_eq!(
//             report.markdown.content.matches("## Page 1").count(),
//             1,
//             "{name}"
//         );
//         assert_eq!(
//             report.markdown.content.matches("Hello page one").count(),
//             1,
//             "{name}"
//         );
//         assert_eq!(
//             report
//                 .claims
//                 .iter()
//                 .filter(|claim| claim.key == "directory_components")
//                 .count(),
//             1,
//             "{name}"
//         );
//         assert_eq!(report.pages.len(), 1, "{name}: {:?}", report.pages);
//         assert_eq!(report.counts.pages, Some(1), "{name}");
//         assert!(report.djvu.is_none(), "{name}: stale structured analysis");
//         assert!(report.text.truncated, "{name}: text was marked complete");
//         assert!(
//             report.markdown.truncated,
//             "{name}: Markdown was marked complete"
//         );
//         assert!(
//             report.errors.iter().any(|error| error.stage == "djvu"),
//             "{name}: missing structural error: {:?}",
//             report.errors
//         );
//     }
// }
//
// /// Assert that counts follow published geometry rather than raw INFO records.
// ///
// /// Accepts no inputs and returns `()`. Panics if zero-dimension pages inflate
// /// counts or `max_pages` incorrectly caps the published geometry count.
// #[test]
// fn counts_reconcile_with_published_pages_and_preserve_page_limits() {
//     let valid = page_form(Some(&info(600, 800)), &[]);
//     let zero = page_form(Some(&info(0, 0)), &[]);
//     let single = standalone("counts-zero-standalone.djvu", &zero);
//     let report = inspect(&single, &Default::default());
//     assert!(report.pages.is_empty());
//     assert_eq!(report.counts.pages, Some(0));
//
//     let bundled = bundle("counts-zero-bundle.djvu", &[valid.clone(), zero]);
//     let report = inspect(
//         &bundled,
//         &InspectOptions {
//             max_pages: None,
//             ..Default::default()
//         },
//     );
//     assert_eq!(report.pages.len(), 1);
//     assert_eq!(report.counts.pages, Some(1));
//
//     let limited = bundle("counts-max-pages.djvu", &[valid.clone(), valid]);
//     let report = inspect(
//         &limited,
//         &InspectOptions {
//             max_pages: Some(1),
//             ..Default::default()
//         },
//     );
//     assert_eq!(report.pages.len(), 2);
//     assert_eq!(report.counts.pages, Some(2));
//     assert_eq!(report.djvu.unwrap().pages[1].ocr_status, "page_limit");
// }
//
// /// Encode a solid-white RGB JPEG without invoking a DjVu decoder.
// ///
// /// Accepts image width and height. Returns a valid JPEG payload. Panics if the
// /// test image allocation or JPEG encoding fails.
// fn white_jpeg(width: u32, height: u32) -> Vec<u8> {
//     let image = image::RgbImage::from_pixel(width, height, image::Rgb([255; 3]));
//     let mut encoded = std::io::Cursor::new(Vec::new());
//     image::DynamicImage::ImageRgb8(image)
//         .write_to(&mut encoded, image::ImageFormat::Jpeg)
//         .unwrap();
//     encoded.into_inner()
// }
//
// /// Assert that oversized JPEG layers fail before their decoder or OCR runs.
// ///
// /// Accepts no inputs and returns `()`. Panics if INFO geometry can bypass the
// /// decoded-layer budget. The valid 4000x4000 JPEG fixtures remain in the
// /// controller evidence directory for fresh-process RSS verification.
// #[test]
// fn info_one_pixel_cannot_bypass_jpeg_layer_budget() {
//     let jpeg = white_jpeg(4000, 4000);
//     for (name, layer) in [("BGjp", b"BGjp"), ("FGjp", b"FGjp")] {
//         let page = page_form(Some(&info(1, 1)), &[chunk(layer, &jpeg)]);
//         let path = standalone(
//             &format!("info-1x1-valid-{name}-4000x4000.djvu").to_lowercase(),
//             &page,
//         );
//         let report = inspect(
//             &path,
//             &InspectOptions {
//                 identify: true,
//                 max_pages: None,
//                 ..Default::default()
//             },
//         );
//         let analysis = report.djvu.as_ref().unwrap();
//         let signal = &analysis.pages[0];
//         assert_eq!(signal.ocr_status, "budget", "{name}: {analysis:?}");
//         assert!(signal.probe.is_none(), "{name}: OCR ran before rejection");
//         assert!(
//             report
//                 .errors
//                 .iter()
//                 .any(|error| error.stage == "djvu_text" && error.message.contains("budget")),
//             "{name}: missing budget error: {:?}",
//             report.errors
//         );
//         assert!(
//             !report.errors.iter().any(|error| error.stage == "djvu_ocr"),
//             "{name}: decoder or OCR ran before rejection: {:?}",
//             report.errors
//         );
//         assert!(
//             report.text.truncated,
//             "{name}: incomplete text not disclosed"
//         );
//     }
// }
//
// /// Assert that both Smmr header forms fail before large MMR row allocation.
// ///
// /// Accepts no inputs and returns `()`. Panics if legacy or MMR-prefixed
// /// 8192x8192 dimensions bypass the cumulative budget or reach OCR. Header-only
// /// fixtures must be rejected before their large row vectors can be allocated.
// #[test]
// fn info_one_pixel_cannot_bypass_smmr_dimension_budget() {
//     let variants = [
//         ("legacy", vec![0x20, 0x00, 0x20, 0x00]),
//         ("mmr", vec![b'M', b'M', b'R', 0, 0x20, 0x00, 0x20, 0x00]),
//     ];
//     let mut failures = Vec::new();
//     for (name, payload) in variants {
//         let page = page_form(Some(&info(1, 1)), &[chunk(b"Smmr", &payload)]);
//         let path = mmr_standalone(&format!("info-1x1-smmr-8192x8192-{name}.djvu"), &page);
//         let report = inspect(
//             &path,
//             &InspectOptions {
//                 identify: true,
//                 max_pages: None,
//                 ..Default::default()
//             },
//         );
//         let analysis = report.djvu.as_ref().unwrap();
//         if analysis.pages[0].ocr_status != "budget" {
//             failures.push(format!("{name}: unexpected status {analysis:?}"));
//         }
//         if analysis.pages[0].probe.is_some() {
//             failures.push(format!("{name}: OCR probe ran after a budget bypass"));
//         }
//         if !report
//             .errors
//             .iter()
//             .any(|error| error.stage == "djvu_text" && error.message.contains("budget"))
//         {
//             failures.push(format!("{name}: missing budget error {:?}", report.errors));
//         }
//         if report.errors.iter().any(|error| error.stage == "djvu_ocr") {
//             failures.push(format!("{name}: OCR error {:?}", report.errors));
//         }
//     }
//     assert!(failures.is_empty(), "{failures:#?}");
// }
//
// /// Assert that ordinary raw native text remains complete and unduplicated.
// ///
// /// Accepts no inputs and returns `()`. Panics on any regression in the normal
// /// standalone path. The baseline fixture remains in the evidence directory.
// #[test]
// fn baseline_raw_text_is_preserved() {
//     let page = page_form(
//         Some(&info(600, 800)),
//         &[chunk(b"TXTa", &text_payload("Baseline native words"))],
//     );
//     let path = standalone("baseline-raw-text.djvu", &page);
//     let report = inspect(&path, &Default::default());
//     assert!(report.errors.is_empty(), "{:?}", report.errors);
//     assert_eq!(report.text.content, "Baseline native words\n\n");
//     assert_eq!(report.markdown.content.matches("## Page 1").count(), 1);
//     assert_eq!(report.counts.pages, Some(1));
//     assert!(!report.text.truncated);
//     assert!(!report.markdown.truncated);
//     assert!(report.djvu.unwrap().text_complete);
// }
//
// /// Assert that small valid image layers still reach OCR without BZZ decoding.
// ///
// /// Accepts no inputs and returns `()`. Panics on unexpected render or OCR
// /// errors, a missing probe, or false incompleteness. Requires local English
// /// Tesseract data and leaves all three baseline layer fixtures for the controller.
// #[test]
// #[ignore = "requires local eng.traineddata, set TESSDATA_PREFIX"]
// fn baseline_valid_layers_reach_ocr() {
//     let jpeg = white_jpeg(16, 16);
//     let variants = [
//         ("bgjp", chunk(b"BGjp", &jpeg)),
//         ("fgjp", chunk(b"FGjp", &jpeg)),
//         ("smmr", chunk(b"Smmr", &[0, 16, 0, 16])),
//     ];
//     for (name, layer) in variants {
//         let page = page_form(Some(&info(1, 1)), &[layer]);
//         let path = standalone(&format!("baseline-valid-{name}-16x16.djvu"), &page);
//         let report = inspect(
//             &path,
//             &InspectOptions {
//                 identify: true,
//                 max_pages: None,
//                 ..Default::default()
//             },
//         );
//         assert!(report.errors.is_empty(), "{name}: {:?}", report.errors);
//         let analysis = report.djvu.unwrap();
//         assert!(analysis.text_complete, "{name}: {analysis:?}");
//         assert_eq!(analysis.pages[0].ocr_status, "no_text_detected");
//         assert!(analysis.pages[0].probe.is_some());
//     }
// }
// ----- END ORIGINAL extensionalities/crates/litx-inspect/tests/djvu_wave4.rs -----
// ----- HISTORICAL CLI DJVU OCR EXCERPT ONLY; PDF ASSERTIONS REMAIN ACTIVE IN inspect_cli.rs -----
// ----- ORIGIN extensionalities/crates/litx/tests/inspect_cli.rs; WHOLE-FILE SHA256 36b26495efe85bd8e9def56668b2e60c7ca9096eaf5a438c8e650faa27b9261a -----
// ----- BEGIN HISTORICAL CLI EXCERPT -----
// #[test]
// fn missing_ocr_model_marks_pdf_and_djvu_extraction_incomplete() {
//     // LITX-AUD-010 + LITX-AUD-019: a failed OCR attempt must surface as
//     // incomplete text, never as a clean complete report. The per-process
//     // TESSDATA_PREFIX keeps this isolated from parallel OCR tests.
//     let missing = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
//         .join("../../target/tests")
//         .join(format!(
//             "missing-tessdata-prov-{}",
//             std::time::SystemTime::now()
//                 .duration_since(std::time::UNIX_EPOCH)
//                 .unwrap()
//                 .as_nanos()
//         ));
//     assert!(!missing.exists());
//     // native.pdf stays within the five-page scan limit, so incomplete flags
//     // can only come from the failed OCR attempt itself.
//     let output = Command::new(env!("CARGO_BIN_EXE_litx"))
//         .args(["inspect", "--identify", "../../fixtures/native.pdf"])
//         .env("TESSDATA_PREFIX", &missing)
//         .current_dir(env!("CARGO_MANIFEST_DIR"))
//         .output()
//         .unwrap();
//     assert!(
//         output.status.success(),
//         "{}",
//         String::from_utf8_lossy(&output.stderr)
//     );
//     let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
//     assert!(
//         r["errors"]
//             .as_array()
//             .unwrap()
//             .iter()
//             .any(|e| e["stage"] == "pdf_ocr"),
//         "{:?}",
//         r["errors"]
//     );
//     assert_eq!(r["pdf"]["text_complete"], false);
//     assert_eq!(r["text"]["truncated"], true);
//     assert!(
//         r["pdf"]["identification_pages"]
//             .as_array()
//             .unwrap()
//             .iter()
//             .any(|p| p["ocr_status"] == "failed")
//     );
//     // DjVu: a blank single-page document whose only text path is OCR.
//     let info: &[u8] = &[2, 88, 3, 32, 26, 0, 44, 1, 22, 1];
//     let mut page = b"DJVU".to_vec();
//     page.extend_from_slice(b"INFO");
//     page.extend_from_slice(&(info.len() as u32).to_be_bytes());
//     page.extend_from_slice(info);
//     let mut form = b"FORM".to_vec();
//     form.extend_from_slice(&(page.len() as u32).to_be_bytes());
//     form.extend_from_slice(&page);
//     let mut bytes = b"AT&T".to_vec();
//     bytes.extend_from_slice(&form);
//     let djvu = missing.with_file_name(format!(
//         "blank-{}.djvu",
//         std::time::SystemTime::now()
//             .duration_since(std::time::UNIX_EPOCH)
//             .unwrap()
//             .as_nanos()
//     ));
//     std::fs::write(&djvu, bytes).unwrap();
//     let output = Command::new(env!("CARGO_BIN_EXE_litx"))
//         .args(["inspect", "--identify"])
//         .arg(&djvu)
//         .env("TESSDATA_PREFIX", &missing)
//         .current_dir(env!("CARGO_MANIFEST_DIR"))
//         .output()
//         .unwrap();
//     assert!(
//         output.status.success(),
//         "{}",
//         String::from_utf8_lossy(&output.stderr)
//     );
//     let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
//     assert!(
//         r["errors"]
//             .as_array()
//             .unwrap()
//             .iter()
//             .any(|e| e["stage"] == "djvu_ocr"),
//         "{:?}",
//         r["errors"]
//     );
//     assert_eq!(r["djvu"]["text_complete"], false);
//     assert_eq!(r["text"]["truncated"], true);
//     assert!(!missing.exists());
// }
//
// ----- END HISTORICAL CLI EXCERPT -----
