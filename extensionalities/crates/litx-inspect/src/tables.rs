//! Logical document tables. No raster geometry or inferred header semantics.
use crate::odf::{T, TABLE};
use crate::{
    InspectOptions, InspectReport,
    office::{A, W},
    xml::{self, Node},
};
use serde::Serialize;
use std::collections::BTreeMap;

/// Cumulative expanded-text cap for a single ODF document table cell.
const ODF_CELL_TEXT_CAP: usize = 16 << 20;

#[derive(Debug, Serialize)]
pub struct DocumentTable {
    pub index: usize,
    pub source: &'static str,
    pub source_part: String,
    pub unit_kind: &'static str,
    pub unit_number: usize,
    pub parent_table: Option<usize>,
    pub name: Option<String>,
    pub rows: usize,
    pub cols: usize,
    pub header_rows: Vec<usize>,
    pub cells: Vec<TableCell>,
    pub complete: bool,
}

#[derive(Debug, Serialize)]
pub struct TableCell {
    pub row: usize,
    pub column: usize,
    pub row_span: usize,
    pub column_span: usize,
    pub text: String,
    pub text_chars: usize,
    pub text_truncated: bool,
    pub nested_tables: Vec<usize>,
}

pub(crate) struct Context<'a> {
    pub report: &'a mut InspectReport,
    pub part: &'a str,
    pub kind: &'static str,
    pub unit: usize,
    remaining: usize,
    cell_count: usize,
    expansion: Option<&'a mut crate::odf::ExpansionBudget>,
}

impl<'a> Context<'a> {
    pub fn new(
        report: &'a mut InspectReport,
        part: &'a str,
        kind: &'static str,
        unit: usize,
        options: &InspectOptions,
    ) -> Self {
        // One shared text budget for tables across the report, separate from the flat preview.
        let used: usize = report
            .tables
            .iter()
            .flat_map(|t| &t.cells)
            .map(|c| c.text.chars().count())
            .sum();
        let cell_count = report.tables.iter().map(|t| t.cells.len()).sum();
        Self {
            report,
            part,
            kind,
            unit,
            remaining: options.max_chars.saturating_sub(used),
            cell_count,
            expansion: None,
        }
    }

    /// Attach the document-wide ODF text-expansion budget shared across all
    /// table cells in the document. Required before `odf_tables`.
    pub fn with_expansion(mut self, budget: &'a mut crate::odf::ExpansionBudget) -> Self {
        self.expansion = Some(budget);
        self
    }

    fn empty(&mut self, parent: Option<usize>) -> xml::Result<usize> {
        if self.report.tables.len() >= 4096 {
            return Err("document table-count cap exceeded".into());
        }
        let index = self.report.tables.len() + 1;
        self.report.tables.push(DocumentTable {
            index,
            source: "archive_entry",
            source_part: self.part.into(),
            unit_kind: self.kind,
            unit_number: self.unit,
            parent_table: parent,
            name: None,
            rows: 0,
            cols: 0,
            header_rows: vec![],
            cells: vec![],
            complete: false,
        });
        Ok(index)
    }

    fn cell(
        &mut self,
        row: usize,
        column: usize,
        row_span: usize,
        column_span: usize,
        text: &str,
    ) -> xml::Result<TableCell> {
        self.cell_count += 1;
        if self.cell_count > 200_000
            || row == 0
            || row > 1_048_576
            || column == 0
            || column
                .checked_add(column_span.saturating_sub(1))
                .is_none_or(|n| n > 16_384)
            || row
                .checked_add(row_span.saturating_sub(1))
                .is_none_or(|n| n > 1_048_576)
            || row_span == 0
            || column_span == 0
        {
            return Err("document table cell/grid cap exceeded".into());
        }
        let chars = text.chars().count();
        let take = chars.min(self.remaining);
        self.remaining -= take;
        Ok(TableCell {
            row,
            column,
            row_span,
            column_span,
            text: text.chars().take(take).collect(),
            text_chars: chars,
            text_truncated: chars > take,
            nested_tables: vec![],
        })
    }

