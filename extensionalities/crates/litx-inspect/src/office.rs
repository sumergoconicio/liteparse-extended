use crate::{
    Claim, InspectOptions, InspectReport, Preflight,
    archive::Archive,
    detect,
    xml::{self, Node},
};

pub(crate) const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(crate) const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub(crate) const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
pub(crate) const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
pub(crate) const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const DC: &str = "http://purl.org/dc/elements/1.1/";

pub(crate) fn read_xml(archive: &mut Archive, name: &str) -> xml::Result<Node> {
    xml::parse(
        &archive
            .read(name)?
            .ok_or_else(|| format!("missing package part {name}"))?,
    )
}

pub(crate) fn preflight(
    report: &mut InspectReport,
    kind: &'static str,
    index: usize,
    chars: usize,
    images: bool,
    options: &InspectOptions,
) {
    if index >= options.max_units {
        return;
    }
    let mut reasons = vec![];
    if chars < 20 {
        reasons.push("text_length_below_20".into());
    }
    if images {
        reasons.push("has_images".into());
    }
    report.preflight.push(Preflight {
        page_number: index + 1,
        unit_kind: kind,
        needs_ocr: !reasons.is_empty(),
        reasons,
        confidence: "deterministic",
        coverage_available: false,
    });
}

pub(crate) fn identify(report: &mut InspectReport, ext: &str, mime: &str, name: &str) {
    report.format.detected_mime = Some(mime.into());
    report.format.detected_name = Some(name.into());
    report.format.bucket = detect::bucket(ext).into();
    report.format.basis = "bytes".into();
    report.format.disagreement =
        !report.format.extension.is_empty() && report.format.extension != ext;
    report.format.detector = "file-format+package-xml".into();
}

pub(crate) fn metadata(archive: &mut Archive, report: &mut InspectReport) {
    for name in ["docProps/core.xml", "docProps/app.xml", "meta.xml"] {
        if !archive.names.iter().any(|n| n == name) {
            continue;
        }
        match read_xml(archive, name) {
            Ok(root) => {
                let expected = match name {
                    "docProps/core.xml" => (
                        "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
                        "coreProperties",
                    ),
                    "docProps/app.xml" => (
                        "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
                        "Properties",
                    ),
                    _ => (crate::odf::O, "document-meta"),
                };
                if !root.is(expected.0, expected.1) {
                    report.error("metadata", format!("{name}: namespace/root mismatch"));
                    continue;
                }
                root.visit(&mut |node| {
                    if ![
                        expected.0,
                        DC,
                        "http://purl.org/dc/terms/",
                        "urn:oasis:names:tc:opendocument:xmlns:meta:1.0",
                    ]
                    .contains(&node.ns.as_str())
                    {
                        return;
                    }
                    if node.name == "language" && node.ns != DC {
                        return;
                    }
                    if node.children().next().is_some() {
                        return;
                    }
                    let value = node.text();
                    if value.trim().is_empty() {
                        return;
                    }
                    let key = if node.ns == DC && node.name == "creator" {
                        "author"
                    } else {
                        node.name.as_str()
                    };
                    report.claims.push(Claim {
                        key: key.into(),
                        value,
                        source: "container_metadata",
                    });
                });
            }
            Err(e) => report.error("metadata", format!("{name}: {e}")),
        }
    }
}

pub(crate) fn inspect(archive: &mut Archive, report: &mut InspectReport, options: &InspectOptions) {
    let result = inspect_inner(archive, report, options);
    if let Err(error) = result {
        report.error("office", error);
    }
}

