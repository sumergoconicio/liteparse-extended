//! OpenDocument inspection for the `litx-inspect` crate.
//! File: `odf.rs`.
//! Responsibility: extract bounded ODF text, slides, and spreadsheet grids
//! with cumulative expansion and workbook work budgets.
use crate::{
    InspectOptions, InspectReport,
    archive::Archive,
    office,
    profile::WorkBudget,
    xml::{self, Content, Node},
};

pub(crate) const O: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
pub(crate) const T: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
pub(crate) const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";

pub(crate) fn text(node: &Node) -> xml::Result<String> {
    text_capped(node, 16 << 20, "ODF expanded text cap exceeded")
}

fn text_capped(node: &Node, cap: usize, label: &'static str) -> xml::Result<String> {
    fn append(node: &Node, out: &mut String, cap: usize, label: &'static str) -> xml::Result<()> {
        if node.is(T, "s") {
            let count: usize = node.attr_ns(T, "c").unwrap_or("1").parse()?;
            if count > 16_384 {
                return Err("ODF explicit-space cap exceeded".into());
            }
            out.extend(std::iter::repeat_n(' ', count));
        } else if node.is(T, "tab") {
            out.push('\t');
        } else if node.is(T, "line-break") {
            out.push('\n');
        } else {
            for item in &node.content {
                match item {
                    Content::Text(text) => out.push_str(text),
                    Content::Element(child) => append(child, out, cap, label)?,
                }
            }
        }
        if out.len() > cap {
            return Err(label.into());
        }
        Ok(())
    }
    let mut out = String::new();
    append(node, &mut out, cap, label)?;
    Ok(out)
}

/// Cumulative expanded-value byte cap across an ODF document.
pub(crate) const ODF_EXPANDED_CAP: usize = 256 << 20;

/// Shared document-wide budget for expanded ODF text (`<text:s>` and paragraph
/// joins) across all document-table cells, tables, and prose paragraphs.
/// Charged with full expanded bytes before any retention decision; the per-node
/// 16 MiB cap still applies inside the remaining budget.
pub(crate) struct ExpansionBudget {
    remaining: usize,
}

impl ExpansionBudget {
    pub(crate) fn new(cap: usize) -> Self {
        Self { remaining: cap }
    }

    /// Expand one text node within the remaining document budget and charge it.
    pub(crate) fn text(&mut self, node: &Node) -> xml::Result<String> {
        let (cap, label) = if self.remaining < 16 << 20 {
            (self.remaining, "ODF document expansion budget exceeded")
        } else {
            (16 << 20, "ODF expanded text cap exceeded")
        };
        let out = text_capped(node, cap, label)?;
        self.remaining -= out.len();
        Ok(out)
    }
}

/// Joins ODF paragraphs within both expanded-value and workbook work budgets.
/// Accepts paragraph nodes, previously expanded bytes, logical repetition
/// weight, the 256 MiB expanded cap, and the shared workbook budget; returns
/// the joined source-cell value after charging its full logical bytes; rejects
/// malformed space counts, arithmetic overflow, either expanded-value cap, or
/// cumulative value-byte exhaustion without expanding into the next budget.
fn cell_text(
    paragraphs: &[&Node],
    expanded: usize,
    weight: usize,
    cap: usize,
    work: &mut WorkBudget,
) -> xml::Result<String> {
    let odf_value_cap = cap.saturating_sub(expanded) / weight;
    let work_value_cap = work.value_bytes_remaining() / weight;
    let work_is_limit = work_value_cap <= odf_value_cap;
    let value_cap = work_value_cap.min(odf_value_cap);
    let mut value = String::new();
    for (i, p) in paragraphs.iter().enumerate() {
        if i > 0 {
            if value.len() >= value_cap {
                if work_is_limit {
                    work.exhaust_value_bytes();
                    return Err(crate::profile::WORK_VALUE_BUDGET_EXCEEDED.into());
                }
                return Err("ODF expanded value byte cap exceeded".into());
            }
            value.push('\n');
        }
        let remaining = value_cap.saturating_sub(value.len());
        let (paragraph_cap, label) = if remaining < 16 << 20 {
            (
                remaining,
                if work_is_limit {
                    crate::profile::WORK_VALUE_BUDGET_EXCEEDED
                } else {
                    "ODF expanded value byte cap exceeded"
                },
            )
        } else {
            (16 << 20, "ODF expanded text cap exceeded")
        };
        let expanded_text = if paragraph_cap == 16 << 20 {
            text(p)
        } else {
            text_capped(p, paragraph_cap, label)
        };
        match expanded_text {
            Ok(text) => value.push_str(&text),
            Err(error) => {
                if work_is_limit && error.to_string() == crate::profile::WORK_VALUE_BUDGET_EXCEEDED
                {
                    work.exhaust_value_bytes();
                }
                return Err(error);
            }
        }
        if expanded
            .checked_add(
                value
                    .len()
                    .checked_mul(weight)
                    .ok_or("ODF text size overflow")?,
            )
            .is_none_or(|total| total > cap)
        {
            return Err("ODF expanded value byte cap exceeded".into());
        }
    }
    work.charge_value_bytes(
        value
            .len()
            .checked_mul(weight)
            .ok_or("ODF text size overflow")?,
    )?;
    Ok(value)
}