    fn add_continuation(&mut self, anchor: &mut TableCell, value: &str) {
        if value.is_empty() {
            return;
        }
        let separator = usize::from(anchor.text_chars > 0);
        let chars = value.chars().count() + separator;
        anchor.text_chars += chars;
        // A truncated prefix must never acquire later text out of order.
        if !anchor.text_truncated {
            let append = if separator > 0 {
                format!("\n{value}")
            } else {
                value.into()
            };
            let take = chars.min(self.remaining);
            anchor.text.extend(append.chars().take(take));
            self.remaining -= take;
        }
        anchor.text_truncated = anchor.text.chars().count() < anchor.text_chars;
    }

    fn guarded(
        &mut self,
        node: &Node,
        parent: Option<usize>,
        parse: fn(&mut Self, &Node, Option<usize>) -> xml::Result<usize>,
    ) -> Option<usize> {
        let start = self.report.tables.len();
        let remaining = self.remaining;
        match parse(self, node, parent) {
            Ok(index) => Some(index),
            Err(error) => {
                self.report.tables.truncate(start);
                // Output budget is re-granted with the dropped table, but failed
                // expansion work stays charged to the aggregate cell_count.
                self.remaining = remaining;
                self.report
                    .error("table", format!("{}: {error}", self.part));
                None
            }
        }
    }

    pub fn word_tables(&mut self, root: &Node) {
        if root.is(W, "tbl") {
            self.guarded(root, None, Self::word);
            return;
        }
        if root.is(W, "p") && !root.descendants(W, "sectPr").is_empty() {
            self.unit += 1;
        }
        for child in root.children() {
            self.word_tables(child);
        }
        self.report.counts.tables = Some(self.report.tables.len());
    }

    pub fn drawing_tables(&mut self, root: &Node) {
        if root.is(A, "tbl") {
            self.guarded(root, None, Self::drawing);
        } else {
            for child in root.children() {
                self.drawing_tables(child);
            }
        }
        self.report.counts.tables = Some(self.report.tables.len());
    }

    pub fn odf_tables(&mut self, root: &Node) {
        if root.is(TABLE, "table") {
            self.guarded(root, None, Self::odf);
        } else {
            for child in root.children() {
                self.odf_tables(child);
            }
        }
        self.report.counts.tables = Some(self.report.tables.len());
    }