fn inspect_inner(
    archive: &mut Archive,
    report: &mut InspectReport,
    options: &InspectOptions,
) -> xml::Result<()> {
    if archive.names.iter().any(|n| n == "mimetype") {
        crate::odf::inspect(archive, report, options)?;
    }
    if !archive.names.iter().any(|n| n == "[Content_Types].xml") {
        return Ok(());
    }
    let types = read_xml(archive, "[Content_Types].xml")?;
    if !types.is(CT, "Types") {
        return Err("invalid OOXML content types root".into());
    }
    let mut main = None;
    for node in types.children().filter(|n| n.is(CT, "Override")) {
        let content_type = node.attr("ContentType").unwrap_or("");
        let kind = match content_type {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml" => {
                Some(("docx", W, "document"))
            }
            "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml" => {
                Some(("pptx", P, "presentation"))
            }
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml" => {
                Some(("xlsx", S, "workbook"))
            }
            "application/vnd.ms-excel.sheet.macroEnabled.main+xml" => Some(("xlsm", S, "workbook")),
            _ => None,
        };
        if let Some(kind) = kind {
            if main.is_some() {
                return Err("ambiguous OOXML main parts".into());
            }
            main = Some((
                kind,
                node.attr("PartName")
                    .ok_or("main part has no name")?
                    .trim_start_matches('/')
                    .to_string(),
            ));
        }
    }
    let Some(((ext, ns, root_name), part)) = main else {
        return Ok(());
    };
    if part.split('/').any(|p| matches!(p, ".." | "." | "")) || part.contains('\\') {
        return Err("unsafe OOXML main part name".into());
    }
    let mut root = read_xml(archive, &part)?;
    if !root.is(ns, root_name) {
        return Err("OOXML main part namespace/root mismatch".into());
    }
    match ext {
        "docx" => {
            identify(
                report,
                ext,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "Word document",
            );
            metadata(archive, report);
            let paragraphs = root.descendants(W, "p").len();
            let partial = limit_word_sections(&mut root, options.text_unit_limit());
            docx(&root, report, options);
            crate::tables::Context::new(report, &part, "section", 1, options).word_tables(&root);
            crate::markdown::word(&root, report, options);
            report.counts.paragraphs = Some(paragraphs);
            report.text.truncated |= partial;
        }
        "pptx" => {
            identify(
                report,
                ext,
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                "PowerPoint presentation",
            );
            metadata(archive, report);
            pptx(archive, &root, &part, report, options)?;
        }
        "xlsx" | "xlsm" => {
            identify(
                report,
                ext,
                if ext == "xlsx" {
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                } else {
                    "application/vnd.ms-excel.sheet.macroEnabled.12"
                },
                "Excel workbook",
            );
            metadata(archive, report);
            crate::spreadsheet::xlsx(archive, &root, &part, report, options)?;
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn resolve_part(base: &str, target: &str) -> xml::Result<String> {
    if target.contains(['\\', ':', '?', '#', '\0']) {
        return Err("external or invalid package relationship target".into());
    }
    let mut components: Vec<&str> = if target.starts_with('/') {
        vec![]
    } else {
        base.rsplit_once('/')
            .map(|(dir, _)| dir.split('/').collect())
            .unwrap_or_default()
    };
    for component in target.trim_start_matches('/').split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components
                    .pop()
                    .ok_or("relationship escapes package root")?;
            }
            _ => components.push(component),
        }
    }
    if components.is_empty() {
        return Err("empty relationship target".into());
    }
    Ok(components.join("/"))
}

pub(crate) fn relationships(archive: &mut Archive, part: &str) -> xml::Result<Node> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let name = if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    };
    let root = read_xml(archive, &name)?;
    if !root.is(PKG, "Relationships") {
        return Err("invalid package relationships root".into());
    }
    Ok(root)
}

pub(crate) fn related(rels: &Node, part: &str, id: &str, kind: &str) -> xml::Result<String> {
    let found: Vec<_> = rels
        .children()
        .filter(|n| n.is(PKG, "Relationship") && n.attr("Id") == Some(id))
        .collect();
    if found.len() != 1 {
        return Err("missing or duplicate relationship id".into());
    }
    let rel = found[0];
    if rel.attr("TargetMode").is_some_and(|m| m != "Internal")
        || rel.attr("Type") != Some(&format!("{R}/{kind}"))
    {
        return Err("external or mismatched package relationship".into());
    }
    resolve_part(
        part,
        rel.attr("Target").ok_or("relationship missing target")?,
    )
}