pub(crate) fn inspect(
    archive: &mut Archive,
    report: &mut InspectReport,
    options: &InspectOptions,
) -> xml::Result<()> {
    let Some(bytes) = archive.read("mimetype")? else {
        return Ok(());
    };
    let mime = std::str::from_utf8(&bytes)?.trim();
    let (ext, body_kind, name) = match mime {
        "application/vnd.oasis.opendocument.text" => ("odt", "text", "OpenDocument text"),
        "application/vnd.oasis.opendocument.presentation" => {
            ("odp", "presentation", "OpenDocument presentation")
        }
        "application/vnd.oasis.opendocument.spreadsheet" => {
            ("ods", "spreadsheet", "OpenDocument spreadsheet")
        }
        _ => return Ok(()),
    };
    let root = office::read_xml(archive, "content.xml")?;
    if !root.is(O, "document-content") {
        return Err("invalid ODF content namespace/root".into());
    }
    let body = root
        .children()
        .find(|n| n.is(O, "body"))
        .and_then(|n| n.children().find(|n| n.is(O, body_kind)))
        .ok_or("ODF body does not match mimetype")?;
    office::identify(report, ext, mime, name);
    office::metadata(archive, report);
    // One document-wide budget for all ODF text expansion in tables and prose.
    let mut expansion = ExpansionBudget::new(ODF_EXPANDED_CAP);
    if ext == "ods" {
        spreadsheet(body, report, options)?;
    } else if ext == "odp" {
        let slides: Vec<_> = body.children().filter(|n| n.is(DRAW, "page")).collect();
        report.counts.slides = Some(slides.len());
        for (index, slide) in slides.iter().take(options.text_unit_limit()).enumerate() {
            unit(slide, "slide", index, report, options, &mut expansion)?;
        }
        report.text.truncated |= slides.len() > options.text_unit_limit();
    } else if ext == "odt" {
        report.counts.paragraphs =
            Some(body.descendants(T, "p").len() + body.descendants(T, "h").len());
        let sections: Vec<_> = body.children().filter(|n| n.is(T, "section")).collect();
        if sections.is_empty() {
            unit(body, "section", 0, report, options, &mut expansion)?;
        } else {
            for (index, child) in body.children().take(options.text_unit_limit()).enumerate() {
                unit(child, "section", index, report, options, &mut expansion)?;
            }
            report.text.truncated |= body.children().count() > options.text_unit_limit();
        }
    }
    Ok(())
}

fn positive(node: &Node, key: &str, cap: usize) -> xml::Result<usize> {
    let n = node.attr_ns(TABLE, key).unwrap_or("1").parse::<usize>()?;
    if n == 0 || n > cap {
        return Err(format!("ODF {key} cap exceeded").into());
    }
    Ok(n)
}

