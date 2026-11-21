//! Spreadsheet (XLSX/XLSM) inspection for the `litx-inspect` crate.
//! File: `spreadsheet.rs`.
//! Responsibility: walk a bounded OOXML workbook, profile each worksheet
//! grid with shared-string merging, and emit honest partial text/Markdown
//! previews with a capped sheet count and checked row arithmetic.
use crate::{
    InspectOptions, InspectReport,
    archive::Archive,
    office::{self, PKG, R, S},
    profile::{Budget, Grid, WorkBudget, accumulate},
    xml::{self, Node},
};
use std::{borrow::Cow, collections::HashMap};

/// Highest valid worksheet row (the 1,048,576-row sheet grid limit).
const MAX_ROWS: usize = 1_048_576;
/// Hard ceiling on processed worksheets regardless of the requested page limit.
pub(crate) const WORKBOOK_SHEET_CAP: usize = 4096;

/// Resolves one relationship through a pre-indexed workbook relationship map.
/// Accepts the ID index, source part path, requested relationship ID, and
/// expected type suffix; returns the validated internal target path; rejects
/// missing or duplicate IDs, external or mismatched relationships, missing
/// targets, and package-root escapes exactly like linear relationship lookup.
fn related_indexed(
    relationships: &HashMap<&str, Vec<&Node>>,
    part: &str,
    id: &str,
    kind: &str,
) -> xml::Result<String> {
    let found = relationships
        .get(id)
        .filter(|matches| matches.len() == 1)
        .ok_or("missing or duplicate relationship id")?;
    let rel = found[0];
    if rel
        .attr("TargetMode")
        .is_some_and(|mode| mode != "Internal")
        || rel.attr("Type") != Some(&format!("{R}/{kind}"))
    {
        return Err("external or mismatched package relationship".into());
    }
    office::resolve_part(
        part,
        rel.attr("Target").ok_or("relationship missing target")?,
    )
}

/// Resolves the 1-based ordinal of a `<row>` element. Uses the explicit `r`
/// attribute when present, otherwise the lazy successor `previous_row + 1`.
/// Accepts the row element and the previous row's resolved ordinal (0 before
/// the first row). Returns the resolved ordinal, or an error when `r` is not
/// a valid `usize`, the lazy successor would overflow `usize`, or the result
/// falls outside the valid worksheet row range 1..=1048576; every failure
/// closes the sheet instead of panicking or wrapping (W4-SYS-SPREADSHEET-ROWOVERFLOW).
fn row_number(row: &Node, previous_row: usize) -> xml::Result<usize> {
    let number = match row.attr("r") {
        Some(value) => value.parse::<usize>()?,
        None => previous_row
            .checked_add(1)
            .ok_or("worksheet row overflow")?,
    };
    if number == 0 || number > MAX_ROWS {
        return Err("worksheet row out of range 1..=1048576".into());
    }
    Ok(number)
}

fn rich_text(node: &Node) -> String {
    let mut text = String::new();
    for child in node.children() {
        if child.is(S, "t") {
            text.push_str(&child.text());
        } else if child.is(S, "r") {
            text.push_str(&rich_text(child));
        }
    }
    text
}