fn pptx(
    archive: &mut Archive,
    root: &Node,
    part: &str,
    report: &mut InspectReport,
    options: &InspectOptions,
) -> xml::Result<()> {
    let slides = root.descendants(P, "sldId");
    report.counts.slides = Some(slides.len());
    let rels = relationships(archive, part)?;
    for (i, slide) in slides.iter().take(options.text_unit_limit()).enumerate() {
        let result = (|| -> xml::Result<()> {
            let target = related(
                &rels,
                part,
                slide
                    .attr_ns(R, "id")
                    .ok_or("slide has no relationship id")?,
                "slide",
            )?;
            let slide = read_xml(archive, &target)?;
            if !slide.is(P, "sld") {
                return Err("invalid slide namespace/root".into());
            }
            let table_start = report.tables.len();
            crate::tables::Context::new(report, &target, "slide", i + 1, options)
                .drawing_tables(&slide);
            crate::markdown::append(
                report,
                &format!("## Slide {}\n\n", i + 1),
                options,
                "native_structure",
            );
            crate::markdown::drawing(&slide, report, options, table_start);
            let mut chars = 0;
            for paragraph in slide.descendants(A, "p") {
                let mut text = String::new();
                paragraph.visit(&mut |n| {
                    if n.is(A, "t") {
                        text.push_str(&n.text());
                    }
                    if n.is(A, "br") {
                        text.push('\n');
                    }
                });
                chars += text.chars().count();
                report.append_text(&text, options.max_chars);
                report.append_text("\n", options.max_chars);
            }
            preflight(
                report,
                "slide",
                i,
                chars,
                !slide.descendants(A, "blip").is_empty() || !slide.descendants(P, "pic").is_empty(),
                options,
            );
            Ok(())
        })();
        if let Err(e) = result {
            report.error("slide", format!("slide {}: {e}", i + 1));
        }
    }
    report.text.truncated |= slides.len() > options.text_unit_limit();
    Ok(())
}

fn limit_word_sections(root: &mut Node, limit: usize) -> bool {
    use crate::xml::Content;
    if limit == usize::MAX {
        return false;
    }
    let Some(body) = root.content.iter_mut().find_map(|item| match item {
        Content::Element(node) if node.is(W, "body") => Some(node),
        _ => None,
    }) else {
        return false;
    };
    let mut sections = 0;
    let mut partial = false;
    body.content.retain(|item| {
        if sections >= limit {
            partial |= match item {
                Content::Element(node) => !node.is(W, "sectPr"),
                Content::Text(text) => !text.trim().is_empty(),
            };
            return false;
        }
        if let Content::Element(node) = item {
            sections += node.descendants(W, "sectPr").len();
        }
        true
    });
    partial
}

fn docx(root: &Node, report: &mut InspectReport, options: &InspectOptions) {
    let paragraphs = root.descendants(W, "p");
    report.counts.paragraphs = Some(paragraphs.len());
    let (mut chars, mut images, mut unit) = (0, false, 0);
    for paragraph in paragraphs {
        let mut value = String::new();
        paragraph.visit(&mut |n| {
            if n.is(W, "t") {
                value.push_str(&n.text());
            }
            if n.is(W, "tab") {
                value.push('\t');
            }
            if n.is(W, "br") || n.is(W, "cr") {
                value.push('\n');
            }
        });
        chars += value.chars().count();
        images |= !paragraph.descendants(A, "blip").is_empty()
            || !paragraph.descendants(W, "pict").is_empty()
            || !paragraph.descendants(W, "drawing").is_empty();
        report.append_text(&value, options.max_chars);
        report.append_text("\n", options.max_chars);
        if !paragraph.descendants(W, "sectPr").is_empty() {
            preflight(report, "section", unit, chars, images, options);
            unit += 1;
            chars = 0;
            images = false;
        }
    }
    if chars > 0 || images || unit == 0 {
        preflight(report, "section", unit, chars, images, options);
    }
}

pub(crate) fn language(report: &InspectReport) -> &'static str {
    let mut language = None;
    for claim in report
        .claims
        .iter()
        .filter(|c| c.key == "language" && c.source == "container_metadata")
    {
        let code = claim.value.to_ascii_lowercase();
        let code = code.split(['-', '_']).next().unwrap_or("");
        let bucket = match code {
            "en" | "eng" => "English",
            "zh" | "zho" | "chi" | "yue" => "Chinese",
            "fr" | "fra" | "fre" => "French",
            "de" | "deu" | "ger" => "German",
            "es" | "spa" => "Spanish",
            "nl" | "it" | "ru" | "pl" | "el" | "sv" | "da" | "no" | "fi" | "hu" | "pt" | "uk"
            | "cs" | "ro" | "bg" | "hr" | "sk" | "sl" | "et" | "lv" | "lt" | "is" | "ga" | "cy" => {
                "Other European"
            }
            "ar" | "he" | "fa" | "tr" | "ara" | "heb" | "fas" | "tur" => "Middle Eastern",
            "hi" | "ur" | "bn" | "ta" | "te" => "South Asian",
            "ja" | "ko" | "vi" | "id" | "tl" | "fil" | "jpn" | "kor" => "East & SE Asian",
            "sw" | "yo" | "qu" | "eu" | "ht" => "Other / Indigenous",
            _ => "N/A",
        };
        if language.is_some_and(|l| l != bucket) {
            return "N/A";
        }
        language = Some(bucket);
    }
    language.unwrap_or("N/A")
}