/// Profiles ODS tables under shared sheet, cell, and value-byte limits.
/// Accepts the spreadsheet body, report under construction, and inspection
/// options; returns unit after retaining honest partial output; per-table
/// structure and budget failures become sheet diagnostics, while cumulative
/// work exhaustion stops every later table.
fn spreadsheet(
    body: &Node,
    report: &mut InspectReport,
    options: &InspectOptions,
) -> xml::Result<()> {
    use crate::profile::{Budget, Grid, accumulate, column_name};
    let tables: Vec<_> = body.children().filter(|n| n.is(TABLE, "table")).collect();
    report.counts.sheets = Some(tables.len());
    let sheet_limit = options
        .text_unit_limit()
        .min(crate::spreadsheet::WORKBOOK_SHEET_CAP);
    let sheet_capped = tables.len() > sheet_limit;
    let mut budget = Budget::default();
    let mut work = WorkBudget::default();
    let mut expanded_bytes = 0usize;
    let mut work_capped = false;
    let mut attempted_sheets = 0usize;
    for (index, table) in tables.iter().take(sheet_limit).enumerate() {
        attempted_sheets = index + 1;
        let result = (|| -> xml::Result<()> {
            let mut grid = Grid::new(table.attr_ns(TABLE, "name").unwrap_or("").into(), index + 1);
            let mut markdown = crate::markdown::SheetRows::default();
            let mut row_number = 1usize;
            let mut sheet_chars = 0usize;
            for row in table.descendants(TABLE, "table-row") {
                let repeat_rows = positive(row, "number-rows-repeated", 1_048_576)?;
                if row_number
                    .checked_add(repeat_rows - 1)
                    .is_none_or(|n| n > 1_048_576)
                {
                    return Err("ODF row cap exceeded".into());
                }
                let mut col = 1usize;
                let (mut row_prefix, mut row_chars) = (String::new(), 0usize);
                let mut preview_chars = 0usize;
                let mut last_text_col = 0usize;
                let mut append = |value: &str, repeats: usize| {
                    let chars = value.chars().count();
                    accumulate(&mut row_chars, chars, repeats);
                    for _ in 0..repeats
                        .min(options.max_chars.saturating_sub(preview_chars) / chars.max(1) + 1)
                    {
                        let take = chars.min(options.max_chars.saturating_sub(preview_chars));
                        row_prefix.extend(value.chars().take(take));
                        preview_chars += take;
                    }
                };
                for cell in row
                    .children()
                    .filter(|n| n.is(TABLE, "table-cell") || n.is(TABLE, "covered-table-cell"))
                {
                    let repeat_cols = positive(cell, "number-columns-repeated", 16_384)?;
                    if col.checked_add(repeat_cols - 1).is_none_or(|n| n > 16_384) {
                        return Err("ODF column cap exceeded".into());
                    }
                    let paragraphs = cell.descendants(T, "p");
                    let kind_attr = cell.attr_ns(O, "value-type").unwrap_or("string");
                    // A typed value attribute overrides paragraph text; never expand it.
                    let typed = match kind_attr {
                        "float" | "percentage" | "currency" => cell.attr_ns(O, "value"),
                        "boolean" => cell.attr_ns(O, "boolean-value"),
                        "date" => cell.attr_ns(O, "date-value"),
                        "time" => cell.attr_ns(O, "time-value"),
                        "string" => cell.attr_ns(O, "string-value"),
                        _ => None,
                    };
                    let weight = repeat_rows
                        .checked_mul(repeat_cols)
                        .ok_or("ODF repeat overflow")?;
                    let formula = cell.attr_ns(TABLE, "formula").is_some();
                    // Fully blank tail cells are already O(1): charge their
                    // source visit without rejecting the established fast path.
                    // Values and formulas charge every logical repeat before
                    // expansion, profiling, or output; empty sheets never reset
                    // charges retained from earlier sheets.
                    let has_repeated_work = formula
                        || !paragraphs.is_empty()
                        || typed.is_some_and(|value| !value.is_empty());
                    work.charge_cells(if has_repeated_work { weight } else { 1 })?;
                    let value = match typed {
                        Some(raw) => {
                            let value = raw.to_string();
                            work.charge_value_bytes(
                                value
                                    .len()
                                    .checked_mul(weight)
                                    .ok_or("ODF text size overflow")?,
                            )?;
                            value
                        }
                        None => cell_text(
                            &paragraphs,
                            expanded_bytes,
                            weight,
                            ODF_EXPANDED_CAP,
                            &mut work,
                        )?,
                    };
                    let kind = match kind_attr {
                        "float" | "percentage" | "currency" => {
                            if !value.is_empty() && !value.parse::<f64>().is_ok_and(f64::is_finite)
                            {
                                return Err("invalid ODF number".into());
                            }
                            "number"
                        }
                        "boolean" => "bool",
                        "date" => "date",
                        "time" => "time",
                        "string" => "string",
                        _ => return Err("unsupported ODF cell type".into()),
                    };
                    if !value.is_empty() || formula {
                        expanded_bytes = expanded_bytes
                            .checked_add(
                                value
                                    .len()
                                    .checked_mul(weight)
                                    .ok_or("ODF text size overflow")?,
                            )
                            .ok_or("ODF text size overflow")?;
                        if expanded_bytes > ODF_EXPANDED_CAP {
                            return Err("ODF expanded value byte cap exceeded".into());
                        }
                        for column in col..col + repeat_cols {
                            grid.cell(
                                (row_number, column),
                                &value,
                                kind,
                                formula,
                                repeat_rows,
                                &mut budget,
                            )?;
                            if column > 1 {
                                append("\t", column - last_text_col.max(1));
                            }
                            append(&value, 1);
                            last_text_col = column;
                        }
                        let span_cols = positive(cell, "number-columns-spanned", 16_384)?;
                        let span_rows = positive(cell, "number-rows-spanned", 1_048_576)?;
                        if span_cols > 1 || span_rows > 1 {
                            if repeat_rows > 1 || repeat_cols > 1 {
                                return Err("repeated merged ODF cells are unsupported".into());
                            }
                            if col + span_cols - 1 > 16_384
                                || row_number + span_rows - 1 > 1_048_576
                            {
                                return Err("ODF merged range out of bounds".into());
                            }
                            grid.merge(format!(
                                "{}{}:{}{}",
                                column_name(col),
                                row_number,
                                column_name(col + span_cols - 1),
                                row_number + span_rows - 1
                            ))?;
                        }
                    }
                    col += repeat_cols;
                }
                if last_text_col > 0 {
                    append("\n", 1);
                    markdown.push(
                        row_prefix
                            .trim_end_matches('\n')
                            .split('\t')
                            .enumerate()
                            .map(|(i, value)| (i + 1, value.to_string()))
                            .collect(),
                        repeat_rows,
                        options.max_chars,
                    );
                    accumulate(&mut sheet_chars, row_chars, repeat_rows);
                    let preview_repeats = repeat_rows.min(7usize.saturating_sub(row_number));
                    report.append_counted(
                        &row_prefix,
                        row_chars,
                        preview_repeats,
                        options.max_chars,
                    );
                    report.counts.chars = report
                        .counts
                        .chars
                        .saturating_add(row_chars.saturating_mul(repeat_rows - preview_repeats));
                }
                row_number += repeat_rows;
            }
            office::preflight(
                report,
                "sheet",
                index,
                sheet_chars,
                !table.descendants(DRAW, "image").is_empty(),
                options,
            );
            let mut sheet = grid.finish();
            markdown.write(&mut sheet, report, options);
            report.sheets.push(sheet);
            Ok(())
        })();
        if let Err(error) = result {
            report.error("sheet", format!("sheet {}: {error}", index + 1));
            report.text.truncated = true;
            if work.is_exhausted() {
                work_capped = true;
                break;
            }
        }
    }
    if work_capped {
        report.error(
            "sheet",
            format!(
                "workbook work budget exhausted after {attempted_sheets} sheets; {} declared sheets skipped",
                tables.len().saturating_sub(attempted_sheets)
            ),
        );
        report.text.truncated = true;
    }
    if sheet_capped {
        report.error(
            "sheet",
            format!(
                "workbook sheet cap exceeded: {} sheets declared, processing limited to the first {sheet_limit} sheets",
                tables.len()
            ),
        );
        report.text.truncated = true;
    }
    crate::spreadsheet::finish(report, &budget);
    Ok(())
}

