//! Markdown is an output, not a claim that plain text contains recoverable layout.
use crate::{InspectOptions, InspectReport, office::W, xml::Node};
use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct Markdown {
    pub content: String,
    pub basis: &'static str,
    pub truncated: bool,
}

#[derive(Default)]
pub(crate) struct SheetRows {
    rows: Vec<(Vec<(usize, String)>, usize)>,
    chars: usize,
    truncated: bool,
    sampled_rows: usize,
    more_rows: bool,
}

impl SheetRows {
    pub fn push(&mut self, cells: Vec<(usize, String)>, repeats: usize, cap: usize) {
        let take = repeats.min(6usize.saturating_sub(self.sampled_rows));
        self.more_rows |= take < repeats;
        if take == 0 {
            return;
        }
        if self.truncated {
            return;
        }
        self.chars = self.chars.saturating_add(
            cells
                .iter()
                .map(|(_, s)| s.chars().count().saturating_add(4))
                .sum::<usize>(),
        );
        self.truncated = self.chars > cap;
        if !self.truncated {
            self.rows.push((cells, take));
            self.sampled_rows += take;
        }
    }

    /// True once the preview character budget is exhausted mid-row.
    pub(crate) fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// Buffer one preview row, bounding each stored cell to the remaining
    /// character budget and marking the preview truncated when text is cut.
    pub(crate) fn push_bounded<S: AsRef<str>>(
        &mut self,
        cells: &[(usize, S)],
        repeats: usize,
        cap: usize,
    ) {
        let take = repeats.min(6usize.saturating_sub(self.sampled_rows));
        self.more_rows |= take < repeats;
        if take == 0 || self.truncated {
            return;
        }
        let mut bounded = Vec::with_capacity(cells.len());
        for (col, value) in cells {
            let value = value.as_ref();
            let remaining = cap.saturating_sub(self.chars).saturating_sub(4);
            let chars = value.chars().count();
            let kept = chars.min(remaining);
            self.chars = self.chars.saturating_add(kept.saturating_add(4));
            bounded.push((*col, value.chars().take(kept).collect::<String>()));
            // Empty cells charge 4 chars of overhead: charging past the cap
            // without cutting text is still truncation.
            if kept < chars || self.chars > cap {
                self.truncated = true;
                break;
            }
        }
        self.rows.push((bounded, take));
        self.sampled_rows += take;
    }
    pub fn write(
        self,
        sheet: &mut crate::Sheet,
        report: &mut InspectReport,
        options: &InspectOptions,
    ) {
        append(
            report,
            &format!("## Sheet: {}\n\n", escape(&sheet.name)),
            options,
            "native_structure",
        );
        let mut header = true;
        sheet.preview_truncated = self.more_rows || self.truncated;
        for (cells, repeats) in self.rows {
            let cells: std::collections::BTreeMap<_, _> = cells.into_iter().collect();
            let mut line = String::from("|");
            for col in 1..=sheet.cols {
                let value = cells.get(&col).map(String::as_str).unwrap_or("");
                line.push_str(&format!(
                    " {} |",
                    escape(value)
                        .replace('|', "\\|")
                        .replace(['\n', '\t'], "<br>")
                ));
                if line.len() > options.max_chars.saturating_mul(4) {
                    break;
                }
            }
            line.push('\n');
            for _ in 0..repeats {
                if !header {
                    sheet.preview_rows.push(
                        (1..=sheet.cols)
                            .map(|col| {
                                let value = cells.get(&col).map(String::as_str).unwrap_or("");
                                if value.chars().count() > 512 {
                                    sheet.preview_truncated = true;
                                }
                                value.chars().take(512).collect()
                            })
                            .collect(),
                    );
                }
                append(report, &line, options, "native_structure");
                if header {
                    let separator = format!("|{}\n", " --- |".repeat(sheet.cols));
                    append(report, &separator, options, "native_structure");
                    header = false;
                }
                if report.markdown.truncated {
                    // The Markdown cap stopped row handling mid-sheet.
                    sheet.preview_truncated = true;
                    break;
                }
            }
            if report.markdown.truncated {
                sheet.preview_truncated = true;
                break;
            }
        }
        report.markdown.truncated |= self.truncated;
        if self.more_rows {
            append(
                report,
                "\n> Dataset preview: first five data rows; source data was not exported.\n",
                options,
                "native_structure",
            );
        }
        append(report, "\n", options, "native_structure");
    }
}