/// Profiles every worksheet of an OOXML workbook into the report. Accepts
/// the open archive, the parsed workbook root, its part path, the report
/// under construction, and the inspection options. Returns unit on success or
/// an error when the workbook-level relationships or shared strings are
/// invalid; per-sheet failures are recorded as `sheet` stage errors with the
/// partial output flagged truncated instead of aborting the whole report.
pub(crate) fn xlsx(
    archive: &mut Archive,
    root: &Node,
    part: &str,
    report: &mut InspectReport,
    options: &InspectOptions,
) -> xml::Result<()> {
    let sheets = root.descendants(S, "sheet");
    report.counts.sheets = Some(sheets.len());
    // W3-SYS-SPREADSHEET-SHEETS: a hard ceiling binds both full extraction and
    // arbitrarily large explicit page limits; smaller user limits still win.
    let sheet_limit = options.text_unit_limit().min(WORKBOOK_SHEET_CAP);
    let sheet_capped = sheets.len() > sheet_limit;
    let rels = office::relationships(archive, part)?;
    // Index once instead of rescanning every relationship for every sheet.
    // Vectors preserve duplicate-ID detection for each referenced ID.
    let mut relationships: HashMap<&str, Vec<&Node>> = HashMap::new();
    for rel in rels.children().filter(|n| n.is(PKG, "Relationship")) {
        if let Some(id) = rel.attr("Id") {
            relationships.entry(id).or_default().push(rel);
        }
    }
    let string_rels: Vec<_> = rels
        .children()
        .filter(|n| {
            n.is(PKG, "Relationship") && n.attr("Type") == Some(&format!("{R}/sharedStrings"))
        })
        .collect();
    if string_rels.len() > 1 {
        return Err("duplicate shared strings relationship".into());
    }
    let mut strings = vec![];
    for rel in string_rels {
        let target = related_indexed(
            &relationships,
            part,
            rel.attr("Id").ok_or("shared strings missing id")?,
            "sharedStrings",
        )?;
        let root = office::read_xml(archive, &target)?;
        if !root.is(S, "sst") {
            return Err("invalid shared strings root".into());
        }
        strings = root
            .children()
            .filter(|n| n.is(S, "si"))
            .map(rich_text)
            .collect();
    }
    let mut budget = Budget::default();
    let mut work = WorkBudget::default();
    let mut work_capped = false;
    let mut attempted_sheets = 0usize;
    for (index, sheet) in sheets.iter().take(sheet_limit).enumerate() {
        attempted_sheets = index + 1;
        let result = (|| -> xml::Result<()> {
            let target = related_indexed(
                &relationships,
                part,
                sheet.attr_ns(R, "id").ok_or("sheet missing id")?,
                "worksheet",
            )?;
            let root = office::read_xml(archive, &target)?;
            if !root.is(S, "worksheet") {
                return Err("invalid worksheet root".into());
            }
            let mut grid = Grid::new(sheet.attr("name").unwrap_or("").into(), index + 1);
            let mut markdown = crate::markdown::SheetRows::default();
            let mut chars = 0;
            let mut previous_row = 0;
            for (preview_index, row) in root.descendants(S, "row").into_iter().enumerate() {
                let text_cap = if preview_index < 6 {
                    options.max_chars
                } else {
                    report.text.chars
                };
                // Only previewed rows buffer cell text, bounded by push_bounded.
                let previewing = preview_index < 6 && !markdown.is_truncated();
                let mut row_values: Vec<(usize, Cow<'_, str>)> = Vec::new();
                let row_number = row_number(row, previous_row)?;
                if row_number <= previous_row {
                    return Err("unordered or duplicate worksheet row".into());
                }
                previous_row = row_number;
                let mut col = 0;
                for cell in row.children().filter(|n| n.is(S, "c")) {
                    let (cell_row, column) = cell
                        .attr("r")
                        .map(crate::profile::position)
                        .transpose()?
                        .unwrap_or((row_number, col + 1));
                    if cell_row != row_number || column <= col {
                        return Err("unordered or duplicate cell coordinate".into());
                    }
                    col = column;
                    let raw = cell
                        .children()
                        .find(|n| n.is(S, "v"))
                        .map(Node::text)
                        .unwrap_or_default();
                    let (value, kind): (Cow<'_, str>, &'static str) =
                        match cell.attr("t").unwrap_or("n") {
                            // Shared strings are borrowed per cell, never cloned whole.
                            "s" => (
                                Cow::Borrowed(
                                    strings
                                        .get(raw.parse::<usize>()?)
                                        .ok_or("shared string index out of bounds")?
                                        .as_str(),
                                ),
                                "string",
                            ),
                            "inlineStr" => (
                                Cow::Owned(
                                    cell.children()
                                        .find(|n| n.is(S, "is"))
                                        .map(rich_text)
                                        .unwrap_or_default(),
                                ),
                                "string",
                            ),
                            "str" => (Cow::Owned(raw), "string"),
                            "b" => (
                                Cow::Borrowed(match raw.as_str() {
                                    "1" => "true",
                                    "0" => "false",
                                    _ => return Err("invalid boolean cell".into()),
                                }),
                                "bool",
                            ),
                            "d" => (Cow::Owned(raw), "date"),
                            "e" => (Cow::Owned(raw), "error"),
                            "n" => {
                                if !raw.is_empty() && !raw.parse::<f64>().is_ok_and(f64::is_finite)
                                {
                                    return Err("invalid numeric cell".into());
                                }
                                (Cow::Owned(raw), "number")
                            }
                            _ => return Err("unsupported cell type".into()),
                        };
                    let formula = cell.children().any(|n| n.is(S, "f"));
                    // Charge every logical cell and full value before Grid's
                    // hash/prose scans, chars().count(), or append_text().
                    work.charge_cells(1)?;
                    work.charge_value_bytes(value.len())?;
                    grid.cell((row_number, col), &value, kind, formula, 1, &mut budget)?;
                    // W3-SYS-SPREADSHEET-WASM32: saturating accounting through
                    // the width-parameterized `accumulate` seam so repeated
                    // shared strings cannot wrap or panic the per-sheet
                    // counter on narrow `usize` targets (wasm32).
                    accumulate(&mut chars, value.chars().count(), 1);
                    if col > 1 {
                        report.append_text("\t", text_cap);
                    }
                    report.append_text(&value, text_cap);
                    if previewing {
                        row_values.push((col, value));
                    }
                }
                report.append_text("\n", text_cap);
                // Always inform SheetRows: rows skipped past the six-row preview
                // cutoff must mark more_rows so the sheet reports truncation.
                markdown.push_bounded(&row_values, 1, options.max_chars);
            }
            for merge in root.descendants(S, "mergeCell") {
                let range = merge.attr("ref").ok_or("merge missing reference")?;
                let (a, b) = range.split_once(':').unwrap_or((range, range));
                crate::profile::position(a)?;
                crate::profile::position(b)?;
                grid.merge(range.into())?;
            }
            office::preflight(
                report,
                "sheet",
                index,
                chars,
                !root.descendants(S, "drawing").is_empty()
                    || !root.descendants(S, "legacyDrawing").is_empty(),
                options,
            );
            let mut sheet = grid.finish();
            markdown.write(&mut sheet, report, options);
            report.sheets.push(sheet);
            Ok(())
        })();
        if let Err(error) = result {
            report.error("sheet", format!("sheet {}: {error}", index + 1));
            // A failed sheet leaves the buffered text partial: flag the
            // cutoff so text/Markdown never claim complete output.
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
                sheets.len().saturating_sub(attempted_sheets)
            ),
        );
        report.text.truncated = true;
    }
    if sheet_capped {
        report.error(
            "sheet",
            format!(
                "workbook sheet cap exceeded: {} sheets declared, processing limited to the first {sheet_limit} sheets",
                sheets.len()
            ),
        );
        report.text.truncated = true;
    }
    finish(report, &budget);
    Ok(())
}