    fn odf(&mut self, root: &Node, parent: Option<usize>) -> xml::Result<usize> {
        let id = self.empty(parent)?;
        let mut cells: Vec<TableCell> = vec![];
        let mut occupied = Occupancy::default();
        let mut row_number = 0usize;
        let mut cols = 0;
        let mut headers = vec![];
        let mut expanded_cells = 0usize;
        for (row, header) in odf_rows(root) {
            let repeat_rows = number(row.attr_ns(TABLE, "number-rows-repeated"), 1)?;
            if repeat_rows == 0 || repeat_rows > 200_000usize.saturating_sub(row_number) {
                return Err("ODF document table row expansion cap exceeded".into());
            }
            // Expand each source cell's text once, outside the repetition loops.
            let mut sources = Vec::new();
            for cell in row
                .children()
                .filter(|n| n.is(TABLE, "table-cell") || n.is(TABLE, "covered-table-cell"))
            {
                let repeat_cols = number(cell.attr_ns(TABLE, "number-columns-repeated"), 1)?;
                if repeat_cols == 0 {
                    return Err("ODF document table column expansion cap exceeded".into());
                }
                let (row_span, column_span, value, nested) = if cell.is(TABLE, "covered-table-cell")
                {
                    (0, 0, String::new(), vec![])
                } else {
                    let row_span = number(cell.attr_ns(TABLE, "number-rows-spanned"), 1)?;
                    let column_span = number(cell.attr_ns(TABLE, "number-columns-spanned"), 1)?;
                    let (value, nested) = odf_cell_content(
                        cell,
                        ODF_CELL_TEXT_CAP,
                        self.expansion
                            .as_deref_mut()
                            .ok_or("ODF document expansion budget missing")?,
                    )?;
                    (row_span, column_span, value, nested)
                };
                sources.push(ExpandedCell {
                    node: cell,
                    repeat_cols,
                    row_span,
                    column_span,
                    value,
                    nested,
                });
            }
            for _ in 0..repeat_rows {
                row_number += 1;
                occupied.row(row_number);
                if header {
                    headers.push(row_number);
                }
                let mut column = 0usize;
                for source in &sources {
                    if column
                        .checked_add(source.repeat_cols)
                        .is_none_or(|n| n > 16_384)
                    {
                        return Err("ODF document table column expansion cap exceeded".into());
                    }
                    for _ in 0..source.repeat_cols {
                        column += 1;
                        expanded_cells += 1;
                        if expanded_cells > 200_000 {
                            return Err("ODF document table cell expansion cap exceeded".into());
                        }
                        if source.node.is(TABLE, "covered-table-cell") {
                            occupied.cover(column).ok_or("orphan ODF covered cell")?;
                            continue;
                        }
                        let mut origin = self.cell(
                            row_number,
                            column,
                            source.row_span,
                            source.column_span,
                            &source.value,
                        )?;
                        occupied.insert(&origin, cells.len())?;
                        for &nested in &source.nested {
                            origin.nested_tables.push(self.odf(nested, Some(id))?);
                        }
                        cols = cols.max(column + source.column_span - 1);
                        cells.push(origin);
                    }
                }
                cols = cols.max(column);
            }
        }
        if cells.iter().any(|c| c.row + c.row_span - 1 > row_number) {
            return Err("ODF row span exceeds table".into());
        }
        let table = &mut self.report.tables[id - 1];
        table.name = root.attr_ns(TABLE, "name").map(str::to_string);
        table.rows = row_number;
        table.cols = cols;
        table.cells = cells;
        table.header_rows = headers;
        table.complete = true;
        Ok(id)
    }

    fn drawing(&mut self, root: &Node, parent: Option<usize>) -> xml::Result<usize> {
        let id = self.empty(parent)?;
        let mut cells = vec![];
        let mut occupied = Occupancy::default();
        let mut rows = 0;
        let mut cols = root
            .children()
            .find(|n| n.is(A, "tblGrid"))
            .map(|n| n.children().filter(|n| n.is(A, "gridCol")).count())
            .unwrap_or(0);
        for row in root.children().filter(|n| n.is(A, "tr")) {
            rows += 1;
            occupied.row(rows);
            let mut column = 0;
            for cell in row.children().filter(|n| n.is(A, "tc")) {
                column += 1;
                if column > 16_384 {
                    return Err("DrawingML column cap exceeded".into());
                }
                let horizontal = matches!(cell.attr("hMerge"), Some("1" | "true"));
                let vertical = matches!(cell.attr("vMerge"), Some("1" | "true"));
                let value = drawing_cell_content(cell);
                if horizontal || vertical {
                    let anchor = occupied
                        .cover(column)
                        .ok_or("orphan DrawingML merge continuation")?;
                    let origin: &mut TableCell = &mut cells[anchor];
                    if (horizontal && origin.column == column) || (vertical && origin.row == rows) {
                        return Err("DrawingML merge direction disagrees with anchor".into());
                    }
                    self.add_continuation(origin, &value);
                    continue;
                }
                let row_span = number(cell.attr("rowSpan"), 1)?;
                let column_span = number(cell.attr("gridSpan"), 1)?;
                let origin = self.cell(rows, column, row_span, column_span, &value)?;
                occupied.insert(&origin, cells.len())?;
                cols = cols.max(column + column_span - 1);
                cells.push(origin);
            }
            cols = cols.max(column);
        }
        if cells.iter().any(|c| c.row + c.row_span - 1 > rows) {
            return Err("DrawingML row span exceeds table".into());
        }
        let table = &mut self.report.tables[id - 1];
        table.rows = rows;
        table.cols = cols;
        table.cells = cells;
        table.complete = true;
        Ok(id)
    }