pub(crate) fn escape(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '#' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn append(
    report: &mut InspectReport,
    block: &str,
    options: &InspectOptions,
    basis: &'static str,
) {
    if report.markdown.truncated {
        return;
    }
    let remaining = options
        .max_chars
        .saturating_sub(report.markdown.content.chars().count());
    let take = block.chars().count().min(remaining);
    report.markdown.content.extend(block.chars().take(take));
    report.markdown.truncated = take < block.chars().count();
    report.markdown.basis = if report.markdown.basis.is_empty() || report.markdown.basis == basis {
        basis
    } else {
        "mixed"
    };
}

pub(crate) fn table(table: &crate::DocumentTable, cap: usize) -> String {
    use std::collections::BTreeMap;
    let cells: BTreeMap<_, _> = table
        .cells
        .iter()
        .map(|c| ((c.row, c.column), c.text.as_str()))
        .collect();
    let mut out = String::new();
    let row = |number| {
        let mut line = String::from("|");
        for column in 1..=table.cols {
            let value = cells.get(&(number, column)).copied().unwrap_or("");
            line.push_str(&format!(
                " {} |",
                escape(value).replace('|', "\\|").replace('\n', "<br>")
            ));
            if line.len() > cap.saturating_mul(4) {
                break;
            }
        }
        line.push('\n');
        line
    };
    let has_header = table.header_rows.contains(&1);
    if has_header {
        out.push_str(&row(1));
    } else {
        out.push('|');
        for col in 1..=table.cols {
            out.push_str(&format!(" Column {col} |"));
            if out.len() > cap.saturating_mul(4) {
                break;
            }
        }
        out.push('\n');
    }
    out.push('|');
    for _ in 1..=table.cols {
        out.push_str(" --- |");
        if out.len() > cap.saturating_mul(4) {
            break;
        }
    }
    out.push('\n');
    for number in if has_header { 2 } else { 1 }..=table.rows {
        if out.len() > cap.saturating_mul(4) {
            break;
        }
        out.push_str(&row(number));
    }
    out.push('\n');
    out
}

pub(crate) fn word(root: &Node, report: &mut InspectReport, options: &InspectOptions) {
    fn walk(node: &Node, report: &mut InspectReport, options: &InspectOptions, index: &mut usize) {
        if node.is(W, "tbl") {
            if let Some(model) = report.tables.get(*index) {
                let block = table(model, options.max_chars);
                append(report, &block, options, "native_structure");
            }
            *index += node.descendants(W, "tbl").len();
        } else if node.is(W, "p") {
            let text = node
                .descendants(W, "t")
                .iter()
                .map(|t| t.text())
                .collect::<Vec<_>>()
                .join("");
            let heading = node
                .descendants(W, "outlineLvl")
                .first()
                .and_then(|n| n.attr_ns(W, "val"))
                .and_then(|s| s.parse::<usize>().ok())
                .map(|n| n + 1)
                .or_else(|| {
                    node.descendants(W, "pStyle")
                        .first()
                        .and_then(|n| n.attr_ns(W, "val"))
                        .and_then(|s| s.strip_prefix("Heading"))
                        .and_then(|s| s.parse::<usize>().ok())
                });
            let prefix = if let Some(level) = heading {
                format!("{} ", "#".repeat(level.clamp(1, 6)))
            } else if !node.descendants(W, "numPr").is_empty() {
                "- ".into()
            } else {
                String::new()
            };
            if !text.is_empty() {
                append(
                    report,
                    &format!("{prefix}{}\n\n", escape(&text)),
                    options,
                    "native_structure",
                );
            }
            if !node.descendants(W, "sectPr").is_empty() {
                append(report, "\n---\n\n", options, "native_structure");
            }
        } else {
            for child in node.children() {
                walk(child, report, options, index);
            }
        }
    }
    walk(root, report, options, &mut 0);
}

pub(crate) fn drawing(
    root: &Node,
    report: &mut InspectReport,
    options: &InspectOptions,
    mut index: usize,
) {
    use crate::office::{A, P};
    fn walk(
        node: &Node,
        title: bool,
        report: &mut InspectReport,
        options: &InspectOptions,
        index: &mut usize,
    ) {
        if node.is(A, "tbl") {
            if let Some(model) = report.tables.get(*index) {
                let block = table(model, options.max_chars);
                append(report, &block, options, "native_structure");
            }
            *index += 1;
        } else if node.is(A, "p") {
            let text = node
                .descendants(A, "t")
                .iter()
                .map(|n| n.text())
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() {
                return;
            }
            let level = node
                .descendants(A, "pPr")
                .first()
                .and_then(|n| n.attr("lvl"))
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0)
                .min(8);
            let prefix = if title {
                "### ".into()
            } else if !node.descendants(A, "buAutoNum").is_empty() {
                format!("{}1. ", "  ".repeat(level))
            } else if !node.descendants(A, "buChar").is_empty() {
                format!("{}- ", "  ".repeat(level))
            } else {
                String::new()
            };
            append(
                report,
                &format!("{prefix}{}\n\n", escape(&text)),
                options,
                "native_structure",
            );
        } else {
            let title = title
                || (node.is(P, "sp")
                    && node
                        .descendants(P, "ph")
                        .iter()
                        .any(|n| matches!(n.attr("type"), Some("title" | "ctrTitle"))));
            for child in node.children() {
                walk(child, title, report, options, index);
            }
        }
    }
    walk(root, false, report, options, &mut index);
}