fn unit(
    node: &Node,
    kind: &'static str,
    index: usize,
    report: &mut InspectReport,
    options: &InspectOptions,
    expansion: &mut ExpansionBudget,
) -> xml::Result<()> {
    let table_start = report.tables.len();
    crate::tables::Context::new(report, "content.xml", kind, index + 1, options)
        .with_expansion(expansion)
        .odf_tables(node);
    if kind == "slide" {
        crate::markdown::append(
            report,
            &format!("## Slide {}\n\n", index + 1),
            options,
            "native_structure",
        );
    }
    crate::markdown::odf(node, report, options, table_start, expansion);
    let mut chars = 0;
    let mut result = Ok(());
    node.visit(&mut |n| {
        if n.is(T, "p") || n.is(T, "h") {
            match expansion.text(n) {
                Ok(value) => {
                    chars += value.chars().count();
                    report.append_text(&value, options.max_chars);
                    report.append_text("\n", options.max_chars);
                }
                Err(e) => result = Err(e),
            }
        }
    });
    result?;
    office::preflight(
        report,
        kind,
        index,
        chars,
        !node.descendants(DRAW, "image").is_empty(),
        options,
    );
    Ok(())
}

#[cfg(test)]
mod bounds_tests {
    fn paragraphs(texts: &[&str]) -> Vec<crate::xml::Node> {
        texts
            .iter()
            .map(|text| {
                let bytes = format!("<text:p xmlns:text=\"{}\">{}</text:p>", crate::odf::T, text);
                crate::xml::parse(bytes.as_bytes()).unwrap()
            })
            .collect()
    }