    fn word(&mut self, root: &Node, parent: Option<usize>) -> xml::Result<usize> {
        let id = self.empty(parent)?;
        let mut cells: Vec<TableCell> = vec![];
        let mut rows = 0;
        let mut cols = root
            .children()
            .find(|n| n.is(W, "tblGrid"))
            .map(|n| n.children().filter(|n| n.is(W, "gridCol")).count())
            .unwrap_or(0);
        if cols > 16_384 {
            return Err("Word table grid cap exceeded".into());
        }
        let mut header_rows = vec![];
        // Column -> (anchor cell index, width). Only immediately preceding rows may continue.
        let mut prior: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
        for row in root.children().filter(|n| n.is(W, "tr")) {
            rows += 1;
            let props = row.children().find(|n| n.is(W, "trPr"));
            if props.is_some_and(|n| {
                n.children()
                    .any(|p| p.is(W, "tblHeader") && enabled(p.attr_ns(W, "val")))
            }) {
                header_rows.push(rows);
            }
            let mut column = props
                .and_then(|p| p.children().find(|n| n.is(W, "gridBefore")))
                .map(|p| number(p.attr_ns(W, "val"), 0))
                .transpose()?
                .unwrap_or(0)
                + 1;
            let mut next = BTreeMap::new();
            let mut horizontal: Option<usize> = None;
            for cell in row.children().filter(|n| n.is(W, "tc")) {
                let props = cell.children().find(|n| n.is(W, "tcPr"));
                let span = props
                    .and_then(|p| p.children().find(|n| n.is(W, "gridSpan")))
                    .map(|p| number(p.attr_ns(W, "val"), 1))
                    .transpose()?
                    .unwrap_or(1);
                if span == 0 || column.checked_add(span - 1).is_none_or(|n| n > 16_384) {
                    return Err("Word table column span out of bounds".into());
                }
                let vertical = props.and_then(|p| p.children().find(|n| n.is(W, "vMerge")));
                let hmerge = props.and_then(|p| p.children().find(|n| n.is(W, "hMerge")));
                let (value, nested) = word_cell_content(cell);
                let anchor = if let Some(merge) =
                    vertical.filter(|n| n.attr_ns(W, "val") != Some("restart"))
                {
                    if !matches!(merge.attr_ns(W, "val"), None | Some("continue")) {
                        return Err("invalid Word vertical merge value".into());
                    }
                    let &(anchor, width) = prior
                        .get(&column)
                        .ok_or("orphan Word vertical merge continuation")?;
                    if width != span {
                        return Err("Word vertical merge widths disagree".into());
                    }
                    cells[anchor].row_span += 1;
                    self.add_continuation(&mut cells[anchor], &value);
                    next.insert(column, (anchor, span));
                    anchor
                } else if let Some(merge) =
                    hmerge.filter(|n| n.attr_ns(W, "val") != Some("restart"))
                {
                    if !matches!(merge.attr_ns(W, "val"), None | Some("continue")) {
                        return Err("invalid Word horizontal merge value".into());
                    }
                    let anchor = horizontal.ok_or("orphan Word horizontal merge continuation")?;
                    cells[anchor].column_span += span;
                    self.add_continuation(&mut cells[anchor], &value);
                    anchor
                } else {
                    let anchor = cells.len();
                    cells.push(self.cell(rows, column, 1, span, &value)?);
                    if vertical.is_some() {
                        next.insert(column, (anchor, span));
                    }
                    if hmerge.is_some() {
                        horizontal = Some(anchor);
                    } else {
                        horizontal = None;
                    }
                    anchor
                };
                for nested in nested {
                    let nested_id = self.word(nested, Some(id))?;
                    cells[anchor].nested_tables.push(nested_id);
                }
                cols = cols.max(column + span - 1);
                column += span;
            }
            let after = props
                .and_then(|p| p.children().find(|n| n.is(W, "gridAfter")))
                .map(|p| number(p.attr_ns(W, "val"), 0))
                .transpose()?
                .unwrap_or(0);
            cols = cols.max(column.saturating_sub(1) + after);
            if cols > 16_384 {
                return Err("Word table grid cap exceeded".into());
            }
            prior = next;
        }
        let table = &mut self.report.tables[id - 1];
        table.rows = rows;
        table.cols = cols;
        table.cells = cells;
        table.header_rows = header_rows;
        table.complete = true;
        Ok(id)
    }
}