pub(crate) fn odf(
    root: &Node,
    report: &mut InspectReport,
    options: &InspectOptions,
    mut index: usize,
    budget: &mut crate::odf::ExpansionBudget,
) {
    use crate::odf::{T, TABLE};
    fn walk(
        node: &Node,
        depth: usize,
        report: &mut InspectReport,
        options: &InspectOptions,
        index: &mut usize,
        budget: &mut crate::odf::ExpansionBudget,
        failed: &mut bool,
    ) {
        if node.is(TABLE, "table") {
            for _ in 0..node.descendants(TABLE, "table").len() {
                if let Some(model) = report.tables.get(*index) {
                    let block = table(model, options.max_chars);
                    append(report, &block, options, "native_structure");
                }
                *index += 1;
            }
        } else if node.is(T, "p") || node.is(T, "h") {
            match budget.text(node) {
                Ok(text) => {
                    let prefix = if node.is(T, "h") {
                        let level = node
                            .attr_ns(T, "outline-level")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(1)
                            .clamp(1, 6);
                        format!("{} ", "#".repeat(level))
                    } else if depth > 0 {
                        format!("{}- ", "  ".repeat(depth.saturating_sub(1).min(8)))
                    } else {
                        String::new()
                    };
                    append(
                        report,
                        &format!("{prefix}{}\n\n", escape(&text)),
                        options,
                        "native_structure",
                    );
                }
                Err(error) => {
                    // One honest error per walk; later paragraphs fail the same way.
                    if !*failed {
                        *failed = true;
                        report.error("markdown", format!("ODF text expansion: {error}"));
                    }
                }
            }
        } else {
            let depth = depth + usize::from(node.is(T, "list"));
            for child in node.children() {
                walk(child, depth, report, options, index, budget, failed);
            }
            if node.is(T, "section") {
                append(report, "\n---\n\n", options, "native_structure");
            }
        }
    }
    let mut failed = false;
    walk(root, 0, report, options, &mut index, budget, &mut failed);
}

pub(crate) fn xhtml(root: &Node, report: &mut InspectReport, options: &InspectOptions) {
    use crate::xml::Content;
    fn walk(node: &Node, out: &mut String, cap: usize, ordered: bool) {
        if out.len() >= cap.saturating_mul(4)
            || matches!(node.name.as_str(), "head" | "script" | "style")
        {
            return;
        }
        let name = node.name.as_str();
        let marker = match name {
            "em" | "i" => "*",
            "strong" | "b" => "**",
            "code" => "`",
            _ => "",
        };
        if matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            out.push_str(&format!(
                "\n{} ",
                "#".repeat(name.as_bytes()[1].saturating_sub(b'0') as usize)
            ));
        } else if name == "li" {
            out.push_str(if ordered { "1. " } else { "- " });
        } else if name == "hr" {
            out.push_str("\n---\n\n");
        } else if name == "br" {
            out.push_str("  \n");
        }
        out.push_str(marker);
        for item in &node.content {
            match item {
                Content::Text(text) => out.push_str(&escape(text)),
                Content::Element(child) => walk(child, out, cap, ordered || name == "ol"),
            }
            if out.len() >= cap.saturating_mul(4) {
                break;
            }
        }
        out.push_str(marker);
        if matches!(
            name,
            "p" | "div" | "section" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol"
        ) {
            out.push_str("\n\n");
        } else if name == "li" {
            out.push('\n');
        }
    }
    let mut out = String::new();
    walk(root, &mut out, options.max_chars, false);
    append(report, &out, options, "native_structure");
}