    #[test]
    fn expansion_budget_charges_full_expansion_across_nodes() {
        let owned = paragraphs(&["aaaa", "bbbb", "cccc", "dddd"]);
        // Full expanded bytes are charged per node, not a retained prefix:
        // 12 bytes fit exactly, and the fourth 4-byte paragraph exceeds the
        // exhausted budget before any retention decision.
        let mut budget = super::ExpansionBudget::new(12);
        assert_eq!(budget.text(&owned[0]).unwrap(), "aaaa");
        assert_eq!(budget.text(&owned[1]).unwrap(), "bbbb");
        assert_eq!(budget.text(&owned[2]).unwrap(), "cccc");
        let error = budget.text(&owned[3]).unwrap_err();
        assert!(
            error.to_string().contains("document expansion budget"),
            "{error}"
        );
    }

    #[test]
    fn expansion_budget_charges_expanded_spaces_not_source_bytes() {
        // <text:s text:c="8"/> expands to 8 bytes from a tiny source node.
        let owned = paragraphs(&["<text:s text:c=\"8\"/>x"]);
        let mut budget = super::ExpansionBudget::new(10);
        assert_eq!(budget.text(&owned[0]).unwrap(), "        x");
        let error = budget.text(&owned[0]).unwrap_err();
        assert!(
            error.to_string().contains("document expansion budget"),
            "{error}"
        );
    }
    #[test]
    /// Checks incremental ODF and workbook value-byte limits with small inputs.
    /// Accepts no inputs; returns unit; panics if exact fits, expanded-cap
    /// failures, or cumulative work exhaustion are charged incorrectly.
    fn ods_cell_text_charges_remaining_budget_incrementally() {
        let owned = paragraphs(&["aaaa", "bbbb", "cccc"]);
        let paragraphs: Vec<&crate::xml::Node> = owned.iter().collect();
        // Weight 2: joined "aaaa\nbbbb\ncccc" is 14 bytes; 8 + 14*2 = 36 fits 40.
        let text =
            super::cell_text(&paragraphs, 8, 2, 40, &mut super::WorkBudget::default()).unwrap();
        assert_eq!(text, "aaaa\nbbbb\ncccc");
        // Cap 35 is exceeded once the third paragraph joins; error before append.
        let error =
            super::cell_text(&paragraphs, 8, 2, 35, &mut super::WorkBudget::default()).unwrap_err();
        assert!(error.to_string().contains("cap"), "{error}");
        // A single oversized paragraph is rejected at the first incremental check.
        let error = super::cell_text(
            &paragraphs[..1],
            38,
            1,
            40,
            &mut super::WorkBudget::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("cap"), "{error}");
        // The tighter 27-byte work budget stops the 28-byte logical value and
        // latches exhaustion so the caller cannot retry later sheets.
        let mut work = super::WorkBudget::new(10, 27);
        let error = super::cell_text(&paragraphs, 8, 2, 40, &mut work).unwrap_err();
        assert!(error.to_string().contains("value-byte budget"), "{error}");
        assert!(work.is_exhausted());
    }
}