#[derive(Default)]
struct Occupancy(BTreeMap<usize, (usize, usize, usize)>);
impl Occupancy {
    fn row(&mut self, row: usize) {
        self.0.retain(|_, (_, end_row, _)| *end_row >= row);
    }
    fn cover(&self, column: usize) -> Option<usize> {
        self.0
            .range(..=column)
            .next_back()
            .and_then(|(_, (end, _, anchor))| (*end >= column).then_some(*anchor))
    }
    fn insert(&mut self, cell: &TableCell, anchor: usize) -> xml::Result<()> {
        let end_col = cell.column + cell.column_span - 1;
        if self.cover(cell.column).is_some() || self.0.range(cell.column..=end_col).next().is_some()
        {
            return Err("overlapping document table cells".into());
        }
        self.0
            .insert(cell.column, (end_col, cell.row + cell.row_span - 1, anchor));
        Ok(())
    }
}

fn drawing_cell_content(cell: &Node) -> String {
    let mut text = String::new();
    for paragraph in cell.descendants(A, "p") {
        if !text.is_empty() {
            text.push('\n');
        }
        paragraph.visit(&mut |n| {
            if n.is(A, "t") {
                text.push_str(&n.text());
            }
            if n.is(A, "br") {
                text.push('\n');
            }
        });
    }
    text
}

fn odf_rows(root: &Node) -> Vec<(&Node, bool)> {
    fn visit<'a>(node: &'a Node, header: bool, rows: &mut Vec<(&'a Node, bool)>) {
        if node.is(TABLE, "table") {
            return;
        }
        if node.is(TABLE, "table-row") {
            rows.push((node, header));
            return;
        }
        let header = header || node.is(TABLE, "table-header-rows");
        for child in node.children() {
            visit(child, header, rows);
        }
    }
    let mut rows = vec![];
    for child in root.children() {
        visit(child, false, &mut rows);
    }
    rows
}

/// Source ODF table cell with its text expanded once, before row/column repetition.
struct ExpandedCell<'a> {
    node: &'a Node,
    repeat_cols: usize,
    row_span: usize,
    column_span: usize,
    value: String,
    nested: Vec<&'a Node>,
}

fn odf_cell_content<'a>(
    cell: &'a Node,
    cap: usize,
    budget: &mut crate::odf::ExpansionBudget,
) -> xml::Result<(String, Vec<&'a Node>)> {
    fn visit<'a>(
        node: &'a Node,
        text: &mut String,
        nested: &mut Vec<&'a Node>,
        cap: usize,
        budget: &mut crate::odf::ExpansionBudget,
    ) -> xml::Result<()> {
        if node.is(TABLE, "table") {
            nested.push(node);
            return Ok(());
        }
        if node.is(T, "p") || node.is(T, "h") {
            let piece = budget.text(node)?;
            let separator = usize::from(!text.is_empty());
            if text
                .len()
                .saturating_add(separator)
                .saturating_add(piece.len())
                > cap
            {
                return Err("ODF cell text cap exceeded".into());
            }
            if separator > 0 {
                text.push('\n');
            }
            text.push_str(&piece);
            return Ok(());
        }
        for child in node.children() {
            visit(child, text, nested, cap, budget)?;
        }
        Ok(())
    }
    let (mut text, mut nested) = (String::new(), vec![]);
    for child in cell.children() {
        visit(child, &mut text, &mut nested, cap, budget)?;
    }
    Ok((text, nested))
}