pub(crate) fn finish(report: &mut InspectReport, options: &InspectOptions) {
    if report.markdown.content.is_empty() {
        let body = if !report.text.content.is_empty() {
            escape(&report.text.content)
        } else {
            format!(
                "# File inspection\n\n- Format: {}\n- Bytes: {}\n",
                escape(&report.format.bucket),
                report.counts.bytes
            )
        };
        append(report, &body, options, "text_fallback");
    }
    if let Some(budget) = options.token_budget()
        && crate::truncate_words(&mut report.markdown.content, budget)
    {
        report.markdown.truncated = true;
    }
    report.markdown.truncated |= report.text.truncated || !report.errors.is_empty();
    if report.markdown.truncated {
        report.markdown.content.push_str("\n\n> Partial output: preview budget, safety limit, or extraction error. See the JSON report.\n");
    }
}

#[cfg(test)]
mod bounds_tests {
    #[test]
    fn push_bounded_marks_truncated_when_cell_overhead_exceeds_cap() {
        let mut rows = super::SheetRows::default();
        rows.push_bounded(&[(1usize, "H")], 1, 8);
        assert!(!rows.is_truncated(), "header fits the 8-char budget");
        // Three empty cells charge 4 chars of per-cell overhead each:
        // 5 + 12 = 17 exceeds the cap even though no text was cut.
        rows.push_bounded(&[(1usize, ""), (2, ""), (3, "")], 1, 8);
        assert!(
            rows.is_truncated(),
            "empty-cell overhead charged past the cap must mark truncation"
        );
    }

    /// W3-SYS-MARKDOWN-ROWS: truncation records only retained preview rows;
    /// later repeated and skipped rows set the existing truncation flags
    /// without pretending their unstored rows entered the preview. Accepts no
    /// arguments and returns unit; assertions panic on inconsistent row semantics.
    #[test]
    fn push_bounded_after_truncation_keeps_stored_row_count_honest() {
        let mut rows = super::SheetRows::default();
        rows.push_bounded(&[(1usize, "H")], 1, 8);
        rows.push_bounded(&[(1usize, ""), (2, ""), (3, "")], 1, 8);
        assert!(rows.truncated);
        assert_eq!(rows.sampled_rows, 2);
        assert_eq!(rows.rows.len(), 2);

        rows.push_bounded(&[(1usize, "later")], 3, 8);
        rows.push_bounded(&[(1usize, "skipped")], 1, 8);
        assert_eq!(rows.sampled_rows, 2);
        assert_eq!(rows.rows.len(), 2);
        assert!(!rows.more_rows);
        assert!(rows.truncated);

        rows.push_bounded(&[(1usize, "repeated")], 10, 8);
        assert_eq!(rows.sampled_rows, 2);
        assert_eq!(rows.rows.len(), 2);
        assert!(rows.more_rows);
        assert!(rows.truncated);
    }

    /// W3-SYS-MARKDOWN-ROWS: six retained rows fill the preview exactly; a
    /// seventh observed row is skipped and sets `more_rows` without changing
    /// the retained row count. Accepts no arguments and returns unit; assertion
    /// failures identify an incorrect boundary, count, or truncation flag.
    #[test]
    fn push_bounded_honors_the_exact_six_row_boundary() {
        let mut rows = super::SheetRows::default();
        for _ in 0..6 {
            rows.push_bounded(&[(1usize, "row")], 1, 100);
        }
        assert_eq!(rows.sampled_rows, 6);
        assert_eq!(rows.rows.len(), 6);
        assert!(!rows.more_rows);
        assert!(!rows.truncated);

        rows.push_bounded(&[(1usize, "seventh")], 1, 100);
        assert_eq!(rows.sampled_rows, 6);
        assert_eq!(rows.rows.len(), 6);
        assert!(rows.more_rows);
        assert!(!rows.truncated);
    }
}