pub(crate) fn finish(report: &mut InspectReport, budget: &Budget) {
    if let Some(index) = report
        .sheets
        .iter()
        .enumerate()
        .max_by_key(|(i, s)| (s.rows, s.non_empty_cells, std::cmp::Reverse(*i)))
        .map(|(i, _)| i)
    {
        report.sheets[index].is_longest = true;
    }
    if budget.capped {
        report.error("profile", "cardinality capped: 16384 columns or 524288 distinct hashes; distinct counts are lower bounds");
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Builds a minimal `<row>` node carrying only an optional `r` attribute.
    fn row_node(r: Option<&str>) -> Node {
        let attrs = match r {
            Some(value) => BTreeMap::from([((String::new(), "r".to_string()), value.to_string())]),
            None => BTreeMap::new(),
        };
        Node {
            ns: S.to_string(),
            name: "row".to_string(),
            attrs,
            content: vec![],
        }
    }

    #[test]
    fn chars_accounting_saturates_at_reduced_width_instead_of_wrapping() {
        // W3-SYS-SPREADSHEET-WASM32: the per-sheet preflight counter is
        // charged through the width-parameterized `accumulate` seam; the
        // u32 instantiation proves 32-bit targets (wasm32) saturate where
        // the previous unchecked `chars += value.chars().count()` wrapped
        // or panicked, without any huge fixture allocation.
        let mut total = u32::MAX - 9;
        accumulate(&mut total, 100, 1);
        assert_eq!(total, u32::MAX, "past-cap addition must saturate");
    }

    #[test]
    fn chars_accounting_is_exact_on_wide_targets() {
        // Native 64-bit accounting stays exact for every in-range value.
        let mut total = 4_000_000_000usize;
        accumulate(&mut total, 7, 3);
        assert_eq!(total, 4_000_000_021);
    }

    #[test]
    fn row_number_is_range_checked_and_overflow_free() {
        // W4-SYS-SPREADSHEET-ROWOVERFLOW: explicit, lazy, boundary and
        // overflow cases all resolve or fail closed without panicking.
        assert_eq!(row_number(&row_node(Some("1048576")), 5).unwrap(), MAX_ROWS);
        assert_eq!(row_number(&row_node(None), 5).unwrap(), 6);
        for (row, previous, expected) in [
            (Some("18446744073709551615"), 0usize, "out of range"),
            (Some("0"), 0, "out of range"),
            (Some("1048577"), 0, "out of range"),
            (None, MAX_ROWS, "out of range"),
            (None, usize::MAX, "row overflow"),
        ] {
            let error = row_number(&row_node(row), previous)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "row {row:?}: {error}");
        }
    }
}