fn enabled(value: Option<&str>) -> bool {
    !matches!(value, Some("0" | "false" | "off"))
}
fn number(value: Option<&str>, default: usize) -> xml::Result<usize> {
    let n = value
        .map(str::parse::<usize>)
        .transpose()?
        .unwrap_or(default);
    if n > 1_048_576 {
        return Err("table numeric attribute cap exceeded".into());
    }
    Ok(n)
}

fn word_cell_content(cell: &Node) -> (String, Vec<&Node>) {
    fn visit<'a>(node: &'a Node, text: &mut String, nested: &mut Vec<&'a Node>) {
        if node.is(W, "tbl") {
            nested.push(node);
            return;
        }
        if node.is(W, "t") {
            text.push_str(&node.text());
            return;
        }
        if node.is(W, "tab") {
            text.push('\t');
            return;
        }
        if node.is(W, "br") || node.is(W, "cr") {
            text.push('\n');
            return;
        }
        if node.is(W, "p") && !text.is_empty() {
            text.push('\n');
        }
        for child in node.children() {
            visit(child, text, nested);
        }
    }
    let (mut text, mut nested) = (String::new(), vec![]);
    for child in cell.children() {
        visit(child, &mut text, &mut nested);
    }
    (text, nested)
}

#[cfg(test)]
mod bounds_tests {
    fn cell(paragraphs: &str) -> crate::xml::Node {
        let bytes = format!(
            "<table:table-cell xmlns:table=\"{}\">{}</table:table-cell>",
            crate::odf::TABLE,
            paragraphs
        );
        crate::xml::parse(bytes.as_bytes()).unwrap()
    }

    #[test]
    fn odf_cell_content_shares_document_budget_across_cells() {
        let paragraph = |text: &str| {
            format!(
                "<text:p xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">{text}</text:p>"
            )
        };
        // One shared document budget across cells: 10 - 4 - 4 leaves 2 bytes,
        // so the third cell's 4-byte paragraph is rejected before allocation.
        let mut budget = crate::odf::ExpansionBudget::new(10);
        let a = cell(&paragraph("aaaa"));
        let (text, _) = super::odf_cell_content(&a, 64, &mut budget).unwrap();
        assert_eq!(text, "aaaa");
        let b = cell(&paragraph("bbbb"));
        let (text, _) = super::odf_cell_content(&b, 64, &mut budget).unwrap();
        assert_eq!(text, "bbbb");
        let c = cell(&paragraph("cccc"));
        let error = super::odf_cell_content(&c, 64, &mut budget).unwrap_err();
        assert!(
            error.to_string().contains("document expansion budget"),
            "{error}"
        );
    }
    #[test]
    fn odf_cell_content_enforces_cumulative_cap_before_append() {
        let paragraphs = "<text:p xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">aaaa</text:p>\
                          <text:p xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">bbbb</text:p>\
                          <text:p xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">cccc</text:p>";
        let cell = cell(paragraphs);
        let mut budget = crate::odf::ExpansionBudget::new(1 << 20);
        let (text, nested) = super::odf_cell_content(&cell, 64, &mut budget).unwrap();
        assert_eq!(text, "aaaa\nbbbb\ncccc");
        assert!(nested.is_empty());
        // Cap 9 fits "aaaa\nbbbb" exactly; the third paragraph is rejected before append.
        let error = super::odf_cell_content(&cell, 9, &mut budget).unwrap_err();
        assert!(error.to_string().contains("cap"), "{error}");
        // Cap 5 rejects the second paragraph even though the first fits alone.
        let error = super::odf_cell_content(&cell, 5, &mut budget).unwrap_err();
        assert!(error.to_string().contains("cap"), "{error}");
    }
}
